//! Non-destructive effect chain.
//!
//! Effects are plain parameter structs stored on the track. They're applied by a **streaming**
//! engine ([`Chain`]): every stage is stateful and block-size independent, and stages with
//! look-ahead (limiter, spectral NR, RNNoise) hide their own latency (they emit nothing at first,
//! then flush at the end). So the exact same code gives:
//!  * export / measure: [`process_chain`] runs the whole buffer through the chain;
//!  * playback: the render-ahead thread feeds small blocks and can swap parameters live.
//! Whole-file effects (Normalize, Loudness) need a measurement first: [`resolve_gains`] works out
//! their static gain, which the streaming chain then applies. Export and playback share the result.
use crate::waveform::Decoded;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::f32::consts::PI;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EqBand { pub freq: f32, pub gain_db: f32, pub q: f32 }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Effect {
    Gain { db: f32 },
    Eq { hpf_hz: f32, hpf_on: bool, low_shelf: EqBand, peaks: [EqBand; 3], high_shelf: EqBand },
    Compressor { threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32, makeup_db: f32 },
    Gate { threshold_db: f32, floor_db: f32, attack_ms: f32, hold_ms: f32, release_ms: f32 },
    NoiseReduction { reduction_db: f32, sensitivity_db: f32, profile: Vec<f32> },
    Normalize { peak_db: f32 },
    /// With `ceiling_db`, loudness is reached *through* a limiter (iterated), so peaks stay safe and the target is still hit.
    Loudness { target_lufs: f32, #[serde(default)] ceiling_db: Option<f32> },
    Fade { in_ms: f32, out_ms: f32 },
    Limiter { ceiling_db: f32, release_ms: f32 },
    /// Split-band de-esser: only the band above `freq_hz` is turned down when it gets hot.
    DeEsser { freq_hz: f32, threshold_db: f32, max_reduction_db: f32 },
    /// RNNoise neural denoiser (nnnoiseless, pure Rust). `mix` 0..1 = dry..wet.
    Denoise { mix: f32 },
}

impl Effect {
    /// Whole-file effects whose gain must be measured before streaming.
    pub fn is_global(&self) -> bool { matches!(self, Effect::Normalize { .. } | Effect::Loudness { .. }) }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Slot { #[serde(default)] pub bypass: bool, pub fx: Effect }

#[inline] fn db2lin(db: f32) -> f32 { 10f32.powf(db / 20.0) }
#[inline] fn lin2db(v: f32) -> f32 { 20.0 * v.max(1e-9).log10() }
fn coef(ms: f32, sr: f32) -> f32 { if ms <= 0.0 { 0.0 } else { (-1.0 / (ms * 0.001 * sr)).exp() } }
#[inline] fn finite(v: f32) -> f32 { if v.is_finite() { v } else { 0.0 } }

// =====================================================================================
// public API
// =====================================================================================

/// Run the whole chain over `d` (export, measuring, Studio Sound). Identical to playback.
pub fn process_chain(d: &mut Decoded, slots: &[Slot]) {
    let gains = resolve_gains(d, slots, None).unwrap_or_else(|| vec![None; slots.len()]);
    let total = d.samples.len() / d.channels.max(1);
    let mut chain = Chain::new(slots, &gains, d.channels, d.sample_rate, total, 0);
    let mut out = Vec::with_capacity(d.samples.len());
    chain.process(&d.samples, &mut out);
    chain.flush(&mut out);
    debug_assert_eq!(out.len(), d.samples.len());
    d.samples = out;
}

/// Single effect, for tests and tools.
#[allow(dead_code)]
pub fn process(d: &mut Decoded, fx: &Effect) { process_chain(d, &[Slot { bypass: false, fx: fx.clone() }]) }

/// Measure the static gain of each whole-file stage (Normalize / Loudness) by running the chain
/// up to that point over the whole input. `None` entries = not a global stage.
/// `cancel` lets a newer request abort a stale measurement (returns None).
pub fn resolve_gains(d: &Decoded, slots: &[Slot], cancel: Option<&dyn Fn() -> bool>) -> Option<Vec<Option<f32>>> {
    let mut gains = vec![None; slots.len()];
    let last_global = match slots.iter().rposition(|s| !s.bypass && s.fx.is_global()) { Some(i) => i, None => return Some(gains) };
    let (ch, sr) = (d.channels.max(1), d.sample_rate);
    let total = d.samples.len() / ch;
    let mut buf = d.samples.clone();
    for (i, s) in slots.iter().enumerate().take(last_global + 1) {
        if cancel.is_some_and(|c| c()) { return None }
        if s.bypass { continue }
        let g = match &s.fx {
            Effect::Normalize { peak_db } => {
                let pk = buf.iter().fold(0f32, |m, v| m.max(finite(*v).abs()));
                Some(if pk > 1e-9 { db2lin(*peak_db) / pk } else { 1.0 })
            }
            Effect::Loudness { target_lufs, ceiling_db } => Some(loudness_gain(&buf, ch, sr, *target_lufs, *ceiling_db)),
            _ => None,
        };
        gains[i] = g;
        if i < last_global {
            let mut one = vec![None; 1]; one[0] = g;
            let mut st = Chain::new(std::slice::from_ref(s), &one, ch, sr, total, 0);
            let mut out = Vec::with_capacity(buf.len());
            st.process(&buf, &mut out); st.flush(&mut out);
            buf = out;
        }
    }
    Some(gains)
}

fn loudness_gain(buf: &[f32], ch: usize, sr: u32, target: f32, ceiling: Option<f32>) -> f32 {
    let Some(l) = lufs_of(buf, ch, sr) else { return 1.0 };
    let mut gain_db = target - l as f32;
    let Some(c) = ceiling else { return db2lin(gain_db) };
    // reach the target *through* the limiter (secant iterations)
    let mut prev: Option<(f32, f32)> = None;
    let mut tmp = Vec::with_capacity(buf.len());
    for _ in 0..12 {
        let g = db2lin(gain_db);
        let mut lim = Limiter::new(db2lin(c), coef(80.0, sr as f32), ch, sr);
        tmp.clear();
        let gained: Vec<f32> = buf.iter().map(|v| finite(*v) * g).collect();
        lim.process(&gained, &mut tmp); lim.flush(&mut tmp);
        let Some(now) = lufs_of(&tmp, ch, sr) else { break };
        let now = now as f32; let err = target - now;
        if err.abs() < 0.05 { break }
        let slope = match prev { Some((pg, pl)) if (gain_db - pg).abs() > 1e-3 => ((now - pl) / (gain_db - pg)).clamp(0.05, 1.0), _ => 1.0 };
        prev = Some((gain_db, now));
        gain_db = (gain_db + err / slope).min(60.0);
    }
    db2lin(gain_db)
}

fn lufs_of(s: &[f32], ch: usize, sr: u32) -> Option<f64> {
    let mut m = ebur128::EbuR128::new(ch as u32, sr, ebur128::Mode::I).ok()?;
    m.add_frames_f32(s).ok()?;
    m.loudness_global().ok().filter(|l| l.is_finite())
}
pub fn integrated_lufs(d: &Decoded) -> Option<f64> { lufs_of(&d.samples, d.channels, d.sample_rate) }

// =====================================================================================
// streaming chain
// =====================================================================================

/// A built, stateful effect chain. Feed interleaved blocks of any size.
pub struct Chain { stages: Vec<Stage>, ch: usize, scratch: Vec<f32>, scratch2: Vec<f32> }

struct Stage { bypass: bool, fx: Effect, kind: Kind }

enum Kind {
    Gain(f32),
    Eq(Vec<Vec<Biquad>>), // [filter][channel]
    Comp { thr: f32, ratio: f32, a: f32, r: f32, mk: f32, gr_db: f32 },
    Gate { thr: f32, floor: f32, a: f32, r: f32, hold: usize, env_r: f32, g: f32, held: usize, env: f32 },
    DeEsser { lp: Vec<Biquad>, a: f32, r: f32, thr: f32, min_g: f32, env: f32, g: f32 },
    Fade { fi: usize, fo: usize, total: usize, pos: usize },
    Limiter(Limiter),
    LoudLim(f32, Limiter),
    Nr(Box<Nr>),
    Denoise(Box<Rn>),
}

impl Chain {
    /// `gains`: output of [`resolve_gains`] (or previous values while re-measuring).
    /// `start_frame`: where playback starts (only Fade cares).
    pub fn new(slots: &[Slot], gains: &[Option<f32>], ch: usize, sr: u32, total_frames: usize, start_frame: usize) -> Self {
        let ch = ch.max(1); let srf = sr as f32;
        let stages = slots.iter().enumerate().map(|(i, s)| {
            let g = gains.get(i).copied().flatten().unwrap_or(1.0);
            Stage { bypass: s.bypass, fx: s.fx.clone(), kind: Kind::build(&s.fx, g, ch, sr, srf, total_frames, start_frame) }
        }).collect();
        Self { stages, ch, scratch: Vec::new(), scratch2: Vec::new() }
    }

    /// Swap in new parameters while keeping filter/envelope/buffer state where the effect type
    /// at the same position is unchanged, so live slider moves don't click or reset.
    pub fn update(&mut self, slots: &[Slot], gains: &[Option<f32>], sr: u32, total_frames: usize, pos_frame: usize) {
        let mut new = Chain::new(slots, gains, self.ch, sr, total_frames, pos_frame);
        let mut old: Vec<Option<Stage>> = std::mem::take(&mut self.stages).into_iter().map(Some).collect();
        for (i, st) in new.stages.iter_mut().enumerate() {
            if let Some(Some(o)) = old.get_mut(i).map(|o| o.take()) { st.kind.carry(o.kind) }
        }
        self.stages = std::mem::take(&mut new.stages);
    }

    pub fn set_gains(&mut self, gains: &[Option<f32>]) {
        for (st, g) in self.stages.iter_mut().zip(gains) {
            if let Some(g) = g { match &mut st.kind { Kind::Gain(x) if st.fx.is_global() => *x = *g, Kind::LoudLim(x, _) => *x = *g, _ => {} } }
        }
    }

    /// Process a block. May emit fewer samples than given while look-ahead stages fill up.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let ch = self.ch;
        let mut cur = std::mem::take(&mut self.scratch); cur.clear(); cur.extend(input.iter().map(|v| finite(*v)));
        let mut nxt = std::mem::take(&mut self.scratch2);
        for st in self.stages.iter_mut() {
            if st.bypass { continue }
            if st.kind.in_place() { st.kind.run_in_place(&mut cur, ch) }
            else { nxt.clear(); st.kind.run(&cur, &mut nxt); std::mem::swap(&mut cur, &mut nxt) }
        }
        out.extend_from_slice(&cur);
        self.scratch = cur; self.scratch2 = nxt;
    }

    /// End of stream: drain look-ahead stages (in order, feeding later stages).
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let ch = self.ch;
        let n = self.stages.len();
        let mut carry: Vec<f32> = Vec::new();
        for i in 0..n {
            // pass what earlier stages flushed through this stage, then flush it
            let st = &mut self.stages[i];
            if st.bypass { continue }
            let mut tmp = Vec::new();
            if st.kind.in_place() { st.kind.run_in_place(&mut carry, ch); tmp = std::mem::take(&mut carry); }
            else { st.kind.run(&carry, &mut tmp); st.kind.flush(&mut tmp); }
            carry = tmp;
        }
        out.extend_from_slice(&carry);
    }
}

impl Kind {
    fn build(fx: &Effect, g: f32, ch: usize, sr: u32, srf: f32, total: usize, start: usize) -> Kind {
        match fx {
            Effect::Gain { db } => Kind::Gain(db2lin(*db)),
            Effect::Normalize { .. } => Kind::Gain(g),
            Effect::Loudness { ceiling_db: None, .. } => Kind::Gain(g),
            Effect::Loudness { ceiling_db: Some(c), .. } => Kind::LoudLim(g, Limiter::new(db2lin(*c), coef(80.0, srf), ch, sr)),
            Effect::Eq { hpf_hz, hpf_on, low_shelf, peaks, high_shelf } => {
                let mut f = vec![];
                if *hpf_on { f.push(Biquad::hpf(*hpf_hz, 0.707, srf)) }
                if low_shelf.gain_db.abs() > 0.01 { f.push(Biquad::shelf(low_shelf.freq, low_shelf.gain_db, srf, false)) }
                for p in peaks { if p.gain_db.abs() > 0.01 { f.push(Biquad::peak(p.freq, p.gain_db, p.q, srf)) } }
                if high_shelf.gain_db.abs() > 0.01 { f.push(Biquad::shelf(high_shelf.freq, high_shelf.gain_db, srf, true)) }
                Kind::Eq(f.into_iter().map(|b| vec![b; ch]).collect())
            }
            Effect::Compressor { threshold_db, ratio, attack_ms, release_ms, makeup_db } =>
                Kind::Comp { thr: *threshold_db, ratio: ratio.max(1.0), a: coef(*attack_ms, srf), r: coef(*release_ms, srf), mk: db2lin(*makeup_db), gr_db: 0.0 },
            Effect::Gate { threshold_db, floor_db, attack_ms, hold_ms, release_ms } => {
                let floor = db2lin(*floor_db);
                Kind::Gate { thr: *threshold_db, floor, a: coef(*attack_ms, srf), r: coef(*release_ms, srf), hold: (hold_ms.max(0.0) * 0.001 * srf) as usize,
                    env_r: coef(10.0, srf), g: floor, held: 0, env: 0.0 }
            }
            Effect::DeEsser { freq_hz, threshold_db, max_reduction_db } =>
                Kind::DeEsser { lp: vec![Biquad::lpf(*freq_hz, 0.707, srf); ch], a: coef(1.0, srf), r: coef(60.0, srf), thr: *threshold_db,
                    min_g: db2lin(-max_reduction_db.abs()), env: 0.0, g: 1.0 },
            Effect::Fade { in_ms, out_ms } => Kind::Fade { fi: ((in_ms.max(0.0) * 0.001 * srf) as usize).min(total), fo: ((out_ms.max(0.0) * 0.001 * srf) as usize).min(total), total, pos: start },
            Effect::Limiter { ceiling_db, release_ms } => Kind::Limiter(Limiter::new(db2lin(*ceiling_db), coef(*release_ms, srf), ch, sr)),
            Effect::NoiseReduction { reduction_db, sensitivity_db, profile } => Kind::Nr(Box::new(Nr::new(ch, profile.clone(), *reduction_db, *sensitivity_db))),
            Effect::Denoise { mix } => Kind::Denoise(Box::new(Rn::new(ch, mix.clamp(0.0, 1.0)))),
        }
    }

    /// Move runtime state from `old` into `self` if they're the same kind.
    fn carry(&mut self, old: Kind) {
        match (self, old) {
            (Kind::Eq(n), Kind::Eq(o)) if n.len() == o.len() => for (nf, of) in n.iter_mut().zip(o) { for (a, b) in nf.iter_mut().zip(of) { a.z1 = b.z1; a.z2 = b.z2 } },
            (Kind::Comp { gr_db, .. }, Kind::Comp { gr_db: o, .. }) => *gr_db = o,
            (Kind::Gate { g, held, env, .. }, Kind::Gate { g: og, held: oh, env: oe, .. }) => { *g = og; *held = oh; *env = oe }
            (Kind::DeEsser { lp, env, g, .. }, Kind::DeEsser { lp: olp, env: oe, g: og, .. }) => { for (a, b) in lp.iter_mut().zip(olp) { a.z1 = b.z1; a.z2 = b.z2 } *env = oe; *g = og }
            (Kind::Fade { pos, .. }, Kind::Fade { pos: op, .. }) => *pos = op,
            (Kind::Limiter(n), Kind::Limiter(o)) => n.take_state(o),
            (Kind::LoudLim(_, n), Kind::LoudLim(_, o)) => n.take_state(o),
            (Kind::Nr(n), Kind::Nr(o)) => n.take_state(*o),
            (Kind::Denoise(n), Kind::Denoise(o)) => { let mix = n.mix; **n = *o; n.mix = mix }
            _ => {}
        }
    }

    fn in_place(&self) -> bool { !matches!(self, Kind::Limiter(_) | Kind::LoudLim(..) | Kind::Nr(_) | Kind::Denoise(_)) }

    fn run_in_place(&mut self, buf: &mut [f32], ch: usize) {
        match self {
            Kind::Gain(g) => { let g = *g; buf.iter_mut().for_each(|s| *s *= g) }
            Kind::Eq(filters) => for f in filters.iter_mut() { for fr in buf.chunks_mut(ch) { for (c, s) in fr.iter_mut().enumerate() { *s = f[c].tick(*s) } } },
            Kind::Comp { thr, ratio, a, r, mk, gr_db } => for f in buf.chunks_mut(ch) {
                let lvl = lin2db(f.iter().fold(0f32, |m, s| m.max(s.abs())));
                let target = if lvl > *thr { (*thr + (lvl - *thr) / *ratio) - lvl } else { 0.0 };
                let k = if target < *gr_db { *a } else { *r };
                *gr_db = target + k * (*gr_db - target);
                let g = db2lin(*gr_db) * *mk; f.iter_mut().for_each(|s| *s *= g);
            },
            Kind::Gate { thr, floor, a, r, hold, env_r, g, held, env } => for f in buf.chunks_mut(ch) {
                let pk = f.iter().fold(0f32, |m, s| m.max(s.abs()));
                *env = if pk > *env { pk } else { pk + *env_r * (*env - pk) };
                let open = lin2db(*env) > *thr;
                if open { *held = *hold } else if *held > 0 { *held -= 1 }
                let target = if open || *held > 0 { 1.0 } else { *floor };
                let k = if target > *g { *a } else { *r };
                *g = target + k * (*g - target);
                f.iter_mut().for_each(|s| *s *= *g);
            },
            Kind::DeEsser { lp, a, r, thr, min_g, env, g } => for f in buf.chunks_mut(ch) {
                let mut pk = 0f32;
                let mut hi = [0f32; 8];
                for (c, s) in f.iter().enumerate() { let h = *s - lp[c].tick(*s); if c < 8 { hi[c] = h } pk = pk.max(h.abs()) }
                *env = if pk > *env { pk + *a * (*env - pk) } else { pk + *r * (*env - pk) };
                let over = lin2db(*env) - *thr;
                let target = if over > 0.0 { db2lin(-over).max(*min_g) } else { 1.0 };
                *g = if target < *g { target + *a * (*g - target) } else { target + *r * (*g - target) };
                for (c, s) in f.iter_mut().enumerate() { if c < 8 { *s = (*s - hi[c]) + hi[c] * *g } }
            },
            Kind::Fade { fi, fo, total, pos } => for f in buf.chunks_mut(ch) {
                let n = *pos; let mut g = 1f32;
                if n < *fi { g *= n as f32 / *fi as f32 }
                if *fo > 0 && n + *fo >= *total { g *= (total.saturating_sub(n)) as f32 / *fo as f32 }
                let g = g * g; f.iter_mut().for_each(|s| *s *= g);
                *pos += 1;
            },
            _ => unreachable!("not an in-place stage"),
        }
    }
    fn run(&mut self, input: &[f32], out: &mut Vec<f32>) {
        match self {
            Kind::Limiter(l) => l.process(input, out),
            Kind::LoudLim(g, l) => { let g = *g; let tmp: Vec<f32> = input.iter().map(|s| s * g).collect(); l.process(&tmp, out) }
            Kind::Nr(n) => n.process(input, out),
            Kind::Denoise(d) => d.process(input, out),
            _ => unreachable!(),
        }
    }
    fn flush(&mut self, out: &mut Vec<f32>) {
        match self { Kind::Limiter(l) | Kind::LoudLim(_, l) => l.flush(out), Kind::Nr(n) => n.flush(out), Kind::Denoise(d) => d.flush(out), _ => {} }
    }
}

// ---------------- look-ahead brickwall limiter (5 ms), streaming ----------------
struct Limiter { ceil: f32, rel: f32, ch: usize, la: usize, g: f32,
    frames: VecDeque<f32>, needs: VecDeque<f32>, mono: VecDeque<(usize, f32)>, in_idx: usize, out_idx: usize }
impl Limiter {
    fn new(ceil: f32, rel: f32, ch: usize, sr: u32) -> Self {
        Self { ceil, rel, ch, la: (sr as f32 * 0.005) as usize + 1, g: 1.0, frames: VecDeque::new(), needs: VecDeque::new(), mono: VecDeque::new(), in_idx: 0, out_idx: 0 }
    }
    fn take_state(&mut self, o: Limiter) { let (c, r) = (self.ceil, self.rel); *self = o; self.ceil = c; self.rel = r; }
    fn emit(&mut self, out: &mut Vec<f32>) {
        while self.mono.front().is_some_and(|&(i, _)| i < self.out_idx) { self.mono.pop_front(); }
        let target = self.mono.front().map(|&(_, n)| n).unwrap_or(1.0);
        self.g = if target < self.g { target } else { target + self.rel * (self.g - target) };
        self.needs.pop_front();
        for _ in 0..self.ch { let s = self.frames.pop_front().unwrap_or(0.0); out.push((finite(s) * self.g).clamp(-self.ceil, self.ceil)) }
        self.out_idx += 1;
    }
    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for f in input.chunks(self.ch) {
            let p = f.iter().fold(0f32, |m, s| m.max(finite(*s).abs()));
            let need = if p > self.ceil { self.ceil / p } else { 1.0 };
            while self.mono.back().is_some_and(|&(_, n)| n >= need) { self.mono.pop_back(); }
            self.mono.push_back((self.in_idx, need));
            self.frames.extend(f.iter().copied()); self.needs.push_back(need);
            self.in_idx += 1;
            // window [out_idx, out_idx + la) complete?
            while self.in_idx >= self.out_idx + self.la { self.emit(out) }
        }
    }
    fn flush(&mut self, out: &mut Vec<f32>) { while self.out_idx < self.in_idx { self.emit(out) } }
}

// ---------------- spectral noise reduction, streaming STFT ----------------
pub const NR_FFT: usize = 2048;
const NR_HOP: usize = NR_FFT / 4;

pub fn hann() -> Vec<f32> { (0..NR_FFT).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / NR_FFT as f32).cos()).collect() }

/// Average magnitude spectrum (mono mix) of a noise-only selection.
pub fn learn_noise_profile(d: &Decoded) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let ch = d.channels.max(1); let w = hann();
    let mono: Vec<f32> = d.samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(NR_FFT);
    let mut acc = vec![0f32; NR_FFT / 2 + 1]; let mut n = 0;
    let mut pos = 0;
    let mut buf = vec![Complex::new(0f32, 0.0); NR_FFT];
    while pos + NR_FFT <= mono.len() {
        for i in 0..NR_FFT { buf[i] = Complex::new(mono[pos + i] * w[i], 0.0) }
        fft.process(&mut buf);
        for (k, a) in acc.iter_mut().enumerate() { *a += buf[k].norm() }
        n += 1; pos += NR_HOP;
    }
    if n > 0 { acc.iter_mut().for_each(|a| *a /= n as f32) }
    acc
}

struct NrCh { inbuf: Vec<f32>, outacc: Vec<f32>, gains: Vec<f32> }
struct Nr {
    ch: usize, active: bool, floor: f32, thr: f32, prof_sum: Vec<f32>,
    w: Vec<f32>, fwd: std::sync::Arc<dyn rustfft::Fft<f32>>, inv: std::sync::Arc<dyn rustfft::Fft<f32>>,
    chans: Vec<NrCh>, pending: Vec<f32>, ready: VecDeque<f32>, skip: usize, received: usize, emitted: usize,
    buf: Vec<rustfft::num_complex::Complex<f32>>, mags: Vec<f32>,
}
impl Nr {
    fn new(ch: usize, profile: Vec<f32>, reduction_db: f32, sens_db: f32) -> Self {
        use rustfft::FftPlanner;
        let bins = NR_FFT / 2 + 1;
        let active = profile.len() == bins; // no profile learned yet -> pass-through
        let mut prof_sum = vec![0f32; bins + 1];
        if active { for k in 0..bins { prof_sum[k + 1] = prof_sum[k] + profile[k] } }
        let mut pl = FftPlanner::<f32>::new();
        Self { ch, active, floor: db2lin(-reduction_db.abs()), thr: db2lin(sens_db), prof_sum, w: hann(),
            fwd: pl.plan_fft_forward(NR_FFT), inv: pl.plan_fft_inverse(NR_FFT),
            chans: (0..ch).map(|_| NrCh { inbuf: vec![0.0; NR_FFT], outacc: vec![0.0; NR_FFT], gains: vec![1.0; bins] }).collect(),
            pending: Vec::with_capacity(NR_HOP * ch), ready: VecDeque::new(), skip: (NR_FFT - NR_HOP) * ch, received: 0, emitted: 0,
            buf: vec![rustfft::num_complex::Complex::new(0.0, 0.0); NR_FFT], mags: vec![0.0; bins] }
    }
    fn take_state(&mut self, o: Nr) {
        if !(self.active && o.active) || o.ch != self.ch { return } // profile appeared/disappeared: start clean
        let (floor, thr, prof) = (self.floor, self.thr, std::mem::take(&mut self.prof_sum));
        *self = o; self.floor = floor; self.thr = thr; self.prof_sum = prof;
    }
    fn hop(&mut self) {
        // pending holds NR_HOP interleaved frames
        let bins = NR_FFT / 2 + 1; let norm = 1.0 / (NR_FFT as f32 * 2.0);
        for c in 0..self.ch {
            let st = &mut self.chans[c];
            st.inbuf.copy_within(NR_HOP.., 0);
            for i in 0..NR_HOP { st.inbuf[NR_FFT - NR_HOP + i] = self.pending[i * self.ch + c] }
            for i in 0..NR_FFT { self.buf[i] = rustfft::num_complex::Complex::new(st.inbuf[i] * self.w[i], 0.0) }
            self.fwd.process(&mut self.buf);
            for k in 0..bins { self.mags[k] = self.buf[k].norm() }
            for k in 0..bins {
                let (lo, hi) = (k.saturating_sub(2), (k + 3).min(bins));
                let m: f32 = self.mags[lo..hi].iter().sum(); let p = self.prof_sum[hi] - self.prof_sum[lo];
                let target = if m > p * self.thr { 1.0 } else { self.floor };
                st.gains[k] = if target > st.gains[k] { target } else { st.gains[k] * 0.6 + target * 0.4 };
                let g = st.gains[k];
                self.buf[k] *= g; if k > 0 && k < NR_FFT / 2 { self.buf[NR_FFT - k] *= g }
            }
            self.inv.process(&mut self.buf);
            for i in 0..NR_FFT { st.outacc[i] += self.buf[i].re * norm }
        }
        for i in 0..NR_HOP { for c in 0..self.ch { self.ready.push_back(finite(self.chans[c].outacc[i])) } }
        for st in self.chans.iter_mut() { st.outacc.copy_within(NR_HOP.., 0); for v in &mut st.outacc[NR_FFT - NR_HOP..] { *v = 0.0 } }
        self.pending.clear();
    }
    fn drain(&mut self, out: &mut Vec<f32>) {
        while self.skip > 0 && self.ready.pop_front().is_some() { self.skip -= 1 }
        while self.emitted < self.received { match self.ready.pop_front() { Some(v) => { out.push(v); self.emitted += 1 } None => break } }
    }
    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if !self.active { out.extend_from_slice(input); return }
        self.received += input.len();
        for s in input { self.pending.push(*s); if self.pending.len() == NR_HOP * self.ch { self.hop() } }
        self.drain(out);
    }
    fn flush(&mut self, out: &mut Vec<f32>) {
        if !self.active { return }
        let mut guard = 0;
        while self.emitted < self.received && guard < 64 {
            while self.pending.len() < NR_HOP * self.ch { self.pending.push(0.0) }
            self.hop(); self.drain(out); guard += 1;
        }
    }
}

// ---------------- RNNoise (nnnoiseless), streaming ----------------
// RNNoise is trained for 48 kHz; at other rates it runs as-is (slightly shifted bands).
struct Rn { ch: usize, mix: f32, states: Vec<Box<nnnoiseless::DenoiseState<'static>>>, pending: Vec<f32>,
    dry: VecDeque<f32>, wet: VecDeque<f32>, skip: usize, received: usize, emitted: usize }
const RN_F: usize = nnnoiseless::DenoiseState::FRAME_SIZE;
impl Rn {
    fn new(ch: usize, mix: f32) -> Self {
        Self { ch, mix, states: (0..ch).map(|_| nnnoiseless::DenoiseState::new()).collect(), pending: Vec::with_capacity(RN_F * ch),
            dry: VecDeque::new(), wet: VecDeque::new(), skip: RN_F * ch, received: 0, emitted: 0 }
    }
    fn frame(&mut self) {
        let mut inp = [0f32; RN_F]; let mut outp = [0f32; RN_F];
        let mut res = vec![0f32; RN_F * self.ch];
        for c in 0..self.ch {
            for i in 0..RN_F { inp[i] = self.pending[i * self.ch + c] * 32768.0 }
            self.states[c].process_frame(&mut outp, &inp);
            for i in 0..RN_F { res[i * self.ch + c] = finite(outp[i] / 32768.0) }
        }
        self.wet.extend(res);
        self.pending.clear();
    }
    fn drain(&mut self, out: &mut Vec<f32>) {
        while self.skip > 0 && self.wet.pop_front().is_some() { self.skip -= 1 } // model's one-frame lag
        while self.emitted < self.received {
            let Some(w) = self.wet.pop_front() else { break };
            let d = self.dry.pop_front().unwrap_or(0.0);
            out.push(d * (1.0 - self.mix) + w * self.mix); self.emitted += 1;
        }
    }
    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.received += input.len();
        self.dry.extend(input.iter().copied());
        for s in input { self.pending.push(*s); if self.pending.len() == RN_F * self.ch { self.frame() } }
        self.drain(out);
    }
    fn flush(&mut self, out: &mut Vec<f32>) {
        let mut guard = 0;
        while self.emitted < self.received && guard < 8 {
            while self.pending.len() < RN_F * self.ch { self.pending.push(0.0) }
            self.frame(); self.drain(out); guard += 1;
        }
    }
}

// ---------------- biquads (RBJ cookbook) ----------------
#[derive(Clone, Copy)]
struct Biquad { b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, z1: f32, z2: f32 }
impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self { Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 } }
    fn w(f: f32, sr: f32) -> (f32, f32) { let w = 2.0 * PI * f.clamp(10.0, sr * 0.49) / sr; (w.cos(), w.sin()) }
    fn hpf(f: f32, q: f32, sr: f32) -> Self { let (c, s) = Self::w(f, sr); let al = s / (2.0 * q); Self::new((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al) }
    fn lpf(f: f32, q: f32, sr: f32) -> Self { let (c, s) = Self::w(f, sr); let al = s / (2.0 * q); Self::new((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al) }
    fn peak(f: f32, g: f32, q: f32, sr: f32) -> Self { let a = 10f32.powf(g / 40.0); let (c, s) = Self::w(f, sr); let al = s / (2.0 * q.max(0.1));
        Self::new(1.0 + al * a, -2.0 * c, 1.0 - al * a, 1.0 + al / a, -2.0 * c, 1.0 - al / a) }
    fn shelf(f: f32, g: f32, sr: f32, high: bool) -> Self {
        let a = 10f32.powf(g / 40.0); let (c, s) = Self::w(f, sr); let al = s / 2.0 * 2f32.sqrt(); let sa = 2.0 * a.sqrt() * al;
        if !high { Self::new(a * ((a + 1.0) - (a - 1.0) * c + sa), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - sa), (a + 1.0) + (a - 1.0) * c + sa, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - sa) }
        else { Self::new(a * ((a + 1.0) + (a - 1.0) * c + sa), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - sa), (a + 1.0) - (a - 1.0) * c + sa, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - sa) }
    }
    #[inline] fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1; self.z1 = self.b1 * x - self.a1 * y + self.z2; self.z2 = self.b2 * x - self.a2 * y;
        if !y.is_finite() { self.z1 = 0.0; self.z2 = 0.0; return 0.0 } // never let a NaN latch the filter
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sine(freq: f32, amp: f32, secs: f32, ch: usize) -> Decoded {
        let sr = 48000; let n = (secs * sr as f32) as usize;
        let mut s = Vec::with_capacity(n * ch);
        for i in 0..n { let v = amp * (2.0 * PI * freq * i as f32 / sr as f32).sin(); for _ in 0..ch { s.push(v) } }
        Decoded { samples: s, channels: ch, sample_rate: sr }
    }
    fn noise(amp: f32, n: usize, seed: &mut u32) -> Vec<f32> { (0..n).map(|_| { *seed ^= *seed << 13; *seed ^= *seed >> 17; *seed ^= *seed << 5; (*seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp }).collect() }
    fn rms(s: &[f32]) -> f32 { (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt() }
    fn eq_flat() -> Effect { let b = |f| EqBand { freq: f, gain_db: 0.0, q: 1.0 }; Effect::Eq { hpf_hz: 80.0, hpf_on: false, low_shelf: b(120.0), peaks: [b(300.0), b(1000.0), b(4000.0)], high_shelf: b(8000.0) } }

    #[test] fn gain_is_exact() {
        let mut d = sine(440.0, 0.5, 0.1, 2); let orig = d.samples.clone();
        process(&mut d, &Effect::Gain { db: -6.0 });
        for (a, b) in d.samples.iter().zip(&orig) { assert!((a - b * 0.501187).abs() < 1e-6) }
    }
    #[test] fn bypass_is_identity() {
        let mut d = sine(440.0, 0.5, 0.5, 2); let orig = d.samples.clone();
        let all = vec![Effect::Gain { db: 12.0 }, eq_flat(), Effect::Compressor { threshold_db: -30.0, ratio: 8.0, attack_ms: 5.0, release_ms: 50.0, makeup_db: 6.0 },
            Effect::Gate { threshold_db: -20.0, floor_db: -80.0, attack_ms: 1.0, hold_ms: 10.0, release_ms: 50.0 }, Effect::Normalize { peak_db: -1.0 },
            Effect::Loudness { target_lufs: -14.0, ceiling_db: None }, Effect::Fade { in_ms: 100.0, out_ms: 100.0 }, Effect::Limiter { ceiling_db: -6.0, release_ms: 50.0 }];
        let chain: Vec<Slot> = all.into_iter().map(|fx| Slot { bypass: true, fx }).collect();
        process_chain(&mut d, &chain);
        assert_eq!(d.samples, orig);
    }
    #[test] fn flat_eq_is_identity() {
        let mut d = sine(1000.0, 0.5, 0.2, 1); let orig = d.samples.clone();
        process(&mut d, &eq_flat()); assert_eq!(d.samples, orig);
    }
    #[test] fn hpf_removes_low_end() {
        let mut d = sine(30.0, 0.5, 1.0, 1);
        if let Effect::Eq { hpf_hz, hpf_on, low_shelf, peaks, high_shelf } = eq_flat() { let _ = hpf_hz;
            process(&mut d, &Effect::Eq { hpf_hz: 200.0, hpf_on: !hpf_on, low_shelf, peaks, high_shelf }) }
        assert!(rms(&d.samples[24000..]) < 0.5 / 2f32.sqrt() * 0.1);
    }
    #[test] fn peak_eq_boosts_by_gain() {
        let mut d = sine(1000.0, 0.1, 1.0, 1);
        process(&mut d, &Effect::Eq { hpf_hz: 80.0, hpf_on: false, low_shelf: EqBand { freq: 100.0, gain_db: 0.0, q: 1.0 },
            peaks: [EqBand { freq: 1000.0, gain_db: 6.0, q: 1.0 }, EqBand { freq: 300.0, gain_db: 0.0, q: 1.0 }, EqBand { freq: 300.0, gain_db: 0.0, q: 1.0 }],
            high_shelf: EqBand { freq: 8000.0, gain_db: 0.0, q: 1.0 } });
        let r = lin2db(rms(&d.samples[24000..]) / (0.1 / 2f32.sqrt()));
        assert!((r - 6.0).abs() < 0.2, "got {r} dB");
    }
    #[test] fn loudness_hits_target_within_half_lu() {
        for target in [-14.0f32, -16.0, -23.0] {
            let mut d = sine(1000.0, 0.05, 5.0, 2);
            process(&mut d, &Effect::Loudness { target_lufs: target, ceiling_db: None });
            let l = integrated_lufs(&d).unwrap() as f32;
            assert!((l - target).abs() < 0.5, "target {target} got {l}");
        }
    }
    #[test] fn normalize_and_limiter_ceilings() {
        let mut d = sine(440.0, 0.3, 0.5, 2); process(&mut d, &Effect::Normalize { peak_db: -1.0 });
        let pk = d.samples.iter().fold(0f32, |m, s| m.max(s.abs())); assert!((lin2db(pk) + 1.0).abs() < 0.01);
        let mut d = sine(440.0, 1.0, 0.5, 2); process(&mut d, &Effect::Limiter { ceiling_db: -3.0, release_ms: 50.0 });
        let pk = d.samples.iter().fold(0f32, |m, s| m.max(s.abs())); assert!(pk <= db2lin(-3.0) + 1e-6);
    }
    #[test] fn compressor_reduces_loud_signal() {
        let mut d = sine(440.0, 1.0, 1.0, 1);
        process(&mut d, &Effect::Compressor { threshold_db: -20.0, ratio: 4.0, attack_ms: 1.0, release_ms: 50.0, makeup_db: 0.0 });
        // peak 0 dBFS, 20 dB over -> 5 dB over => about -15 dBFS peak
        let pk = lin2db(d.samples[24000..].iter().fold(0f32, |m, s| m.max(s.abs())));
        assert!((pk + 15.0).abs() < 1.5, "got {pk}");
    }
    #[test] fn gate_silences_quiet_part() {
        let mut d = sine(440.0, 0.001, 1.0, 1);
        process(&mut d, &Effect::Gate { threshold_db: -40.0, floor_db: -80.0, attack_ms: 1.0, hold_ms: 10.0, release_ms: 20.0 });
        assert!(rms(&d.samples[24000..]) < 1e-6);
    }
    #[test] fn noise_reduction_lowers_noise_keeps_tone() {
        let mut seed = 12345u32; let sr = 48000;
        let nz = noise(0.02, sr * 2, &mut seed);
        let profile = learn_noise_profile(&Decoded { samples: nz[..sr].to_vec(), channels: 1, sample_rate: sr as u32 });
        let tone = sine(1000.0, 0.3, 1.0, 1);
        let mut mix: Vec<f32> = nz[sr..].to_vec(); mix.extend(nz[..sr].iter().zip(&tone.samples).map(|(n, t)| n + t));
        let mut d = Decoded { samples: mix, channels: 1, sample_rate: sr as u32 };
        process(&mut d, &Effect::NoiseReduction { reduction_db: 24.0, sensitivity_db: 6.0, profile });
        let noise_after = rms(&d.samples[sr / 4..sr * 3 / 4]);
        assert!(lin2db(noise_after / rms(&nz[sr..])) < -15.0, "noise only reduced to {} dB", lin2db(noise_after / rms(&nz[sr..])));
        let tone_after = rms(&d.samples[sr + sr / 4..sr + sr * 3 / 4]);
        let t = lin2db(tone_after / rms(&tone.samples)); assert!(t.abs() < 1.0, "tone changed by {t} dB");
    }
    #[test] fn deesser_tames_only_highs() {
        let mut lo = sine(300.0, 0.3, 0.5, 1); let o = lo.samples.clone();
        let fx = Effect::DeEsser { freq_hz: 5000.0, threshold_db: -30.0, max_reduction_db: 12.0 };
        process(&mut lo, &fx);
        assert!((lin2db(rms(&lo.samples[12000..]) / rms(&o[12000..]))).abs() < 0.5);
        let mut hi = sine(8000.0, 0.3, 0.5, 1); process(&mut hi, &fx);
        let r = lin2db(rms(&hi.samples[12000..]) / (0.3 / 2f32.sqrt())); assert!(r < -4.0, "sibilant band only down {r} dB");
    }
    #[test] fn rnnoise_runs_and_reduces_noise() {
        let mut seed = 99u32; let n = 48000;
        let mut d = Decoded { samples: noise(0.05, n, &mut seed), channels: 1, sample_rate: 48000 };
        let before = rms(&d.samples); process(&mut d, &Effect::Denoise { mix: 1.0 });
        let r = lin2db(rms(&d.samples[4800..]) / before); assert!(r < -3.0, "rnnoise only {r} dB");
    }
    fn full_chain(profile: Vec<f32>) -> Vec<Slot> {
        let b = |f| EqBand { freq: f, gain_db: 2.0, q: 1.0 };
        vec![
            Slot { bypass: false, fx: Effect::NoiseReduction { reduction_db: 18.0, sensitivity_db: 6.0, profile } },
            Slot { bypass: false, fx: Effect::Denoise { mix: 0.5 } },
            Slot { bypass: false, fx: Effect::Eq { hpf_hz: 90.0, hpf_on: true, low_shelf: b(150.0), peaks: [b(400.0), b(2000.0), b(5000.0)], high_shelf: b(9000.0) } },
            Slot { bypass: true, fx: Effect::Gain { db: 30.0 } },
            Slot { bypass: false, fx: Effect::Gate { threshold_db: -50.0, floor_db: -20.0, attack_ms: 2.0, hold_ms: 50.0, release_ms: 100.0 } },
            Slot { bypass: false, fx: Effect::DeEsser { freq_hz: 5500.0, threshold_db: -30.0, max_reduction_db: 8.0 } },
            Slot { bypass: false, fx: Effect::Compressor { threshold_db: -24.0, ratio: 3.0, attack_ms: 5.0, release_ms: 100.0, makeup_db: 2.0 } },
            Slot { bypass: false, fx: Effect::Normalize { peak_db: -3.0 } },
            Slot { bypass: false, fx: Effect::Fade { in_ms: 100.0, out_ms: 200.0 } },
            Slot { bypass: false, fx: Effect::Loudness { target_lufs: -16.0, ceiling_db: Some(-1.0) } },
            Slot { bypass: false, fx: Effect::Limiter { ceiling_db: -1.0, release_ms: 50.0 } },
        ]
    }
    fn test_signal(ch: usize) -> Decoded {
        let mut seed = 7u32; let n = 48000 * 2;
        let nz = noise(0.01, n * ch, &mut seed);
        let samples = (0..n * ch).map(|i| { let t = (i / ch) as f32 / 48000.0; nz[i] + if (t % 0.5) < 0.3 { 0.3 * (2.0 * PI * 220.0 * t).sin() } else { 0.0 } }).collect();
        Decoded { samples, channels: ch, sample_rate: 48000 }
    }
    /// Playback (render-ahead in odd-sized blocks) must be bit-identical to export.
    #[test] fn playback_blocks_match_export_exactly() {
        for ch in [1usize, 2] {
            let d = test_signal(ch);
            let profile = learn_noise_profile(&Decoded { samples: noise(0.01, 48000, &mut 3), channels: 1, sample_rate: 48000 });
            let slots = full_chain(profile);
            let mut exported = Decoded { samples: d.samples.clone(), channels: ch, sample_rate: 48000 };
            process_chain(&mut exported, &slots);
            assert_eq!(exported.samples.len(), d.samples.len(), "length preserved");
            let gains = resolve_gains(&d, &slots, None).unwrap();
            let mut chain = Chain::new(&slots, &gains, ch, 48000, d.samples.len() / ch, 0);
            let mut out = vec![]; let mut pos = 0; let mut k = 1usize;
            while pos < d.samples.len() / ch {
                let n = ((k * 7919) % 1500 + 1).min(d.samples.len() / ch - pos); k += 1;
                chain.process(&d.samples[pos * ch..(pos + n) * ch], &mut out); pos += n;
            }
            chain.flush(&mut out);
            assert_eq!(out.len(), exported.samples.len());
            let maxdiff = out.iter().zip(&exported.samples).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(maxdiff == 0.0, "ch {ch}: playback differs from export by {maxdiff}");
            assert!((integrated_lufs(&exported).unwrap() + 16.0).abs() < 0.5);
        }
    }
    /// A live update with identical parameters must not change the output at all (state carried over).
    #[test] fn live_update_keeps_state() {
        let d = test_signal(2);
        let slots = full_chain(learn_noise_profile(&Decoded { samples: noise(0.01, 48000, &mut 3), channels: 1, sample_rate: 48000 }));
        let gains = resolve_gains(&d, &slots, None).unwrap();
        let total = d.samples.len() / 2;
        let run = |update: bool| {
            let mut c = Chain::new(&slots, &gains, 2, 48000, total, 0); let mut out = vec![];
            let half = d.samples.len() / 2 / 2 * 2;
            c.process(&d.samples[..half], &mut out);
            if update { c.update(&slots, &gains, 48000, total, half / 2) }
            c.process(&d.samples[half..], &mut out); c.flush(&mut out); out
        };
        assert_eq!(run(false), run(true));
    }
    #[test] fn bypassing_everything_is_identity_with_lookahead_stages() {
        let d = test_signal(2);
        let slots: Vec<Slot> = full_chain(vec![0.1; NR_FFT / 2 + 1]).into_iter().map(|mut s| { s.bypass = true; s }).collect();
        let mut out = Decoded { samples: d.samples.clone(), channels: 2, sample_rate: 48000 };
        process_chain(&mut out, &slots); assert_eq!(out.samples, d.samples);
    }
    #[test] fn nan_input_does_not_poison_output() {
        let mut d = sine(440.0, 0.5, 0.2, 1); d.samples[100] = f32::NAN; d.samples[200] = f32::INFINITY;
        process_chain(&mut d, &full_chain(vec![0.01; NR_FFT / 2 + 1]));
        assert!(d.samples.iter().all(|v| v.is_finite()));
    }
    #[test] fn fade_starts_and_ends_silent() {
        let mut d = sine(440.0, 0.5, 1.0, 1); d.samples.iter_mut().for_each(|s| *s = 0.5);
        process(&mut d, &Effect::Fade { in_ms: 100.0, out_ms: 100.0 });
        assert_eq!(d.samples[0], 0.0); assert!(d.samples[d.samples.len() - 1].abs() < 1e-3); assert_eq!(d.samples[24000], 0.5);
    }
}
