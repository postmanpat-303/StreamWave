//! Silence detection for one-click auto-trim of pauses / filler gaps.
use crate::waveform::Decoded;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SilenceRegion { pub start: f64, pub end: f64 }

/// Find track-time regions whose level stays below the threshold for at least `min_ms`.
/// `threshold_db = None` picks a threshold from the audio itself (10th-percentile floor + 8 dB),
/// which works without calibration for typical speech recordings.
/// `pad_ms` keeps a little audio on both sides of each cut so word tails aren't clipped.
pub fn detect_silence(d: &Decoded, threshold_db: Option<f64>, min_ms: f64, pad_ms: f64) -> Vec<SilenceRegion> {
    const HOP_SECS: f64 = 0.010; // 10 ms hops
    let ch = d.channels.max(1);
    let sr = d.sample_rate.max(1) as f64;
    let hop = (sr * HOP_SECS) as usize;
    if d.samples.len() < hop * ch { return vec![] }
    // per-hop mono RMS in dBFS
    let db: Vec<f64> = d.samples.chunks(hop * ch).map(|c| {
        let n = (c.len() / ch).max(1) as f64;
        let sum: f64 = c.iter().map(|&v| (v as f64) * (v as f64)).sum();
        20.0 * (sum / n).sqrt().max(1e-9).log10()
    }).collect();
    if db.is_empty() { return vec![] }
    let thr = threshold_db.unwrap_or_else(|| {
        let mut s = db.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        s[s.len() / 10] + 8.0
    });
    // auto-threshold guard: if the track has almost no dynamic range (constant tone/noise bed),
    // there is no "quiet" to distinguish — trimming would eat the whole file.
    if threshold_db.is_none() {
        let peak = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if peak - thr < -2.0 { return vec![] }
    }
    let min_hops = ((min_ms / (HOP_SECS * 1000.0)).ceil() as usize).max(1);
    let pad = (pad_ms / 1000.0).max(0.0);
    let mut out = vec![];
    let mut i = 0;
    while i < db.len() {
        if db[i] >= thr { i += 1; continue }
        let j = i;
        while i < db.len() && db[i] < thr { i += 1 }
        if i - j >= min_hops {
            out.push(SilenceRegion {
                start: ((j * hop) as f64 / sr - pad).max(0.0),
                end: ((i * hop) as f64 / sr + pad).min(d.samples.len() as f64 / ch as f64 / sr),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn synth(sections: &[(f64, f64)], sr: u32) -> Decoded { // (seconds, amplitude) pairs
        let mut samples = vec![];
        let mut ph = 0f32;
        for &(secs, amp) in sections {
            for _ in 0..(secs * sr as f64) as usize {
                samples.push((ph * 6.2831).sin() * amp as f32);
                ph += 440.0 / sr as f32;
                if ph > 1.0 { ph -= 1.0 }
            }
        }
        Decoded { samples, channels: 1, sample_rate: sr }
    }
    #[test] fn finds_only_long_enough_silence() {
        // 1 s tone, 0.5 s silence, 1 s tone; 50 ms silence must be ignored
        let d = synth(&[(1.0, 0.5), (0.5, 0.0), (0.05, 0.0), (1.0, 0.5)], 48000);
        let r = detect_silence(&d, Some(-40.0), 200.0, 50.0);
        assert_eq!(r.len(), 1);
        assert!((r[0].start - 0.95).abs() < 0.03, "start {}", r[0].start);
        assert!((r[0].end - 1.60).abs() < 0.03, "end {}", r[0].end);
    }
    #[test] fn auto_threshold_finds_the_pause() {
        let d = synth(&[(1.0, 0.4), (0.8, 0.0), (1.0, 0.4)], 48000);
        let r = detect_silence(&d, None, 300.0, 0.0);
        assert_eq!(r.len(), 1);
        assert!((r[0].start - 1.0).abs() < 0.05);
    }
    #[test] fn nothing_found_in_constant_tone() {
        let d = synth(&[(2.0, 0.5)], 48000);
        assert!(detect_silence(&d, Some(-40.0), 200.0, 50.0).is_empty());
        assert!(detect_silence(&d, None, 200.0, 50.0).is_empty());
    }
}
