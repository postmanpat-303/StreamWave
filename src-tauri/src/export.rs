//! Export to open formats. WAV is written natively (hound); everything else goes through an
//! ffmpeg sidecar (bundled next to the app, or found on PATH).
use crate::waveform::Decoded;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Tags { pub title: String, pub artist: String, pub album: String, pub date: String, pub genre: String, pub comment: String }

#[derive(Deserialize, Clone, Debug)]
pub struct ExportOptions {
    /// wav | flac | ogg | opus | mp3 | wavpack | speex | mka | webm
    pub format: String,
    #[serde(default)] pub sample_rate: Option<u32>,
    #[serde(default)] pub channels: Option<u16>,
    /// WAV: 16 | 24 | 32 (float)
    #[serde(default)] pub bit_depth: Option<u16>,
    /// kbps for lossy formats
    #[serde(default)] pub bitrate_kbps: Option<u32>,
    /// VBR quality (Vorbis 0-10, MP3 0-9 lower=better, FLAC compression 0-12, Speex 0-10)
    #[serde(default)] pub quality: Option<f32>,
    #[serde(default)] pub tags: Tags,
}

#[derive(Serialize)]
pub struct FfmpegInfo { pub found: bool, pub path: Option<String>, pub version: Option<String>, pub message: String }

pub fn find_ffmpeg() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    // 1) bundled sidecar next to our executable (Tauri externalBin puts it there, maybe with a target triple suffix)
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n == exe || (n.starts_with("ffmpeg-") && (n.ends_with(".exe") || !cfg!(windows))) { return Some(e.path()) }
                }
            }
        }
    }
    // 2) PATH (plus Homebrew/MacPorts on macOS, where apps started from Finder get a minimal PATH)
    tool_dirs().into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

/// Directories searched for ffmpeg/ffprobe after the bundled sidecar.
pub fn tool_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(target_os = "macos") {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] { let d = PathBuf::from(d); if !v.contains(&d) { v.push(d) } }
    }
    v
}

pub fn ffmpeg_info() -> FfmpegInfo {
    match find_ffmpeg() {
        Some(p) => {
            let mut cmd = Command::new(&p); cmd.arg("-version"); hide_window(&mut cmd);
            let v = cmd.output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.lines().next().map(|l| l.to_string()));
            FfmpegInfo { found: true, path: Some(p.to_string_lossy().into()), version: v, message: "All formats available".into() }
        }
        None => FfmpegInfo { found: false, path: None, version: None,
            message: "FFmpeg wasn't found, so only WAV export is available. Install FFmpeg (see README) or put ffmpeg next to StreamWave to unlock FLAC, MP3, Ogg, Opus and more.".into() },
    }
}

fn hide_window(_c: &mut Command) {
    #[cfg(windows)] { use std::os::windows::process::CommandExt; _c.creation_flags(0x0800_0000); } // CREATE_NO_WINDOW
}

/// Channel up/down-mix and linear resample (used for WAV and before handing to ffmpeg).
pub fn conform(d: &Decoded, channels: Option<u16>, rate: Option<u32>) -> Decoded {
    let out_ch = channels.map(|c| c.clamp(1, 8) as usize).unwrap_or(d.channels);
    let in_ch = d.channels; let frames = d.samples.len() / in_ch;
    let mut mixed = Vec::with_capacity(frames * out_ch);
    for f in d.samples.chunks(in_ch) {
        if out_ch == 1 { mixed.push(f.iter().sum::<f32>() / in_ch as f32) } else { for c in 0..out_ch { mixed.push(f[c % in_ch]) } }
    }
    let out_sr = rate.unwrap_or(d.sample_rate);
    if out_sr == d.sample_rate || frames < 2 { return Decoded { samples: mixed, channels: out_ch, sample_rate: out_sr } }
    let ratio = d.sample_rate as f64 / out_sr as f64;
    let n = (frames as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(n * out_ch);
    for i in 0..n {
        let p = i as f64 * ratio; let a = p as usize; let b = (a + 1).min(frames - 1); let t = (p - a as f64) as f32;
        for c in 0..out_ch { let x = mixed[a * out_ch + c]; out.push(x + (mixed[b * out_ch + c] - x) * t) }
    }
    Decoded { samples: out, channels: out_ch, sample_rate: out_sr }
}

pub fn write_wav_depth(d: &Decoded, path: &Path, bits: u16) -> anyhow::Result<()> {
    let float = bits == 32;
    let spec = hound::WavSpec { channels: d.channels as u16, sample_rate: d.sample_rate, bits_per_sample: if float { 32 } else { bits.clamp(16, 24) },
        sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int } };
    let tmp = path.with_extension("part.wav");
    let mut w = hound::WavWriter::create(&tmp, spec)?;
    if float { for s in &d.samples { w.write_sample(*s)? } }
    else {
        let max = ((1i32 << (spec.bits_per_sample - 1)) - 1) as f32;
        // TPDF dither when reducing to integer
        let mut seed = 0x1234_5678u32;
        let mut rnd = || { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; seed as f32 / u32::MAX as f32 };
        for s in &d.samples { let dth = (rnd() - rnd()) / max; w.write_sample(((s + dth).clamp(-1.0, 1.0) * max).round() as i32)? }
    }
    w.finalize()?; std::fs::rename(tmp, path)?; Ok(())
}

pub fn export(d: &Decoded, path: &Path, o: &ExportOptions) -> anyhow::Result<()> {
    let fmt = o.format.to_lowercase();
    let mut rate = o.sample_rate;
    // Opus only supports these rates
    if matches!(fmt.as_str(), "opus" | "webm") { let r = rate.unwrap_or(d.sample_rate); if ![8000, 12000, 16000, 24000, 48000].contains(&r) { rate = Some(48000) } }
    if fmt == "speex" { let r = rate.unwrap_or(d.sample_rate); rate = Some(if r <= 8000 { 8000 } else if r <= 16000 { 16000 } else { 32000 }) }
    let mut channels = o.channels;
    if fmt == "speex" { channels = Some(1) } // ffmpeg's libspeex encoder is happiest in mono
    let d = conform(d, channels, rate);
    if fmt == "wav" { return write_wav_depth(&d, path, o.bit_depth.unwrap_or(24)) }

    let ff = find_ffmpeg().ok_or_else(|| anyhow::anyhow!("{}", ffmpeg_info().message))?;
    let tmp = std::env::temp_dir().join(format!("streamwave_export_{}_{}.wav", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()));
    write_wav_depth(&d, &tmp, 32)?;
    let br = |def: u32| format!("{}k", o.bitrate_kbps.unwrap_or(def));
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(), "-i".into(), tmp.to_string_lossy().into()];
    let mut a = |v: &[&str]| args.extend(v.iter().map(|s| s.to_string()));
    match fmt.as_str() {
        "flac" => { let q = o.quality.unwrap_or(5.0).clamp(0.0, 12.0) as u32; a(&["-c:a", "flac", "-compression_level", &q.to_string(), "-f", "flac"]) }
        "ogg" => { let q = o.quality.unwrap_or(6.0).clamp(0.0, 10.0); a(&["-c:a", "libvorbis", "-q:a", &q.to_string(), "-f", "ogg"]) }
        "opus" => a(&["-c:a", "libopus", "-b:a", &br(128), "-f", "opus"]),
        "mp3" => match o.quality { Some(q) => a(&["-c:a", "libmp3lame", "-q:a", &(q.clamp(0.0, 9.0) as u32).to_string(), "-f", "mp3"]),
                                    None => a(&["-c:a", "libmp3lame", "-b:a", &br(192), "-f", "mp3"]) },
        "wavpack" => a(&["-c:a", "wavpack", "-f", "wv"]),
        "speex" => { let q = o.quality.unwrap_or(8.0).clamp(0.0, 10.0) as u32; a(&["-c:a", "libspeex", "-cbr_quality", &q.to_string(), "-f", "ogg"]) }
        "mka" => a(&["-c:a", "flac", "-f", "matroska"]),
        "webm" => a(&["-c:a", "libopus", "-b:a", &br(128), "-f", "webm"]),
        f => anyhow::bail!("unknown format {f}"),
    }
    let t = &o.tags;
    for (k, v) in [("title", &t.title), ("artist", &t.artist), ("album", &t.album), ("date", &t.date), ("genre", &t.genre), ("comment", &t.comment)] {
        if !v.is_empty() { args.push("-metadata".into()); args.push(format!("{k}={v}")); }
    }
    args.push(path.to_string_lossy().into());
    let mut cmd = Command::new(ff); cmd.args(&args); hide_window(&mut cmd);
    let out = cmd.output();
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    anyhow::ensure!(out.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone() -> Decoded {
        let s: Vec<f32> = (0..48000).flat_map(|i| { let v = 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin(); [v, v] }).collect();
        Decoded { samples: s, channels: 2, sample_rate: 48000 }
    }
    fn opts(f: &str) -> ExportOptions { ExportOptions { format: f.into(), sample_rate: None, channels: None, bit_depth: None, bitrate_kbps: None, quality: None,
        tags: Tags { title: "Test take".into(), artist: "Del".into(), ..Default::default() } } }

    #[test] fn wav_round_trip_with_conform() {
        let dir = std::env::temp_dir().join("tw_exp_test"); std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        export(&tone(), &p, &ExportOptions { sample_rate: Some(44100), channels: Some(1), bit_depth: Some(16), ..opts("wav") }).unwrap();
        let d = crate::waveform::decode_file(&p).unwrap();
        assert_eq!((d.channels, d.sample_rate), (1, 44100));
        assert!(((d.samples.len() as i64) - 44100).abs() <= 1);
        let pk = d.samples.iter().fold(0f32, |m, s| m.max(s.abs())); assert!((pk - 0.4).abs() < 0.01);
    }

    #[test] fn ffmpeg_encoders_round_trip_when_available() {
        if find_ffmpeg().is_none() { eprintln!("ffmpeg not found - skipping"); return }
        let dir = std::env::temp_dir().join("tw_exp_test"); std::fs::create_dir_all(&dir).unwrap();
        // formats symphonia can decode back for verification
        for (f, ext) in [("flac", "flac"), ("ogg", "ogg"), ("mp3", "mp3"), ("mka", "mka"), ("webm", "webm"), ("opus", "opus")] {
            let p = dir.join(format!("t.{ext}"));
            export(&tone(), &p, &opts(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert!(std::fs::metadata(&p).unwrap().len() > 1000, "{f} too small");
            if matches!(f, "flac" | "ogg" | "mp3" | "mka") {
                let d = crate::waveform::decode_file(&p).unwrap_or_else(|e| panic!("decode {f}: {e}"));
                let secs = d.samples.len() as f32 / d.channels as f32 / d.sample_rate as f32;
                assert!((secs - 1.0).abs() < 0.1, "{f} duration {secs}");
                if f == "flac" { assert_eq!(d.channels, 2) }
            }
        }
        for (f, ext) in [("wavpack", "wv"), ("speex", "spx")] {
            let p = dir.join(format!("t.{ext}"));
            export(&tone(), &p, &opts(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert!(std::fs::metadata(&p).unwrap().len() > 500, "{f} too small");
        }
        // tags made it into the FLAC
        let out = Command::new(find_ffmpeg().unwrap()).args(["-hide_banner", "-i"]).arg(dir.join("t.flac")).output().unwrap();
        let s = String::from_utf8_lossy(&out.stderr).to_lowercase();
        assert!(s.contains("test take") && s.contains("del"), "tags missing: {s}");
    }
}
