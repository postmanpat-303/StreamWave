//! Render an edited clip list (non-destructive) to a new WAV. Sources are only read.
use crate::waveform::{decode_file, Decoded};
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

#[derive(Deserialize, Clone, Debug)]
pub struct ClipSpec {
    pub src: String, pub start: f64, pub end: f64,
    /// Per-clip gain in dB (applied while rendering; original files untouched).
    #[serde(default)] pub gain_db: Option<f64>,
    /// Raised-cosine fade lengths in milliseconds.
    #[serde(default)] pub fade_in_ms: Option<f64>,
    #[serde(default)] pub fade_out_ms: Option<f64>,
}

/// Decoded sources, most-recently-used first. Bounded so long sessions with many big files
/// don't eat all RAM (a 1 h stereo 48 kHz file is ~1.4 GB as f32).
#[derive(Default)]
pub struct DecodeCache(Mutex<Vec<(String, Arc<Decoded>)>>);
const CACHE_BYTES: usize = 1_500_000_000;

impl DecodeCache {
    pub fn get(&self, path: &str) -> anyhow::Result<Arc<Decoded>> {
        {
            let mut c = self.0.lock();
            if let Some(i) = c.iter().position(|(p, _)| p == path) { let e = c.remove(i); let d = e.1.clone(); c.insert(0, e); return Ok(d) }
        }
        // decode outside the lock so other tracks aren't blocked
        let d = Arc::new(decode_file(Path::new(path))?);
        let mut c = self.0.lock();
        c.retain(|(p, _)| p != path);
        c.insert(0, (path.to_string(), d.clone()));
        let mut bytes = 0usize;
        c.retain(|(_, x)| { bytes += x.samples.len() * 4; bytes <= CACHE_BYTES || Arc::ptr_eq(x, &d) });
        Ok(d)
    }
}

/// Mix-free concatenation: output rate/channels follow the first clip's source.
pub fn render(cache: &DecodeCache, clips: &[ClipSpec]) -> anyhow::Result<Decoded> {
    anyhow::ensure!(!clips.is_empty(), "nothing to render");
    let first = cache.get(&clips[0].src)?;
    let (rate, ch) = (first.sample_rate, first.channels);
    let mut out = Vec::new();
    // clip lengths are taken from the running timeline position, so splitting a clip anywhere
    // renders exactly the same samples as the unsplit clip (no dropped/doubled sample at the cut)
    let mut tl = 0.0f64;
    for c in clips {
        let d = cache.get(&c.src)?;
        let frames = d.samples.len() / d.channels;
        let ratio = d.sample_rate as f64 / rate as f64;
        let len = (c.end - c.start).max(0.0);
        let g0 = (tl * rate as f64).round();
        let n = ((tl + len) * rate as f64).round() as usize - g0 as usize;
        // output frame g reads source position start + (g - tl*rate) * ratio, same as without the cut
        let s0 = c.start * d.sample_rate as f64 + (g0 - tl * rate as f64) * ratio;
        tl += len;
        // per-clip gain + raised-cosine fades
        let gain = c.gain_db.map(|db| 10f64.powf(db / 20.0) as f32).unwrap_or(1.0);
        let fi = c.fade_in_ms.map(|ms| ((ms.max(0.0) / 1000.0) * rate as f64).round() as usize);
        let fo = c.fade_out_ms.map(|ms| ((ms.max(0.0) / 1000.0) * rate as f64).round() as usize);
        for f in 0..n {
            let pos = s0 + f as f64 * ratio;
            let i = pos as usize;
            if i >= frames { break }
            let j = (i + 1).min(frames - 1); let t = (pos - i as f64) as f32;
            let mut g = gain;
            if let Some(fi) = fi { if fi > 0 && f < fi { g *= (0.5 * (1.0 - (std::f64::consts::PI * f as f64 / fi as f64).cos())) as f32 } }
            if let Some(fo) = fo { if fo > 0 && f + fo >= n { let k = n - 1 - f; g *= (0.5 * (1.0 - (std::f64::consts::PI * k as f64 / fo as f64).cos())) as f32 } }
            for k in 0..ch {
                let sc = k % d.channels;
                let a = d.samples[i * d.channels + sc]; let b = d.samples[j * d.channels + sc];
                out.push((a + (b - a) * t) * g);
            }
        }
    }
    Ok(Decoded { samples: out, channels: ch, sample_rate: rate })
}

#[derive(Deserialize, Clone, Debug)]
pub struct TrackSpec { pub clips: Vec<ClipSpec>, #[serde(default)] pub fx: Vec<crate::fx::Slot>, #[serde(default = "one")] pub gain: f32 }
fn one() -> f32 { 1.0 }

/// Render each track with its effects and sum them (rate/channels of the first track).
pub fn render_mix(cache: &DecodeCache, tracks: &[TrackSpec]) -> anyhow::Result<Decoded> {
    let mut out: Option<Decoded> = None;
    for t in tracks.iter().filter(|t| !t.clips.is_empty()) {
        let mut d = render(cache, &t.clips)?;
        crate::fx::process_chain(&mut d, &t.fx);
        match &mut out {
            None => { if t.gain != 1.0 { d.samples.iter_mut().for_each(|s| *s *= t.gain) } out = Some(d) }
            Some(o) => {
                let d = crate::export::conform(&d, Some(o.channels as u16), Some(o.sample_rate));
                if d.samples.len() > o.samples.len() { o.samples.resize(d.samples.len(), 0.0) }
                for (a, b) in o.samples.iter_mut().zip(&d.samples) { *a += b * t.gain }
            }
        }
    }
    out.ok_or_else(|| anyhow::anyhow!("nothing to mix"))
}

pub fn write_wav(d: &Decoded, path: &Path) -> anyhow::Result<()> {
    let spec = hound::WavSpec { channels: d.channels as u16, sample_rate: d.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let tmp = path.with_extension("tmp.wav");
    let mut w = hound::WavWriter::create(&tmp, spec)?;
    for s in &d.samples { w.write_sample(*s)?; }
    w.finalize()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_clips_without_touching_source() {
        let dir = std::env::temp_dir().join("tw_render_test"); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 1000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&src, spec).unwrap();
        for i in 0..1000 { w.write_sample(i as f32 / 1000.0).unwrap(); }
        w.finalize().unwrap();
        let before = std::fs::read(&src).unwrap();
        let cache = DecodeCache::default();
        let p = src.to_string_lossy().to_string();
        // keep 0..0.2 and 0.5..0.6 => 300 samples
        let d = render(&cache, &[ClipSpec { src: p.clone(), start: 0.0, end: 0.2, gain_db: None, fade_in_ms: None, fade_out_ms: None }, ClipSpec { src: p, start: 0.5, end: 0.6, gain_db: None, fade_in_ms: None, fade_out_ms: None }]).unwrap();
        assert_eq!(d.samples.len(), 300);
        assert!((d.samples[200] - 0.5).abs() < 1e-3);
        let out = dir.join("out.wav"); write_wav(&d, &out).unwrap();
        assert_eq!(hound::WavReader::open(&out).unwrap().len(), 300);
        assert_eq!(std::fs::read(&src).unwrap(), before);
    }
    #[test]
    fn a_split_clip_renders_exactly_like_the_whole_clip() {
        let dir = std::env::temp_dir().join("tw_render_split"); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("s.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 8000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&src, spec).unwrap();
        for i in 0..16000 { let x = (i as f32 * 0.37).sin(); w.write_sample(x).unwrap(); w.write_sample(-x).unwrap(); }
        w.finalize().unwrap();
        let p = src.to_string_lossy().to_string(); let cache = DecodeCache::default();
        let c = |a: f64, b: f64| ClipSpec { src: p.clone(), start: a, end: b, gain_db: None, fade_in_ms: None, fade_out_ms: None };
        let whole = render(&cache, &[c(0.1, 1.9)]).unwrap();
        for cuts in [vec![0.33371], vec![0.2, 0.70001, 1.234567], vec![1.0 / 3.0, 2.0 / 3.0]] {
            let mut pts = vec![0.1]; pts.extend(cuts.iter().copied()); pts.push(1.9);
            let parts: Vec<ClipSpec> = pts.windows(2).map(|w| c(w[0], w[1])).collect();
            let split = render(&cache, &parts).unwrap();
            assert_eq!(split.samples.len(), whole.samples.len(), "{cuts:?}");
            let diff = split.samples.iter().zip(&whole.samples).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(diff < 1e-5, "{cuts:?}: {diff}");
        }
    }
    #[test] fn applies_clip_gain_and_fades() {
        let dir = std::env::temp_dir().join("tw_render_fades"); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 1000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&src, spec).unwrap();
        for _ in 0..1000 { w.write_sample(1.0f32).unwrap(); }
        w.finalize().unwrap();
        let cache = DecodeCache::default();
        let p = src.to_string_lossy().to_string();
        // −6.02 dB ≈ half amplitude
        let d = render(&cache, &[ClipSpec { src: p.clone(), start: 0.0, end: 0.5, gain_db: Some(-6.0206), fade_in_ms: None, fade_out_ms: None }]).unwrap();
        assert!((d.samples[400] - 0.5).abs() < 1e-3);
        // 500 ms fade-in over a 500 ms clip: starts at 0, ends at full scale
        let d = render(&cache, &[ClipSpec { src: p, start: 0.0, end: 0.5, gain_db: None, fade_in_ms: Some(500.0), fade_out_ms: None }]).unwrap();
        assert!(d.samples[0].abs() < 1e-6);
        assert!((d.samples[499] - 1.0).abs() < 1e-3);
    }
}
