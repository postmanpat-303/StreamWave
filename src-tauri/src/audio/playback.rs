//! Real-time playback with live effects.
//!
//! ```text
//!  dry clip render (Arc) --> [render-ahead thread: fx::Chain -> resampler] --rtrb--> cpal callback
//!                                   ^ UpdateFx / gains (from a background resolver)
//! ```
//! The cpal callback only pops from a lock-free ring (no locks, no allocation). The render thread
//! keeps ~80 ms queued, so slider moves and A/B are heard within about a tenth of a second without
//! restarting. The chain is the same code export uses, so playback == export.
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

pub enum Ctl { Fx(Vec<Slot>), Gains(u64, Vec<Option<f32>>) }

/// Shared with the UI meter thread and the callback.
#[derive(Default)]
pub struct PlayShared { pub pos: AtomicU64, pub len: AtomicU64, pub rate: AtomicU64, pub playing: AtomicBool, pub paused: AtomicBool, pub done: AtomicBool, pub underruns: AtomicU64 }

pub struct Session { _stream: cpal::Stream, stop: Arc<AtomicBool>, ctl: Sender<Ctl>, render: Option<JoinHandle<()>> }

impl Session {
    pub fn update_fx(&self, slots: Vec<Slot>) { let _ = self.ctl.send(Ctl::Fx(slots)); }
}
impl Drop for Session {
    fn drop(&mut self) { self.stop.store(true, Relaxed); if let Some(h) = self.render.take() { let _ = h.join(); } }
}

/// Streaming linear resampler + channel mapper (identity fast-path when nothing changes).
pub struct Resampler { ratio: f64, t: f64, prev: Vec<f32>, have_prev: bool, in_ch: usize, out_ch: usize }
impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, in_ch: usize, out_ch: usize) -> Self {
        Self { ratio: in_rate as f64 / out_rate as f64, t: 0.0, prev: vec![0.0; in_ch], have_prev: false, in_ch: in_ch.max(1), out_ch: out_ch.max(1) }
    }
    fn map(&self, f: &[f32], out: &mut Vec<f32>) {
        if self.out_ch == 1 && self.in_ch > 1 { out.push(f.iter().sum::<f32>() / self.in_ch as f32) }
        else { for c in 0..self.out_ch { out.push(f[c % self.in_ch]) } }
    }
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.ratio == 1.0 {
            if self.in_ch == self.out_ch { out.extend_from_slice(input) } else { for f in input.chunks(self.in_ch) { self.map(f, out) } }
            return;
        }
        let mut fr = vec![0f32; self.in_ch];
        for cur in input.chunks(self.in_ch) {
            if !self.have_prev { self.prev.copy_from_slice(cur); self.have_prev = true; continue }
            while self.t < 1.0 {
                let t = self.t as f32;
                for c in 0..self.in_ch { fr[c] = self.prev[c] + (cur[c] - self.prev[c]) * t }
                self.map(&fr, out);
                self.t += self.ratio;
            }
            self.t -= 1.0; self.prev.copy_from_slice(cur);
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

#[allow(clippy::too_many_arguments)]
fn render_loop(req: PlayRequest, start_src: usize, total: usize, out_sr: u32, out_ch: usize, mut prod: rtrb::Producer<f32>, target: usize,
               ctl: Receiver<Ctl>, res_tx: Sender<Ctl>, stop: Arc<AtomicBool>, sh: Arc<PlayShared>) {
    let d = req.dry;
    let ch = d.channels.max(1);
    let mut gains = req.gains;
    let mut slots = req.slots;
    let mut chain = Chain::new(&slots, &gains, ch, d.sample_rate, total, start_src);
    let mut rs = Resampler::new(d.sample_rate, out_sr, ch, out_ch);
    let mut pos = start_src;
    let (mut fxout, mut devout): (Vec<f32>, Vec<f32>) = (Vec::with_capacity(8192), Vec::with_capacity(8192));
    let mut pending: Vec<f32> = Vec::new(); // produced but not yet pushed
    let mut flushed = false;
    let mut gen = 0u64;
    let gen_now = Arc::new(AtomicU64::new(0)); // lets a stale re-measure give up early
    const BLOCK: usize = 512;
    while !stop.load(Relaxed) {
        // control messages
        while let Ok(m) = ctl.try_recv() {
            match m {
                Ctl::Fx(new) => {
                    // keep gains for unchanged global stages until the re-measure lands
                    let carried: Vec<Option<f32>> = new.iter().enumerate().map(|(i, s)| {
                        slots.get(i).filter(|o| o.fx == s.fx && o.bypass == s.bypass).and_then(|_| gains.get(i).copied().flatten())
                            .or_else(|| if s.fx.is_global() { gains.get(i).copied().flatten() } else { None })
                    }).collect();
                    chain.update(&new, &carried, d.sample_rate, total, pos);
                    gains = carried;
                    let needs = new.iter().zip(&slots).any(|(a, b)| a != b) || new.len() != slots.len();
                    slots = new;
                    if needs && slots.iter().any(|s| !s.bypass && s.fx.is_global()) {
                        gen += 1; gen_now.store(gen, Relaxed);
                        let (d2, s2, tx, my, latest) = (d.clone(), slots.clone(), res_tx.clone(), gen, gen_now.clone());
                        std::thread::Builder::new().name("sw-fx-resolve".into()).spawn(move || {
                            if let Some(g) = fx::resolve_gains(&d2, &s2, Some(&|| latest.load(Relaxed) != my)) { let _ = tx.send(Ctl::Gains(my, g)); }
                        }).ok();
                    }
                }
                Ctl::Gains(g, v) => if g == gen && v.len() == slots.len() { chain.set_gains(&v); gains = v; },
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
        if pos >= total {
            if !flushed { fxout.clear(); chain.flush(&mut fxout); devout.clear(); rs.process(&fxout, &mut devout); pending.extend_from_slice(&devout); flushed = true; continue }
            if pending.is_empty() { sh.done.store(true, Relaxed) }
            std::thread::sleep(Duration::from_millis(5)); continue;
        }
        let n = BLOCK.min(total - pos);
        fxout.clear(); chain.process(&d.samples[pos * ch..(pos + n) * ch], &mut fxout); pos += n;
        devout.clear(); rs.process(&fxout, &mut devout);
        pending.extend_from_slice(&devout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn resampler_identity_and_rate_change() {
        let x: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = Resampler::new(48000, 48000, 1, 1); let mut o = vec![]; r.process(&x, &mut o); assert_eq!(o, x);
        let mut r = Resampler::new(48000, 44100, 1, 2); let mut o = vec![];
        for c in x.chunks(37) { r.process(c, &mut o) }
        let frames = o.len() / 2; assert!((frames as f32 - 2000.0 * 44100.0 / 48000.0).abs() < 3.0, "{frames}");
        assert!(o.chunks(2).all(|f| f[0] == f[1]));
        let mut r = Resampler::new(48000, 48000, 2, 1); let mut o = vec![]; r.process(&[1.0, 0.0, 0.5, 0.5], &mut o); assert_eq!(o, vec![0.5, 0.5]);
    }
}
