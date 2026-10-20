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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct PlayRequest { pub dry: Arc<Decoded>, pub slots: Vec<Slot>, pub gains: Vec<Option<f32>>, pub start_secs: f64 }

pub enum Ctl { Fx(Vec<Slot>), Gains(u64, Vec<Option<f32>>), Dry(Arc<Decoded>) }

/// Shared with the UI meter thread and the callback.
#[derive(Default)]
pub struct PlayShared { pub pos: AtomicU64, pub len: AtomicU64, pub rate: AtomicU64, pub playing: AtomicBool, pub paused: AtomicBool, pub done: AtomicBool, pub underruns: AtomicU64,
    /// Rate / channels of the dry audio being played (a re-render can only be swapped in if they match).
    pub src_rate: AtomicU64, pub src_ch: AtomicU64 }

pub struct Session { _stream: cpal::Stream, stop: Arc<AtomicBool>, ctl: Sender<Ctl>, render: Option<JoinHandle<()>> }

impl Session {
    pub fn update_fx(&self, slots: Vec<Slot>) { let _ = self.ctl.send(Ctl::Fx(slots)); }
    pub fn update_dry(&self, d: Arc<Decoded>) { let _ = self.ctl.send(Ctl::Dry(d)); }
}
impl Drop for Session {
    fn drop(&mut self) { self.stop.store(true, Relaxed); if let Some(h) = self.render.take() { let _ = h.join(); } }
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

pub fn start(dev: &cpal::Device, req: PlayRequest, sh: Arc<PlayShared>, buffer_frames: u32) -> anyhow::Result<Session> {
    let sup = dev.default_output_config()?;
    let mut cfg: cpal::StreamConfig = sup.config();
    cfg.buffer_size = match sup.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => cpal::BufferSize::Fixed(buffer_frames.clamp(*min, *max)),
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };
    let (out_ch, out_sr) = (cfg.channels as usize, cfg.sample_rate.0);
    let d = req.dry.clone();
    let total_src = d.samples.len() / d.channels.max(1);
    let start_src = ((req.start_secs.max(0.0) * d.sample_rate as f64) as usize).min(total_src);
    let to_dev = |src: usize| (src as f64 * out_sr as f64 / d.sample_rate as f64) as u64;
    sh.rate.store(out_sr as u64, Relaxed);
    sh.src_rate.store(d.sample_rate as u64, Relaxed); sh.src_ch.store(d.channels as u64, Relaxed);
    sh.len.store(to_dev(total_src), Relaxed);
    sh.pos.store(to_dev(start_src), Relaxed);
    sh.done.store(false, Relaxed); sh.paused.store(false, Relaxed); sh.underruns.store(0, Relaxed);

    // ring: up to 250 ms; render thread keeps ~80 ms queued for snappy live changes
    let cap = (out_sr as usize * out_ch / 4).max(4096);
    let (prod, cons) = rtrb::RingBuffer::<f32>::new(cap);
    let target = (out_sr as usize * out_ch * 8 / 100).max(2048);

    let build = |cfg: &cpal::StreamConfig, cons: rtrb::Consumer<f32>| -> anyhow::Result<cpal::Stream> {
        Ok(match sup.sample_format() {
            cpal::SampleFormat::F32 => out_stream::<f32>(dev, cfg, cons, sh.clone())?,
            cpal::SampleFormat::I16 => out_stream::<i16>(dev, cfg, cons, sh.clone())?,
            cpal::SampleFormat::U16 => out_stream::<u16>(dev, cfg, cons, sh.clone())?,
            cpal::SampleFormat::I32 => out_stream::<i32>(dev, cfg, cons, sh.clone())?,
            cpal::SampleFormat::F64 => out_stream::<f64>(dev, cfg, cons, sh.clone())?,
            f => anyhow::bail!("unsupported output format {f:?}"),
        })
    };
    let stream = build(&cfg, cons)?;

    let stop = Arc::new(AtomicBool::new(false));
    let (ctl_tx, ctl_rx) = mpsc::channel();
    let res_tx = ctl_tx.clone();
    let (stop2, sh2) = (stop.clone(), sh.clone());
    let render = std::thread::Builder::new().name("sw-play-render".into()).spawn(move || {
        render_loop(req, start_src, total_src, out_sr, out_ch, prod, target, ctl_rx, res_tx, stop2, sh2)
    })?;
    // give the render thread a head start so the first callback isn't empty
    std::thread::sleep(Duration::from_millis(15));
    sh.playing.store(true, Relaxed);
    stream.play()?;
    Ok(Session { _stream: stream, stop, ctl: ctl_tx, render: Some(render) })
}

fn out_stream<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, mut cons: rtrb::Consumer<f32>, sh: Arc<PlayShared>) -> anyhow::Result<cpal::Stream>
where T: SizedSample + FromSample<f32> {
    let ch = cfg.channels as u64;
    let s = dev.build_output_stream(cfg, move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        // REAL-TIME: no locks, no allocation, no syscalls.
        if sh.paused.load(Relaxed) || !sh.playing.load(Relaxed) { for o in out.iter_mut() { *o = T::EQUILIBRIUM } return }
        let mut n = 0u64;
        for o in out.iter_mut() {
            match cons.pop() { Ok(v) => { *o = T::from_sample(v); n += 1 } Err(_) => *o = T::EQUILIBRIUM }
        }
        sh.pos.fetch_add(n / ch, Relaxed);
        if (n as usize) < out.len() {
            if sh.done.load(Relaxed) { sh.playing.store(false, Relaxed) } else { sh.underruns.fetch_add(1, Relaxed); }
        }
    }, |e| log::warn!("output stream error: {e}"), None)?;
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
fn render_loop(req: PlayRequest, start_src: usize, _total: usize, out_sr: u32, out_ch: usize, mut prod: rtrb::Producer<f32>, target: usize,
               ctl: Receiver<Ctl>, res_tx: Sender<Ctl>, stop: Arc<AtomicBool>, sh: Arc<PlayShared>) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
