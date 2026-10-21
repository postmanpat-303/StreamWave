//! Persisted user settings (remembered devices, latency profile, OBS connection).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum LatencyProfile { LowLatency, #[default] Balanced, Safe }

/// Level-triggered ("voice activated") recording configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceTrigger { pub threshold_db: f32, pub hold_ms: u64 }

impl LatencyProfile {
    /// Buffer size in frames requested from the driver.
    pub fn frames(self) -> u32 { match self { Self::LowLatency => 128, Self::Balanced => 512, Self::Safe => 2048 } }
    /// Next safer profile (used for auto-backoff on xruns).
    pub fn safer(self) -> Option<Self> { match self { Self::LowLatency => Some(Self::Balanced), Self::Balanced => Some(Self::Safe), Self::Safe => None } }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub input_device: Option<String>,   // DeviceInfo.id  ("Host::Device name")
    pub output_device: Option<String>,
    pub latency: LatencyProfile,
    pub auto_backoff: bool,
    pub record_dir: Option<String>,
    pub obs_host: String,
    pub obs_port: u16,
    pub obs_password: String,
    pub obs_auto_import: bool,
    #[serde(default = "d30")] pub preroll_secs: f32,
    pub preroll_prepend: bool,
    pub recent_projects: Vec<String>,
    pub follow_obs: bool,
    /// Also write a −12 dB safety copy of every recording (clipping rescue).
    pub safety_track: bool,
    /// Level-triggered auto record (None = off).
    pub voice_trigger: Option<VoiceTrigger>,
    /// Path to a whisper.cpp ggml model for transcription (None = look next to the exe).
    pub whisper_model: Option<String>,
    /// whisper-cli the user chose ("I already have it"); tried before StreamWave's own copy and PATH
    pub whisper_cli: Option<String>,
    /// first-run setup finished or skipped
    pub onboarded: bool,
    /// quiet update check at startup (at most once a day)
    #[serde(default = "dtrue")] pub update_auto_check: bool,
    /// unix ms of the last automatic update check
    pub update_last_check: Option<i64>,
    /// version the user chose "Skip this version" for
    pub update_skip_version: Option<String>,
    /// keep the playhead in view while playing / recording (View > Follow Playhead, F)
    #[serde(default = "dtrue")] pub follow_playhead: bool,
}
fn dtrue() -> bool { true }
fn d30() -> f32 { 30.0 }

impl Settings {
    pub fn add_recent(&mut self, p: &str) {
        self.recent_projects.retain(|x| x != p);
        self.recent_projects.insert(0, p.to_string());
        self.recent_projects.truncate(10);
    }
    pub fn with_defaults() -> Self {
        Self { obs_host: "localhost".into(), obs_port: 4455, auto_backoff: true, obs_auto_import: true, preroll_secs: 30.0, update_auto_check: true, follow_playhead: true, ..Default::default() }
    }
    fn path(dir: &PathBuf) -> PathBuf { dir.join("settings.json") }
    pub fn load(dir: &PathBuf) -> Self {
        std::fs::read_to_string(Self::path(dir)).ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(Self::with_defaults)
    }
    pub fn save(&self, dir: &PathBuf) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, Self::path(dir))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_check_defaults_on_for_old_settings_files() {
        // a 0.5.x settings.json has no update_* keys
        let s: Settings = serde_json::from_str(r#"{"obs_host":"localhost","obs_port":4455,"onboarded":true}"#).unwrap();
        assert!(s.update_auto_check);
        assert_eq!(s.update_last_check, None);
        assert_eq!(s.update_skip_version, None);
        assert!(Settings::with_defaults().update_auto_check);
        let off: Settings = serde_json::from_str(r#"{"update_auto_check":false,"update_skip_version":"0.6.1"}"#).unwrap();
        assert!(!off.update_auto_check);
        assert_eq!(off.update_skip_version.as_deref(), Some("0.6.1"));
    }
    #[test]
    fn follow_playhead_defaults_on_and_persists_off() {
        let old: Settings = serde_json::from_str(r#"{"obs_host":"localhost","obs_port":4455}"#).unwrap();
        assert!(old.follow_playhead, "settings files from 0.11.0 and earlier get Follow playhead ON");
        assert!(Settings::with_defaults().follow_playhead);
        let off: Settings = serde_json::from_str(r#"{"follow_playhead":false}"#).unwrap();
        assert!(!off.follow_playhead);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert!(!back.follow_playhead);
    }
}
