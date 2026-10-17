//! One-click "Studio Sound": analyse a voice track and build an effect chain automatically.
use crate::fx::{self, Effect, EqBand, Slot, NR_FFT};
use crate::waveform::Decoded;
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug)]
pub struct Analysis {
    pub noise_floor_db: f32,
    pub speech_level_db: f32,
    pub snr_db: f32,
    pub lufs: Option<f64>,
    /// band levels relative to the 500-2k band: [80-200, 200-500, 2k-5k, 5k-10k]
    pub balance_db: [f32; 4],
    pub sibilance_db: f32,
    pub plosives: usize,
    pub reverb_ms: f32,
    pub notes: Vec<String>,
    #[serde(skip)] pub noise_profile: Vec<f32>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Strength { Subtle, Medium, Strong }
impl Strength { fn k(self) -> f32 { match self { Self::Subtle => 0.5, Self::Medium => 0.75, Self::Strong => 1.0 } } }

const HOP: usize = NR_FFT / 2;
/// Target voice curve, relative to 500-2k (dB).
const TARGET: [f32; 4] = [-4.0, -2.0, -6.0, -14.0];

fn db(v: f32) -> f32 { 20.0 * v.max(1e-9).log10() }
fn pct(v: &mut Vec<f32>, p: f32) -> f32 {
    if v.is_empty() { return -120.0 }
    v.retain(|x| x.is_finite());
    if v.is_empty() { return -120.0 }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * p) as usize]
}

pub fn analyse(d: &Decoded) -> Analysis {
    let ch = d.channels; let sr = d.sample_rate as f32;
    let mono: Vec<f32> = d.samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    let w = fx::hann();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(NR_FFT);
    let bins = NR_FFT / 2 + 1;
    let hz = |k: usize| k as f32 * sr / NR_FFT as f32;
    let band = |lo: f32, hi: f32| ((lo * NR_FFT as f32 / sr) as usize).min(bins - 1)..((hi * NR_FFT as f32 / sr) as usize).min(bins);

    struct Fr { rms_db: f32, mag: Vec<f32> }
    let mut frames = vec![];
    let mut pos = 0;
    while pos + NR_FFT <= mono.len() {
        let seg = &mono[pos..pos + NR_FFT];
        let rms = (seg.iter().map(|v| v * v).sum::<f32>() / NR_FFT as f32).sqrt();
        let mut buf: Vec<Complex<f32>> = seg.iter().zip(&w).map(|(x, w)| Complex::new(x * w, 0.0)).collect();
        fft.process(&mut buf);
        frames.push(Fr { rms_db: db(rms), mag: buf[..bins].iter().map(|c| c.norm()).collect() });
        pos += HOP;
    }
    let mut notes = vec![];
    if frames.len() < 4 {
        return Analysis { noise_floor_db: -120.0, speech_level_db: -120.0, snr_db: 0.0, lufs: None, balance_db: [0.0; 4], sibilance_db: -60.0, plosives: 0, reverb_ms: 0.0,
            notes: vec!["Track is too short to analyse".into()], noise_profile: vec![] };
    }
    let mut levels: Vec<f32> = frames.iter().map(|f| f.rms_db).collect();
    let floor = pct(&mut levels.clone(), 0.10);
    let speech = pct(&mut levels, 0.80);

    // noise profile from the quietest frames (within 3 dB of the floor)
    let mut prof = vec![0f32; bins]; let mut n = 0;
    for f in frames.iter().filter(|f| f.rms_db <= floor + 3.0) { for k in 0..bins { prof[k] += f.mag[k] } n += 1 }
    if n > 0 { prof.iter_mut().for_each(|p| *p /= n as f32) }

    // spectral balance + sibilance on speech frames
    let loud: Vec<&Fr> = frames.iter().filter(|f| f.rms_db >= speech - 10.0 && f.rms_db > floor + 10.0).collect();
    let e = |f: &Fr, r: std::ops::Range<usize>| f.mag[r].iter().map(|m| m * m).sum::<f32>();
    let mut bal = [0f32; 4];
    let (bands, mid) = ([band(80.0, 200.0), band(200.0, 500.0), band(2000.0, 5000.0), band(5000.0, 10000.0)], band(500.0, 2000.0));
    let mut sib = vec![];
    if !loud.is_empty() {
        let mid_e: f32 = loud.iter().map(|f| e(f, mid.clone())).sum::<f32>().max(1e-12);
        for (i, b) in bands.iter().enumerate() { bal[i] = 10.0 * (loud.iter().map(|f| e(f, b.clone())).sum::<f32>().max(1e-12) / mid_e).log10() }
        for f in &loud { let tot = e(f, 0..bins).max(1e-12); sib.push(10.0 * (e(f, band(5000.0, 9000.0)).max(1e-12) / tot).log10()) }
    }
    let sibilance = pct(&mut sib, 0.9);

    // plosives: low-frequency bursts well above the usual low-end level
    let low = band(20.0, 120.0);
    let lows: Vec<f32> = frames.iter().map(|f| 10.0 * e(f, low.clone()).max(1e-12).log10()).collect();
    let low_med = pct(&mut lows.clone(), 0.5);
    let plosives = frames.iter().zip(&lows).filter(|(f, l)| **l > low_med + 15.0 && **l > 10.0 * e(f, mid.clone()).max(1e-12).log10()).count();
    let _ = hz;

    // reverb: typical time for level to fall 20 dB after a loud frame ends
    let fms = HOP as f32 / sr * 1000.0;
    let mut tails = vec![];
    let lv: Vec<f32> = frames.iter().map(|f| f.rms_db).collect();
    for i in 1..lv.len().saturating_sub(1) {
        if lv[i] > speech - 3.0 && lv[i + 1] < lv[i] - 3.0 {
            let mut j = i + 1;
            while j < lv.len() && lv[j] > lv[i] - 20.0 && lv[j] > floor + 3.0 && (j - i) < 40 { j += 1 }
            tails.push((j - i) as f32 * fms);
        }
    }
    let reverb_ms = pct(&mut tails, 0.5).max(0.0);
    let snr = speech - floor;
    if snr < 25.0 { notes.push(format!("Background noise is noticeable (only {snr:.0} dB below the voice)")) }
    if sibilance > -14.0 { notes.push("Strong 's' sounds (sibilance)".into()) }
    if plosives > 0 { notes.push(format!("{plosives} pops/plosives detected")) }
    if reverb_ms > 250.0 { notes.push(format!("Roomy / echoey recording (~{reverb_ms:.0} ms tail)")) }
    if bal[0] - TARGET[0] > 3.0 { notes.push("Boomy low end".into()) }
    if bal[2] - TARGET[2] < -3.0 { notes.push("A bit dull/muffled".into()) }
    Analysis { noise_floor_db: floor, speech_level_db: speech, snr_db: snr, lufs: fx::integrated_lufs(d), balance_db: bal, sibilance_db: sibilance,
        plosives, reverb_ms, notes, noise_profile: prof }
}

pub fn build_chain(a: &Analysis, s: Strength) -> Vec<Slot> {
    let k = s.k();
    let on = |fx| Slot { bypass: false, fx };
    let mut c = vec![];
    // 1) denoise: learned spectral profile (+ RNNoise on Strong)
    if a.snr_db < 45.0 && !a.noise_profile.is_empty() {
        let red = ((50.0 - a.snr_db).clamp(8.0, 30.0)) * k;
        c.push(on(Effect::NoiseReduction { reduction_db: red, sensitivity_db: 6.0, profile: a.noise_profile.clone() }));
    }
    c.push(Slot { bypass: s != Strength::Strong, fx: Effect::Denoise { mix: 0.8 } });
    // 2) EQ toward the voice target, high-pass higher if there are plosives
    let g = |i: usize| ((TARGET[i] - a.balance_db[i]) * 0.6 * k).clamp(-5.0, 5.0);
    c.push(on(Effect::Eq { hpf_hz: if a.plosives > 0 { 110.0 } else { 80.0 }, hpf_on: true,
        low_shelf: EqBand { freq: 150.0, gain_db: g(0), q: 0.7 },
        peaks: [EqBand { freq: 350.0, gain_db: g(1), q: 1.0 }, EqBand { freq: 3500.0, gain_db: g(2), q: 0.9 }, EqBand { freq: 1000.0, gain_db: 0.0, q: 1.0 }],
        high_shelf: EqBand { freq: 8000.0, gain_db: g(3), q: 0.7 } }));
    // 2b) gentle downward expander between phrases (keeps pauses clean after compression)
    if a.snr_db > 10.0 {
        c.push(on(Effect::Gate { threshold_db: a.noise_floor_db + (a.snr_db * 0.35).clamp(4.0, 15.0), floor_db: -(4.0 + 12.0 * k),
            attack_ms: 3.0, hold_ms: 40.0, release_ms: 80.0 }));
    }
    // 3) de-esser when sibilant
    if a.sibilance_db > -16.0 {
        c.push(on(Effect::DeEsser { freq_hz: 5500.0, threshold_db: a.speech_level_db - 6.0 + (1.0 - k) * 6.0, max_reduction_db: 4.0 + 6.0 * k }));
    }
    // 4) compression, 5) loudness, 6) limiter
    c.push(on(Effect::Compressor { threshold_db: a.speech_level_db - 4.0 - 6.0 * k, ratio: 1.5 + 2.5 * k, attack_ms: 4.0, release_ms: 150.0, makeup_db: 0.0 }));
    c.push(on(Effect::Loudness { target_lufs: -16.0, ceiling_db: Some(-1.0) }));
    c.push(on(Effect::Limiter { ceiling_db: -1.0, release_ms: 80.0 }));
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Voice-like: 140 Hz harmonic stack in 250 ms "syllables" with gaps, plus some hiss bursts and noise.
    fn noisy_voice(seed: &mut u32) -> (Decoded, Vec<bool>) {
        let sr = 48000usize; let n = sr * 8;
        let mut s = vec![0f32; n]; let mut voiced = vec![false; n];
        for i in 0..n {
            let t = i as f32 / sr as f32;
            let cyc = t % 0.4; // 250 ms on, 150 ms off
            if cyc < 0.25 {
                let env = (std::f32::consts::PI * cyc / 0.25).sin().powf(0.5);
                // harmonics with a gently falling spectrum and Schroeder phases (speech-like crest factor)
                let mut v = 0.0; for h in 1..30 { let hf = h as f32; v += (2.0 * std::f32::consts::PI * 140.0 * hf * t + std::f32::consts::PI * hf * hf / 29.0).sin() / hf.powf(0.8) }
                s[i] = 0.06 * env * v; voiced[i] = cyc > 0.03 && cyc < 0.22;
            }
            *seed ^= *seed << 13; *seed ^= *seed >> 17; *seed ^= *seed << 5;
            s[i] += (*seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.01; // noise ~ -45 dBFS rms
        }
        (Decoded { samples: s, channels: 1, sample_rate: sr as u32 }, voiced)
    }
    fn snr(d: &Decoded, voiced: &[bool]) -> f32 {
        let (mut sv, mut nv, mut sg, mut ng) = (0f64, 0usize, 0f64, 0usize);
        let sr = 48000usize;
        for (i, x) in d.samples.iter().enumerate().skip(sr) { // skip first second (filters settling)
            let cyc = (i as f32 / sr as f32) % 0.4;
            if voiced[i] { sv += (*x as f64).powi(2); nv += 1 } else if cyc > 0.28 && cyc < 0.37 { sg += (*x as f64).powi(2); ng += 1 }
        }
        10.0 * ((sv / nv as f64) / (sg / ng as f64)).log10() as f32
    }
    #[test] fn studio_sound_improves_snr_and_hits_loudness() {
        let (d, voiced) = noisy_voice(&mut 4242);
        let a = analyse(&d);
        assert!(a.snr_db > 15.0 && a.snr_db < 45.0, "snr est {}", a.snr_db);
        assert!(!a.noise_profile.is_empty());
        for s in [Strength::Subtle, Strength::Medium, Strength::Strong] {
            let mut out = Decoded { samples: d.samples.clone(), channels: 1, sample_rate: 48000 };
            fx::process_chain(&mut out, &build_chain(&a, s));
            let (before, after) = (snr(&d, &voiced), snr(&out, &voiced));
            let need = if s == Strength::Subtle { 3.0 } else { 6.0 };
            assert!(after > before + need, "{s:?}: SNR {before:.1} -> {after:.1}");
            eprintln!("{s:?}: SNR {before:.1} -> {after:.1} dB");
            let l = fx::integrated_lufs(&out).unwrap();
            assert!((l + 16.0).abs() < 0.5, "{s:?}: loudness {l:.2} LUFS");
            let pk = out.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(pk <= 10f32.powf(-1.0 / 20.0) + 1e-4);
        }
    }
}
