//! "Never miss a take" rolling pre-record buffer and live mic check.
//! Both run off the realtime thread (fed from a lock-free ring by the engine loop).
use serde::Serialize;
use std::collections::VecDeque;

/// Keeps the most recent `cap` interleaved samples.
pub struct PreRoll { buf: VecDeque<f32>, cap: usize, pub channels: u16, pub sample_rate: u32 }

impl PreRoll {
    pub fn new(secs: f32, channels: u16, sample_rate: u32) -> Self {
        let cap = (secs.clamp(0.0, 60.0) * sample_rate as f32) as usize * channels as usize;
        Self { buf: VecDeque::with_capacity(cap), cap, channels, sample_rate }
    }
    pub fn push(&mut self, s: &[f32]) {
        if self.cap == 0 { return }
        let s = if s.len() > self.cap { &s[s.len() - self.cap..] } else { s };
        let over = (self.buf.len() + s.len()).saturating_sub(self.cap);
        self.buf.drain(..over);
        self.buf.extend(s.iter().copied());
    }
    /// Last `secs` seconds (whole frames), oldest first.
    pub fn last(&self, secs: f32) -> Vec<f32> {
        let ch = self.channels.max(1) as usize;
        let want = ((secs.max(0.0) * self.sample_rate as f32) as usize * ch).min(self.buf.len());
        let want = want - want % ch;
        self.buf.iter().skip(self.buf.len() - want).copied().collect()
    }
    #[allow(dead_code)]
    pub fn secs(&self) -> f32 { self.buf.len() as f32 / (self.channels.max(1) as f32 * self.sample_rate.max(1) as f32) }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, Hash, Copy)]
pub enum MicIssue { Clipping, TooQuiet, Silence, DcOffset, Hum, OneChannelSilent }

#[derive(Serialize, Clone, Debug)]
pub struct MicReport { pub issues: Vec<MicIssue>, pub rms_db: f32, pub peak_db: f32, /// per-channel peak (dBFS)
    pub channel_peak_db: Vec<f32> }

/// Analyses ~1 s windows of interleaved audio, every channel.
pub struct MicCheck { ch: usize, sr: f32, n: usize, chans: Vec<ChStat>, silent_windows: u32 }

struct ChStat { sum: f64, sq: f64, peak: f32, clips: usize, g: [Goertzel; 4] }

#[derive(Clone, Copy, Default)]
struct Goertzel { coeff: f32, s1: f32, s2: f32 }
impl Goertzel {
    fn new(f: f32, sr: f32) -> Self { Self { coeff: 2.0 * (2.0 * std::f32::consts::PI * f / sr).cos(), s1: 0.0, s2: 0.0 } }
    #[inline] fn push(&mut self, x: f32) { let s = x + self.coeff * self.s1 - self.s2; self.s2 = self.s1; self.s1 = s; }
    fn power(&mut self) -> f32 { let p = self.s1 * self.s1 + self.s2 * self.s2 - self.coeff * self.s1 * self.s2; self.s1 = 0.0; self.s2 = 0.0; p }
}

impl MicCheck {
    pub fn new(channels: u16, sample_rate: u32) -> Self {
        let sr = sample_rate as f32; let ch = channels.max(1) as usize;
        let chans = (0..ch).map(|_| ChStat { sum: 0.0, sq: 0.0, peak: 0.0, clips: 0,
            g: [Goertzel::new(50.0, sr), Goertzel::new(100.0, sr), Goertzel::new(60.0, sr), Goertzel::new(120.0, sr)] }).collect();
        Self { ch, sr, n: 0, chans, silent_windows: 0 }
    }
    /// Feed samples; returns a report each time a full ~1 s window completes.
    pub fn push(&mut self, s: &[f32]) -> Option<MicReport> {
        let mut out = None;
        for f in s.chunks_exact(self.ch) {
            for (c, &x) in f.iter().enumerate() {
                let st = &mut self.chans[c];
                let x = if x.is_finite() { x } else { 0.0 };
                st.sum += x as f64; st.sq += (x * x) as f64; st.peak = st.peak.max(x.abs());
                if x.abs() >= 0.999 { st.clips += 1 }
                for g in st.g.iter_mut() { g.push(x) }
            }
            self.n += 1;
            if self.n as f32 >= self.sr { out = Some(self.finish()) }
        }
        out
    }
    fn finish(&mut self) -> MicReport {
        let n = self.n as f64;
        let db = |v: f32| 20.0 * v.max(1e-9).log10();
        let amp = |p: f32| 2.0 * p.max(0.0).sqrt() / n as f32; // Goertzel power -> amplitude
        let mut issues = vec![];
        let (mut any_clip, mut dc, mut hum) = (false, false, false);
        let mut rms_best = 0f32; let mut peak_best = 0f32; let mut peaks = vec![];
        let silent_th = db_to(-75.0);
        for st in self.chans.iter_mut() {
            let mean = (st.sum / n) as f32;
            let rms_ac = ((st.sq / n) as f32 - mean * mean).max(0.0).sqrt();
            let h = amp(st.g[0].power()).max(amp(st.g[1].power())).max(amp(st.g[2].power())).max(amp(st.g[3].power())) / 2f32.sqrt();
            if st.clips >= 3 { any_clip = true }
            if mean.abs() > 0.02 { dc = true }
            if rms_ac > db_to(-60.0) && h > 0.5 * rms_ac { hum = true }
            rms_best = rms_best.max(rms_ac); peak_best = peak_best.max(st.peak); peaks.push(db(st.peak));
            st.sum = 0.0; st.sq = 0.0; st.peak = 0.0; st.clips = 0;
        }
        if any_clip { issues.push(MicIssue::Clipping) }
        if peak_best < silent_th { self.silent_windows += 1 } else { self.silent_windows = 0 }
        if self.silent_windows >= 3 { issues.push(MicIssue::Silence) }
        else if db(rms_best) < -50.0 && peak_best >= silent_th { issues.push(MicIssue::TooQuiet) }
        // e.g. a mono mic on input 1 of a stereo interface: one side is dead
        if self.ch >= 2 && peak_best > db_to(-50.0) && peaks.iter().any(|p| *p < -75.0) { issues.push(MicIssue::OneChannelSilent) }
        if dc { issues.push(MicIssue::DcOffset) }
        if hum { issues.push(MicIssue::Hum) }
        self.n = 0;
        MicReport { issues, rms_db: db(rms_best), peak_db: db(peak_best), channel_peak_db: peaks }
    }
}
fn db_to(db: f32) -> f32 { 10f32.powf(db / 20.0) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn preroll_keeps_last_n_seconds() {
        let mut p = PreRoll::new(2.0, 2, 10); // cap = 40 samples
        let data: Vec<f32> = (0..100).map(|i| i as f32).collect();
        for c in data.chunks(7) { p.push(c) }
        assert!((p.secs() - 2.0).abs() < 1e-6);
        assert_eq!(p.last(10.0), (60..100).map(|i| i as f32).collect::<Vec<_>>());
        assert_eq!(p.last(0.5), vec![90.0, 91.0, 92.0, 93.0, 94.0, 95.0, 96.0, 97.0, 98.0, 99.0]);
        p.push(&(0..1000).map(|i| i as f32).collect::<Vec<_>>()); // bigger than cap
        assert_eq!(p.last(60.0).first(), Some(&960.0));
        let mut z = PreRoll::new(0.0, 1, 48000); z.push(&[1.0, 2.0]); assert!(z.last(5.0).is_empty());
    }
    fn tone(f: f32, amp: f32, dc: f32) -> Vec<f32> { (0..48000).map(|i| dc + amp * (2.0 * std::f32::consts::PI * f * i as f32 / 48000.0).sin()).collect() }
    #[test] fn mic_check_detects_issues() {
        let mut m = MicCheck::new(1, 48000);
        assert!(m.push(&tone(440.0, 0.3, 0.0)).unwrap().issues.is_empty());
        assert!(m.push(&tone(440.0, 1.5, 0.0).iter().map(|v| v.clamp(-1.0, 1.0)).collect::<Vec<_>>()).unwrap().issues.contains(&MicIssue::Clipping));
        assert_eq!(m.push(&tone(440.0, 0.001, 0.0)).unwrap().issues, vec![MicIssue::TooQuiet]);
        assert!(m.push(&tone(440.0, 0.2, 0.1)).unwrap().issues.contains(&MicIssue::DcOffset));
        assert!(m.push(&tone(50.0, 0.1, 0.0)).unwrap().issues.contains(&MicIssue::Hum));
        assert!(!m.push(&tone(1000.0, 0.1, 0.0)).unwrap().issues.contains(&MicIssue::Hum));
        let z = vec![0f32; 48000];
        m.push(&z); m.push(&z); assert!(m.push(&z).unwrap().issues.contains(&MicIssue::Silence));
    }
    #[test] fn mic_check_looks_at_every_channel() {
        let left = tone(440.0, 0.3, 0.0);
        let st = |l: &[f32], r: &[f32]| l.iter().zip(r).flat_map(|(a, b)| [*a, *b]).collect::<Vec<f32>>();
        let mut m = MicCheck::new(2, 48000);
        // clipping only on the right channel must still be caught
        let clipped: Vec<f32> = tone(440.0, 1.5, 0.0).iter().map(|v| v.clamp(-1.0, 1.0)).collect();
        assert!(m.push(&st(&left, &clipped)).unwrap().issues.contains(&MicIssue::Clipping));
        // dead right channel
        let r = m.push(&st(&left, &vec![0.0; 48000])).unwrap();
        assert!(r.issues.contains(&MicIssue::OneChannelSilent), "{:?}", r.issues);
        assert_eq!(r.channel_peak_db.len(), 2);
        // hum only on the right
        assert!(m.push(&st(&left, &tone(50.0, 0.1, 0.0))).unwrap().issues.contains(&MicIssue::Hum));
        // healthy stereo
        assert!(m.push(&st(&left, &left)).unwrap().issues.is_empty());
    }
}
