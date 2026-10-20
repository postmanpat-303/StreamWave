//! Render an edited clip list (non-destructive) to a new WAV. Sources are only read.
use crate::waveform::{decode_file, Decoded};
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct ClipSpec {
    /// Source file (empty for a gap).
    pub src: String, pub start: f64, pub end: f64,
    /// Per-clip gain in dB (applied while rendering; original files untouched).
    #[serde(default)] pub gain_db: Option<f64>,
    /// Raised-cosine fade lengths in milliseconds.
    #[serde(default)] pub fade_in_ms: Option<f64>,
    #[serde(default)] pub fade_out_ms: Option<f64>,
    /// Silence of `end - start` seconds (positions clips on the timeline; no source).
    #[serde(default)] pub gap: bool,
    /// The clip's own effect stack ("Layering Effects"), run before clip gain/fades.
    #[serde(default)] pub fx: Vec<crate::fx::Slot>,
    /// Shape of this clip's own fade-in/out: None = raised cosine (S-curve, the pre-0.11 sound), "linear", "equal_power".
    #[serde(default)] pub fade_curve: Option<String>,
    /// Crossfade with the PREVIOUS clip (both audio), centred on the cut: the previous clip plays on into its source
    /// beyond its end and this clip starts earlier from before its start ("handles"), so timeline positions stay put.
    #[serde(default)] pub xfade_ms: Option<f64>,
    /// "linear" (gains sum to 1) or "equal_power" (powers sum to 1; default).
    #[serde(default)] pub xfade_curve: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Curve { SCurve, Linear, EqualPower }
impl Curve {
    fn parse(s: &Option<String>, default: Curve) -> Curve {
        match s.as_deref() { Some("linear") => Curve::Linear, Some("equal_power") => Curve::EqualPower, Some("s_curve") => Curve::SCurve, _ => default }
    }
    /// Fade-in gain at x in [0, 1]; the matching fade-out is `gain(1 - x)`.
    pub fn gain(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self { Curve::SCurve => 0.5 * (1.0 - (std::f64::consts::PI * x).cos()), Curve::Linear => x, Curve::EqualPower => (x * std::f64::consts::FRAC_PI_2).sin() }
    }
    /// (fade-in, fade-out) gains for frame k of a 2h-frame crossfade. Linear sums to exactly 1, equal power to power 1.
    pub fn xfade(self, k: usize, h2: usize) -> (f32, f32) {
        let t = (k as f64 + 0.5) / h2 as f64;
        match self {
            Curve::Linear => (t as f32, (1.0 - t) as f32),
            Curve::EqualPower => ((t * std::f64::consts::FRAC_PI_2).sin() as f32, (t * std::f64::consts::FRAC_PI_2).cos() as f32),
            Curve::SCurve => { let a = self.gain(t); (a as f32, (1.0 - a) as f32) }
        }
    }
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

/// Mix-free concatenation: output rate/channels follow the first audio (non-gap) clip's source.
///
/// Per clip: read the source (linear interpolation for other rates) -> the clip's effect stack (same
/// `fx::Chain` as track effects) -> clip gain and fades. Gaps are zeros. Clip *i* covers output frames
/// `round(tl*rate) .. round((tl+len)*rate)` of the running timeline position `tl`, and every clip fills its
/// frames exactly (zero-padded at a source's end), so positions are sample-exact however clips are arranged.
///
/// Crossfades (`xfade_ms` on the incoming clip): over the `2h` frames centred on the cut, the outgoing clip continues
/// into its source tail and the incoming clip starts `h` frames early from its source head; the two are summed with
/// complementary curves. `h` is clamped to the handles the sources really have and to half of either clip, so a
/// crossfade can never read past a source or create a dip. Clips without crossfades render exactly as before.
pub fn render(cache: &DecodeCache, clips: &[ClipSpec]) -> anyhow::Result<Decoded> {
    anyhow::ensure!(!clips.is_empty(), "nothing to render");
    let (rate, ch) = match clips.iter().find(|c| !c.gap) {
        Some(c) => { let d = cache.get(&c.src)?; (d.sample_rate, d.channels.max(1)) }
        None => (48000, 2), // only silence (e.g. a selection inside a gap)
    };
    let r = rate as f64;
    // clip lengths are taken from the running timeline position, so splitting a clip anywhere
    // renders exactly the same samples as the unsplit clip (no dropped/doubled sample at the cut)
    let mut g = Vec::with_capacity(clips.len() + 1); // first output frame of each clip, + the end
    let mut tl = 0.0f64;
    for c in clips { g.push((tl * r).round() as usize); tl += (c.end - c.start).max(0.0); }
    g.push((tl * r).round() as usize);
    let total = g[clips.len()];
    // crossfade half-lengths (output frames) at the head of each clip, clamped to the real handles
    let mut hs = vec![0usize; clips.len()];
    for i in 1..clips.len() {
        let (p, c) = (&clips[i - 1], &clips[i]);
        let want = c.xfade_ms.unwrap_or(0.0); if want <= 0.0 || c.gap || p.gap { continue }
        let (dp, dc) = (cache.get(&p.src)?, cache.get(&c.src)?);
        let tail = ((dp.samples.len() / dp.channels.max(1)) as f64 / dp.sample_rate as f64 - p.end).max(0.0); // seconds after p's end
        let head = c.start.max(0.0);                                                                         // seconds before c's start
        let h = ((want / 2000.0) * r).round().min((tail * r).floor()).min((head * r).floor()) as usize;
        hs[i] = h.min((g[i + 1] - g[i]) / 2).min((g[i] - g[i - 1]) / 2);
    }
    let mut out = vec![0.0f32; total * ch];
    let mut seg: Vec<f32> = Vec::new();
    let mut tl = 0.0f64;
    for (ci, c) in clips.iter().enumerate() {
        let len = (c.end - c.start).max(0.0);
        let g0 = (tl * r).round();
        let n = g[ci + 1] - g[ci];
        let (hin, hout) = (hs[ci], hs.get(ci + 1).copied().unwrap_or(0));
        if c.gap { tl += len; continue }
        let d = cache.get(&c.src)?;
        let frames = d.samples.len() / d.channels;
        let ratio = d.sample_rate as f64 / r;
        // output frame g reads source position start + (g - tl*rate) * ratio, same as without the cut
        let s0 = c.start * d.sample_rate as f64 + (g0 - tl * r) * ratio;
        tl += len;
        let m = hin + n + hout;
        seg.clear(); seg.resize(m * ch, 0.0); // a clip always fills its slot (+ handles) exactly; zeros past the source
        for f in 0..m {
            let pos = s0 + (f as f64 - hin as f64) * ratio;
            if pos <= -1.0 { continue } // before the source (only in a head handle); (-1, 0) reads frame 0 as before
            let i = pos as usize; // saturates to 0 for -1 < pos < 0, exactly like the pre-crossfade renderer
            if i >= frames || frames == 0 { break }
            let j = (i + 1).min(frames - 1); let t = (pos - i as f64) as f32;
            for k in 0..ch {
                let sc = k % d.channels;
                let a = d.samples[i * d.channels + sc]; let b = d.samples[j * d.channels + sc];
                seg[f * ch + k] = a + (b - a) * t;
            }
        }
        // 1) the clip's own effects (Layering Effects): the same streaming chain as track effects
        if c.fx.iter().any(|s| !s.bypass) {
            let mut sd = Decoded { samples: std::mem::take(&mut seg), channels: ch, sample_rate: rate };
            crate::fx::process_chain(&mut sd, &c.fx);
            seg = sd.samples;
        }
        // 2) per-clip gain + fades (own fades on edges without a crossfade) + crossfade curves on the handles
        let gain = c.gain_db.map(|db| 10f64.powf(db / 20.0) as f32).unwrap_or(1.0);
        let fi = if hin > 0 { None } else { c.fade_in_ms.map(|ms| ((ms.max(0.0) / 1000.0) * r).round() as usize) };
        let fo = if hout > 0 { None } else { c.fade_out_ms.map(|ms| ((ms.max(0.0) / 1000.0) * r).round() as usize) };
        let fc = Curve::parse(&c.fade_curve, Curve::SCurve);
        let xin = Curve::parse(&c.xfade_curve, Curve::EqualPower);
        let xout = clips.get(ci + 1).map(|x| Curve::parse(&x.xfade_curve, Curve::EqualPower)).unwrap_or(Curve::EqualPower);
        if gain != 1.0 || fi.is_some_and(|x| x > 0) || fo.is_some_and(|x| x > 0) || hin > 0 || hout > 0 {
            for (f, fr) in seg.chunks_mut(ch).enumerate() {
                let mut gg = gain;
                if hin > 0 && f < 2 * hin { gg *= xin.xfade(f, 2 * hin).0 }
                if hout > 0 && f >= hin + n - hout { gg *= xout.xfade(f - (hin + n - hout), 2 * hout).1 }
                let fc_core = f.wrapping_sub(hin); // frame index inside the clip itself
                if let Some(fi) = fi { if fi > 0 && fc_core < fi { gg *= fc.gain(fc_core as f64 / fi as f64) as f32 } }
                if let Some(fo) = fo { if fo > 0 && fc_core < n && fc_core + fo >= n { let k = n - 1 - fc_core; gg *= fc.gain(k as f64 / fo as f64) as f32 } }
                fr.iter_mut().for_each(|v| *v *= gg);
            }
        }
        // 3) overlap-add at the clip's position (handles reach into the neighbours' slots)
        let at = (g[ci] - hin) * ch;
        for (o, v) in out[at..at + seg.len()].iter_mut().zip(&seg) { *o += *v }
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
        let d = render(&cache, &[ClipSpec { src: p.clone(), start: 0.0, end: 0.2, ..Default::default() }, ClipSpec { src: p, start: 0.5, end: 0.6, ..Default::default() }]).unwrap();
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
        let c = |a: f64, b: f64| ClipSpec { src: p.clone(), start: a, end: b, ..Default::default() };
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
        let d = render(&cache, &[ClipSpec { src: p.clone(), start: 0.0, end: 0.5, gain_db: Some(-6.0206), ..Default::default() }]).unwrap();
        assert!((d.samples[400] - 0.5).abs() < 1e-3);
        // 500 ms fade-in over a 500 ms clip: starts at 0, ends at full scale
        let d = render(&cache, &[ClipSpec { src: p, start: 0.0, end: 0.5, fade_in_ms: Some(500.0), ..Default::default() }]).unwrap();
        assert!(d.samples[0].abs() < 1e-6);
        assert!((d.samples[499] - 1.0).abs() < 1e-3);
    }

    fn wav(dir: &std::path::Path, name: &str, rate: u32, ch: u16, frames: usize, f: impl Fn(usize) -> f32) -> String {
        let p = dir.join(name);
        let spec = hound::WavSpec { channels: ch, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for i in 0..frames { for _ in 0..ch { w.write_sample(f(i)).unwrap(); } }
        w.finalize().unwrap(); p.to_string_lossy().to_string()
    }
    fn tmp(name: &str) -> std::path::PathBuf { let d = std::env::temp_dir().join(format!("sw_{name}_{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap(); d }
    fn gap(len: f64) -> ClipSpec { ClipSpec { gap: true, end: len, ..Default::default() } }
    fn clip(src: &str, a: f64, b: f64) -> ClipSpec { ClipSpec { src: src.into(), start: a, end: b, ..Default::default() } }
    fn slot(fx: crate::fx::Effect) -> crate::fx::Slot { crate::fx::Slot { bypass: false, fx } }

    /// A moved clip (gap before it) starts on exactly the sample its timeline position says.
    #[test] fn gaps_place_clips_on_exact_samples() {
        let d = tmp("gap"); let cache = DecodeCache::default();
        let src = wav(&d, "ones.wav", 48000, 2, 4800, |_| 1.0);
        for k in [1usize, 12345, 48000, 480_007] {
            let out = render(&cache, &[gap(k as f64 / 48000.0), clip(&src, 0.0, 0.1)]).unwrap();
            assert_eq!(out.samples.len(), (k + 4800) * 2, "k={k}");
            assert_eq!(out.samples[(k - 1) * 2], 0.0); assert_eq!(out.samples[k * 2], 1.0);
        }
        // positions built from many float pieces (as repeated moves produce) still land exactly
        let pieces: Vec<ClipSpec> = (0..37).map(|i| gap((i * 131 + 7) as f64 / 48000.0)).collect();
        let k: usize = (0..37).map(|i| i * 131 + 7).sum();
        let mut cl = pieces; cl.push(clip(&src, 0.0, 0.05));
        let out = render(&cache, &cl).unwrap();
        assert_eq!(out.samples[(k - 1) * 2], 0.0); assert_eq!(out.samples[k * 2], 1.0); assert_eq!(out.samples.len(), (k + 2400) * 2);
        // a selection that is only silence renders as silence
        assert!(render(&cache, &[gap(0.5)]).unwrap().samples.iter().all(|v| *v == 0.0));
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Sequential multi-file import with mixed rates/channels: files butt together frame-exactly at the track rate.
    #[test] fn sequential_files_with_mixed_rates_are_contiguous() {
        let d = tmp("seq"); let cache = DecodeCache::default();
        let a = wav(&d, "a44.wav", 44100, 1, 44100, |_| 0.25);       // 1 s mono 44.1k
        let b = wav(&d, "b48.wav", 48000, 2, 24000, |_| 0.5);         // 0.5 s stereo 48k
        let c = wav(&d, "c96.wav", 96000, 1, 9601, |_| -0.5);         // 9601 frames at 96k (non-integer at 44.1k)
        let out = render(&cache, &[clip(&a, 0.0, 1.0), clip(&b, 0.0, 0.5), clip(&c, 0.0, 9601.0 / 96000.0)]).unwrap();
        assert_eq!((out.sample_rate, out.channels), (44100, 1));
        let n_c = ((1.5 + 9601.0 / 96000.0) * 44100.0f64).round() as usize - 66150;
        assert_eq!(out.samples.len(), 44100 + 22050 + n_c);
        assert_eq!(out.samples[44099], 0.25); assert_eq!(out.samples[44100], 0.5); // no gap, no overlap
        assert_eq!(out.samples[66149], 0.5); assert_eq!(out.samples[66150], -0.5);
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Clip effects run in stack order on that clip only, before clip gain/fades, using the track-effect engine.
    #[test] fn clip_effects_run_in_order_before_gain() {
        use crate::fx::Effect;
        let d = tmp("cfx"); let cache = DecodeCache::default();
        let src = wav(&d, "s.wav", 48000, 1, 48000, |i| 0.5 * (i as f32 * 0.05).sin());
        let lim = Effect::Limiter { ceiling_db: -12.0, release_ms: 50.0 };
        let up = Effect::Gain { db: 12.0 };
        let peak = |v: &[f32]| v.iter().fold(0f32, |m, x| m.max(x.abs()));
        let mut a = clip(&src, 0.0, 1.0); a.fx = vec![slot(up.clone()), slot(lim.clone())]; // boost, then limit to -12 dBFS
        let mut b = clip(&src, 0.0, 1.0); b.fx = vec![slot(lim.clone()), slot(up.clone())]; // limit, then boost
        let (ra, rb) = (render(&cache, &[a.clone()]).unwrap(), render(&cache, &[b]).unwrap());
        assert!(peak(&ra.samples) <= 0.2512 + 1e-4); assert!(peak(&rb.samples) > 0.9);
        // exactly what the fx engine gives for the clip on its own
        let mut manual = render(&cache, &[clip(&src, 0.0, 1.0)]).unwrap(); crate::fx::process_chain(&mut manual, &a.fx);
        assert_eq!(ra.samples, manual.samples);
        // then clip gain: -6.02 dB on top halves it
        let mut g = a.clone(); g.gain_db = Some(-6.0206);
        let rg = render(&cache, &[g]).unwrap();
        assert!(rg.samples.iter().zip(&ra.samples).all(|(x, y)| (x - y * 0.5).abs() < 1e-4));
        // bypassed layers do nothing; other clips on the track are untouched
        let mut by = a.clone(); by.fx.iter_mut().for_each(|s| s.bypass = true);
        assert_eq!(render(&cache, &[by]).unwrap().samples, render(&cache, &[clip(&src, 0.0, 1.0)]).unwrap().samples);
        let two = render(&cache, &[a, clip(&src, 0.0, 1.0)]).unwrap();
        assert_eq!(&two.samples[48000..], &render(&cache, &[clip(&src, 0.0, 1.0)]).unwrap().samples[..]);
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Export path (render -> track chain over the whole buffer) and the live playback path (render -> the
    /// render-ahead Feeder in blocks) give identical samples for a project with moved clips, clip effects,
    /// clip gain/fades and track effects. A live hand-over of the same dry audio mid-stream changes nothing.
    #[test] fn export_and_playback_render_identically() {
        use crate::fx::{Effect, EqBand};
        use crate::audio::playback::Feeder;
        let d = tmp("evp"); let cache = DecodeCache::default();
        let src = wav(&d, "v.wav", 48000, 2, 96000, |i| 0.3 * (i as f32 * 0.031).sin() + 0.05 * ((i * 7919 % 1000) as f32 / 1000.0 - 0.5));
        let b = |f| EqBand { freq: f, gain_db: 3.0, q: 1.0 };
        let mut c1 = clip(&src, 0.25, 1.25);
        c1.fx = vec![slot(Effect::Eq { hpf_hz: 100.0, hpf_on: true, low_shelf: b(150.0), peaks: [b(400.0), b(2000.0), b(5000.0)], high_shelf: b(9000.0) }),
                     slot(Effect::Compressor { threshold_db: -20.0, ratio: 4.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 3.0 }),
                     slot(Effect::Normalize { peak_db: -3.0 })];
        c1.gain_db = Some(-2.0); c1.fade_in_ms = Some(30.0); c1.fade_out_ms = Some(120.0);
        let mut c2 = clip(&src, 1.5, 2.0); c2.fx = vec![slot(Effect::Gate { threshold_db: -30.0, floor_db: -40.0, attack_ms: 1.0, hold_ms: 20.0, release_ms: 60.0 })];
        let clips = vec![gap(0.123456), c1, gap(2.0 + 17.0 / 48000.0), c2];
        let track_fx = vec![slot(Effect::DeEsser { freq_hz: 5500.0, threshold_db: -30.0, max_reduction_db: 6.0 }),
                            slot(Effect::Loudness { target_lufs: -16.0, ceiling_db: Some(-1.0) }), slot(Effect::Limiter { ceiling_db: -1.0, release_ms: 50.0 })];
        // export
        let mut exported = render(&cache, &clips).unwrap(); crate::fx::process_chain(&mut exported, &track_fx);
        // playback, as the render-ahead thread runs it
        let dry = Arc::new(render(&cache, &clips).unwrap());
        let gains = crate::fx::resolve_gains(&dry, &track_fx, None).unwrap();
        let play = |swap_at: Option<usize>| {
            let mut f = Feeder::new(dry.clone(), track_fx.clone(), gains.clone(), 0); let mut out = vec![]; let mut blocks = 0;
            while f.next(&mut out) { blocks += 1; if Some(blocks) == swap_at { assert!(f.swap_dry(Arc::new(render(&cache, &clips).unwrap()))) } }
            out
        };
        let played = play(None);
        assert_eq!(played.len(), exported.samples.len());
        assert!(played == exported.samples, "playback differs from export");
        assert!(play(Some(97)) == exported.samples, "hand-over of identical dry audio changed the output");
        // the moved clip's first sample is where the timeline says
        let first = (0.123456f64 * 48000.0).round() as usize;
        // (silence before it; its 30 ms fade-in starts from zero, so check the next few samples)
        assert!(dry.samples[..first * 2].iter().all(|v| *v == 0.0));
        assert!(dry.samples[first * 2..(first + 50) * 2].iter().any(|v| *v != 0.0));
        let _ = std::fs::remove_dir_all(&d);
    }
    /// A re-render with a different format can't be swapped in (playback restarts instead).
    #[test] fn feeder_refuses_format_change() {
        use crate::audio::playback::Feeder;
        let a = Arc::new(Decoded { samples: vec![0.1; 4800], channels: 2, sample_rate: 48000 });
        let mut f = Feeder::new(a, vec![], vec![], 0); let mut o = vec![]; f.next(&mut o);
        assert!(!f.swap_dry(Arc::new(Decoded { samples: vec![0.1; 4800], channels: 1, sample_rate: 48000 })));
        assert!(f.swap_dry(Arc::new(Decoded { samples: vec![0.2; 9600], channels: 2, sample_rate: 48000 })));
        assert_eq!(f.total, 4800);
        o.clear(); f.next(&mut o); assert!(o.iter().all(|v| *v == 0.2)); // new audio from the current position on
    }

    fn xf(mut c: ClipSpec, ms: f64, curve: &str) -> ClipSpec { c.xfade_ms = Some(ms); c.xfade_curve = Some(curve.into()); c }

    /// Curves: linear gains sum to exactly 1, equal-power gains to power 1; both monotonic and symmetric.
    #[test] fn crossfade_curves_are_complementary() {
        for h2 in [2usize, 3, 96, 4801] {
            let mut last = (-1f32, 2f32);
            for k in 0..h2 {
                let (a, b) = Curve::Linear.xfade(k, h2); assert!((a + b - 1.0).abs() < 1e-6, "linear {k}/{h2}");
                let (p, q) = Curve::EqualPower.xfade(k, h2); assert!((p * p + q * q - 1.0).abs() < 1e-5, "power {k}/{h2}");
                assert!(p > last.0 && q < last.1); last = (p, q);
                let (pm, qm) = Curve::EqualPower.xfade(h2 - 1 - k, h2); assert!((p - qm).abs() < 1e-6 && (q - pm).abs() < 1e-6, "symmetric");
            }
        }
    }
    /// Two different sources: inside the crossfade the output is exactly out·A + in·B, outside it is untouched,
    /// the timeline length and every position stay the same, and the crossfade is centred on the cut.
    #[test] fn crossfade_mixes_exactly_and_keeps_positions() {
        let d = tmp("xf"); let cache = DecodeCache::default();
        let a = wav(&d, "a.wav", 1000, 1, 2000, |_| 1.0);
        let b = wav(&d, "b.wav", 1000, 1, 2000, |_| 0.5);
        let plain = render(&cache, &[clip(&a, 0.5, 1.0), clip(&b, 0.5, 1.0)]).unwrap();
        for curve in ["linear", "equal_power"] {
            let out = render(&cache, &[clip(&a, 0.5, 1.0), xf(clip(&b, 0.5, 1.0), 100.0, curve)]).unwrap();
            assert_eq!(out.samples.len(), plain.samples.len(), "{curve}: length changed");
            let c = if curve == "linear" { Curve::Linear } else { Curve::EqualPower };
            for f in 0..1000 {
                let want = if (450..550).contains(&f) { let (gi, go) = c.xfade(f - 450, 100); go * 1.0 + gi * 0.5 } else { plain.samples[f] };
                assert!((out.samples[f] - want).abs() < 1e-6, "{curve} frame {f}: {} vs {want}", out.samples[f]);
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Crossfading across a cut in continuous material (a split clip) reproduces the uncut audio: no gap, no click.
    #[test] fn crossfade_over_a_cut_in_continuous_audio_is_seamless() {
        let d = tmp("xs"); let cache = DecodeCache::default();
        let src = wav(&d, "s.wav", 48000, 2, 96000, |i| 0.4 * (i as f32 * 0.013).sin() + 0.2 * (i as f32 * 0.0021).cos());
        let whole = render(&cache, &[clip(&src, 0.2, 1.8)]).unwrap();
        let out = render(&cache, &[clip(&src, 0.2, 0.9), xf(clip(&src, 0.9, 1.8), 80.0, "linear")]).unwrap();
        assert_eq!(out.samples.len(), whole.samples.len());
        let diff = out.samples.iter().zip(&whole.samples).fold(0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(diff < 1e-5, "linear crossfade of continuous audio differs by {diff}");
        // largest sample-to-sample step around the joint is no bigger than in the original signal (no click)
        let step = |v: &[f32], a: usize, b: usize| (a..b).map(|f| (v[(f + 1) * 2] - v[f * 2]).abs()).fold(0f32, f32::max);
        let cut = (0.7 * 48000.0) as usize;
        assert!(step(&out.samples, cut - 3000, cut + 3000) <= step(&whole.samples, cut - 3000, cut + 3000) + 1e-5);
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Without spare source audio the crossfade shrinks to what exists (here: none) instead of reading past a file.
    #[test] fn crossfade_is_clamped_to_real_handles() {
        let d = tmp("xc"); let cache = DecodeCache::default();
        let a = wav(&d, "a.wav", 1000, 1, 1000, |_| 1.0); // 1 s: a clip ending at 1.0 has no tail
        let b = wav(&d, "b.wav", 1000, 1, 1000, |_| 0.5);
        let none = render(&cache, &[clip(&a, 0.5, 1.0), clip(&b, 0.0, 0.5)]).unwrap();
        let want = render(&cache, &[clip(&a, 0.5, 1.0), xf(clip(&b, 0.0, 0.5), 200.0, "linear")]).unwrap();
        assert_eq!(none.samples, want.samples);
        // 40 ms of tail and head available -> a 40 ms (not 200 ms) crossfade
        let c = render(&cache, &[clip(&a, 0.5, 0.98), xf(clip(&b, 0.02, 0.5), 200.0, "linear")]).unwrap();
        assert_eq!(c.samples[479 - 21], 1.0); assert_eq!(c.samples[480 + 20], 0.5);
        assert!(c.samples[470] < 1.0 && c.samples[470] > 0.5);
        // a crossfade after a gap or on the first clip is ignored
        let g = render(&cache, &[gap(0.1), xf(clip(&b, 0.2, 0.5), 100.0, "linear")]).unwrap();
        assert_eq!(g.samples, render(&cache, &[gap(0.1), clip(&b, 0.2, 0.5)]).unwrap().samples);
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Clip fades keep the old S-curve by default; "linear" / "equal_power" change only the shape.
    #[test] fn fade_curves() {
        let d = tmp("fc"); let cache = DecodeCache::default();
        let a = wav(&d, "a.wav", 1000, 1, 1000, |_| 1.0);
        let mut c = clip(&a, 0.0, 1.0); c.fade_in_ms = Some(100.0);
        let s = render(&cache, &[c.clone()]).unwrap();
        assert!((s.samples[25] - 0.5 * (1.0 - (std::f32::consts::PI * 0.25).cos())).abs() < 1e-6);
        c.fade_curve = Some("linear".into()); assert!((render(&cache, &[c.clone()]).unwrap().samples[25] - 0.25).abs() < 1e-6);
        c.fade_curve = Some("equal_power".into()); assert!((render(&cache, &[c]).unwrap().samples[25] - (std::f32::consts::FRAC_PI_2 * 0.25).sin()).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&d);
    }
    /// A project with crossfades plays exactly as it exports (same renderer, then the track chain).
    #[test] fn crossfades_play_like_they_export() {
        use crate::audio::playback::Feeder; use crate::fx::Effect;
        let d = tmp("xpe"); let cache = DecodeCache::default();
        let a = wav(&d, "a.wav", 48000, 2, 96000, |i| 0.3 * (i as f32 * 0.02).sin());
        let b = wav(&d, "b.wav", 48000, 2, 96000, |i| 0.25 * (i as f32 * 0.035).cos());
        let mut c1 = clip(&a, 0.3, 1.1); c1.fx = vec![slot(Effect::Gain { db: -3.0 })]; c1.fade_in_ms = Some(20.0);
        let c2 = xf(clip(&b, 0.4, 1.2), 250.0, "equal_power");
        let c3 = xf(clip(&a, 1.3, 1.7), 120.0, "linear");
        let clips = vec![gap(0.05), c1, c2, c3];
        let fx = vec![slot(Effect::Compressor { threshold_db: -18.0, ratio: 3.0, attack_ms: 5.0, release_ms: 60.0, makeup_db: 2.0 })];
        let mut exported = render(&cache, &clips).unwrap(); crate::fx::process_chain(&mut exported, &fx);
        let dry = Arc::new(render(&cache, &clips).unwrap());
        let gains = crate::fx::resolve_gains(&dry, &fx, None).unwrap();
        let mut f = Feeder::new(dry.clone(), fx.clone(), gains, 0); let mut out = vec![]; while f.next(&mut out) {}
        assert!(out == exported.samples, "playback differs from export with crossfades");
        assert_eq!(dry.samples.len(), ((0.05f64 + 0.8 + 0.8 + 0.4) * 48000.0).round() as usize * 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
