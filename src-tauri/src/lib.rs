mod audio;
mod perf;
mod obs;
mod render;
mod fx;
mod project;
mod export;
mod studio;
mod media;
mod aup3;
mod spectrogram;
mod settings;
mod waveform;
mod trim;
mod whisper;

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
/// How this copy was installed ("appimage", "deb", "rpm", "nsis", "msi", "app", or "unknown" for dev builds).
/// The bundler stamps it into the binary. Only the AppImage, the Windows installer and the Mac app can use the
/// in-app updater (latest.json only lists the AppImage for Linux); .deb/.rpm/Arch packages are updated by hand.
#[tauri::command] fn install_kind() -> &'static str {
    use tauri::utils::config::BundleType;
    match tauri::utils::platform::bundle_type() {
        Some(BundleType::AppImage) => "appimage",
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::Nsis) => "nsis",
        Some(BundleType::Msi) => "msi",
        Some(BundleType::App) | Some(BundleType::Dmg) => "app",
        _ => "unknown",
    }
}

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

/// Dry render + measured gains for a track, from the PlayCache when nothing changed since the last time.
fn prepare_track(app: &tauri::AppHandle, clips: &[render::ClipSpec], slots: &[fx::Slot]) -> anyhow::Result<(std::sync::Arc<waveform::Decoded>, Vec<Option<f32>>)> {
    let (cache, pc) = (app.state::<render::DecodeCache>(), app.state::<render::PlayCache>());
    let key = render::key_of(clips);
    let d = pc.dry(key, || render::render(&cache, clips))?;
    perf::mark("play_clips: dry rendered");
    let gains = if slots.iter().any(|s| !s.bypass && s.fx.is_global()) {
        pc.gains(render::key_of(&(key, slots)), || fx::resolve_gains(&d, slots, None).unwrap_or_else(|| vec![None; slots.len()]))
    } else { vec![None; slots.len()] };
    Ok((d, gains))
}

/// Start real-time playback of the edited clips; effects run live and can be changed with `update_fx`.
/// Unchanged audio is not re-rendered or re-measured (PlayCache), and the output stream stays open, so Play is quick.
#[tauri::command]
async fn play_clips(app: tauri::AppHandle, st: State<'_, AppState>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>, start: Option<f64>) -> Result<(), String> {
    perf::mark("play_clips: command in");
    let slots = fx.unwrap_or_default();
    let (d, slots, gains) = tauri::async_runtime::spawn_blocking(move || prepare_track(&app, &clips, &slots).map(|(d, g)| (d, slots, g)))
        .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    perf::mark("play_clips: gains resolved");
    st.send(Cmd::PlaySession(audio::playback::PlayRequest { dry: d, slots, gains, start_secs: start.unwrap_or(0.0) }))
}
/// Render (and measure) the active track in the background after loads/edits, so the next Play doesn't wait for it.
#[tauri::command]
async fn prerender_clips(app: tauri::AppHandle, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>) -> Result<(), String> {
    if clips.is_empty() { return Ok(()) }
    tauri::async_runtime::spawn_blocking(move || prepare_track(&app, &clips, &fx.unwrap_or_default()).map(|_| ()))
        .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

/// Render every track (with its effects) and play the full mix.
#[tauri::command]
async fn play_mix(st: State<'_, AppState>, cache: State<'_, render::DecodeCache>, pc: State<'_, render::PlayCache>, tracks: Vec<render::TrackSpec>, start: Option<f64>) -> Result<(), String> {
    perf::mark("play_mix: command in");
    let d = pc.dry(render::key_of(&tracks[..]), || render::render_mix(&cache, &tracks)).map_err(|e| e.to_string())?;
    perf::mark("play_mix: mix rendered");
    st.send(Cmd::PlaySession(audio::playback::PlayRequest { dry: d, slots: vec![], gains: vec![], start_secs: start.unwrap_or(0.0) }))
}
/// Hand re-rendered dry audio to the running playback (no restart). Ok(false) = not playing or the format
/// changed, so the app restarts playback instead.
fn hand_over(st: &AppState, d: std::sync::Arc<waveform::Decoded>) -> Result<bool, String> {
    use std::sync::atomic::Ordering::Relaxed;
    let p = &st.audio.shared.play;
    if !(p.playing.load(Relaxed) || p.paused.load(Relaxed)) { return Ok(false) }
    if p.src_rate.load(Relaxed) != d.sample_rate as u64 || p.src_ch.load(Relaxed) != d.channels as u64 { return Ok(false) }
    st.send(Cmd::UpdateDry(d))?; Ok(true)
}
/// Clip effects changed while a track plays: re-render its clips and swap them in at the current position.
#[tauri::command]
async fn update_play_clips(st: State<'_, AppState>, cache: State<'_, render::DecodeCache>, pc: State<'_, render::PlayCache>, clips: Vec<render::ClipSpec>) -> Result<bool, String> {
    let d = pc.dry(render::key_of(&clips[..]), || render::render(&cache, &clips)).map_err(|e| e.to_string())?;
    hand_over(&st, d)
}
/// Same for "Mix all" playback.
#[tauri::command]
async fn update_play_mix(st: State<'_, AppState>, cache: State<'_, render::DecodeCache>, pc: State<'_, render::PlayCache>, tracks: Vec<render::TrackSpec>) -> Result<bool, String> {
    let d = pc.dry(render::key_of(&tracks[..]), || render::render_mix(&cache, &tracks)).map_err(|e| e.to_string())?;
    hand_over(&st, d)
}
/// View > Follow Playhead check mark (kept in step when F or the transport toggle changes it).
struct FollowMenu(tauri::menu::CheckMenuItem<tauri::Wry>);
/// Persist "Follow playhead" and update the menu check mark.
#[tauri::command]
fn set_follow_playhead(app: tauri::AppHandle, st: State<AppState>, on: bool) -> Result<(), String> {
    st.settings.lock().follow_playhead = on; st.save()?;
    if let Some(m) = app.try_state::<FollowMenu>() { let _ = m.0.set_checked(on); }
    Ok(())
}
/// Open the output stream ahead of the first Play (a project was loaded).
#[tauri::command] fn prepare_playback(st: State<AppState>) -> Result<(), String> { st.send(Cmd::PrepareOutput) }
/// The project is being replaced or closed: stop playback (with the de-click fade) and drop the old project's
/// rendered and decoded audio so it doesn't linger in memory.
#[tauri::command]
fn release_project_audio(st: State<AppState>, cache: State<render::DecodeCache>, pc: State<render::PlayCache>) -> Result<(), String> {
    st.send(Cmd::StopPlay)?; pc.clear(); cache.clear(); Ok(())
}
#[tauri::command] fn update_fx(st: State<AppState>, fx: Vec<fx::Slot>) -> Result<(), String> { st.send(Cmd::UpdateFx(fx)) }
#[tauri::command] fn pause_playback(st: State<AppState>, pause: bool) -> Result<(), String> { st.send(Cmd::PausePlay(pause)) }

#[tauri::command]
fn set_preroll(st: State<AppState>, secs: f32, prepend: bool) -> Result<(), String> {
    { let mut s = st.settings.lock(); s.preroll_secs = secs.clamp(0.0, 60.0); s.preroll_prepend = prepend; }
    st.save()?; st.send(Cmd::SetPreRoll(secs, prepend))
}
#[tauri::command] fn save_last(st: State<AppState>, secs: f32) -> Result<(), String> { let d = st.rec_dir(); st.send(Cmd::SaveLast(secs, d)) }

/// Toggle the −12 dB safety copy written alongside each recording.
#[tauri::command]
fn set_safety(st: State<AppState>, on: bool) -> Result<(), String> {
    st.settings.lock().safety_track = on;
    st.save()?; st.send(Cmd::SetSafety(on))
}

/// Enable/disable level-triggered recording. `cfg` = threshold (dBFS) + hold time (ms).
#[tauri::command]
fn set_voice_trigger(st: State<AppState>, cfg: Option<settings::VoiceTrigger>) -> Result<(), String> {
    { let mut s = st.settings.lock(); s.voice_trigger = cfg.clone(); }
    st.save()?;
    let dir = st.rec_dir();
    st.send(Cmd::SetVoiceTrigger(cfg.map(|c| audio::VtCfg { threshold_db: c.threshold_db, hold_ms: c.hold_ms, dir })))
}

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
/// Selection effects (context menu): render `clips` through `fx` (or as silence) into a new WAV
/// under `dir` (default <recordings>/Edits). Sources are only read; the edit is undoable in the app.
#[tauri::command]
async fn render_selection(st: State<'_, AppState>, cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>, silence: Option<bool>, name: String, dir: Option<String>) -> Result<String, String> {
    let d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    let dir = dir.map(std::path::PathBuf::from).unwrap_or_else(|| st.rec_dir()).join("Edits");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).take(60).collect();
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S%.3f");
    let path = dir.join(format!("{} {stamp}.wav", safe.trim()));
    let slots = fx.unwrap_or_default();
    let silence = silence.unwrap_or(false);
    let out = path.clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<()> {
        let mut d = d;
        if silence { d.samples.iter_mut().for_each(|s| *s = 0.0) } else { fx::process_chain(&mut d, &slots) }
        render::write_wav(&d, &out)
    }).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}
/// Spectrogram of the edited clips (binary, see spectrogram.rs).
#[tauri::command]
async fn spectrogram(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, cols: usize, fft: Option<usize>) -> Result<tauri::ipc::Response, String> {
    let d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    let v = tauri::async_runtime::spawn_blocking(move || spectrogram::compute(&d, cols, fft.unwrap_or(2048))).await.map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(v))
}
/// Save a small text file the user picked in a save dialog (label export). Only .txt.
#[tauri::command]
fn save_text_file(path: String, contents: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.extension().map(|e| e.eq_ignore_ascii_case("txt")).unwrap_or(false) { return Err("Labels can only be saved as a .txt file".into()) }
    std::fs::write(p, contents).map_err(|e| e.to_string())
}
/// Read a label text file the user picked (label import). Only .txt, at most 10 MB.
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    if !p.extension().map(|e| e.eq_ignore_ascii_case("txt")).unwrap_or(false) { return Err("Please pick a .txt label file".into()) }
    if std::fs::metadata(p).map_err(|e| e.to_string())?.len() > 10_000_000 { return Err("That file is too big to be a label file".into()) }
    std::fs::read_to_string(p).map_err(|e| e.to_string())
}
/// Read-only import of an Audacity 3 project: audio is extracted to WAV files in `media_dir`
/// (default: <recordings>/Audacity imports/<name>), the .aup3 itself is never written to.
#[tauri::command]
async fn import_aup3(app: tauri::AppHandle, st: State<'_, AppState>, path: String, media_dir: Option<String>) -> Result<aup3::ImportResult, String> {
    let p = std::path::PathBuf::from(&path);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Audacity project".into());
    let base = match media_dir { Some(d) => std::path::PathBuf::from(d).join(format!("Audacity - {stem}")), None => st.rec_dir().join("Audacity imports").join(&stem) };
    let out = aup3::unique_dir(&base);
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = std::time::Instant::now();
        let r = aup3::import(&p, &out, &mut |f| { if last.elapsed().as_millis() > 150 || f >= 1.0 { last = std::time::Instant::now(); let _ = app.emit("aup3-progress", f); } });
        if r.is_err() { let _ = std::fs::remove_dir_all(&out); } // don't leave half an import behind
        r
    }).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
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

/// Detect silence/pause regions on a track (for one-click auto-trim).
#[tauri::command]
async fn detect_silence(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, threshold_db: Option<f64>, min_ms: f64, pad_ms: f64) -> Result<Vec<trim::SilenceRegion>, String> {
    let d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || Ok(trim::detect_silence(&d, threshold_db, min_ms, pad_ms))).await.map_err(|e| e.to_string())?
}

// ---- Whisper transcription (external whisper.cpp CLI, same pattern as FFmpeg) ----
#[tauri::command]
fn whisper_status(st: State<AppState>) -> whisper::WhisperInfo {
    let m = st.settings.lock().whisper_model.clone().map(PathBuf::from);
    whisper::info(m.as_deref())
}
#[tauri::command]
async fn transcribe(st: State<'_, AppState>, path: String) -> Result<Vec<whisper::Segment>, String> {
    let model = st.settings.lock().whisper_model.clone().map(PathBuf::from);
    tauri::async_runtime::spawn_blocking(move || whisper::transcribe(std::path::Path::new(&path), model.as_deref()))
        .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

/// Export the edited clip list to a new WAV file (originals untouched).
#[tauri::command]
async fn export_clips(cache: State<'_, render::DecodeCache>, clips: Vec<render::ClipSpec>, fx: Option<Vec<fx::Slot>>, path: String) -> Result<String, String> {
    let mut d = render::render(&cache, &clips).map_err(|e| e.to_string())?;
    fx::process_chain(&mut d, &fx.unwrap_or_default());
    render::write_wav(&d, std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(path)
}
#[tauri::command] fn stop_playback(st: State<AppState>) -> Result<(), String> { perf::mark("stop_playback: command in"); st.send(Cmd::StopPlay) }
/// Latency tracing from the UI side (no-op unless `STREAMWAVE_PERF` is set). `at` = JS epoch ms.
#[tauri::command] fn perf_mark(what: String, at: f64) { if perf::on() { perf::mark_at((at * 1000.0) as u64, &format!("ui: {what}")) } }
#[tauri::command] fn perf_enabled() -> bool { perf::on() }

#[tauri::command]
async fn import_audio(app: tauri::AppHandle, path: String, buckets: Option<usize>) -> Result<waveform::Waveform, String> {
    // decode through the playback cache: the first Play of a freshly opened project needs no decode
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<waveform::Waveform> {
        let d = app.state::<render::DecodeCache>().get(&path)?;
        Ok(waveform::waveform_of(std::path::Path::new(&path), &d, buckets.unwrap_or(4000)))
    })
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

/// Extract a `.twt` path from the command line (Windows "Open with" / file-association launch).
fn twt_arg(args: &[String]) -> Option<String> {
    args.iter().skip(1).find(|a| a.to_lowercase().ends_with(".twt")).cloned()
}

fn build_menu(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
    let follow_on = app.state::<AppState>().settings.lock().follow_playhead;
    let follow = CheckMenuItem::with_id(app, "follow", "Follow Playhead  (F)", true, follow_on, None::<&str>)?;
    let it = |id: &str, text: &str, acc: Option<&str>| MenuItem::with_id(app, id, text, true, acc);
    let file = Submenu::with_items(app, "File", true, &[
        &it("new", "New Project", Some("CmdOrCtrl+N"))?,
        &it("open", "Open Project...", Some("CmdOrCtrl+O"))?,
        &it("save", "Save Project", Some("CmdOrCtrl+S"))?,
        &it("saveas", "Save Project As...", Some("CmdOrCtrl+Shift+S"))?,
        &it("close", "Close Project", Some("CmdOrCtrl+W"))?,
        &PredefinedMenuItem::separator(app)?,
        &it("import", "Import Audio...", Some("CmdOrCtrl+I"))?,
        &it("importaup3", "Import Audacity Project...", None)?,
        &it("export", "Export...", Some("CmdOrCtrl+E"))?,
        &it("sendvideo", "Send Back to Video...", None)?,
        &PredefinedMenuItem::separator(app)?,
        &it("importlabels", "Import Labels...", None)?,
        &it("exportlabels", "Export Labels...", None)?,
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
        &PredefinedMenuItem::separator(app)?,
        &it("split", "Split Clip", Some("CmdOrCtrl+K"))?,
        &it("join", "Join Clips", Some("CmdOrCtrl+J"))?,
        &PredefinedMenuItem::separator(app)?,
        &it("addlabel", "Add Label at Selection", Some("CmdOrCtrl+B"))?,
        &it("labeltrack", "New Label Track", None)?,
        &it("labelsmarkers", "Copy Labels to Markers", None)?,
    ])?;
    let view = Submenu::with_items(app, "View", true, &[
        &it("toggleview", "Waveform / Spectrogram (selected tracks)", Some("CmdOrCtrl+Shift+G"))?,
        &it("zoomin", "Zoom In  (+)", None)?,
        &it("zoomout", "Zoom Out  (−)", None)?,
        &PredefinedMenuItem::separator(app)?,
        &follow,
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
    let help = Submenu::with_items(app, "Help", true, &[
        &it("checkupdates", "Check for Updates…", None)?,
        &PredefinedMenuItem::separator(app)?,
        &it("about", "About StreamWave", None)?,
    ])?;
    app.set_menu(Menu::with_items(app, &[&file, &edit, &view, &transport, &help])?)?;
    app.manage(FollowMenu(follow));
    app.on_menu_event(|app, ev| { let _ = app.emit("menu", ev.id().0.clone()); });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::twt_arg;
    #[test] fn finds_twt_path_in_args() {
        assert_eq!(twt_arg(&["StreamWave.exe".into()]), None);
        assert_eq!(twt_arg(&["exe".into(), "C:\\takes\\show.twt".into()]), Some("C:\\takes\\show.twt".into()));
        assert_eq!(twt_arg(&["exe".into(), "/tmp/SHOW.TWT".into()]), Some("/tmp/SHOW.TWT".into()));
        assert_eq!(twt_arg(&["exe".into(), "C:\\takes\\audio.wav".into()]), None);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let default_rec_dir = app.path().audio_dir().or_else(|_| app.path().document_dir()).unwrap_or_else(|_| config_dir.clone()).join("StreamWave");
            let settings = Settings::load(&config_dir);
            let rec_dir = settings.record_dir.clone().map(PathBuf::from).unwrap_or_else(|| default_rec_dir.clone());
            // crash recovery: repair any recording that was interrupted last time
            let recovered = audio::wavfix::recover_dir(&rec_dir);
            let audio = audio::spawn(app.handle().clone(), settings.input_device.clone(), settings.output_device.clone(), settings.latency, settings.auto_backoff, (settings.preroll_secs, settings.preroll_prepend), settings.safety_track);
            if let Some(vt) = &settings.voice_trigger {
                let _ = audio.tx.send(Cmd::SetVoiceTrigger(Some(audio::VtCfg { threshold_db: vt.threshold_db, hold_ms: vt.hold_ms, dir: rec_dir.clone() })));
            }
            app.manage(AppState { audio, settings: Mutex::new(settings), config_dir, default_rec_dir });
            app.manage(Arc::new(obs::ObsState::default()));
            app.manage(render::DecodeCache::default());
            app.manage(render::PlayCache::default());
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            build_menu(app)?;
            // launched by double-clicking a .twt file? open it once the UI is ready
            if let Some(p) = twt_arg(&std::env::args().collect::<Vec<_>>()) {
                let h = app.handle().clone();
                std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs(2)); let _ = h.emit("open-project-path", p); });
            }
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
            list_devices, list_hosts, get_settings, recordings_dir, install_kind, select_input, select_output, set_latency, save_settings,
            start_recording, stop_recording, play_file, play_clips, play_mix, update_fx, update_play_clips, update_play_mix, pause_playback, export_clips, learn_noise, measure_lufs, detect_silence, whisper_status, transcribe, set_preroll, save_last, set_safety, set_voice_trigger, project_save, project_open, recent_projects, autosave_write, autosave_check, autosave_clear, ffmpeg_status, export_track, studio_sound, obs_split, import_aup3, render_selection, spectrogram, save_text_file, read_text_file, media_probe, send_to_video, pause_recording, obs_pause_record, stop_playback, perf_mark, perf_enabled, prepare_playback, release_project_audio, set_follow_playhead, prerender_clips, import_audio,
            obs_connect, obs_disconnect, obs_status, obs_start_record, obs_stop_record
        ])
        .run(tauri::generate_context!())
        .expect("error while running StreamWave");
}
