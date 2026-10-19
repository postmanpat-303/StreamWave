//! Whisper transcription via an external whisper.cpp CLI (same pattern as FFmpeg:
//! detected next to the executable, then on PATH). No model is bundled; point
//! Settings → Whisper model at a ggml .bin file (see the user guide).
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize)]
pub struct WhisperInfo { pub found: bool, pub path: Option<String>, pub model_found: bool, pub message: String }

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Segment { pub start: f64, pub end: f64, pub text: String }

pub fn find_whisper() -> Option<PathBuf> {
    let names: [&str; 3] = if cfg!(windows) { ["whisper-cli.exe", "whisper.exe", "main.exe"] } else { ["whisper-cli", "whisper", "main"] };
    // 1) next to our executable
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            for n in names { let p = dir.join(n); if p.is_file() { return Some(p) } }
        }
    }
    // 2) PATH
    let path = std::env::var_os("PATH")?;
    for d in std::env::split_paths(&path) {
        for n in names { let p = d.join(n); if p.is_file() { return Some(p) } }
    }
    None
}

/// A ggml model: explicit path, or any `ggml-*.bin` next to the exe.
fn find_model(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(m) = explicit { if m.is_file() { return Some(m.to_path_buf()) } }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.starts_with("ggml-") && n.ends_with(".bin") { return Some(e.path()) }
                }
            }
        }
    }
    None
}

pub fn info(model: Option<&Path>) -> WhisperInfo {
    match find_whisper() {
        Some(p) => {
            let m = find_model(model);
            let message = if m.is_some() { "Whisper ready".into() } else {
                "Whisper CLI found, but no model. Download a ggml model (e.g. ggml-base.en.bin) from huggingface.co/ggerganov/whisper.cpp and set it in Settings.".into() };
            WhisperInfo { found: true, path: Some(p.to_string_lossy().into()), model_found: m.is_some(), message }
        }
        None => WhisperInfo { found: false, path: None, model_found: false,
            message: "whisper.cpp not found. Install it (winget install whisper.cpp, or build from source) and make sure whisper-cli is on PATH — then you can transcribe tracks here.".into() },
    }
}

/// Transcribe an audio file, returning timestamped segments (track-time seconds).
pub fn transcribe(audio: &Path, model: Option<&Path>) -> anyhow::Result<Vec<Segment>> {
    let bin = find_whisper().ok_or_else(|| anyhow::anyhow!("whisper.cpp not found — install whisper-cli and put it on PATH"))?;
    let model = find_model(model).ok_or_else(|| anyhow::anyhow!("no Whisper model — download a ggml .bin (e.g. ggml-base.en.bin) and set it in Settings"))?;
    let dir = std::env::temp_dir().join(format!("sw_whisper_{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let base = dir.join("out"); // whisper appends the extension
    let mut cmd = Command::new(&bin);
    cmd.arg("-m").arg(&model).arg("-f").arg(audio).arg("--output-json").arg("--output-file").arg(&base);
    hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| anyhow::anyhow!("could not run {}: {e}", bin.display()))?;
    let json = base.with_extension("json");
    let alt = base.clone();
    let j = std::fs::read_to_string(&json).or_else(|_| std::fs::read_to_string(&alt))
        .map_err(|_| anyhow::anyhow!("whisper produced no output: {}", String::from_utf8_lossy(&out.stderr).chars().take(400).collect::<String>()))?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(parse_output_json(&j))
}

/// Parse whisper.cpp `--output-json` (tolerant of both the `offsets` ms format and the
/// `timestamps` "HH:MM:SS.mmm" format).
pub fn parse_output_json(j: &str) -> Vec<Segment> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(j) else { return vec![] };
    let mut out = vec![];
    let Some(list) = v.get("transcription").and_then(|t| t.as_array()) else { return out };
    for s in list {
        let (start, end) = match (s.get("offsets"), s.get("timestamps")) {
            (Some(o), _) => (field_secs(o.get("from"), true), field_secs(o.get("to"), true)),
            (None, Some(t)) => (field_secs(t.get("from"), false), field_secs(t.get("to"), false)),
            _ => (0.0, 0.0),
        };
        let text = s.get("text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        if !text.is_empty() { out.push(Segment { start, end, text }) }
    }
    out
}

/// Number fields (offsets) are milliseconds; strings (timestamps) are clock times.
fn field_secs(v: Option<&serde_json::Value>, is_ms: bool) -> f64 {
    match v {
        Some(serde_json::Value::Number(n)) => { let x = n.as_f64().unwrap_or(0.0); if is_ms { x / 1000.0 } else { x } }
        Some(serde_json::Value::String(s)) => clock_to_secs(s).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// "00:01:23,456" / "00:01:23.456" / "01:23.456" -> seconds.
fn clock_to_secs(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let mut secs = 0f64;
    for part in s.split(':') { secs = secs * 60.0 + part.parse::<f64>().ok()?; }
    Some(secs)
}

fn hide_window(_c: &mut Command) {
    #[cfg(windows)] { use std::os::windows::process::CommandExt; _c.creation_flags(0x0800_0000); } // CREATE_NO_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_offsets_and_timestamps() {
        let j = r#"{"transcription":[
            {"offsets":{"from":0,"to":1500},"text":" Hello"},
            {"offsets":{"from":1500,"to":4000},"text":"world"},
            {"timestamps":{"from":"00:00:04,000","to":"00:00:07.500"},"text":" again "}
        ]}"#;
        let segs = parse_output_json(j);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], Segment { start: 0.0, end: 1.5, text: "Hello".into() });
        assert_eq!(segs[1].end, 4.0);
        assert_eq!(segs[2].start, 4.0);
        assert_eq!(segs[2].end, 7.5);
        assert_eq!(segs[2].text, "again");
        assert!(parse_output_json("not json").is_empty());
    }
    #[test] fn clock_parsing() {
        assert_eq!(clock_to_secs("00:01:23,456"), Some(83.456));
        assert_eq!(clock_to_secs("01:23.5"), Some(83.5));
        assert_eq!(clock_to_secs("7.25"), Some(7.25));
    }
}
