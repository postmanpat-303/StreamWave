//! Device enumeration across every host API cpal was compiled with
//! (WASAPI / ASIO on Windows, ALSA (+Pulse/PipeWire via ALSA plugins) / JACK on Linux, CoreAudio on macOS).
use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DeviceInfo {
    /// Stable id "Host::raw name" used for remembering the choice.
    pub id: String,
    pub host: String,
    pub raw_name: String,
    /// Human friendly name for the UI.
    pub friendly_name: String,
    pub is_input: bool,
    pub is_default: bool,
    pub channels: u16,
    pub sample_rate: u32,
}

/// Turn cryptic driver names into something a human understands.
pub fn friendly_name(host: &str, raw: &str) -> String {
    let mut n = raw.to_string();
    // ALSA style "sysdefault:CARD=USB" / "hw:CARD=PCH,DEV=0"
    if let Some(idx) = n.find("CARD=") {
        let card = n[idx + 5..].split(',').next().unwrap_or("").to_string();
        let kind = n.split(':').next().unwrap_or("");
        let prefix = match kind { "default" | "sysdefault" => "", "hw" => "Direct ", "plughw" => "Direct (converted) ", "front" => "Front ", _ => "" };
        n = format!("{prefix}{card}");
    }
    match raw {
        "default" => n = "System default".into(),
        "pulse" => n = "PulseAudio / PipeWire".into(),
        "pipewire" => n = "PipeWire".into(),
        "jack" => n = "JACK".into(),
        _ => {}
    }
    // WASAPI names look like "Microphone (Realtek(R) Audio)" -> drop (R)/(TM)
    n = n.replace("(R)", "").replace("(TM)", "").replace("  ", " ");
    if host != "Wasapi" && host != "Alsa" { n = format!("{n} [{host}]"); }
    n.trim().to_string()
}

pub fn host_ids() -> Vec<cpal::HostId> { cpal::available_hosts() }

/// StreamWave never takes exclusive control of a device, so OBS/Discord can share the mic.
/// WASAPI is always opened in shared mode (cpal's only mode). On ALSA, raw `hw:`/`plughw:`-style
/// PCMs grab the card exclusively, so we only offer the sound-server routes
/// (default / pulse / pipewire / jack) when any of them exist.
pub fn is_shared_safe(host: &str, raw: &str) -> bool {
    if host != "Alsa" { return true }
    let kind = raw.split(':').next().unwrap_or(raw);
    matches!(kind, "default" | "pulse" | "pipewire" | "jack" | "sysdefault" | "dsnoop" | "dmix")
        && !(kind == "sysdefault" && raw.contains("CARD=")) // sysdefault:CARD=x is a direct hw route
}

pub fn list_devices() -> Vec<DeviceInfo> {
    let all = list_all_devices();
    let shared: Vec<DeviceInfo> = all.iter().filter(|d| is_shared_safe(&d.host, &d.raw_name)).cloned().collect();
    // keep each direction usable even on unusual systems without a sound server
    let has_in = shared.iter().any(|d| d.is_input); let has_out = shared.iter().any(|d| !d.is_input);
    all.into_iter().filter(|d| is_shared_safe(&d.host, &d.raw_name) || (d.is_input && !has_in) || (!d.is_input && !has_out)).collect()
}

fn list_all_devices() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    for hid in host_ids() {
        let Ok(host) = cpal::host_from_id(hid) else { continue };
        let hname = hid.name().to_string();
        let def_in = host.default_input_device().and_then(|d| d.name().ok());
        let def_out = host.default_output_device().and_then(|d| d.name().ok());
        if let Ok(devs) = host.input_devices() {
            for d in devs {
                let Ok(name) = d.name() else { continue };
                let Ok(cfg) = d.default_input_config() else { continue };
                out.push(DeviceInfo { id: format!("{hname}::{name}"), host: hname.clone(), friendly_name: friendly_name(&hname, &name),
                    is_input: true, is_default: def_in.as_deref() == Some(&name), channels: cfg.channels(), sample_rate: cfg.sample_rate().0, raw_name: name });
            }
        }
        if let Ok(devs) = host.output_devices() {
            for d in devs {
                let Ok(name) = d.name() else { continue };
                let Ok(cfg) = d.default_output_config() else { continue };
                out.push(DeviceInfo { id: format!("{hname}::{name}"), host: hname.clone(), friendly_name: friendly_name(&hname, &name),
                    is_input: false, is_default: def_out.as_deref() == Some(&name), channels: cfg.channels(), sample_rate: cfg.sample_rate().0, raw_name: name });
            }
        }
    }
    out
}

/// Resolve a remembered id to a device; falls back to the default device of the default host.
pub fn find_device(id: Option<&str>, input: bool) -> Option<(cpal::Device, String)> {
    if let Some(id) = id {
        if let Some((h, name)) = id.split_once("::").filter(|(h, n)| is_shared_safe(h, n)) {
            for hid in host_ids() {
                if hid.name() != h { continue }
                if let Ok(host) = cpal::host_from_id(hid) {
                    let devs = if input { host.input_devices().ok().map(|d| d.collect::<Vec<_>>()) } else { host.output_devices().ok().map(|d| d.collect::<Vec<_>>()) };
                    if let Some(d) = devs.unwrap_or_default().into_iter().find(|d| d.name().ok().as_deref() == Some(name)) {
                        return Some((d, id.to_string()));
                    }
                }
            }
        }
    }
    let host = cpal::default_host();
    let d = if input { host.default_input_device() } else { host.default_output_device() }?;
    let id = format!("{}::{}", host.id().name(), d.name().unwrap_or_default());
    Some((d, id))
}

#[cfg(test)]
mod tests {
    #[test] fn only_shared_routes_on_alsa() {
        use super::is_shared_safe as s;
        assert!(s("Alsa", "default") && s("Alsa", "pulse") && s("Alsa", "pipewire"));
        assert!(!s("Alsa", "hw:CARD=PCH,DEV=0") && !s("Alsa", "plughw:CARD=USB,DEV=0") && !s("Alsa", "sysdefault:CARD=USB"));
        assert!(s("Wasapi", "Microphone (USB Audio)"));
    }
}
