//! OBS media helpers: probe a recording, split each audio track into its own file (aligned),
//! and remux processed audio back into a copy of the video without re-encoding the picture.
use crate::export::find_ffmpeg;
use crate::waveform::Decoded;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

fn cmd(p: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(p);
    #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
    c
}
pub fn find_ffprobe() -> Option<PathBuf> {
    let ff = find_ffmpeg()?;
    let name = ff.file_name()?.to_string_lossy().replacen("ffmpeg", "ffprobe", 1);
    let p = ff.with_file_name(name);
    if p.is_file() { return Some(p) }
    let exe = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    crate::export::tool_dirs().into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

#[derive(Serialize, Clone, Debug)]
pub struct AudioStream { pub index: usize, pub codec: String, pub channels: u32, pub sample_rate: u32, pub title: Option<String>, pub start_time: f64 }
#[derive(Serialize, Clone, Debug)]
pub struct MediaInfo { pub duration: f64, pub video_codec: Option<String>, pub audio: Vec<AudioStream>, pub chapters: usize }

pub fn probe(path: &Path) -> anyhow::Result<MediaInfo> {
    let fp = find_ffprobe().ok_or_else(|| anyhow::anyhow!("ffprobe not found (it ships with FFmpeg)"))?;
    let out = cmd(&fp).args(["-v", "error", "-show_streams", "-show_format", "-show_chapters", "-of", "json"]).arg(path).output()?;
    anyhow::ensure!(out.status.success(), "ffprobe failed: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let mut audio = vec![]; let mut video = None;
    for s in v["streams"].as_array().cloned().unwrap_or_default() {
        match s["codec_type"].as_str() {
            Some("video") if video.is_none() && s["disposition"]["attached_pic"].as_i64() != Some(1) => video = s["codec_name"].as_str().map(String::from),
            Some("audio") => audio.push(AudioStream { index: audio.len(), codec: s["codec_name"].as_str().unwrap_or("?").into(),
                channels: s["channels"].as_u64().unwrap_or(0) as u32, sample_rate: s["sample_rate"].as_str().and_then(|x| x.parse().ok()).unwrap_or(0),
                title: s["tags"]["title"].as_str().or(s["tags"]["TITLE"].as_str()).or(s["tags"]["handler_name"].as_str()).filter(|t| !t.is_empty() && !t.contains("SoundHandler")).map(String::from),
                start_time: s["start_time"].as_str().and_then(|x| x.parse().ok()).unwrap_or(0.0) }),
            _ => {}
        }
    }
    let duration = v["format"]["duration"].as_str().and_then(|x| x.parse().ok()).unwrap_or(0.0);
    Ok(MediaInfo { duration, video_codec: video, audio, chapters: v["chapters"].as_array().map(|a| a.len()).unwrap_or(0) })
}

#[derive(Serialize, Clone, Debug)]
pub struct SplitTrack { pub path: String, pub index: usize, pub label: String }

/// One WAV per audio stream, all aligned to the container start (leading offsets are padded).
pub fn split_audio(path: &Path, out_dir: &Path) -> anyhow::Result<Vec<SplitTrack>> {
    let info = probe(path)?;
    anyhow::ensure!(!info.audio.is_empty(), "This recording has no audio tracks");
    let ff = find_ffmpeg().ok_or_else(|| anyhow::anyhow!("FFmpeg not found"))?;
    std::fs::create_dir_all(out_dir)?;
    let t0 = info.audio.iter().map(|a| a.start_time).fold(f64::INFINITY, f64::min).min(0.0f64.max(0.0)).max(0.0);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "obs".into());
    let mut c = cmd(&ff); c.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(path);
    let mut out = vec![];
    for a in &info.audio {
        let label = a.title.clone().unwrap_or_else(|| format!("Track {}", a.index + 1));
        let safe: String = label.chars().map(|ch| if ch.is_alphanumeric() || ch == ' ' || ch == '-' { ch } else { '_' }).collect();
        let p = out_dir.join(format!("{stem} - {} {safe}.wav", a.index + 1));
        c.args(["-map", &format!("0:a:{}", a.index)]);
        let delay = ((a.start_time - t0) * 1000.0).round() as i64;
        if delay > 0 { c.args(["-af", &format!("adelay={delay}:all=1")]); }
        c.args(["-c:a", "pcm_f32le"]).arg(&p);
        out.push(SplitTrack { path: p.to_string_lossy().into(), index: a.index, label });
    }
    let o = c.output()?;
    anyhow::ensure!(o.status.success(), "ffmpeg split failed: {}", String::from_utf8_lossy(&o.stderr));
    Ok(out)
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Marker { pub time: f64, pub label: String }

/// Replace the audio of `video` with `audio`, copying the video stream bit-for-bit.
pub fn remux(video: &Path, audio: &Decoded, markers: &[Marker], out: &Path) -> anyhow::Result<()> {
    let ff = find_ffmpeg().ok_or_else(|| anyhow::anyhow!("FFmpeg not found"))?;
    let info = probe(video)?;
    let tag = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let wav = std::env::temp_dir().join(format!("tw_remux_{tag}.wav"));
    crate::export::write_wav_depth(audio, &wav, 32)?;
    let ext = out.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (acodec, extra): (&str, &[&str]) = match ext.as_str() {
        "mp4" | "m4v" | "mov" => ("aac", &["-b:a", "256k"]),
        "webm" => ("libopus", &["-b:a", "160k"]),
        _ => ("flac", &[]), // mkv and friends: lossless
    };
    let chapters_ok = !markers.is_empty() && matches!(ext.as_str(), "mp4" | "m4v" | "mov" | "mkv" | "mka" | "webm");
    let meta = std::env::temp_dir().join(format!("tw_chapters_{tag}.txt"));
    if chapters_ok {
        let mut m: Vec<&Marker> = markers.iter().collect(); m.retain(|x| x.time.is_finite() && x.time >= 0.0); m.sort_by(|a, b| a.time.total_cmp(&b.time));
        let end_all = (info.duration.max(audio.samples.len() as f64 / audio.channels as f64 / audio.sample_rate as f64) * 1000.0) as i64;
        let mut s = String::from(";FFMETADATA1\n");
        for (i, mk) in m.iter().enumerate() {
            let st = (mk.time * 1000.0) as i64; let en = m.get(i + 1).map(|n| (n.time * 1000.0) as i64).unwrap_or(end_all).max(st + 1);
            let title = mk.label.replace(['=', ';', '#', '\\', '\n'], " ");
            s += &format!("[CHAPTER]\nTIMEBASE=1/1000\nSTART={st}\nEND={en}\ntitle={title}\n");
        }
        std::fs::write(&meta, s)?;
    }
    let mut c = cmd(&ff);
    c.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(video).arg("-i").arg(&wav);
    if chapters_ok { c.args(["-f", "ffmetadata", "-i"]).arg(&meta); }
    c.args(["-map", "0:v?", "-map", "1:a", "-c:v", "copy", "-c:a", acodec, "-ac", &audio.channels.min(2).max(1).to_string()]).args(extra);
    c.args(["-map_metadata", "0"]);
    if chapters_ok { c.args(["-map_chapters", "2"]); }
    c.arg(out);
    let o = c.output();
    let _ = std::fs::remove_file(&wav); let _ = std::fs::remove_file(&meta);
    let o = o?;
    anyhow::ensure!(o.status.success(), "ffmpeg remux failed: {}", String::from_utf8_lossy(&o.stderr));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn video_md5(p: &Path) -> String {
        let o = Command::new(find_ffmpeg().unwrap()).args(["-v", "error", "-i"]).arg(p).args(["-map", "0:v", "-c", "copy", "-f", "md5", "-"]).output().unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    fn zero_cross_hz(d: &Decoded) -> f32 {
        let m: Vec<f32> = d.samples.chunks(d.channels).map(|f| f[0]).collect();
        let z = m.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        z as f32 / (m.len() as f32 / d.sample_rate as f32)
    }
    #[test] fn split_and_remux_round_trip() {
        let Some(ff) = find_ffmpeg() else { eprintln!("no ffmpeg - skipping"); return };
        if find_ffprobe().is_none() { return }
        let dir = std::env::temp_dir().join("tw_media_test"); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("obs.mkv");
        let st = Command::new(&ff).args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=10:duration=3",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
            "-map", "0", "-map", "1", "-map", "2", "-c:v", "mpeg4", "-c:a", "flac", "-metadata:s:a:0", "title=Mic", "-metadata:s:a:1", "title=Desktop"]).arg(&src).status().unwrap();
        assert!(st.success());
        let tracks = split_audio(&src, &dir.join("split")).unwrap();
        assert_eq!(tracks.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Mic", "Desktop"]);
        let a = crate::waveform::decode_file(Path::new(&tracks[0].path)).unwrap();
        let b = crate::waveform::decode_file(Path::new(&tracks[1].path)).unwrap();
        assert!((zero_cross_hz(&a) - 440.0).abs() < 5.0 && (zero_cross_hz(&b) - 1000.0).abs() < 5.0);
        assert_eq!(a.samples.len(), b.samples.len(), "tracks must stay aligned");
        // processed "mix" -> back into the video, with chapters
        let mut mix = a.clone(); for (x, y) in mix.samples.iter_mut().zip(&b.samples) { *x = 0.5 * (*x + y) }
        let markers = vec![Marker { time: 0.0, label: "Intro".into() }, Marker { time: 1.5, label: "Scene: Gameplay".into() }];
        for ext in ["mkv", "mp4"] {
            let out = dir.join(format!("out.{ext}"));
            remux(&src, &mix, &markers, &out).unwrap();
            let info = probe(&out).unwrap();
            assert_eq!(info.video_codec.as_deref(), Some("mpeg4"));
            assert_eq!(info.audio.len(), 1);
            assert_eq!(info.chapters, 2, "{ext} chapters");
            assert_eq!(video_md5(&out), video_md5(&src), "{ext}: video must be copied, not re-encoded");
        }
    }
}
