//! Real-time playback with live effects.
//!
//! ```text
//!  dry clip render (Arc) --> [render-ahead thread: fx::Chain -> resampler] --rtrb--> cpal callback
//!                                   ^ UpdateFx / gains (from a background resolver)
//! ```
//! The cpal callback only pops from a lock-free ring (no locks, no allocation). The render thread
//! keeps ~80 ms queued, so slider moves and A/B are heard within about a tenth of a second without
//! restarting. The chain is the same code export uses, so playback == export.
//!
//! Clip effects (Layering Effects) live in the dry render. When they change, the dry audio is re-rendered
//! on a worker thread and handed over as a new `Arc` (`Ctl::Dry`); the render-ahead thread swaps it in at
//! the current position and the track chain keeps its state. The audio callback never sees any of this.
use crate::fx::{self, Chain, Slot};
use crate::waveform::Decoded;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::*};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct PlayRequest { pub dry: Arc<Decoded>, pub slots: Vec<Slot>, pub gains: Vec<Option<f32>>, pub start_secs: f64 }

pub enum Ctl { Fx(Vec<Slot>), Gains(u64, Vec<Option<f32>>), Dry(Arc<Decoded>) }

/// Transport state the callback acts on (`PlayShared::state`).
pub const IDLE: u8 = 0;
pub const PLAYING: u8 = 1;
/// Fading out; the callback then drops whatever is still queued and goes `IDLE`.
pub const STOPPING: u8 = 2;
/// De-click ramp length for stop / pause / resume / mid-file start (ms). Short enough to feel instant,
/// long enough that cutting a loud waveform mid-cycle doesn't click.
pub const DECLICK_MS: f32 = 6.0;

/// Shared with the UI meter thread and the callback.
#[derive(Default)]
pub struct PlayShared { pub pos: AtomicU64, pub len: AtomicU64, pub rate: AtomicU64, pub playing: AtomicBool, pub paused: AtomicBool, pub done: AtomicBool, pub underruns: AtomicU64,
    /// Rate / channels of the dry audio being played (a re-render can only be swapped in if they match).
    pub src_rate: AtomicU64, pub src_ch: AtomicU64,
    /// IDLE / PLAYING / STOPPING (see above); set by the engine, finished by the callback.
    pub state: AtomicU8,
    /// Ramp in when the next session starts (a start mid-file would otherwise click).
    pub fade_in: AtomicBool,
    /// The device stream reported an error; the engine reopens it.
    pub stream_err: AtomicBool,
    /// Latency tracing (`STREAMWAVE_PERF`): first real samples handed to the device / first silence after a stop.
    pub first_audio: crate::perf::Stamp, pub first_cb: crate::perf::Stamp, pub silent: crate::perf::Stamp }

/// The persistent output: one shared-mode device stream that stays open between plays (outputting silence while
/// idle), so pressing Play doesn't pay for device lookup and stream setup. Sessions borrow its ring producer.
pub struct Deck { _stream: cpal::Stream, pub dev_id: String, pub rate: u32, pub ch: usize, prod: Option<rtrb::Producer<f32>>, cap: usize }

/// One playback: the render-ahead thread feeding the deck's ring. Ending it hands the producer back.
pub struct Session { stop: Arc<AtomicBool>, ctl: Sender<Ctl>, render: Option<JoinHandle<rtrb::Producer<f32>>> }

impl Session {
    pub fn update_fx(&self, slots: Vec<Slot>) { let _ = self.ctl.send(Ctl::Fx(slots)); }
    pub fn update_dry(&self, d: Arc<Decoded>) { let _ = self.ctl.send(Ctl::Dry(d)); }
    /// Stop the render thread and get the ring producer back (None if the thread panicked).
    pub fn end(mut self) -> Option<rtrb::Producer<f32>> { self.stop.store(true, Relaxed); self.render.take().and_then(|h| h.join().ok()) }
}
impl Drop for Session {
    fn drop(&mut self) { self.stop.store(true, Relaxed); if let Some(h) = self.render.take() { let _ = h.join(); } }
}

impl Deck {
    /// Open the device in shared mode (WASAPI shared / sound-server routes only, see `devices`). Starts idle.
    pub fn open(dev: &cpal::Device, dev_id: String, sh: Arc<PlayShared>, buffer_frames: u32) -> anyhow::Result<Deck> {
        let sup = dev.default_output_config()?;
        let mut cfg: cpal::StreamConfig = sup.config();
        cfg.buffer_size = match sup.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => cpal::BufferSize::Fixed(buffer_frames.clamp(*min, *max)),
            cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
        };
        let (ch, rate) = (cfg.channels as usize, cfg.sample_rate.0);
        // ring: up to 250 ms; sessions keep ~80 ms queued for snappy live changes
        let cap = (rate as usize * ch / 4).max(4096);
        let (prod, cons) = rtrb::RingBuffer::<f32>::new(cap);
        sh.state.store(IDLE, Relaxed); sh.stream_err.store(false, Relaxed);
        let stream = match sup.sample_format() {
            cpal::SampleFormat::F32 => out_stream::<f32>(dev, &cfg, cons, sh.clone())?,
            cpal::SampleFormat::I16 => out_stream::<i16>(dev, &cfg, cons, sh.clone())?,
            cpal::SampleFormat::U16 => out_stream::<u16>(dev, &cfg, cons, sh.clone())?,
            cpal::SampleFormat::I32 => out_stream::<i32>(dev, &cfg, cons, sh.clone())?,
            cpal::SampleFormat::F64 => out_stream::<f64>(dev, &cfg, cons, sh.clone())?,
            f => anyhow::bail!("unsupported output format {f:?}"),
        };
        stream.play()?;
        Ok(Deck { _stream: stream, dev_id, rate, ch, prod: Some(prod), cap })
    }
    pub fn give_back(&mut self, p: rtrb::Producer<f32>) { self.prod = Some(p) }
    pub fn has_producer(&self) -> bool { self.prod.is_some() }

    /// Start a session. The caller has made sure the callback is IDLE (ring flushed). Returns once ~20 ms are queued
    /// (the render thread runs far faster than real time, so that is ~1 ms), then flips the callback to PLAYING.
    pub fn start(&mut self, req: PlayRequest, sh: Arc<PlayShared>) -> anyhow::Result<Session> {
        let prod = self.prod.take().ok_or_else(|| anyhow::anyhow!("output busy"))?;
        let (out_sr, out_ch) = (self.rate, self.ch);
        let d = req.dry.clone();
        let total_src = d.samples.len() / d.channels.max(1);
        let start_src = ((req.start_secs.max(0.0) * d.sample_rate as f64) as usize).min(total_src);
        let to_dev = |src: usize| (src as f64 * out_sr as f64 / d.sample_rate as f64) as u64;
        sh.rate.store(out_sr as u64, Relaxed);
        sh.src_rate.store(d.sample_rate as u64, Relaxed); sh.src_ch.store(d.channels as u64, Relaxed);
        sh.len.store(to_dev(total_src), Relaxed);
        sh.pos.store(to_dev(start_src), Relaxed);
        sh.done.store(false, Relaxed); sh.paused.store(false, Relaxed); sh.underruns.store(0, Relaxed);
        let target = (out_sr as usize * out_ch * 8 / 100).max(2048).min(self.cap * 3 / 4);
        let prefill = (out_sr as usize * out_ch / 50).min(target); // 20 ms
        let stop = Arc::new(AtomicBool::new(false));
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let res_tx = ctl_tx.clone();
        let (stop2, sh2) = (stop.clone(), sh.clone());
        let queued = Arc::new(AtomicU64::new(0));
        let q2 = queued.clone();
        let render = std::thread::Builder::new().name("sw-play-render".into()).spawn(move || {
            render_loop(req, start_src, out_sr, out_ch, prod, target, prefill, q2, ctl_rx, res_tx, stop2, sh2)
        })?;
        // wait for the prefill (or the end of very short audio), not a fixed sleep
        let t0 = std::time::Instant::now();
        while queued.load(Acquire) == 0 && t0.elapsed() < Duration::from_millis(40) { std::thread::sleep(Duration::from_micros(250)) }
        crate::perf::mark("engine: prefilled");
        sh.fade_in.store(start_src > 0, Relaxed);
        sh.playing.store(true, Relaxed);
        sh.state.store(PLAYING, Release);
        Ok(Session { stop, ctl: ctl_tx, render: Some(render) })
    }
}

/// Callback-side state of the de-click ramp. Pure, so the exact samples it produces are unit-tested.
pub struct Ramp { pub gain: f32, step: f32, last: u8 }
impl Ramp {
    pub fn new(rate: u32) -> Self { Self { gain: 0.0, step: 1.0 / (DECLICK_MS * rate as f32 / 1000.0).max(1.0), last: IDLE } }
    /// Fill `out` (interleaved, `ch` channels) from `pop` (which yields whole frames' samples), following `state`.
    /// Returns (frames consumed from the ring, new state if the callback finished a transition).
    /// - PLAYING: unity gain (bit-exact pass-through) once ramped in; ramps to 0 while `paused` and stops pulling.
    /// - STOPPING: ramps to 0 over `DECLICK_MS`, then reports IDLE (the caller flushes the ring).
    pub fn fill(&mut self, out: &mut [f32], ch: usize, avail_frames: usize, state: u8, paused: bool, fade_in: bool, mut pop: impl FnMut() -> f32) -> (usize, Option<u8>) {
        if state == PLAYING && self.last != PLAYING { self.gain = if fade_in { 0.0 } else { 1.0 } }
        self.last = state;
        if state == IDLE { out.fill(0.0); self.gain = 0.0; return (0, None) }
        let target = if state == PLAYING && !paused { 1.0 } else { 0.0 };
        let mut used = 0usize;
        for f in out.chunks_mut(ch) {
            if self.gain <= 0.0 && target == 0.0 { f.fill(0.0); continue }
            if used >= avail_frames { f.fill(0.0); continue }
            used += 1;
            if self.gain == 1.0 && target == 1.0 { for o in f.iter_mut() { *o = pop() } continue }
            let g = self.gain;
            for o in f.iter_mut() { *o = pop() * g }
            self.gain = if target > g { (g + self.step).min(1.0) } else { (g - self.step).max(0.0) };
        }
        let done = (state == STOPPING && self.gain <= 0.0).then_some(IDLE);
        if done.is_some() { self.last = IDLE }
        (used, done)
    }
}

/// Input frames buffered per rubato call (adds ~21 ms of pipeline latency, only when
/// the output device rate differs from the source rate).
const RESAMPLE_CHUNK: usize = 1024;

/// Streaming resampler: FFT-based rate conversion (rubato) when rates differ, identity
/// fast-path otherwise; channel mapping runs as a separate final stage.
pub struct Resampler {
    inner: Option<rubato::Fft<f32>>,
    buf: Vec<f32>, // whole interleaved input frames not yet fed to rubato
    in_ch: usize, out_ch: usize,
}
impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, in_ch: usize, out_ch: usize) -> Self {
        let (in_ch, out_ch) = (in_ch.max(1), out_ch.max(1));
        let inner = (in_rate != out_rate).then(|| {
            // FixedSync::Both: rubato rounds the chunk to a valid block multiple on both sides
            // (e.g. 48k→44.1k turns 1024 into 1120 in / 1029 out per call) and keeps the ratio exact.
            rubato::Fft::<f32>::new(in_rate as usize, out_rate as usize, RESAMPLE_CHUNK, in_ch, rubato::FixedSync::Both)
                .expect("resampler construction")
        });
        Self { inner, buf: Vec::with_capacity(RESAMPLE_CHUNK * in_ch * 2), in_ch, out_ch }
    }
    fn map(f: &[f32], out: &mut Vec<f32>, in_ch: usize, out_ch: usize) {
        if out_ch == 1 && in_ch > 1 { out.push(f.iter().sum::<f32>() / in_ch as f32) }
        else { for c in 0..out_ch { out.push(f[c % in_ch]) } }
    }
    fn emit(interleaved: &[f32], out: &mut Vec<f32>, in_ch: usize, out_ch: usize) {
        if in_ch == out_ch { out.extend_from_slice(interleaved) }
        else { for f in interleaved.chunks(in_ch) { Self::map(f, out, in_ch, out_ch) } }
    }
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        match &mut self.inner {
            None => Self::emit(input, out, self.in_ch, self.out_ch),
            Some(rs) => {
                use rubato::Resampler as _;
                let (in_ch, out_ch) = (self.in_ch, self.out_ch);
                self.buf.extend_from_slice(input);
                loop {
                    let need = rs.input_frames_next();
                    if self.buf.len() / in_ch < need { break }
                    let view = rubato::audioadapter_buffers::direct::InterleavedSlice::new(&self.buf[..need * in_ch], in_ch, need).expect("adapter view");
                    match rs.process(&view, None) {
                        Ok(o) => { let inter = o.take_data(); Self::emit(&inter, out, in_ch, out_ch) }
                        Err(_) => break,
                    }
                    self.buf.drain(..need * in_ch);
                }
            }
        }
    }
    /// Drain the input tail: zero-pad to a full chunk and mark the real length via `partial_len`.
    /// Drain the input tail: feed the remaining real frames as a partial chunk, then pump
    /// zero chunks until the resampler's internal delay has been emitted.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if let Some(rs) = &mut self.inner {
            use rubato::Resampler as _;
            let (in_ch, out_ch) = (self.in_ch, self.out_ch);
            let mut tmp = std::mem::take(&mut self.buf);
            let avail = tmp.len() / in_ch;
            if avail > 0 {
                let need = rs.input_frames_next();
                tmp.resize(need * in_ch, 0.0);
                let view = rubato::audioadapter_buffers::direct::InterleavedSlice::new(&tmp, in_ch, need).expect("adapter view");
                let idx = rubato::Indexing { input_offset: 0, output_offset: 0, partial_len: Some(avail), active_channels_mask: None };
                if let Ok(o) = rs.process(&view, Some(&idx)) { let inter = o.take_data(); Self::emit(&inter, out, in_ch, out_ch) }
            }
            // the resampler holds back ~output_delay() output frames; pump zeros to release them
            let mut produced = 0usize;
            let delay = rs.output_delay();
            let mut zeros: Vec<f32> = Vec::new();
            while produced < delay {
                let need = rs.input_frames_next();
                zeros.clear(); zeros.resize(need * in_ch, 0.0);
                let view = rubato::audioadapter_buffers::direct::InterleavedSlice::new(&zeros, in_ch, need).expect("adapter view");
                let idx = rubato::Indexing { input_offset: 0, output_offset: 0, partial_len: Some(0), active_channels_mask: None };
                match rs.process(&view, Some(&idx)) {
                    Ok(o) => { let inter = o.take_data(); produced += inter.len() / in_ch; Self::emit(&inter, out, in_ch, out_ch) }
                    Err(_) => break,
                }
            }
        }
    }
}

fn out_stream<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, mut cons: rtrb::Consumer<f32>, sh: Arc<PlayShared>) -> anyhow::Result<cpal::Stream>
where T: SizedSample + FromSample<f32> {
    let ch = cfg.channels as usize;
    let mut ramp = Ramp::new(cfg.sample_rate.0);
    // scratch for the ramp, sized once here: callbacks bigger than this are handled in pieces
    let mut buf = vec![0f32; 8192 * ch];
    let sh2 = sh.clone();
    let s = dev.build_output_stream(cfg, move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        // REAL-TIME: no locks, no allocation, no syscalls (the perf stamps are one clock read, only when tracing).
        sh.first_cb.hit();
        let state = sh.state.load(Acquire);
        let (paused, fade_in) = (sh.paused.load(Relaxed), sh.fade_in.load(Relaxed));
        let mut popped = 0usize; let mut want = 0usize; let mut finished = None;
        for piece in out.chunks_mut(buf.len()) {
            let b = &mut buf[..piece.len()];
            let frames = piece.len() / ch;
            let avail = cons.slots() / ch; // whole frames only, so channels never slip on an underrun
            let (used, fin) = ramp.fill(b, ch, avail, state, paused, fade_in, || cons.pop().unwrap_or(0.0));
            for (o, v) in piece.iter_mut().zip(b.iter()) { *o = T::from_sample(*v) }
            popped += used; if state == PLAYING && !paused { want += frames }
            if fin.is_some() { finished = fin }
        }
        if state == PLAYING {
            if popped > 0 { sh.first_audio.hit() }
            sh.pos.fetch_add(popped as u64, Relaxed);
            if popped < want {
                if sh.done.load(Relaxed) { sh.playing.store(false, Relaxed); sh.state.store(IDLE, Release) } else { sh.underruns.fetch_add(1, Relaxed); }
            }
        }
        if finished == Some(IDLE) {
            // faded out: drop the rest of the old session's queue so the next Play starts clean
            let n = cons.slots(); if let Ok(c) = cons.read_chunk(n) { c.commit_all() }
            sh.state.store(IDLE, Release); sh.silent.hit();
        }
    }, move |e| { log::warn!("output stream error: {e}"); sh2.stream_err.store(true, Relaxed) }, None)?;
    Ok(s)
}

/// Frames per render-ahead block.
pub const BLOCK: usize = 512;

/// The effect stage of the render-ahead thread: dry audio -> track chain, block by block. No device, so
/// tests run exactly what playback runs.
pub struct Feeder { pub d: Arc<Decoded>, chain: Chain, pub slots: Vec<Slot>, pub gains: Vec<Option<f32>>, pub pos: usize, pub total: usize, flushed: bool }
impl Feeder {
    pub fn new(d: Arc<Decoded>, slots: Vec<Slot>, gains: Vec<Option<f32>>, start: usize) -> Self {
        let ch = d.channels.max(1); let total = d.samples.len() / ch;
        let chain = Chain::new(&slots, &gains, ch, d.sample_rate, total, start.min(total));
        Self { d, chain, slots, gains, pos: start.min(total), total, flushed: false }
    }
    pub fn finished(&self) -> bool { self.pos >= self.total && self.flushed }
    /// Append the next processed block (or the chain's flushed tail at the end). False once everything is out.
    pub fn next(&mut self, out: &mut Vec<f32>) -> bool {
        if self.pos >= self.total {
            if self.flushed { return false }
            self.chain.flush(out); self.flushed = true; return true
        }
        let ch = self.d.channels.max(1); let n = BLOCK.min(self.total - self.pos);
        self.chain.process(&self.d.samples[self.pos * ch..(self.pos + n) * ch], out); self.pos += n;
        true
    }
    /// Live track-effect change: keep state where the effect type at a position is unchanged.
    pub fn set_fx(&mut self, slots: Vec<Slot>, gains: Vec<Option<f32>>) {
        self.chain.update(&slots, &gains, self.d.sample_rate, self.total, self.pos);
        self.slots = slots; self.gains = gains;
    }
    pub fn set_gains(&mut self, gains: Vec<Option<f32>>) { self.chain.set_gains(&gains); self.gains = gains; }
    /// Swap in a re-rendered dry buffer (e.g. a clip's effect stack changed) at the current position. The track
    /// chain keeps its filter/envelope state, so there is no click and no restart. False if the format differs or
    /// the stream has already been flushed (then the caller restarts playback instead).
    pub fn swap_dry(&mut self, nd: Arc<Decoded>) -> bool {
        if nd.channels != self.d.channels || nd.sample_rate != self.d.sample_rate || self.flushed { return false }
        self.total = nd.samples.len() / nd.channels.max(1);
        self.d = nd; self.pos = self.pos.min(self.total);
        let (slots, gains) = (self.slots.clone(), self.gains.clone());
        self.chain.update(&slots, &gains, self.d.sample_rate, self.total, self.pos); // refresh lengths (Fade), keep state
        true
    }
}

#[allow(clippy::too_many_arguments)]
fn render_loop(req: PlayRequest, start_src: usize, out_sr: u32, out_ch: usize, mut prod: rtrb::Producer<f32>, target: usize, prefill: usize,
               ready: Arc<AtomicU64>, ctl: Receiver<Ctl>, res_tx: Sender<Ctl>, stop: Arc<AtomicBool>, sh: Arc<PlayShared>) -> rtrb::Producer<f32> {
    let ch = req.dry.channels.max(1);
    let src_rate = req.dry.sample_rate;
    let mut feed = Feeder::new(req.dry, req.slots, req.gains, start_src);
    let mut rs = Resampler::new(src_rate, out_sr, ch, out_ch);
    let (mut fxout, mut devout): (Vec<f32>, Vec<f32>) = (Vec::with_capacity(8192), Vec::with_capacity(8192));
    let mut pending: Vec<f32> = Vec::new(); // produced but not yet pushed
    let mut gen = 0u64;
    let gen_now = Arc::new(AtomicU64::new(0)); // lets a stale re-measure give up early
    // re-measure whole-file stages (Normalize / Loudness) in the background
    let remeasure = |gen: &mut u64, d: Arc<Decoded>, slots: Vec<Slot>| {
        *gen += 1; gen_now.store(*gen, Relaxed);
        let (tx, my, latest) = (res_tx.clone(), *gen, gen_now.clone());
        std::thread::Builder::new().name("sw-fx-resolve".into()).spawn(move || {
            if let Some(g) = fx::resolve_gains(&d, &slots, Some(&|| latest.load(Relaxed) != my)) { let _ = tx.send(Ctl::Gains(my, g)); }
        }).ok();
    };
    while !stop.load(Relaxed) {
        // control messages
        while let Ok(m) = ctl.try_recv() {
            match m {
                Ctl::Fx(new) => {
                    // keep gains for unchanged global stages until the re-measure lands
                    let carried: Vec<Option<f32>> = new.iter().enumerate().map(|(i, s)| {
                        feed.slots.get(i).filter(|o| o.fx == s.fx && o.bypass == s.bypass).and_then(|_| feed.gains.get(i).copied().flatten())
                            .or_else(|| if s.fx.is_global() { feed.gains.get(i).copied().flatten() } else { None })
                    }).collect();
                    let needs = new.iter().zip(&feed.slots).any(|(a, b)| a != b) || new.len() != feed.slots.len();
                    feed.set_fx(new, carried);
                    if needs && feed.slots.iter().any(|s| !s.bypass && s.fx.is_global()) { remeasure(&mut gen, feed.d.clone(), feed.slots.clone()) }
                }
                Ctl::Gains(g, v) => if g == gen && v.len() == feed.slots.len() { feed.set_gains(v) },
                Ctl::Dry(nd) => {
                    if feed.swap_dry(nd) {
                        sh.len.store((feed.total as f64 * out_sr as f64 / src_rate as f64) as u64, Relaxed);
                        if feed.slots.iter().any(|s| !s.bypass && s.fx.is_global()) { remeasure(&mut gen, feed.d.clone(), feed.slots.clone()) }
                    } else { log::warn!("playback: re-rendered audio could not be swapped in (format changed)") }
                }
            }
        }
        // push leftovers first
        if !pending.is_empty() {
            let n = prod.slots().min(pending.len());
            for v in pending.drain(..n) { let _ = prod.push(v); }
            if !pending.is_empty() { std::thread::sleep(Duration::from_millis(3)); continue }
        }
        let queued = prod.buffer().capacity() - prod.slots();
        if queued >= prefill || feed.finished() { ready.store(1, Release) }
        if queued >= target || sh.paused.load(Relaxed) { std::thread::sleep(Duration::from_millis(3)); continue }
        if feed.finished() {
            if pending.is_empty() { sh.done.store(true, Relaxed) }
            std::thread::sleep(Duration::from_millis(5)); continue
        }
        let at_end = feed.pos >= feed.total;
        fxout.clear(); feed.next(&mut fxout);
        devout.clear(); rs.process(&fxout, &mut devout);
        if at_end { rs.flush(&mut devout) }
        pending.extend_from_slice(&devout);
    }
    prod
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Run the callback ramp over `buf` (interleaved stereo) from a queue, like the device callback does.
    fn cb(r: &mut Ramp, q: &mut std::collections::VecDeque<f32>, frames: usize, state: u8, paused: bool, fade_in: bool) -> (Vec<f32>, usize, Option<u8>) {
        let mut out = vec![9.0; frames * 2];
        let avail = q.len() / 2;
        let (n, fin) = r.fill(&mut out, 2, avail, state, paused, fade_in, || q.pop_front().unwrap_or(0.0));
        (out, n, fin)
    }
    #[test] fn ramp_passes_audio_bit_exact_when_playing() {
        // playback == export: once running, the callback stage must not touch the samples
        let x: Vec<f32> = (0..4800).map(|i| ((i as f32) * 0.37).sin() * 0.8).collect();
        let mut q: std::collections::VecDeque<f32> = x.iter().copied().collect();
        let mut r = Ramp::new(48000);
        let (out, n, fin) = cb(&mut r, &mut q, 2400, PLAYING, false, false);
        assert_eq!((n, fin), (2400, None));
        assert_eq!(out, x);
    }
    #[test] fn idle_is_silent_and_pulls_nothing() {
        let mut q: std::collections::VecDeque<f32> = vec![0.5; 200].into();
        let mut r = Ramp::new(48000);
        let (out, n, _) = cb(&mut r, &mut q, 64, IDLE, false, false);
        assert!(out.iter().all(|&v| v == 0.0)); assert_eq!(n, 0); assert_eq!(q.len(), 200);
    }
    #[test] fn stop_fades_out_quickly_without_a_step() {
        let mut q: std::collections::VecDeque<f32> = vec![1.0; 48000].into();
        let mut r = Ramp::new(48000);
        cb(&mut r, &mut q, 256, PLAYING, false, false);
        let (out, n, fin) = cb(&mut r, &mut q, 512, STOPPING, false, false);
        let fade = (DECLICK_MS * 48.0) as usize; // frames
        assert_eq!(fin, Some(IDLE), "the fade completes inside one callback");
        assert!(n >= fade - 1 && n <= fade + 1, "{n} frames used for the fade");
        let l: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert!(l.windows(2).all(|w| w[1] <= w[0] + 1e-6), "monotonic fade");
        assert!(l.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01), "no step bigger than 1 % (no click)");
        assert!(l[fade + 2..].iter().all(|&v| v == 0.0), "silent after {DECLICK_MS} ms");
        assert!(DECLICK_MS <= 10.0);
    }
    #[test] fn pause_fades_holds_and_resumes_where_it_left_off() {
        let x: Vec<f32> = (0..20000).map(|i| i as f32).collect();
        let mut q: std::collections::VecDeque<f32> = x.iter().copied().collect();
        let mut r = Ramp::new(48000);
        let (_, a, _) = cb(&mut r, &mut q, 100, PLAYING, false, false);
        let (_, b, _) = cb(&mut r, &mut q, 1000, PLAYING, true, false); // fade out, then hold
        let held = q.len();
        let (out, c, _) = cb(&mut r, &mut q, 500, PLAYING, true, false);
        assert_eq!(c, 0); assert_eq!(q.len(), held); assert!(out.iter().all(|&v| v == 0.0));
        let (out, _, _) = cb(&mut r, &mut q, 400, PLAYING, false, false); // resume: ramps in from the next sample
        assert_eq!(out[0], 0.0);
        let next = ((a + b) * 2) as f32;
        assert!((out[798] - (next + 798.0)).abs() < 1e-3, "unity again after the ramp, continuing at {next}");
    }
    #[test] fn mid_file_start_ramps_in_but_start_at_zero_does_not() {
        let mut q: std::collections::VecDeque<f32> = vec![1.0; 2000].into();
        let mut r = Ramp::new(48000);
        let (out, _, _) = cb(&mut r, &mut q, 400, PLAYING, false, true);
        assert_eq!(out[0], 0.0); assert_eq!(out[799], 1.0);
        let mut q: std::collections::VecDeque<f32> = vec![1.0; 2000].into();
        let mut r = Ramp::new(48000);
        let (out, _, _) = cb(&mut r, &mut q, 4, PLAYING, false, false);
        assert_eq!(out, vec![1.0; 8]);
    }
    #[test] fn underrun_never_splits_a_frame() {
        let mut q: std::collections::VecDeque<f32> = vec![1.0, 2.0, 3.0].into(); // 1.5 frames queued
        let mut r = Ramp::new(48000);
        let (out, n, _) = cb(&mut r, &mut q, 2, PLAYING, false, false);
        assert_eq!(n, 1); assert_eq!(out, vec![1.0, 2.0, 0.0, 0.0]); assert_eq!(q.len(), 1);
    }
    #[test] fn resampler_identity_and_channel_mapping() {
        let x: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = Resampler::new(48000, 48000, 1, 1); let mut o = vec![]; r.process(&x, &mut o); assert_eq!(o, x);
        // channel mapping without rate conversion
        let mut r = Resampler::new(48000, 48000, 2, 1); let mut o = vec![]; r.process(&[1.0, 0.0, 0.5, 0.5], &mut o); assert_eq!(o, vec![0.5, 0.5]);
        let mut r = Resampler::new(48000, 48000, 1, 2); let mut o = vec![]; r.process(&[0.25, -0.5], &mut o); assert_eq!(o, vec![0.25, 0.25, -0.5, -0.5]);
    }
    #[test] fn resampler_rate_change_matches_length() {
        let x: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = Resampler::new(48000, 44100, 1, 2); let mut o = vec![];
        for c in x.chunks(37) { r.process(c, &mut o) }
        // whole rubato blocks carry the exact ratio (48k→44.1k: 1120 in → 1029 out per block)
        let frames_before_flush = o.len() / 2;
        assert!((frames_before_flush as f32 - 3087.0).abs() < 2.0, "{frames_before_flush}");
        r.flush(&mut o);
        let frames = o.len() / 2;
        assert!(frames > frames_before_flush, "flush must drain the tail, {frames}");
        assert!(frames <= 3087 + 1029 * 2, "{frames}");
        assert!(o.chunks(2).all(|f| f[0] == f[1]));
        // energy is preserved-ish (no wild overshoot from the FFT resampler)
        assert!(o.iter().all(|&v| v.abs() <= 1.2));
    }
}
