//! .twt project files (JSON). The frontend owns the schema; Rust does atomic, crash-safe I/O
//! plus autosave/recovery.
use std::path::{Path, PathBuf};

/// `format` tag in `.twt` files ("StreamWave project").
pub const FORMAT: &str = "streamwave-project";
/// Projects saved before the rename (TrueWave Tracker) still open.
pub const LEGACY_FORMAT: &str = "truewave-project";

pub fn write_atomic(path: &Path, json: &str) -> anyhow::Result<()> {
    let v: serde_json::Value = serde_json::from_str(json)?; // refuse to write garbage
    anyhow::ensure!(matches!(v.get("format").and_then(|f| f.as_str()), Some(FORMAT | LEGACY_FORMAT)), "not a StreamWave project");
    if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
    let tmp = path.with_extension("twt.tmp");
    { use std::io::Write; let mut f = std::fs::File::create(&tmp)?; f.write_all(json.as_bytes())?; f.sync_all()?; }
    if path.exists() { let _ = std::fs::copy(path, path.with_extension("twt.bak")); }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read(path: &Path) -> anyhow::Result<String> {
    let s = std::fs::read_to_string(path)?;
    let v: serde_json::Value = serde_json::from_str(&s).map_err(|e| anyhow::anyhow!("project file is damaged: {e}"))?;
    anyhow::ensure!(matches!(v.get("format").and_then(|f| f.as_str()), Some(FORMAT | LEGACY_FORMAT)), "not a StreamWave project");
    Ok(s)
}

pub fn autosave_path(config_dir: &Path) -> PathBuf { config_dir.join("autosave.twt") }

#[cfg(test)]
mod tests {
    #[test] fn legacy_truewave_projects_still_open() {
        let d = std::env::temp_dir().join(format!("sw_legacy_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("old.twt");
        std::fs::write(&p, r#"{"format":"truewave-project","version":1,"tracks":[]}"#).unwrap();
        assert!(read(&p).is_ok());
        std::fs::write(&p, r#"{"format":"something-else","version":1,"tracks":[]}"#).unwrap();
        assert!(read(&p).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
    use super::*;
    #[test] fn round_trip_and_rejects_garbage() {
        let dir = std::env::temp_dir().join("tw_proj_test"); let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("a.twt");
        let json = r#"{"format":"streamwave-project","version":1,"tracks":[{"id":1,"clips":[{"src":"x.wav","start":0,"end":1}],"fx":[{"bypass":false,"fx":{"type":"NoiseReduction","reduction_db":18,"sensitivity_db":6,"profile":[0.5,0.25]}}]}]}"#;
        write_atomic(&p, json).unwrap();
        assert_eq!(read(&p).unwrap(), json);
        write_atomic(&p, json).unwrap(); assert!(p.with_extension("twt.bak").exists());
        assert!(write_atomic(&p, "{\"hello\":1}").is_err());
        assert_eq!(read(&p).unwrap(), json); // failed save left the old file intact
        std::fs::write(dir.join("bad.twt"), "{oops").unwrap(); assert!(read(&dir.join("bad.twt")).is_err());
    }
}
