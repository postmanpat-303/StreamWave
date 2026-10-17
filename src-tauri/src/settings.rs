//! Persisted user settings (remembered devices, latency profile, OBS connection).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum LatencyProfile { LowLatency, #[default] Balanced, Safe }

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
    /// first-run setup finished or skipped
    pub onboarded: bool,
}
fn d30() -> f32 { 30.0 }

impl Settings {
    pub fn add_recent(&mut self, p: &str) {
        self.recent_projects.retain(|x| x != p);
        self.recent_projects.insert(0, p.to_string());
        self.recent_projects.truncate(10);
    }
    pub fn with_defaults() -> Self {
        Self { obs_host: "localhost".into(), obs_port: 4455, auto_backoff: true, obs_auto_import: true, preroll_secs: 30.0, ..Default::default() }
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
