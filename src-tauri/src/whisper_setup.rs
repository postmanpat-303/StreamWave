//! "Set it up for me" for whisper.cpp: download the official whisper.cpp build for this OS/arch from
//! github.com/ggml-org/whisper.cpp releases plus a ggml speech model from huggingface.co/ggerganov/whisper.cpp,
//! verify both (sha256 from GitHub's asset digest / Hugging Face's LFS id, and size; archives must also unpack
//! cleanly, which checks their CRCs) and install them into StreamWave's app-data folder:
//!
//! ```text
//! <app data>/whisper/bin/      whisper-cli + its libraries (exactly as in the release archive)
//! <app data>/whisper/models/   ggml-*.bin
//! <app data>/whisper/install.json
//! ```
//!
//! StreamWave runs that exact path, so no PATH change is needed. Optionally (Windows only) the bin folder is added
//! to the user's PATH (HKCU\Environment, no admin) so other apps can use it too; Remove undoes both.
//! Only HTTPS URLs on the hosts in [`allowed_url`] are fetched, including every redirect.
//! Also: the per-OS "Open terminal" helper (the user always runs the command themselves).
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const REPO: &str = "ggml-org/whisper.cpp";
pub const HF_REPO: &str = "ggerganov/whisper.cpp";

pub fn root(data: &Path) -> PathBuf { data.join("whisper") }
pub fn bin_dir(data: &Path) -> PathBuf { root(data).join("bin") }
pub fn models_dir(data: &Path) -> PathBuf { root(data).join("models") }
fn manifest_path(data: &Path) -> PathBuf { root(data).join("install.json") }

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Manifest {
    pub release: Option<String>, pub asset: Option<String>, pub asset_sha256: Option<String>,
    pub model: Option<String>, pub model_sha256: Option<String>,
    pub path_added: bool, pub path_dir: Option<String>, pub installed_at: Option<String>,
}
pub fn read_manifest(data: &Path) -> Option<Manifest> { std::fs::read_to_string(manifest_path(data)).ok().and_then(|s| serde_json::from_str(&s).ok()) }
fn write_manifest(data: &Path, m: &Manifest) -> anyhow::Result<()> {
    std::fs::create_dir_all(root(data))?;
    std::fs::write(manifest_path(data), serde_json::to_vec_pretty(m)?)?;
    Ok(())
}

/// The official release archive for an OS/arch (names as published on github.com/ggml-org/whisper.cpp/releases,
/// checked against build b5454 / v1.9.5). None = no official CLI build (macOS: use Homebrew).
pub fn binary_asset(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("whisper-bin-x64.zip"),
        ("windows", "x86") => Some("whisper-bin-Win32.zip"),
        ("windows", "aarch64") => Some("whisper-bin-win-cpu-arm64.zip"),
        ("linux", "x86_64") => Some("whisper-bin-ubuntu-x64.tar.gz"),
        ("linux", "aarch64") => Some("whisper-bin-ubuntu-arm64.tar.gz"),
        _ => None,
    }
}

/// Speech models offered (from huggingface.co/ggerganov/whisper.cpp). Sizes and sha256 are the published LFS
/// values, used if the Hugging Face API can't be reached; the live values win when it can.
pub const MODELS: [(&str, &str, u64, &str); 3] = [
    ("ggml-base.en.bin", "Base, English only (recommended)", 147_964_211, "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"),
    ("ggml-base.bin", "Base, many languages", 147_951_465, "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
    ("ggml-tiny.en.bin", "Tiny, English only (faster, less accurate)", 77_704_715, "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f"),
];

/// Every URL fetched (and every redirect) must be HTTPS on one of these hosts.
pub fn allowed_url(u: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(u) else { return false };
    if url.scheme() != "https" || url.port().is_some_and(|p| p != 443) || !url.username().is_empty() { return false }
    let Some(h) = url.host_str() else { return false };
    let h = h.to_ascii_lowercase();
    matches!(h.as_str(), "github.com" | "api.github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com" | "huggingface.co")
        || h.ends_with(".huggingface.co") || h.ends_with(".hf.co")
}

/// The stable release notes link the matching numbered build ("Nightly build: [b5454](…/releases/tag/b5454)"),
/// which is where the binaries are attached.
pub fn build_tag_from_body(body: &str) -> Option<String> {
    let key = format!("github.com/{REPO}/releases/tag/b");
    let i = body.find(&key)? + key.len();
    let digits: String = body[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() { None } else { Some(format!("b{digits}")) }
}

#[derive(Serialize, Clone, Debug)]
pub struct Download {
    /// "binary" or "model"
    pub kind: String,
    pub name: String,
    pub url: String,
    pub host: String,
    pub size: u64,
    pub sha256: Option<String>,
    /// e.g. "b5454 (whisper.cpp v1.9.5)"
    pub version: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Plan {
    pub os: String, pub arch: String,
    pub downloads: Vec<Download>,
    pub total: u64,
    pub dest: String,
    /// why there is no program download (e.g. macOS), if there isn't one
    pub binary_note: Option<String>,
    pub can_add_path: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Progress { pub stage: String, pub name: String, pub got: u64, pub total: u64, pub done_bytes: u64, pub all_bytes: u64 }

/// The last plan shown in the confirm dialog: install uses this one, never a plan sent back from the UI.
#[derive(Default)]
pub struct SetupState { pub cancel: AtomicBool, pub running: AtomicBool, pub plan: parking_lot::Mutex<Option<Plan>> }

/// Binaries only from the whisper.cpp releases, models only from the ggerganov/whisper.cpp model repo.
pub fn official_source(d: &Download) -> bool {
    allowed_url(&d.url) && match d.kind.as_str() {
        "binary" => d.url.starts_with(&format!("https://github.com/{REPO}/releases/download/")),
        "model" => d.url.starts_with(&format!("https://huggingface.co/{HF_REPO}/resolve/main/ggml-")),
        _ => false,
    }
}

pub fn os_arch() -> (&'static str, &'static str) { (std::env::consts::OS, std::env::consts::ARCH) }

fn client() -> anyhow::Result<reqwest::Client> {
    if rustls::crypto::CryptoProvider::get_default().is_none() { let _ = rustls::crypto::ring::default_provider().install_default(); }
    let policy = reqwest::redirect::Policy::custom(|a| {
        if a.previous().len() > 8 { a.error("too many redirects") }
        else if allowed_url(a.url().as_str()) { a.follow() }
        else { let u = a.url().to_string(); a.error(format!("refused redirect to {u} (only HTTPS on GitHub / Hugging Face)")) }
    });
    Ok(reqwest::Client::builder()
        .redirect(policy).https_only(true)
        .user_agent(concat!("StreamWave/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .read_timeout(std::time::Duration::from_secs(60))
        .build()?)
}

async fn get_json(c: &reqwest::Client, url: &str) -> anyhow::Result<serde_json::Value> {
    anyhow::ensure!(allowed_url(url), "refused URL {url}");
    let r = c.get(url).header("Accept", "application/vnd.github+json").send().await?;
    anyhow::ensure!(r.status().is_success(), "{url}: HTTP {}", r.status().as_u16());
    Ok(r.json().await?)
}

fn host_of(u: &str) -> String { reqwest::Url::parse(u).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default() }

/// Pick `asset` out of a GitHub release JSON object.
pub fn asset_from_release(rel: &serde_json::Value, asset: &str, version: Option<String>) -> Option<Download> {
    let a = rel.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset))?;
    let url = a.get("browser_download_url")?.as_str()?.to_string();
    if !url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) { return None }
    let sha256 = a.get("digest").and_then(|d| d.as_str()).and_then(|d| d.strip_prefix("sha256:")).map(|s| s.to_ascii_lowercase());
    let tag = rel.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    Some(Download { kind: "binary".into(), name: asset.into(), host: host_of(&url), url, size: a.get("size")?.as_u64()?, sha256,
        version: Some(match version { Some(v) => format!("{tag} (whisper.cpp {v})"), None => tag }) })
}

async fn find_binary(c: &reqwest::Client, asset: &str) -> anyhow::Result<Download> {
    let api = format!("https://api.github.com/repos/{REPO}/releases");
    // the latest stable release names its build; the archives are attached to that build
    if let Ok(latest) = get_json(c, &format!("{api}/latest")).await {
        let v = latest.get("tag_name").and_then(|t| t.as_str()).map(|s| s.to_string());
        if let Some(d) = asset_from_release(&latest, asset, None) { return Ok(d) }
        if let Some(tag) = latest.get("body").and_then(|b| b.as_str()).and_then(build_tag_from_body) {
            if let Ok(rel) = get_json(c, &format!("{api}/tags/{tag}")).await {
                if let Some(d) = asset_from_release(&rel, asset, v) { return Ok(d) }
            }
        }
    }
    // otherwise the newest published release that has it
    let list = get_json(c, &format!("{api}?per_page=15")).await?;
    for rel in list.as_array().into_iter().flatten() {
        if rel.get("draft").and_then(|d| d.as_bool()).unwrap_or(false) { continue }
        if let Some(d) = asset_from_release(rel, asset, None) { return Ok(d) }
    }
    anyhow::bail!("no whisper.cpp release on GitHub has {asset} right now")
}

async fn find_model(c: &reqwest::Client, file: &str) -> anyhow::Result<Download> {
    let (_, _, size, sha) = MODELS.iter().find(|m| m.0 == file).copied().ok_or_else(|| anyhow::anyhow!("unknown model {file}"))?;
    let url = format!("https://huggingface.co/{HF_REPO}/resolve/main/{file}");
    let mut d = Download { kind: "model".into(), name: file.into(), host: host_of(&url), url, size, sha256: Some(sha.into()), version: None };
    if let Ok(tree) = get_json(c, &format!("https://huggingface.co/api/models/{HF_REPO}/tree/main")).await {
        if let Some(f) = tree.as_array().into_iter().flatten().find(|f| f.get("path").and_then(|p| p.as_str()) == Some(file)) {
            if let Some(s) = f.get("size").and_then(|s| s.as_u64()) { d.size = s }
            if let Some(o) = f.get("lfs").and_then(|l| l.get("oid")).and_then(|o| o.as_str()) { d.sha256 = Some(o.to_ascii_lowercase()) }
        }
    }
    Ok(d)
}

pub async fn plan(data: &Path, need_binary: bool, model: Option<String>) -> anyhow::Result<Plan> {
    let (os, arch) = os_arch();
    let c = client()?;
    let mut downloads = vec![];
    let mut binary_note = None;
    if need_binary {
        match binary_asset(os, arch) {
            Some(a) => downloads.push(find_binary(&c, a).await?),
            None => binary_note = Some(if os == "macos" {
                "whisper.cpp doesn't publish a ready-made Mac program, so install it with Homebrew (the command above). StreamWave can still download the speech model for you.".into()
            } else { format!("whisper.cpp doesn't publish a ready-made program for {os} {arch}; build it from source (see above).") }),
        }
    }
    if let Some(m) = model { downloads.push(find_model(&c, &m).await?) }
    let total = downloads.iter().map(|d| d.size).sum();
    Ok(Plan { os: os.into(), arch: arch.into(), downloads, total, dest: root(data).to_string_lossy().into(), binary_note, can_add_path: os == "windows" })
}

/// Stream `d` into `out`, hashing as it goes; checks size and sha256.
async fn fetch(c: &reqwest::Client, d: &Download, out: &Path, st: &SetupState, done_before: u64, all: u64, emit: &(dyn Fn(Progress) + Sync)) -> anyhow::Result<()> {
    anyhow::ensure!(official_source(d), "refused URL {}", d.url);
    let mut r = c.get(&d.url).send().await?;
    anyhow::ensure!(r.status().is_success(), "{}: HTTP {}", d.name, r.status().as_u16());
    if let Some(len) = r.content_length() { anyhow::ensure!(d.size == 0 || len == d.size, "{}: server says {len} bytes, expected {}", d.name, d.size) }
    let mut f = std::fs::File::create(out)?;
    let mut h = Sha256::new();
    let mut got = 0u64;
    let mut last = std::time::Instant::now();
    while let Some(chunk) = r.chunk().await? {
        if st.cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled") }
        h.update(&chunk); f.write_all(&chunk)?; got += chunk.len() as u64;
        anyhow::ensure!(d.size == 0 || got <= d.size, "{}: more data than expected", d.name);
        if last.elapsed().as_millis() >= 100 { last = std::time::Instant::now(); emit(Progress { stage: "download".into(), name: d.name.clone(), got, total: d.size, done_bytes: done_before + got, all_bytes: all }) }
    }
    f.flush()?; drop(f);
    emit(Progress { stage: "verify".into(), name: d.name.clone(), got, total: d.size, done_bytes: done_before + got, all_bytes: all });
    anyhow::ensure!(d.size == 0 || got == d.size, "{}: downloaded {got} bytes, expected {} (try again)", d.name, d.size);
    let hex = format!("{:x}", h.finalize());
    if let Some(want) = &d.sha256 { anyhow::ensure!(&hex == want, "{}: checksum mismatch (got {hex}, expected {want}); the file was discarded", d.name) }
    Ok(())
}

/// Path inside the archive with its top folder ("Release/", "whisper-bin-ubuntu-x64/") removed; None for
/// anything unsafe (absolute, `..`) and for the top folder itself.
pub fn strip_top(p: &Path) -> Option<PathBuf> {
    let mut parts = vec![];
    for c in p.components() {
        match c { Component::Normal(s) => parts.push(s.to_os_string()), Component::CurDir => {}, _ => return None }
    }
    let parts = if parts.len() >= 2 { &parts[1..] } else { &parts[..] };
    if parts.is_empty() { return None }
    Some(parts.iter().collect())
}

/// Unpack a verified release archive into `dest` (which must not exist yet).
pub fn extract(archive: &Path, dest: &Path, cancel: &AtomicBool) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if name.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        for i in 0..z.len() {
            if cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled") }
            let mut e = z.by_index(i)?;
            let Some(rel) = e.enclosed_name().and_then(|p| strip_top(&p)) else { continue };
            let out = dest.join(rel);
            if e.is_dir() { if e.name().trim_end_matches('/').contains('/') { std::fs::create_dir_all(&out)?; } continue }
            if let Some(p) = out.parent() { std::fs::create_dir_all(p)? }
            let mut f = std::fs::File::create(&out)?;
            std::io::copy(&mut e, &mut f)?; // reading to the end checks the entry's CRC
        }
    } else if name.ends_with(".tar.gz") {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut t = tar::Archive::new(gz);
        for e in t.entries()? {
            if cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled") }
            let mut e = e?;
            let p = e.path()?.into_owned();
            let Some(rel) = strip_top(&p) else { continue };
            let out = dest.join(&rel);
            let ty = e.header().entry_type();
            if let Some(par) = out.parent() { std::fs::create_dir_all(par)? }
            if ty.is_dir() { if p.components().filter(|c| matches!(c, Component::Normal(_))).count() >= 2 { std::fs::create_dir_all(&out)?; } }
            else if ty.is_file() { e.unpack(&out)?; }
            else if ty.is_symlink() {
                // libwhisper.so -> libwhisper.so.1: only plain names in the same folder
                let target = e.link_name()?.ok_or_else(|| anyhow::anyhow!("bad link in archive"))?.into_owned();
                anyhow::ensure!(target.components().count() == 1 && matches!(target.components().next(), Some(Component::Normal(_))), "unsafe link in archive: {}", target.display());
                #[cfg(unix)] { let _ = std::fs::remove_file(&out); std::os::unix::fs::symlink(&target, &out)?; }
            }
        }
        // read to the end so the gzip CRC is checked
        std::io::copy(&mut t.into_inner(), &mut std::io::sink())?;
    } else { anyhow::bail!("unknown archive type: {name}") }
    Ok(())
}

/// Download, verify and install. Everything is staged in `whisper/.partial` and only moved into place at the end.
pub async fn install(data: &Path, plan: &Plan, add_path: bool, st: &SetupState, emit: &(dyn Fn(Progress) + Sync)) -> anyhow::Result<()> {
    let c = client()?;
    let root = root(data);
    let partial = root.join(".partial");
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial)?;
    let res = async {
        let all: u64 = plan.downloads.iter().map(|d| d.size).sum();
        let mut done = 0u64;
        let mut m = read_manifest(data).unwrap_or_default();
        for d in &plan.downloads {
            anyhow::ensure!(!d.name.contains(['/', '\\']) && !d.name.starts_with('.'), "bad file name {}", d.name);
            let file = partial.join(&d.name);
            fetch(&c, d, &file, st, done, all, emit).await?;
            done += d.size;
            if d.kind == "binary" {
                emit(Progress { stage: "extract".into(), name: d.name.clone(), got: d.size, total: d.size, done_bytes: done, all_bytes: all });
                let staging = partial.join("bin");
                let (f2, s2) = (file.clone(), staging.clone());
                let cancel = std::sync::Arc::new(AtomicBool::new(false));
                tauri::async_runtime::spawn_blocking(move || extract(&f2, &s2, &cancel)).await.map_err(|e| anyhow::anyhow!("{e}"))??;
                let cli = staging.join(crate::whisper::cli_name());
                anyhow::ensure!(cli.is_file(), "{} has no {} inside", d.name, crate::whisper::cli_name());
                let cli2 = cli.clone();
                tauri::async_runtime::spawn_blocking(move || crate::whisper::validate_cli(&cli2)).await.map_err(|e| anyhow::anyhow!("{e}"))?
                    .map_err(|e| anyhow::anyhow!("the downloaded whisper-cli doesn't run on this computer: {e}"))?;
                let bin = bin_dir(data);
                let old = root.join(".old-bin");
                let _ = std::fs::remove_dir_all(&old);
                if bin.exists() { std::fs::rename(&bin, &old)? }
                std::fs::rename(&staging, &bin)?;
                let _ = std::fs::remove_dir_all(&old);
                m.release = d.version.clone(); m.asset = Some(d.name.clone()); m.asset_sha256 = d.sha256.clone();
            } else {
                let dir = models_dir(data);
                std::fs::create_dir_all(&dir)?;
                std::fs::rename(&file, dir.join(&d.name))?;
                m.model = Some(d.name.clone()); m.model_sha256 = d.sha256.clone();
            }
            m.installed_at = Some(chrono::Utc::now().to_rfc3339());
            write_manifest(data, &m)?;
        }
        if add_path && bin_dir(data).join(crate::whisper::cli_name()).is_file() {
            let dir = bin_dir(data);
            if user_path_add(&dir)? { m.path_added = true; m.path_dir = Some(dir.to_string_lossy().into()); write_manifest(data, &m)?; }
        }
        Ok(())
    }.await;
    let _ = std::fs::remove_dir_all(&partial);
    res
}

/// Remove StreamWave's copy (program, models, manifest) and undo the PATH change. Only touches `<app data>/whisper`.
pub fn remove(data: &Path) -> anyhow::Result<()> {
    if let Some(m) = read_manifest(data) {
        if m.path_added { if let Some(d) = m.path_dir { user_path_remove(Path::new(&d))?; } }
    }
    let r = root(data);
    if r.exists() { std::fs::remove_dir_all(&r)? }
    Ok(())
}

// ---- user PATH (Windows: HKCU\Environment, no admin) ----

#[cfg_attr(not(windows), allow(dead_code))]
fn norm(p: &str) -> String { p.trim().trim_matches('"').trim_end_matches(['\\', '/']).to_lowercase() }

/// `;`-separated PATH with `dir` appended, or None if it's already there.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn path_list_add(existing: &str, dir: &str) -> Option<String> {
    if existing.split(';').any(|e| norm(e) == norm(dir)) { return None }
    let base = existing.trim_end_matches(';');
    Some(if base.is_empty() { dir.to_string() } else { format!("{base};{dir}") })
}
/// `;`-separated PATH without `dir`, or None if it wasn't there.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn path_list_remove(existing: &str, dir: &str) -> Option<String> {
    let parts: Vec<&str> = existing.split(';').collect();
    let kept: Vec<&str> = parts.iter().copied().filter(|e| norm(e) != norm(dir)).collect();
    if kept.len() == parts.len() { None } else { Some(kept.join(";")) }
}

#[cfg(windows)]
fn user_path_edit(f: impl Fn(&str) -> Option<String>) -> anyhow::Result<bool> {
    use winreg::{enums::*, RegKey, RegValue};
    let env = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?.0;
    let (cur, vtype) = match env.get_raw_value("Path") {
        Ok(v) => {
            let w: Vec<u16> = v.bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            (String::from_utf16_lossy(&w).trim_end_matches('\0').to_string(), v.vtype)
        }
        Err(_) => (String::new(), REG_EXPAND_SZ),
    };
    let Some(new) = f(&cur) else { return Ok(false) };
    let vtype = if matches!(vtype, REG_SZ | REG_EXPAND_SZ) { vtype } else { REG_EXPAND_SZ };
    let bytes: Vec<u8> = new.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect();
    env.set_raw_value("Path", &RegValue { bytes: bytes.into(), vtype })?;
    broadcast_env_change();
    Ok(true)
}
#[cfg(windows)]
fn broadcast_env_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    let s: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut res = 0usize;
    unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, s.as_ptr() as isize, SMTO_ABORTIFHUNG, 3000, &mut res); }
}
#[cfg(windows)] pub fn user_path_add(dir: &Path) -> anyhow::Result<bool> { let d = dir.to_string_lossy().to_string(); user_path_edit(move |c| path_list_add(c, &d)) }
#[cfg(windows)] pub fn user_path_remove(dir: &Path) -> anyhow::Result<bool> { let d = dir.to_string_lossy().to_string(); user_path_edit(move |c| path_list_remove(c, &d)) }
/// Mac/Linux: not offered (StreamWave uses the file directly; shells read PATH from their own startup files).
#[cfg(not(windows))] pub fn user_path_add(_dir: &Path) -> anyhow::Result<bool> { Ok(false) }
#[cfg(not(windows))] pub fn user_path_remove(_dir: &Path) -> anyhow::Result<bool> { Ok(false) }

// ---- Open terminal ----

/// Shown by the Linux terminal, then the command is put on an editable prompt (bash `read -e -i`): nothing runs
/// until the user presses Enter. The command arrives as `$1`, never spliced into this script.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const PREFILL_SCRIPT: &str = r#"trap 'stty sane 2>/dev/null; echo; exec "${SHELL:-bash}" -i' INT
printf '\033[1mStreamWave\033[0m: here is the command to install whisper.cpp.\nCheck it (edit it if you like), then press Enter to run it, or Ctrl+C to cancel.\n\n'
IFS= read -r -e -i "$1" -p '$ ' line && [ -n "$line" ] && eval "$line"
echo
exec "${SHELL:-bash}" -i"#;

/// Linux terminals to try, best first: the one for the current desktop, the distro default, then common ones.
/// Each entry: program and the flag(s) that come before the command to run.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn linux_terminals(desktop: &str) -> Vec<(&'static str, &'static [&'static str])> {
    const ALL: [(&str, &[&str]); 8] = [
        ("x-terminal-emulator", &["-e"]), ("gnome-terminal", &["--"]), ("konsole", &["-e"]), ("xfce4-terminal", &["-x"]),
        ("kitty", &[]), ("alacritty", &["-e"]), ("ptyxis", &["--"]), ("xterm", &["-e"]),
    ];
    let d = desktop.to_lowercase();
    let first = if d.contains("kde") { Some("konsole") } else if d.contains("xfce") { Some("xfce4-terminal") }
        else if d.contains("gnome") || d.contains("unity") || d.contains("cinnamon") || d.contains("budgie") { Some("gnome-terminal") } else { None };
    let mut v: Vec<(&str, &[&str])> = vec![];
    if let Some(f) = first { v.extend(ALL.iter().filter(|t| t.0 == f).copied()) }
    v.extend(ALL.iter().filter(|t| Some(t.0) != first).copied());
    v
}

#[derive(Serialize)]
pub struct TerminalOpened { pub terminal: String, pub prefilled: bool }

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn which(name: &str) -> Option<PathBuf> { crate::export::tool_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file()) }

fn home() -> Option<PathBuf> { std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from) }

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn spawn_detached(mut c: std::process::Command) -> std::io::Result<()> {
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(h) = home() { c.current_dir(h); }
    let mut ch = c.spawn()?;
    std::thread::spawn(move || { let _ = ch.wait(); });
    Ok(())
}

/// Open a terminal for the user. Linux: with `command` typed in (not run). Windows/macOS can't pre-fill a prompt
/// without running it, so the window opens empty and the app copies the command for pasting.
pub fn open_terminal(command: &str) -> anyhow::Result<TerminalOpened> {
    #[cfg(windows)] {
        let _ = command;
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("powershell.exe");
        c.args(["-NoExit", "-NoLogo", "-Command", "Write-Host 'StreamWave: paste the install command (right-click or Ctrl+V), check it, then press Enter.'"]);
        c.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
        if let Some(h) = home() { c.current_dir(h); }
        let mut ch = c.spawn()?;
        std::thread::spawn(move || { let _ = ch.wait(); });
        return Ok(TerminalOpened { terminal: "PowerShell".into(), prefilled: false });
    }
    #[cfg(target_os = "macos")] {
        let _ = command;
        let mut c = std::process::Command::new("open");
        c.args(["-a", "Terminal"]);
        if let Some(h) = home() { c.arg(h); } // a folder argument opens a new window there
        spawn_detached(c)?;
        return Ok(TerminalOpened { terminal: "Terminal".into(), prefilled: false });
    }
    #[cfg(all(unix, not(target_os = "macos")))] {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let bash = which("bash");
        let mut last_err = None;
        for (t, flags) in linux_terminals(&desktop) {
            let Some(tp) = which(t) else { continue };
            let mut c = std::process::Command::new(&tp);
            c.args(flags.iter());
            let prefilled = if let Some(b) = &bash { c.arg(b).arg("-c").arg(PREFILL_SCRIPT).arg("streamwave").arg(command); true } else { false };
            match spawn_detached(c) {
                Ok(()) => return Ok(TerminalOpened { terminal: t.into(), prefilled }),
                Err(e) => last_err = Some(e),
            }
        }
        anyhow::bail!("couldn't find a terminal program{}", last_err.map(|e| format!(" ({e})")).unwrap_or_default())
    }
    #[allow(unreachable_code)] { anyhow::bail!("not supported on this system") }
}

/// Linux: the /etc/os-release fields the UI uses to pick a package command.
#[derive(Serialize, Default)]
pub struct Distro { pub id: String, pub id_like: String, pub version_id: String, pub name: String }
pub fn parse_os_release(s: &str) -> Distro {
    let mut d = Distro::default();
    for l in s.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_string();
        match k.trim() { "ID" => d.id = v, "ID_LIKE" => d.id_like = v, "VERSION_ID" => d.version_id = v, "PRETTY_NAME" => d.name = v, _ => {} }
    }
    d
}
pub fn distro() -> Distro {
    if !cfg!(target_os = "linux") { return Distro::default() }
    std::fs::read_to_string("/etc/os-release").or_else(|_| std::fs::read_to_string("/usr/lib/os-release")).map(|s| parse_os_release(&s)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn assets_per_os() {
        assert_eq!(binary_asset("windows", "x86_64"), Some("whisper-bin-x64.zip"));
        assert_eq!(binary_asset("windows", "aarch64"), Some("whisper-bin-win-cpu-arm64.zip"));
        assert_eq!(binary_asset("linux", "x86_64"), Some("whisper-bin-ubuntu-x64.tar.gz"));
        assert_eq!(binary_asset("linux", "aarch64"), Some("whisper-bin-ubuntu-arm64.tar.gz"));
        assert_eq!(binary_asset("macos", "aarch64"), None);
        assert_eq!(binary_asset("macos", "x86_64"), None);
    }
    #[test] fn url_allowlist() {
        assert!(allowed_url("https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip"));
        assert!(allowed_url("https://release-assets.githubusercontent.com/github-production-release-asset/1/2"));
        assert!(allowed_url("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"));
        assert!(allowed_url("https://us.aws.cdn.hf.co/xet-bridge-us/abc"));
        assert!(allowed_url("https://cdn-lfs.huggingface.co/x"));
        assert!(!allowed_url("http://github.com/ggml-org/whisper.cpp"));
        assert!(!allowed_url("https://github.com.evil.example/x"));
        assert!(!allowed_url("https://evilhf.co/x"));
        assert!(!allowed_url("https://user@github.com/x"));
        assert!(!allowed_url("https://github.com:8443/x"));
        assert!(!allowed_url("file:///etc/passwd"));
    }
    #[test] fn official_sources_only() {
        let mk = |kind: &str, url: &str| Download { kind: kind.into(), name: "x".into(), url: url.into(), host: String::new(), size: 1, sha256: None, version: None };
        assert!(official_source(&mk("binary", "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip")));
        assert!(!official_source(&mk("binary", "https://github.com/someone/else/releases/download/b1/whisper-bin-x64.zip")));
        assert!(official_source(&mk("model", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin")));
        assert!(!official_source(&mk("model", "https://huggingface.co/other/repo/resolve/main/ggml-base.en.bin")));
        assert!(!official_source(&mk("model", "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip")));
    }
    #[test] fn build_tag() {
        let b = "**Nightly build:** [b5454](https://github.com/ggml-org/whisper.cpp/releases/tag/b5454)\n";
        assert_eq!(build_tag_from_body(b).as_deref(), Some("b5454"));
        assert_eq!(build_tag_from_body("no link"), None);
        assert_eq!(build_tag_from_body("https://github.com/ggml-org/whisper.cpp/releases/tag/bx"), None);
    }
    #[test] fn release_asset_parsing() {
        let rel = serde_json::json!({"tag_name": "b5454", "assets": [
            {"name": "whisper-bin-x64.zip", "size": 8928640, "digest": "sha256:6BA69E", "browser_download_url": "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip"},
            {"name": "evil.zip", "size": 1, "browser_download_url": "https://example.com/evil.zip"}]});
        let d = asset_from_release(&rel, "whisper-bin-x64.zip", Some("v1.9.5".into())).unwrap();
        assert_eq!(d.size, 8928640); assert_eq!(d.sha256.as_deref(), Some("6ba69e"));
        assert_eq!(d.version.as_deref(), Some("b5454 (whisper.cpp v1.9.5)")); assert_eq!(d.host, "github.com");
        assert!(asset_from_release(&rel, "evil.zip", None).is_none(), "assets hosted elsewhere are refused");
        assert!(asset_from_release(&rel, "missing.zip", None).is_none());
    }
    #[test] fn strip_paths() {
        assert_eq!(strip_top(Path::new("Release/whisper-cli.exe")), Some(PathBuf::from("whisper-cli.exe")));
        assert_eq!(strip_top(Path::new("whisper-bin-ubuntu-x64/libggml.so.0")), Some(PathBuf::from("libggml.so.0")));
        assert_eq!(strip_top(Path::new("./Release/sub/x.dll")), Some(PathBuf::from("sub/x.dll")));
        assert_eq!(strip_top(Path::new("a/../../etc/passwd")), None);
        assert_eq!(strip_top(Path::new("/etc/passwd")), None);
    }
    #[test] fn path_lists() {
        assert_eq!(path_list_add(r"C:\a;C:\b", r"C:\w\bin").as_deref(), Some(r"C:\a;C:\b;C:\w\bin"));
        assert_eq!(path_list_add(r"C:\a;C:\W\Bin\", r"c:\w\bin"), None);
        assert_eq!(path_list_add("", r"C:\w").as_deref(), Some(r"C:\w"));
        assert_eq!(path_list_add(r"C:\a;", r"C:\w").as_deref(), Some(r"C:\a;C:\w"));
        assert_eq!(path_list_remove(r"C:\a;C:\w\bin;C:\b", r"C:\W\bin\").as_deref(), Some(r"C:\a;C:\b"));
        assert_eq!(path_list_remove(r"C:\a", r"C:\w"), None);
    }
    #[test] fn terminals_order() {
        let names = |d: &str| linux_terminals(d).iter().map(|t| t.0).collect::<Vec<_>>();
        assert_eq!(names("KDE")[0], "konsole");
        assert_eq!(names("XFCE")[0], "xfce4-terminal");
        assert_eq!(names("ubuntu:GNOME")[0], "gnome-terminal");
        assert_eq!(names("")[0], "x-terminal-emulator");
        assert_eq!(*names("").last().unwrap(), "xterm");
        let k = names("KDE"); assert_eq!(k.iter().filter(|n| **n == "konsole").count(), 1);
        for n in ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "kitty", "alacritty"] { assert!(k.contains(&n)) }
    }
    #[test] fn os_release() {
        let d = parse_os_release("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"26.04\"\nPRETTY_NAME=\"Ubuntu 26.04 LTS\"\n");
        assert_eq!((d.id.as_str(), d.id_like.as_str(), d.version_id.as_str()), ("ubuntu", "debian", "26.04"));
    }
    #[cfg(unix)]
    #[test] fn extract_targz_strips_and_refuses_escapes() {
        let dir = std::env::temp_dir().join(format!("sw_wsx_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let arc = dir.join("t.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&arc).unwrap(), flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu(); h.set_size(3); h.set_mode(0o755); h.set_cksum();
            b.append_data(&mut h, "top/whisper-cli", &b"abc"[..]).unwrap();
            let mut l = tar::Header::new_gnu(); l.set_entry_type(tar::EntryType::Symlink); l.set_size(0); l.set_cksum();
            b.append_link(&mut l, "top/libw.so", "libw.so.1").unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.join("out");
        extract(&arc, &out, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(out.join("whisper-cli")).unwrap(), b"abc");
        assert_eq!(std::fs::read_link(out.join("libw.so")).unwrap(), PathBuf::from("libw.so.1"));
        // a link pointing outside is refused
        let arc2 = dir.join("bad.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&arc2).unwrap(), flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut l = tar::Header::new_gnu(); l.set_entry_type(tar::EntryType::Symlink); l.set_size(0); l.set_cksum();
            b.append_link(&mut l, "top/x", "../../etc/passwd").unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        assert!(extract(&arc2, &dir.join("out2"), &AtomicBool::new(false)).is_err());
        // corrupt data fails the integrity check
        let mut bytes = std::fs::read(&arc).unwrap(); let n = bytes.len(); bytes[n - 9] ^= 0xff; bytes[n / 2] ^= 0xff;
        let arc3 = dir.join("corrupt.tar.gz"); std::fs::write(&arc3, bytes).unwrap();
        assert!(extract(&arc3, &dir.join("out3"), &AtomicBool::new(false)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn extract_zip_strips_top() {
        let dir = std::env::temp_dir().join(format!("sw_wsz_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let arc = dir.join("t.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&arc).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            z.start_file("Release/whisper-cli.exe", o).unwrap(); z.write_all(b"exe").unwrap();
            z.start_file("Release/ggml.dll", o).unwrap(); z.write_all(b"dll").unwrap();
            z.finish().unwrap();
        }
        extract(&arc, &dir.join("o"), &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(dir.join("o/whisper-cli.exe")).unwrap(), b"exe");
        assert!(dir.join("o/ggml.dll").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
