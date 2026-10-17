mod audio;
mod obs;
mod render;
mod fx;
mod project;
mod export;
mod studio;
mod media;
mod settings;
mod waveform;

use audio::{devices, AudioHandle, Cmd};
use parking_lot::Mutex;
use settings::{LatencyProfile, Settings};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

struct AppState { audio: AudioHandle, settings: Mutex<Settings>, config_dir: PathBuf, default_rec_dir: PathBuf }

impl AppState {
    fn rec_dir(&self) -> PathBuf { self.settings.lock().record_dir.clone().map(PathBuf::from).unwrap_or_else(|| self.default_rec_dir.clone()) }
    fn save(&self) -> Result<(), String> { self.settings.lock().save(&self.config_dir).map_err(|e| e.to_string()) }
    fn send(&self, c: Cmd) -> Result<(), String> { self.audio.tx.send(c).map_err(|_| "audio engine stopped".to_string()) }
}

#[tauri::command] fn list_devices() -> Vec<devices::DeviceInfo> { devices::list_devices() }
#[tauri::command] fn list_hosts() -> Vec<String> { devices::host_ids().iter().map(|h| h.name().to_string()).collect() }
#[tauri::command] fn get_settings(st: State<AppState>) -> Settings { st.settings.lock().clone() }
#[tauri::command] fn recordings_dir(st: State<AppState>) -> String { st.rec_dir().to_string_lossy().into() }

#[tauri::command]
fn select_input(st: State<AppState>, id: Option<String>) -> Result<(), String> {
    st.settings.lock().input_device = id.clone(); st.save()?; st.send(Cmd::SelectInput(id))
}
#[tauri::command]
fn select_output(st: State<AppState>, id: Option<String>) -> Result<(), String> {
    st.settings.lock().output_device = id.clone(); st.save()?; st.send(Cmd::SelectOutput(id))
}
#[tauri::command]
fn set_latency(st: State<AppState>, profile: LatencyProfile, auto_backoff: bool) -> Result<(), String> {
    { let mut s = st.settings.lock(); s.latency = profile; s.auto_backoff = auto_backoff; }
    st.save()?; st.send(Cmd::SetLatency(profile, auto_backoff))
}
#[tauri::command]
fn save_settings(st: State<AppState>, settings: Settings) -> Result<(), String> { *st.settings.lock() = settings; st.save() }

#[tauri::command] fn start_recording(st: State<AppState>) -> Result<(), String> { let d = st.rec_dir(); st.send(Cmd::StartRecord(d)) }
#[tauri::command] fn pause_recording(st: State<AppState>, pause: bool) -> Result<(), String> { st.send(Cmd::PauseRecord(pause)) }
#[tauri::command] fn stop_recording(st: State<AppState>) -> Result<(), String> { st.send(Cmd::StopRecord) }
#[tauri::command]
async fn play_file(st: State<'_, AppState>, path: String, start: Option<f64>) -> Result<(), String> {
    let d = tauri::async_runtime::spawn_blocking(move || waveform::decode_file(std::path::Path::new(&path))).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    st.send(Cmd::PlaySession(audio::playback::PlayRequest { dry: std::sync::Arc::new(d), slots: vec![], gains: vec![], start_secs: start.unwrap_or(0.0) }))
}

/// Start real-time playback of the edited clips; effects run live and can be changed with `update_fx`.
#[tauri::command]
async fn play_clips(st: State<'_, AppState>, cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>, start: Option<f64>) -> Result<(), String> {
    let d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    let slots = fx.unwrap_or_default();
    let (d, slots, gains) = tauri::async_runtime::spawn_blocking(move || {
        let g = fx::resolve_gains(&d, &slots, None).unwrap_or_else(|| vec![None; slots.len()]);
        (d, slots, g)
    }).await.map_err(|e| e.to_string())?;
    st.send(Cmd::PlaySession(audio::playback::PlayRequest { dry: std::sync::Arc::new(d), slots, gains, start_secs: start.unwrap_or(0.0) }))
}
#[tauri::command] fn update_fx(st: State<AppState>, fx: Vec<fx::Slot>) -> Result<(), String> { st.send(Cmd::UpdateFx(fx)) }
#[tauri::command] fn pause_playback(st: State<AppState>, pause: bool) -> Result<(), String> { st.send(Cmd::PausePlay(pause)) }

#[tauri::command]
fn set_preroll(st: State<AppState>, secs: f32, prepend: bool) -> Result<(), String> {
    { let mut s = st.settings.lock(); s.preroll_secs = secs.clamp(0.0, 60.0); s.preroll_prepend = prepend; }
    st.save()?; st.send(Cmd::SetPreRoll(secs, prepend))
}
#[tauri::command] fn save_last(st: State<AppState>, secs: f32) -> Result<(), String> { let d = st.rec_dir(); st.send(Cmd::SaveLast(secs, d)) }

// ---- projects ----
#[tauri::command]
fn project_save(st: State<AppState>, path: String, json: String) -> Result<(), String> {
    project::write_atomic(std::path::Path::new(&path), &json).map_err(|e| e.to_string())?;
    st.settings.lock().add_recent(&path); st.save()
}
#[tauri::command]
fn project_open(st: State<AppState>, path: String) -> Result<String, String> {
    let r = project::read(std::path::Path::new(&path));
    match &r { Ok(_) => st.settings.lock().add_recent(&path), Err(_) => st.settings.lock().recent_projects.retain(|p| p != &path) }
    st.save()?; r.map_err(|e| e.to_string())
}
#[tauri::command] fn recent_projects(st: State<AppState>) -> Vec<String> { st.settings.lock().recent_projects.clone() }
#[tauri::command] fn autosave_write(st: State<AppState>, json: String) -> Result<(), String> { project::write_atomic(&project::autosave_path(&st.config_dir), &json).map_err(|e| e.to_string()) }
#[tauri::command] fn autosave_check(st: State<AppState>) -> Option<String> { project::read(&project::autosave_path(&st.config_dir)).ok() }
#[tauri::command] fn autosave_clear(st: State<AppState>) { let _ = std::fs::remove_file(project::autosave_path(&st.config_dir)); }

// ---- export ----
#[tauri::command] fn ffmpeg_status() -> export::FfmpegInfo { export::ffmpeg_info() }
#[tauri::command]
async fn export_track(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>, path: String, options: export::ExportOptions) -> Result<String, String> {
    let mut d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    fx::process_chain(&mut d, &fx.unwrap_or_default());
    let p = path.clone();
    tauri::async_runtime::spawn_blocking(move || export::export(&d, std::path::Path::new(&p), &options)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    Ok(path)
}

// ---- Studio Sound ----
#[derive(serde::Serialize)]
struct StudioResult { analysis: studio::Analysis, chain: Vec<fx::Slot> }
#[tauri::command]
async fn studio_sound(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, strength: studio::Strength) -> Result<StudioResult, String> {
    let d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || { let a = studio::analyse(&d); let chain = studio::build_chain(&a, strength); StudioResult { analysis: a, chain } })
        .await.map_err(|e| e.to_string())
}

// ---- OBS media ----
#[tauri::command]
async fn obs_split(st: State<'_, AppState>, path: String) -> Result<Vec<media::SplitTrack>, String> {
    let p = std::path::PathBuf::from(&path);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let out = st.rec_dir().join("OBS imports").join(stem);
    tauri::async_runtime::spawn_blocking(move || media::split_audio(&p, &out)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}
#[tauri::command]
async fn media_probe(path: String) -> Result<media::MediaInfo, String> {
    tauri::async_runtime::spawn_blocking(move || media::probe(std::path::Path::new(&path))).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}
#[tauri::command]
async fn send_to_video(cache: State<'_, render::DecodeCache>, video: String, tracks: Vec<render::TrackSpec>, markers: Vec<media::Marker>, out: String) -> Result<String, String> {
    let mix = render::render_mix(&cache, &tracks).map_err(|e| e.to_string())?;
    let o = out.clone();
    tauri::async_runtime::spawn_blocking(move || media::remux(std::path::Path::new(&video), &mix, &markers, std::path::Path::new(&o)))
        .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    Ok(out)
}

/// Learn a noise profile from a (noise-only) selection, for the NoiseReduction effect.
#[tauri::command]
async fn learn_noise(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>) -> Result<Vec<f32>, String> {
    let d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    if d.samples.len() / d.channels < fx::NR_FFT { return Err("Select at least ~0.1 s of noise-only audio".into()) }
    Ok(fx::learn_noise_profile(&d))
}

/// Integrated loudness (LUFS) of the processed track, for the UI readout.
#[tauri::command]
async fn measure_lufs(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>) -> Result<Option<f64>, String> {
    let mut d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    fx::process_chain(&mut d, &fx.unwrap_or_default());
    Ok(fx::integrated_lufs(&d))
}

/// Export the edited clip list to a new WAV file (originals untouched).
#[tauri::command]
async fn export_clips(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>, path: String) -> Result<String, String> {
    let mut d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    fx::process_chain(&mut d, &fx.unwrap_or_default());
    render::write_wav(&d, std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(path)
}
#[tauri::command] fn stop_playback(st: State<AppState>) -> Result<(), String> { st.send(Cmd::StopPlay) }

#[tauri::command]
async fn import_audio(path: String, buckets: Option<usize>) -> Result<waveform::Waveform, String> {
    tauri::async_runtime::spawn_blocking(move || waveform::analyze(std::path::Path::new(&path), buckets.unwrap_or(4000)))
        .await.map_err(|e| e.to_string())?.map_err(|e| format!("Could not import: {e}"))
}

#[tauri::command]
async fn obs_connect(app: tauri::AppHandle, st: State<'_, AppState>, obs: State<'_, Arc<obs::ObsState>>, host: String, port: u16, password: String) -> Result<obs::ObsStatus, String> {
    { let mut s = st.settings.lock(); s.obs_host = host.clone(); s.obs_port = port; s.obs_password = password.clone(); }
    st.save()?;
    obs::connect(app, obs.inner().clone(), host, port, password).await
}
#[tauri::command] async fn obs_disconnect(app: tauri::AppHandle, obs: State<'_, Arc<obs::ObsState>>) -> Result<(), String> { obs::disconnect(&app, &obs).await; Ok(()) }
#[tauri::command] async fn obs_status(obs: State<'_, Arc<obs::ObsState>>) -> Result<obs::ObsStatus, String> { obs::status(&obs).await }
#[tauri::command] async fn obs_start_record(obs: State<'_, Arc<obs::ObsState>>) -> Result<(), String> { obs::start_record(&obs).await }
#[tauri::command] async fn obs_pause_record(obs: State<'_, Arc<obs::ObsState>>, pause: bool) -> Result<(), String> { obs::pause_record(&obs, pause).await }
#[tauri::command] async fn obs_stop_record(obs: State<'_, Arc<obs::ObsState>>) -> Result<String, String> { obs::stop_record(&obs).await }

fn build_menu(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let it = |id: &str, text: &str, acc: Option<&str>| MenuItem::with_id(app, id, text, true, acc);
    let file = Submenu::with_items(app, "File", true, &[
        &it("new", "New Project", Some("CmdOrCtrl+N"))?,
        &it("open", "Open Project...", Some("CmdOrCtrl+O"))?,
        &it("save", "Save Project", Some("CmdOrCtrl+S"))?,
        &it("saveas", "Save Project As...", Some("CmdOrCtrl+Shift+S"))?,
        &PredefinedMenuItem::separator(app)?,
        &it("import", "Import Audio...", Some("CmdOrCtrl+I"))?,
        &it("export", "Export...", Some("CmdOrCtrl+E"))?,
        &it("sendvideo", "Send Back to Video...", None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, None)?,
    ])?;
    let edit = Submenu::with_items(app, "Edit", true, &[
        &it("undo", "Undo", Some("CmdOrCtrl+Z"))?,
        &it("redo", "Redo", Some("CmdOrCtrl+Y"))?,
        &it("redo", "Redo", Some("CmdOrCtrl+Shift+Z"))?,
        &PredefinedMenuItem::separator(app)?,
        &it("cut", "Cut", Some("CmdOrCtrl+X"))?,
        &it("copy", "Copy", Some("CmdOrCtrl+C"))?,
        &it("paste", "Paste", Some("CmdOrCtrl+V"))?,
        &it("delete", "Delete  (Del / Backspace)", None)?,
        &PredefinedMenuItem::separator(app)?,
        &it("selectall", "Select All", Some("CmdOrCtrl+A"))?,
    ])?;
    let transport = Submenu::with_items(app, "Transport", true, &[
        &it("playpause", "Play / Pause  (Space)", None)?,
        &it("record", "Record  (R)", None)?,
        &it("pause", "Pause / Resume Recording", Some("CmdOrCtrl+P"))?,
        &it("marker", "Add Marker  (M)", None)?,
        &it("prevmarker", "Previous Marker  ([)", None)?,
        &it("nextmarker", "Next Marker  (])", None)?,
        &it("savelast", "Save Last N Seconds (pre-record buffer)", Some("CmdOrCtrl+Shift+R"))?,
    ])?;
    app.set_menu(Menu::with_items(app, &[&file, &edit, &transport])?)?;
    app.on_menu_event(|app, ev| { let _ = app.emit("menu", ev.id().0.clone()); });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let default_rec_dir = app.path().audio_dir().or_else(|_| app.path().document_dir()).unwrap_or_else(|_| config_dir.clone()).join("StreamWave");
            let settings = Settings::load(&config_dir);
            let rec_dir = settings.record_dir.clone().map(PathBuf::from).unwrap_or_else(|| default_rec_dir.clone());
            // crash recovery: repair any recording that was interrupted last time
            let recovered = audio::wavfix::recover_dir(&rec_dir);
            let audio = audio::spawn(app.handle().clone(), settings.input_device.clone(), settings.output_device.clone(), settings.latency, settings.auto_backoff, (settings.preroll_secs, settings.preroll_prepend));
            app.manage(AppState { audio, settings: Mutex::new(settings), config_dir, default_rec_dir });
            app.manage(Arc::new(obs::ObsState::default()));
            app.manage(render::DecodeCache::default());
            build_menu(app)?;
            if !recovered.is_empty() {
                let h = app.handle().clone();
                let list: Vec<String> = recovered.iter().map(|p| p.to_string_lossy().to_string()).collect();
                std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs(2)); let _ = h.emit("recordings-recovered", list); });
            }
            // hot-plug detection: poll the device list every 2 s
            let h = app.handle().clone();
            std::thread::Builder::new().name("sw-hotplug".into()).spawn(move || {
                let mut last: Vec<String> = vec![];
                loop {
                    let devs = devices::list_devices();
                    let ids: Vec<String> = devs.iter().map(|d| d.id.clone()).collect();
                    if ids != last {
                        if !last.is_empty() {
                            let added: Vec<_> = ids.iter().filter(|i| !last.contains(i)).cloned().collect();
                            let removed: Vec<_> = last.iter().filter(|i| !ids.contains(i)).cloned().collect();
                            let _ = h.emit("devices-changed", serde_json::json!({ "devices": devs, "added": added, "removed": removed }));
                        }
                        if let Some(st) = h.try_state::<AppState>() { let _ = st.send(Cmd::DevicesChanged(ids.clone())); }
                        last = ids;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_devices, list_hosts, get_settings, recordings_dir, select_input, select_output, set_latency, save_settings,
            start_recording, stop_recording, play_file, play_clips, update_fx, pause_playback, export_clips, learn_noise, measure_lufs, set_preroll, save_last, project_save, project_open, recent_projects, autosave_write, autosave_check, autosave_clear, ffmpeg_status, export_track, studio_sound, obs_split, media_probe, send_to_video, pause_recording, obs_pause_record, stop_playback, import_audio,
            obs_connect, obs_disconnect, obs_status, obs_start_record, obs_stop_record
        ])
        .run(tauri::generate_context!())
        .expect("error while running StreamWave");
}
