//! Whisper transcription via an external whisper.cpp CLI (`whisper-cli`). No model is bundled.
//!
//! Which program StreamWave runs, in this order (see [`resolve`]):
//! 1. the file the user chose ("I already have it", saved in settings),
//! 2. StreamWave's own copy from "Set it up for me" (in the app-data folder, see `whisper_setup`),
//! 3. `whisper-cli` next to the StreamWave executable,
//! 4. `whisper-cli` on PATH (plus Homebrew/MacPorts on macOS, like FFmpeg).
//! Only the name `whisper-cli` is looked up automatically: never whisper.cpp's old generic `main` (on PATH that
//! could be any program) and not a bare `whisper` either (usually OpenAI's Python CLI, which takes other options).
//! Paths are always passed to `Command` as separate arguments, never through a shell.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source { Settings, Managed, Bundled, Path }

#[derive(Serialize)]
pub struct WhisperInfo {
    pub found: bool, pub path: Option<String>, pub source: Option<Source>,
    pub model_found: bool, pub model_path: Option<String>, pub message: String,
    /// StreamWave's own copy ("Set it up for me") is installed
    pub managed: bool,
    /// that copy's folder was added to the user's PATH (Windows)
    pub path_added: bool,
    /// the file chosen in settings no longer exists (so a later step was used, or nothing)
    pub chosen_missing: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Segment { pub start: f64, pub end: f64, pub text: String }

/// The CLI's file name on this OS.
pub fn cli_name() -> &'static str { if cfg!(windows) { "whisper-cli.exe" } else { "whisper-cli" } }

/// Where to look, in priority order. Kept as data so the order is unit-testable.
pub struct Locations<'a> {
    pub chosen: Option<&'a Path>,
    pub managed_bin: Option<&'a Path>,
    pub exe_dir: Option<&'a Path>,
    pub path_dirs: &'a [PathBuf],
}

pub fn resolve(l: &Locations) -> Option<(PathBuf, Source)> {
    if let Some(c) = l.chosen { if c.is_file() && plausible_name(c) { return Some((c.to_path_buf(), Source::Settings)) } }
    if let Some(d) = l.managed_bin { let p = d.join(cli_name()); if p.is_file() { return Some((p, Source::Managed)) } }
    if let Some(d) = l.exe_dir { let p = d.join(cli_name()); if p.is_file() { return Some((p, Source::Bundled)) } }
    for d in l.path_dirs { let p = d.join(cli_name()); if p.is_file() { return Some((p, Source::Path)) } }
    None
}

/// A file the user picks must at least be named like whisper.cpp's CLI (whisper-cli, whisper-cpp, whisper…);
/// `main` and other generic names are refused.
pub fn plausible_name(p: &Path) -> bool {
    let Some(n) = p.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { return false };
    let n = n.strip_suffix(".exe").unwrap_or(&n).to_string();
    n.starts_with("whisper")
}

/// Run `<bin> --help` (no shell, 10 s limit) and check it is whisper.cpp's CLI: its help lists `--output-json`,
/// which OpenAI's Python `whisper` does not have. Returns the first help line on success.
pub fn validate_cli(bin: &Path) -> Result<String, String> {
    if !bin.is_file() { return Err(format!("{} doesn't exist", bin.display())) }
    if !plausible_name(bin) { return Err("That file isn't named like whisper.cpp's program (whisper-cli). Choose whisper-cli (whisper-cli.exe on Windows).".into()) }
    let dir = std::env::temp_dir().join(format!("sw_whisper_check_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let (o, e) = (dir.join("o.txt"), dir.join("e.txt"));
    let mut cmd = Command::new(bin);
    cmd.arg("--help").stdin(std::process::Stdio::null());
    if let (Ok(fo), Ok(fe)) = (std::fs::File::create(&o), std::fs::File::create(&e)) { cmd.stdout(fo).stderr(fe); }
    hide_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("couldn't run {}: {e}", bin.display()))?;
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed() < Duration::from_secs(10) => std::thread::sleep(Duration::from_millis(50)),
            _ => { let _ = child.kill(); let _ = child.wait(); let _ = std::fs::remove_dir_all(&dir); return Err(format!("{} didn't answer within 10 seconds", bin.display())) }
        }
    }
    let text = format!("{}{}", std::fs::read_to_string(&o).unwrap_or_default(), std::fs::read_to_string(&e).unwrap_or_default());
    let _ = std::fs::remove_dir_all(&dir);
    if text.contains("--output-json") { Ok(text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()) }
    else if text.trim().is_empty() { Err(format!("{} started but printed nothing; it may be missing files it needs (keep it in its own folder with its .dll/.so files)", bin.display())) }
    else { Err(format!("{} doesn't look like whisper.cpp's whisper-cli (its --help has no --output-json)", bin.display())) }
}

fn exe_dir() -> Option<PathBuf> { std::env::current_exe().ok().and_then(|m| m.parent().map(|d| d.to_path_buf())) }

/// What the app knows: the chosen file (settings), StreamWave's own install, and the model chosen in settings.
#[derive(Clone, Default)]
pub struct Config { pub chosen: Option<PathBuf>, pub model: Option<PathBuf>, pub data_dir: Option<PathBuf> }

impl Config {
    fn managed_bin(&self) -> Option<PathBuf> { self.data_dir.as_ref().map(|d| crate::whisper_setup::bin_dir(d)) }
    pub fn find(&self) -> Option<(PathBuf, Source)> {
        let path_dirs = crate::export::tool_dirs();
        let (mb, ed) = (self.managed_bin(), exe_dir());
        resolve(&Locations { chosen: self.chosen.as_deref(), managed_bin: mb.as_deref(), exe_dir: ed.as_deref(), path_dirs: &path_dirs })
    }
    pub fn find_model(&self) -> Option<PathBuf> {
        let managed = self.data_dir.as_ref().map(|d| crate::whisper_setup::models_dir(d));
        find_model(self.model.as_deref(), managed.as_deref(), exe_dir().as_deref())
    }
}

/// A ggml model: the chosen file, else one StreamWave downloaded, else any `ggml-*.bin` next to the exe.
pub fn find_model(chosen: Option<&Path>, managed: Option<&Path>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(m) = chosen { if m.is_file() { return Some(m.to_path_buf()) } }
    for dir in [managed, exe_dir].into_iter().flatten() {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            n.starts_with("ggml-") && n.ends_with(".bin") && p.is_file()
        }).collect();
        v.sort();
        if let Some(p) = v.into_iter().next() { return Some(p) }
    }
    None
}

pub fn info(cfg: &Config) -> WhisperInfo {
    let managed = cfg.data_dir.as_ref().map(|d| crate::whisper_setup::bin_dir(d).join(cli_name()).is_file()).unwrap_or(false);
    let path_added = cfg.data_dir.as_ref().map(|d| crate::whisper_setup::read_manifest(d).map(|m| m.path_added).unwrap_or(false)).unwrap_or(false);
    let chosen_missing = cfg.chosen.as_ref().map(|c| !c.is_file()).unwrap_or(false);
    let m = cfg.find_model();
    let model_path = m.as_ref().map(|p| p.to_string_lossy().to_string());
    match cfg.find() {
        Some((p, source)) => {
            let message = if m.is_some() { "Whisper ready".into() } else {
                "whisper-cli is installed, but it needs a speech model (a ggml .bin file) to work.".into() };
            WhisperInfo { found: true, path: Some(p.to_string_lossy().into()), source: Some(source), model_found: m.is_some(), model_path, message, managed, path_added, chosen_missing }
        }
        None => WhisperInfo { found: false, path: None, source: None, model_found: m.is_some(), model_path, managed, path_added, chosen_missing,
            message: "whisper-cli wasn't found.".into() },
    }
}

/// Transcribe an audio file, returning timestamped segments (track-time seconds).
pub fn transcribe(audio: &Path, cfg: &Config) -> anyhow::Result<Vec<Segment>> {
    let (bin, _) = cfg.find().ok_or_else(|| anyhow::anyhow!("whisper-cli wasn't found. Open the Transcript panel to set it up."))?;
    let model = cfg.find_model().ok_or_else(|| anyhow::anyhow!("no Whisper model. Download one from the Transcript panel, or choose a ggml .bin in Settings"))?;
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
    fn touch(p: &Path) { std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, b"x").unwrap(); }
    #[test] fn resolution_order() {
        let t = std::env::temp_dir().join(format!("sw_wres_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (chosen_dir, managed, exe, p1, p2) = (t.join("chosen"), t.join("managed"), t.join("exe"), t.join("path1"), t.join("path2"));
        let chosen = chosen_dir.join(cli_name());
        let path_dirs = vec![p1.clone(), p2.clone()];
        let loc = |c: Option<&Path>| -> Option<(PathBuf, Source)> {
            resolve(&Locations { chosen: c, managed_bin: Some(&managed), exe_dir: Some(&exe), path_dirs: &path_dirs })
        };
        assert_eq!(loc(Some(&chosen)), None);
        // PATH: only whisper-cli, never `main` or a bare `whisper`
        let n = if cfg!(windows) { ["main.exe", "whisper.exe"] } else { ["main", "whisper"] };
        for x in n { touch(&p1.join(x)) }
        touch(&exe.join(n[0]));
        assert_eq!(loc(None), None, "main / whisper are never picked up");
        touch(&p2.join(cli_name()));
        assert_eq!(loc(None), Some((p2.join(cli_name()), Source::Path)));
        touch(&p1.join(cli_name()));
        assert_eq!(loc(None), Some((p1.join(cli_name()), Source::Path)), "PATH order is kept");
        touch(&exe.join(cli_name()));
        assert_eq!(loc(None).unwrap().1, Source::Bundled);
        touch(&managed.join(cli_name()));
        assert_eq!(loc(None), Some((managed.join(cli_name()), Source::Managed)), "StreamWave's own copy beats PATH");
        assert_eq!(loc(Some(&chosen)).unwrap().1, Source::Managed, "a chosen file that's gone falls through");
        touch(&chosen);
        assert_eq!(loc(Some(&chosen)), Some((chosen.clone(), Source::Settings)), "the user's choice comes first");
        let bad = chosen_dir.join(if cfg!(windows) { "main.exe" } else { "main" }); touch(&bad);
        assert_eq!(loc(Some(&bad)).unwrap().1, Source::Managed, "a chosen file named main is ignored");
        let _ = std::fs::remove_dir_all(&t);
    }
    #[test] fn names() {
        assert!(plausible_name(Path::new("/usr/bin/whisper-cli")));
        assert!(plausible_name(Path::new("Whisper-CLI.EXE")));
        assert!(plausible_name(Path::new("/opt/whisper-cpp")));
        assert!(!plausible_name(Path::new("/usr/bin/main")));
        assert!(!plausible_name(Path::new("/usr/bin/bash")));
    }
    #[test] fn model_order() {
        let t = std::env::temp_dir().join(format!("sw_wmod_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (m, e) = (t.join("models"), t.join("exe"));
        assert_eq!(find_model(None, Some(&m), Some(&e)), None);
        touch(&e.join("ggml-tiny.bin"));
        assert_eq!(find_model(None, Some(&m), Some(&e)), Some(e.join("ggml-tiny.bin")));
        touch(&m.join("ggml-base.en.bin")); touch(&m.join("notes.txt"));
        assert_eq!(find_model(None, Some(&m), Some(&e)), Some(m.join("ggml-base.en.bin")));
        let c = t.join("mine.bin"); touch(&c);
        assert_eq!(find_model(Some(&c), Some(&m), Some(&e)), Some(c.clone()));
        let _ = std::fs::remove_dir_all(&t);
    }
    #[cfg(unix)]
    #[test] fn validates_a_chosen_cli() {
        use std::os::unix::fs::PermissionsExt;
        let t = std::env::temp_dir().join(format!("sw_wval_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t); std::fs::create_dir_all(&t).unwrap();
        let mk = |name: &str, body: &str| { let p = t.join(name); std::fs::write(&p, body).unwrap(); std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap(); p };
        let good = mk("whisper-cli", "#!/bin/sh\necho 'usage: whisper-cli [options] file0'\necho '  -oj, --output-json  output result in a JSON file' >&2\n");
        assert_eq!(validate_cli(&good).unwrap(), "usage: whisper-cli [options] file0");
        let py = mk("whisper", "#!/bin/sh\necho 'usage: whisper [-h] [--model MODEL] [--output_format {txt,json}] audio'\n");
        assert!(validate_cli(&py).unwrap_err().contains("doesn't look like"));
        let main = mk("main", "#!/bin/sh\necho --output-json\n");
        assert!(validate_cli(&main).is_err(), "never runs a file called main");
        assert!(validate_cli(&t.join("whisper-missing")).is_err());
        let _ = std::fs::remove_dir_all(&t);
    }
    #[test] fn clock_parsing() {
        assert_eq!(clock_to_secs("00:01:23,456"), Some(83.456));
        assert_eq!(clock_to_secs("01:23.5"), Some(83.5));
        assert_eq!(clock_to_secs("7.25"), Some(7.25));
    }
}
