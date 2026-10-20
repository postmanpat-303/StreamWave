//! Audio engine. All cpal streams live on ONE dedicated engine thread (streams are !Send on
//! some hosts). The realtime callbacks never lock or allocate: they push samples into an
//! `rtrb` lock-free ring buffer and publish meter peaks through atomics. A separate writer
//! thread drains the ring and streams a WAV to disk incrementally.
pub mod devices;
pub mod wavfix;
pub mod monitor;
pub mod playback;
use monitor::{MicCheck, PreRoll};

use crate::settings::LatencyProfile;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub enum Cmd {
    /// Open (or re-open) the input device for live metering. None = remembered/default.
    SelectInput(Option<String>),
    SelectOutput(Option<String>),
    SetLatency(LatencyProfile, bool /*auto backoff*/),
    StartRecord(PathBuf /*directory*/),
    StopRecord,
    PauseRecord(bool),
    /// Start a real-time playback session (effects applied live).
    PlaySession(playback::PlayRequest),
    /// Live effect-chain change for the running session.
    UpdateFx(Vec<crate::fx::Slot>),
    /// Re-rendered dry audio (clip effects changed) for the running session, swapped in at the current position.
    UpdateDry(Arc<crate::waveform::Decoded>),
    PausePlay(bool),
    StopPlay,
    DevicesChanged(Vec<String>),
    /// Open the output stream ahead of the first Play (project loaded).
    PrepareOutput,
    /// Pre-roll length (0-60 s) and whether to prepend it when Record is pressed.
    SetPreRoll(f32, bool),
    /// Write the last N seconds of the pre-roll buffer to a WAV in `dir`.
    SaveLast(f32, PathBuf),
    /// Also record a −12 dB safety copy alongside the main take.
    SetSafety(bool),
    /// Level-triggered auto record (None = off).
    SetVoiceTrigger(Option<VtCfg>),
}

#[derive(Default)]
pub struct Shared {
    peak: [AtomicU32; 2],
    pub xruns: AtomicU64,
    pub recording: AtomicBool,
    pub paused: AtomicBool,
    rec_frames: AtomicU64,
    device_lost: AtomicBool,
    pub play: Arc<playback::PlayShared>,
    sample_rate: AtomicU32,
}

#[derive(Serialize, Clone)]
pub struct LevelEvent { pub left: f32, pub right: f32, pub recording: bool, pub paused: bool, pub rec_secs: f64, pub xruns: u64, pub playing: bool, pub play_paused: bool, pub play_secs: f64, pub play_total: f64 }

/// The persistent output stream is closed after this long without playback.
const DECK_IDLE_CLOSE: Duration = Duration::from_secs(600);

#[derive(Serialize, Clone)]
pub struct EngineStatus { pub input_id: Option<String>, pub output_id: Option<String>, pub latency: LatencyProfile, pub buffer_frames: u32, pub sample_rate: u32, pub channels: u16, pub note: Option<String> }

#[allow(dead_code)]
pub struct AudioHandle { pub tx: Sender<Cmd>, pub shared: Arc<Shared> }

fn store_peak(a: &AtomicU32, v: f32) {
    // keep the max until the meter thread reads & resets it
    let mut cur = a.load(Relaxed);
    while v > f32::from_bits(cur) {
        match a.compare_exchange_weak(cur, v.to_bits(), Relaxed, Relaxed) { Ok(_) => break, Err(c) => cur = c }
    }
}

pub fn spawn(app: AppHandle, initial_in: Option<String>, initial_out: Option<String>, latency: LatencyProfile, auto_backoff: bool, preroll: (f32, bool), safety: bool) -> AudioHandle {
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(Shared::default());
    let sh = shared.clone();
    let app2 = app.clone();
    std::thread::Builder::new().name("sw-audio-engine".into()).spawn(move || {
        let mut e = Engine { app: app2, sh, preroll: PreRoll::new(0.0, 1, 48000), preroll_secs: preroll.0, prepend: preroll.1, mic: MicCheck::new(1, 48000), input: None, output: None, deck: None, deck_idle: Instant::now(), input_id: initial_in, output_id: initial_out, latency, auto_backoff,
            writer: None, pending_backoff: false, wanted: None, last_xruns: 0, last_xrun_check: Instant::now(), channels: 2, safety,
            vt: None, vt_above: None, vt_below: None, vt_recording: false, mon_peak: 0.0 };
        e.open_input();
        e.run(rx);
    }).expect("spawn audio engine");

    // meter / progress emitter (~30 Hz)
    let sh = shared.clone();
    std::thread::Builder::new().name("sw-meter".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(33));
        crate::perf::mark_at(sh.play.first_audio.take(), "callback: first audio");
        crate::perf::mark_at(sh.play.first_cb.take(), "callback: first callback");
        crate::perf::mark_at(sh.play.silent.take(), "callback: silent");
        let l = f32::from_bits(sh.peak[0].swap(0, Relaxed));
        let r = f32::from_bits(sh.peak[1].swap(0, Relaxed));
        let sr = sh.sample_rate.load(Relaxed).max(1) as f64;
        let pr = sh.play.rate.load(Relaxed).max(1) as f64;
        let _ = app.emit("level", LevelEvent { left: l, right: r, recording: sh.recording.load(Relaxed), paused: sh.paused.load(Relaxed), rec_secs: sh.rec_frames.load(Relaxed) as f64 / sr,
            xruns: sh.xruns.load(Relaxed), playing: sh.play.playing.load(Relaxed), play_paused: sh.play.paused.load(Relaxed), play_secs: sh.play.pos.load(Relaxed) as f64 / pr, play_total: sh.play.len.load(Relaxed) as f64 / pr });
    }).expect("spawn meter");
    AudioHandle { tx, shared }
}

struct Writer { stop: Arc<AtomicBool>, handle: JoinHandle<anyhow::Result<(PathBuf, rtrb::Consumer<f32>)>> }

/// Engine-side voice-trigger config (settings + the directory recordings go to).
#[derive(Clone)]
pub struct VtCfg { pub threshold_db: f32, pub hold_ms: u64, pub dir: PathBuf }

struct InputStream { _stream: cpal::Stream, consumer: Option<rtrb::Consumer<f32>>, mon: rtrb::Consumer<f32>, channels: u16, sample_rate: u32, buffer_frames: u32 }

struct Engine {
    app: AppHandle, sh: Arc<Shared>,
    preroll: PreRoll, preroll_secs: f32, prepend: bool, mic: MicCheck,
    input: Option<InputStream>, output: Option<playback::Session>,
    /// Persistent shared-mode output stream (kept open between plays; closed after a long idle).
    deck: Option<playback::Deck>, deck_idle: Instant,
    input_id: Option<String>, output_id: Option<String>,
    latency: LatencyProfile, auto_backoff: bool,
    writer: Option<Writer>, pending_backoff: bool, wanted: Option<String>,
    last_xruns: u64, last_xrun_check: Instant, channels: u16, safety: bool,
    vt: Option<VtCfg>, vt_above: Option<Instant>, vt_below: Option<Instant>,
    vt_recording: bool, mon_peak: f32,
}

fn buffer_for(range: &cpal::SupportedBufferSize, want: u32) -> cpal::BufferSize {
    match range {
        cpal::SupportedBufferSize::Range { min, max } => cpal::BufferSize::Fixed(want.clamp(*min, *max)),
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Fixed(want),
    }
}

impl Engine {
    fn status(&self, note: Option<String>) {
        let (sr, ch, bf) = self.input.as_ref().map(|i| (i.sample_rate, i.channels, i.buffer_frames)).unwrap_or((0, 0, 0));
        let _ = self.app.emit("engine-status", EngineStatus { input_id: self.input_id.clone(), output_id: self.output_id.clone(), latency: self.latency,
            buffer_frames: bf, sample_rate: sr, channels: ch, note });
    }

    fn open_input(&mut self) {
        if self.writer.is_some() { return } // never swap the device under an active recording
        self.input = None;
        let Some((dev, id)) = devices::find_device(self.input_id.as_deref(), true) else {
            self.status(Some("No input device found".into())); return };
        let remembered = self.input_id.clone();
        match self.build_input(&dev) {
            Ok(s) => {
                self.sh.sample_rate.store(s.sample_rate, Relaxed);
                self.preroll = PreRoll::new(self.preroll_secs, s.channels, s.sample_rate);
                self.mic = MicCheck::new(s.channels, s.sample_rate);
                self.channels = s.channels;
                self.input = Some(s);
                let note = (remembered.as_deref().is_some_and(|r| r != id)).then(|| "Remembered device not available, using default".to_string());
                self.input_id = Some(id);
                self.status(note);
            }
            Err(e) => self.status(Some(format!("Could not open input: {e}"))),
        }
    }

    fn build_input(&self, dev: &cpal::Device) -> anyhow::Result<InputStream> {
        let sup = dev.default_input_config()?;
        let want = self.latency.frames();
        let mut cfg: cpal::StreamConfig = sup.config();
        cfg.buffer_size = buffer_for(sup.buffer_size(), want);
        let fmt = sup.sample_format();
        let try_build = |cfg: &cpal::StreamConfig| -> anyhow::Result<(cpal::Stream, rtrb::Consumer<f32>, rtrb::Consumer<f32>)> {
            // ~4 seconds of slack between the audio callback and the disk writer
            let (prod, cons) = rtrb::RingBuffer::<f32>::new(cfg.sample_rate.0 as usize * cfg.channels as usize * 4);
            // separate always-on tap for pre-roll + mic check (10 s slack)
            let (mprod, mcons) = rtrb::RingBuffer::<f32>::new(cfg.sample_rate.0 as usize * cfg.channels as usize * 10);
            let prod = (prod, mprod);
            let s = match fmt {
                cpal::SampleFormat::F32 => self.input_stream::<f32>(dev, cfg, prod)?,
                cpal::SampleFormat::I16 => self.input_stream::<i16>(dev, cfg, prod)?,
                cpal::SampleFormat::U16 => self.input_stream::<u16>(dev, cfg, prod)?,
                cpal::SampleFormat::I32 => self.input_stream::<i32>(dev, cfg, prod)?,
                cpal::SampleFormat::I8 => self.input_stream::<i8>(dev, cfg, prod)?,
                cpal::SampleFormat::U8 => self.input_stream::<u8>(dev, cfg, prod)?,
                cpal::SampleFormat::F64 => self.input_stream::<f64>(dev, cfg, prod)?,
                f => anyhow::bail!("unsupported sample format {f:?}"),
            };
            Ok((s, cons, mcons))
        };
        let (stream, (cons, mon), bf) = match try_build(&cfg) {
            Ok((s, c, m)) => (s, (c, m), want),
            Err(_) => { // some drivers reject fixed sizes: fall back to driver default
                cfg.buffer_size = cpal::BufferSize::Default;
                let (s, c, m) = try_build(&cfg)?; (s, (c, m), 0)
            }
        };
        stream.play()?;
        Ok(InputStream { _stream: stream, consumer: Some(cons), mon, channels: cfg.channels, sample_rate: cfg.sample_rate.0, buffer_frames: bf })
    }

    fn input_stream<T>(&self, dev: &cpal::Device, cfg: &cpal::StreamConfig, (mut prod, mut mprod): (rtrb::Producer<f32>, rtrb::Producer<f32>)) -> anyhow::Result<cpal::Stream>
    where T: SizedSample, f32: FromSample<T> {
        let ch = cfg.channels as usize;
        let sh = self.sh.clone();
        let sh_err = self.sh.clone();
        let s = dev.build_input_stream(cfg, move |data: &[T], _: &cpal::InputCallbackInfo| {
            let rec = sh.recording.load(Relaxed) && !sh.paused.load(Relaxed);
            let (mut l, mut r) = (0f32, 0f32);
            for (i, s) in data.iter().enumerate() {
                let v = f32::from_sample(*s);
                if i % ch == 0 { l = l.max(v.abs()) } else if i % ch == 1 { r = r.max(v.abs()) }
                if rec && prod.push(v).is_err() { sh.xruns.fetch_add(1, Relaxed); }
                let _ = mprod.push(v); // pre-roll tap: dropping here is harmless
            }
            if ch == 1 { r = l }
            store_peak(&sh.peak[0], l); store_peak(&sh.peak[1], r);
            if rec { sh.rec_frames.fetch_add((data.len() / ch) as u64, Relaxed); }
        }, move |err| {
            match err {
                cpal::StreamError::DeviceNotAvailable => sh_err.device_lost.store(true, Relaxed),
                _ => { sh_err.xruns.fetch_add(1, Relaxed); }
            }
        }, None)?;
        Ok(s)
    }

    fn start_record(&mut self, dir: PathBuf) {
        if let Err(e) = self.start_record_inner(dir) { let _ = self.app.emit("record-error", e.to_string()); }
    }

    fn start_record_inner(&mut self, dir: PathBuf) -> anyhow::Result<()> {
        if self.writer.is_some() { return Ok(()) }
        if self.input.is_none() { self.open_input(); }
        self.drain_monitor();
        let pre_all = if self.prepend { self.preroll.last(self.preroll_secs) } else { vec![] };
        let Some(inp) = self.input.as_mut() else { anyhow::bail!("No input device") };
        let Some(mut cons) = inp.consumer.take() else { return Ok(()) };
        while cons.pop().is_ok() {} // discard stale samples
        let _ = std::fs::create_dir_all(&dir);
        let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let partial = dir.join(format!("StreamWave_{stamp}.partial.wav"));
        let spec = hound::WavSpec { channels: inp.channels, sample_rate: inp.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let pre = pre_all;
        let pre_frames = (pre.len() / inp.channels.max(1) as usize) as u64;
        let app2 = self.app.clone();
        let safety = self.safety;
        let handle = std::thread::Builder::new().name("sw-wav-writer".into()).spawn(move || -> anyhow::Result<(PathBuf, rtrb::Consumer<f32>)> {
            // WAV headers are 32-bit: roll over to a new part well before 4 GiB.
            let max_samples = ROLLOVER_BYTES / 4 / spec.channels.max(1) as u64 * spec.channels.max(1) as u64;
            let mut part = 1u32;
            let mut first: Option<PathBuf> = None;
            let mut cur = partial.clone();
            let mut w = hound::WavWriter::create(&cur, spec)?;
            // optional −12 dB safety copy ("X_safety.partial.wav"), kept in lock-step with the main file
            let mut cur_s = safety.then(|| safety_name(&partial));
            let mut ws: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>> = None;
            if let Some(ps) = &cur_s { ws = Some(hound::WavWriter::create(ps, spec)?) }
            let mut written = 0u64;
            for v in &pre {
                w.write_sample(*v)?; written += 1;
                if let Some(ws) = ws.as_mut() { ws.write_sample(*v * SAFETY_GAIN)?; }
            }
            let mut last_flush = Instant::now();
            let mut err: Option<anyhow::Error> = None;
            'outer: loop {
                let n = cons.slots();
                if n > 0 {
                    let chunk = match cons.read_chunk(n) { Ok(c) => c, Err(e) => { err = Some(e.into()); break } };
                    let (a, b) = chunk.as_slices();
                    for v in a.iter().chain(b) {
                        if written >= max_samples {
                            let next = part_name(&partial, part + 1);
                            let nw = match hound::WavWriter::create(&next, spec) { Ok(w) => w, Err(e) => { err = Some(e.into()); break 'outer } };
                            let old = std::mem::replace(&mut w, nw);
                            if let Err(e) = old.finalize() { err = Some(e.into()); break 'outer }
                            let done = final_name(&cur);
                            if let Err(e) = std::fs::rename(&cur, &done) { err = Some(e.into()); break 'outer }
                            let _ = app2.emit("record-part", done.to_string_lossy().to_string());
                            first.get_or_insert(done);
                            part += 1;
                            cur = next;
                            written = 0;
                            // roll the safety copy in lock-step
                            if let (Some(ws), Some(ps)) = (ws.as_mut(), cur_s.as_ref()) {
                                let nexts = part_name(ps, part);
                                let nsw = match hound::WavWriter::create(&nexts, spec) { Ok(w) => w, Err(e) => { err = Some(e.into()); break 'outer } };
                                let olds = std::mem::replace(ws, nsw);
                                if let Err(e) = olds.finalize() { err = Some(e.into()); break 'outer }
                                let ds = final_name(ps);
                                if let Err(e) = std::fs::rename(ps, &ds) { err = Some(e.into()); break 'outer }
                                cur_s = Some(nexts);
                            }
                        }
                        if let Err(e) = w.write_sample(*v) { err = Some(e.into()); break 'outer }
                        if let Some(ws) = ws.as_mut() { if let Err(e) = ws.write_sample(*v * SAFETY_GAIN) { err = Some(e.into()); break 'outer } }
                        written += 1;
                    }
                    chunk.commit_all();
                } else if stop2.load(Acquire) {
                    break;
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
                // crash safety: refresh the header + fsync every half second
                if last_flush.elapsed() > Duration::from_millis(500) {
                    if let Err(e) = w.flush() { err = Some(e.into()); break }
                    if let Some(ws) = ws.as_mut() { if let Err(e) = ws.flush() { err = Some(e.into()); break } }
                    last_flush = Instant::now();
                }
            }
            if let Some(e) = err {
                // keep the consumer alive so the next take still works; the .partial.wav is recoverable
                let _ = app2.emit("record-error", format!("Recording write failed: {e}"));
                while cons.pop().is_ok() {}
                return Ok((first.unwrap_or(cur), cons));
            }
            w.finalize()?;
            let fin = final_name(&cur);
            std::fs::rename(&cur, &fin)?;
            if let Some(ws) = ws.take() {
                ws.finalize()?;
                if let Some(ps) = cur_s.take() { let fs = final_name(&ps); std::fs::rename(&ps, &fs)?; }
            }
            if first.is_some() { let _ = app2.emit("record-part", fin.to_string_lossy().to_string()); }
            Ok((first.unwrap_or(fin), cons))
        })?;
        self.sh.rec_frames.store(pre_frames, Relaxed);
        self.sh.recording.store(true, Release);
        self.writer = Some(Writer { stop, handle });
        let _ = self.app.emit("record-started", ());
        Ok(())
    }

    fn stop_record(&mut self) -> Option<PathBuf> {
        let w = self.writer.take()?;
        self.sh.recording.store(false, Release);
        self.sh.paused.store(false, Relaxed);
        w.stop.store(true, Release);
        match w.handle.join() {
            Ok(Ok((path, cons))) => {
                if let Some(i) = self.input.as_mut() { i.consumer = Some(cons) }
                let _ = self.app.emit("record-finished", path.to_string_lossy().to_string());
                Some(path)
            }
            Ok(Err(e)) => { let _ = self.app.emit("record-error", e.to_string()); None }
            Err(_) => { let _ = self.app.emit("record-error", "writer thread panicked"); None }
        }
    }

    /// End the current session with a de-click fade. Returns at once; the callback finishes the fade and flushes
    /// the ring by itself (`wait_idle` before the next start).
    fn end_session(&mut self) {
        let p = &self.sh.play;
        if p.state.load(Acquire) == playback::PLAYING { p.state.store(playback::STOPPING, Release) }
        p.playing.store(false, Relaxed); p.paused.store(false, Relaxed);
        if let Some(s) = self.output.take() {
            match s.end() {
                Some(prod) => if let Some(d) = self.deck.as_mut() { d.give_back(prod) },
                None => self.deck = None, // render thread panicked: rebuild the deck next time
            }
        }
        self.deck_idle = Instant::now();
    }

    /// Wait (bounded) until the callback has finished a fade-out and flushed. False = the stream isn't running.
    fn wait_idle(&self) -> bool {
        let t0 = Instant::now();
        while self.sh.play.state.load(Acquire) != playback::IDLE {
            if t0.elapsed() > Duration::from_millis(250) { return false }
            std::thread::sleep(Duration::from_micros(500));
        }
        true
    }

    /// The open deck, (re)opening it when missing, broken or on another device. Shared mode only.
    fn ensure_deck(&mut self) -> anyhow::Result<()> {
        let broken = self.sh.play.stream_err.load(Relaxed) || self.deck.as_ref().is_some_and(|d| !d.has_producer());
        let wrong = self.deck.as_ref().is_some_and(|d| self.output_id.as_deref().is_some_and(|w| w != d.dev_id));
        if broken || wrong { self.deck = None }
        if self.deck.is_none() {
            let (dev, id) = devices::find_device(self.output_id.as_deref(), false).ok_or_else(|| anyhow::anyhow!("no output device"))?;
            crate::perf::mark("engine: device found");
            self.output_id = Some(id.clone());
            self.deck = Some(playback::Deck::open(&dev, id, self.sh.play.clone(), self.latency.frames().max(256))?);
            crate::perf::mark("engine: deck opened");
        }
        Ok(())
    }

    fn play(&mut self, req: playback::PlayRequest) {
        crate::perf::mark("engine: PlaySession received");
        self.end_session();
        self.sh.play.first_audio.arm(); self.sh.play.first_cb.arm();
        let res = (|| -> anyhow::Result<playback::Session> {
            self.ensure_deck()?;
            if !self.wait_idle() {
                // the stream stopped calling back (device gone / suspended): reopen once
                self.deck = None; self.sh.play.state.store(playback::IDLE, Release); self.ensure_deck()?;
            }
            self.deck.as_mut().expect("deck").start(req, self.sh.play.clone())
        })();
        match res {
            Ok(s) => self.output = Some(s),
            Err(e) => { self.sh.play.state.store(playback::IDLE, Release); let _ = self.app.emit("playback-error", e.to_string()); }
        }
    }

    /// Warm up for a quick first Play (open the deck while idle).
    fn prepare_output(&mut self) {
        if self.output.is_some() { return }
        if let Err(e) = self.ensure_deck() { log::warn!("output warm-up: {e}") }
        self.deck_idle = Instant::now();
    }

    /// Move audio from the always-on tap into the pre-roll buffer and the mic checker.
    fn drain_monitor(&mut self) {
        let Some(inp) = self.input.as_mut() else { return };
        let n = inp.mon.slots(); if n == 0 { return }
        if let Ok(chunk) = inp.mon.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            let mut pk = 0f32;
            for part in [a, b] {
                for &v in part { pk = pk.max(v.abs()) }
                self.preroll.push(part);
                if let Some(r) = self.mic.push(part) { let _ = self.app.emit("mic-check", r); }
            }
            self.mon_peak = pk;
            chunk.commit_all();
        }
    }

    /// Level-triggered record: start when the input stays above the threshold, stop after it
    /// stays below (with a longer hang so speech pauses don't cut the take).
    fn voice_trigger_tick(&mut self) {
        let Some(vt) = self.vt.clone() else { return };
        if self.sh.paused.load(Relaxed) { return }
        let peak_db = 20.0 * self.mon_peak.max(1e-6).log10();
        if self.writer.is_none() {
            self.vt_below = None;
            if peak_db >= vt.threshold_db {
                match self.vt_above {
                    None => self.vt_above = Some(Instant::now()),
                    Some(t) if t.elapsed() >= Duration::from_millis(vt.hold_ms) => {
                        self.vt_recording = true; self.vt_above = None;
                        let _ = self.app.emit("voice-trigger", "recording");
                        self.start_record(vt.dir.clone());
                    }
                    _ => {}
                }
            } else { self.vt_above = None; }
        } else if self.vt_recording {
            self.vt_above = None;
            let hang = Duration::from_millis(vt.hold_ms.max(1500));
            if peak_db < vt.threshold_db {
                match self.vt_below {
                    None => self.vt_below = Some(Instant::now()),
                    Some(t) if t.elapsed() >= hang => {
                        self.vt_recording = false; self.vt_below = None;
                        self.stop_record();
                        let _ = self.app.emit("voice-trigger", "armed");
                    }
                    _ => {}
                }
            } else { self.vt_below = None; }
        }
    }

    fn save_last(&mut self, secs: f32, dir: PathBuf) {
        self.drain_monitor();
        let data = self.preroll.last(secs);
        let res = (|| -> anyhow::Result<PathBuf> {
            anyhow::ensure!(!data.is_empty(), "The pre-record buffer is empty (is it set to 0 s?)");
            std::fs::create_dir_all(&dir)?;
            let p = dir.join(format!("StreamWave_saved-take_{}.wav", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")));
            let spec = hound::WavSpec { channels: self.preroll.channels, sample_rate: self.preroll.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
            let mut w = hound::WavWriter::create(&p, spec)?;
            for v in &data { w.write_sample(*v)?; }
            w.finalize()?; Ok(p)
        })();
        match res { Ok(p) => { let _ = self.app.emit("record-finished", p.to_string_lossy().to_string()); }
                    Err(e) => { let _ = self.app.emit("record-error", e.to_string()); } }
    }

    fn run(&mut self, rx: Receiver<Cmd>) {
        loop {
            self.drain_monitor();
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Cmd::SelectInput(id)) => { if self.writer.is_none() { self.input_id = id; self.open_input(); } }
                Ok(Cmd::SelectOutput(id)) => {
                    // a new device takes effect on the next Play (the deck is reopened there)
                    if self.output.is_none() && id.is_some() && id != self.output_id { self.deck = None }
                    self.output_id = id; self.status(None);
                }
                Ok(Cmd::SetLatency(p, ab)) => { self.latency = p; self.auto_backoff = ab; self.open_input(); }
                Ok(Cmd::StartRecord(dir)) => self.start_record(dir),
                Ok(Cmd::PauseRecord(p)) => { if self.writer.is_some() { self.sh.paused.store(p, Relaxed); let _ = self.app.emit("record-paused", p); } }
                Ok(Cmd::StopRecord) => { self.stop_record(); self.vt_recording = false; self.vt_below = None; if self.pending_backoff { self.pending_backoff = false; self.open_input(); } }
                Ok(Cmd::PlaySession(req)) => self.play(req),
                Ok(Cmd::UpdateFx(slots)) => { if let Some(s) = &self.output { s.update_fx(slots) } }
                Ok(Cmd::UpdateDry(d)) => { if let Some(s) = &self.output { s.update_dry(d) } }
                Ok(Cmd::PausePlay(p)) => { if self.output.is_some() { self.sh.play.paused.store(p, Relaxed) } }
                Ok(Cmd::StopPlay) => { crate::perf::mark("engine: StopPlay received"); if self.sh.play.state.load(Acquire) == playback::PLAYING { self.sh.play.silent.arm() } self.end_session(); crate::perf::mark("engine: session ended (fading)"); }
                Ok(Cmd::PrepareOutput) => self.prepare_output(),
                Ok(Cmd::DevicesChanged(ids)) => {
                    // if our device vanished, treat it like a lost device
                    if let Some(cur) = &self.input_id { if !ids.contains(cur) { self.sh.device_lost.store(true, Relaxed) } }
                    // the open output went away: stop cleanly and reopen on the next Play
                    if self.deck.as_ref().is_some_and(|d| !ids.contains(&d.dev_id)) {
                        if self.output.is_some() { self.end_session(); let _ = self.app.emit("playback-error", "The output device was disconnected"); }
                        self.deck = None;
                    }
                    if let Some(w) = self.wanted.clone() {
                        if ids.contains(&w) && self.writer.is_none() {
                            self.wanted = None; self.input_id = Some(w.clone()); self.open_input();
                            let _ = self.app.emit("device-restored", w);
                        }
                    }
                }
                Ok(Cmd::SetPreRoll(secs, prepend)) => {
                    self.preroll_secs = secs.clamp(0.0, 60.0); self.prepend = prepend;
                    let (c, r) = (self.preroll.channels, self.preroll.sample_rate);
                    self.preroll = PreRoll::new(self.preroll_secs, c, r);
                }
                Ok(Cmd::SaveLast(secs, dir)) => self.save_last(secs, dir),
                Ok(Cmd::SetSafety(on)) => self.safety = on,
                Ok(Cmd::SetVoiceTrigger(cfg)) => { self.vt = cfg; self.vt_above = None; self.vt_below = None; self.vt_recording = false; }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => { self.stop_record(); return }
            }
            self.voice_trigger_tick();
            // natural end of the audio: hand the ring back
            if self.output.is_some() && !self.sh.play.paused.load(Relaxed) && self.sh.play.state.load(Acquire) == playback::IDLE { self.end_session(); }
            // stream died while idle, or idle for a long time: close it (reopened on the next Play)
            if self.output.is_none() && self.deck.is_some() && (self.sh.play.stream_err.load(Relaxed) || self.deck_idle.elapsed() > DECK_IDLE_CLOSE) { self.deck = None }
            // graceful fallback when the recording device disappears
            if self.sh.device_lost.swap(false, Relaxed) {
                let lost = self.input_id.clone();
                let saved = self.stop_record();
                self.vt_recording = false; self.vt_below = None;
                let _ = self.app.emit("record-device-lost", serde_json::json!({ "device": lost, "saved": saved.map(|p| p.to_string_lossy().to_string()) }));
                self.input = None;
                self.input_id = None; // fall back to the system default
                self.open_input();
                self.wanted = lost; // re-attach automatically when it comes back
            }
            // xrun auto-backoff: >3 xruns in 2 seconds => step to a safer latency profile
            if self.last_xrun_check.elapsed() > Duration::from_secs(2) {
                let x = self.sh.xruns.load(Relaxed);
                if x.saturating_sub(self.last_xruns) > 3 && self.auto_backoff {
                    if let Some(next) = self.latency.safer() {
                        self.latency = next;
                        let _ = self.app.emit("latency-backoff", next);
                        if self.writer.is_some() { self.pending_backoff = true } else { self.open_input() }
                    }
                }
                self.last_xruns = x; self.last_xrun_check = Instant::now();
            }
        }
    }
}

/// Bytes per WAV part before rolling over (WAV sizes are 32-bit).
const ROLLOVER_BYTES: u64 = 3_800_000_000;

/// Linear gain of the safety copy (−12 dB): loud enough to be useful, low enough to survive clipping.
const SAFETY_GAIN: f32 = 0.2512;

/// `X.partial.wav` -> `X.wav`, touching only the file name (safe for unicode/odd dirs).
pub(crate) fn final_name(p: &std::path::Path) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = name.strip_suffix(".partial.wav").map(|b| format!("{b}.wav")).unwrap_or(name);
    p.with_file_name(base)
}
/// `X.partial.wav` -> `X_part2.partial.wav`
pub(crate) fn part_name(p: &std::path::Path, n: u32) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = name.strip_suffix(".partial.wav").unwrap_or(&name);
    p.with_file_name(format!("{base}_part{n}.partial.wav"))
}
/// `X.partial.wav` -> `X_safety.partial.wav`
pub(crate) fn safety_name(p: &std::path::Path) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = name.strip_suffix(".partial.wav").unwrap_or(&name);
    p.with_file_name(format!("{base}_safety.partial.wav"))
}

#[cfg(test)]
mod name_tests {
    use super::*;
    #[test] fn partial_names_only_touch_file_name() {
        let d = PathBuf::from("C:/Users/Dé l/my.partial.wav dir/Aufnahme 🎙");
        let p = d.join("StreamWave_1.partial.wav");
        assert_eq!(final_name(&p), d.join("StreamWave_1.wav"));
        assert_eq!(part_name(&p, 2), d.join("StreamWave_1_part2.partial.wav"));
        assert_eq!(final_name(&part_name(&p, 2)), d.join("StreamWave_1_part2.wav"));
        assert_eq!(safety_name(&p), d.join("StreamWave_1_safety.partial.wav"));
        assert_eq!(final_name(&safety_name(&p)), d.join("StreamWave_1_safety.wav"));
        assert_eq!(part_name(&safety_name(&p), 2), d.join("StreamWave_1_safety_part2.partial.wav"));
    }
}
