//! OBS Studio integration over obs-websocket v5 (built into OBS 28+).
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use futures_util::StreamExt;
use obws::events::{Event, OutputState};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct ObsState {
    pub client: Mutex<Option<Arc<obws::Client>>>,
    /// bumped on every connect/disconnect; stale supervisors exit when it changes
    generation: AtomicU64,
}

#[derive(Serialize, Clone)]
pub struct ObsStatus { pub connected: bool, pub recording: bool, pub paused: bool, pub duration_ms: u64, pub obs_version: Option<String>, pub message: Option<String> }

#[derive(Serialize, Clone)]
pub struct ObsRecordEvent { pub active: bool, pub state: String, pub path: Option<String> }

/// Connection status for the UI ("obs-conn" event).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ObsConn { pub state: String, pub attempt: u32, pub retry_in_secs: u64, pub message: Option<String> }

/// Reconnect delay: 1, 2, 4, 8, 16, 30, 30, ... seconds.
pub fn backoff_secs(attempt: u32) -> u64 { 1u64.checked_shl(attempt.saturating_sub(1).min(16)).unwrap_or(30).min(30) }

async fn open(app: &AppHandle, st: &ObsState, host: &str, port: u16, pw: &Option<String>) -> Result<tokio::sync::oneshot::Receiver<String>, String> {
    let client = obws::Client::connect(host.to_string(), port, pw.clone()).await
        .map_err(|e| format!("Could not connect to OBS: {e}. Is OBS running with Tools > WebSocket Server Settings enabled?"))?;
    let client = Arc::new(client);
    let mut events = client.events().map_err(|e| e.to_string())?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app2 = app.clone();
    tokio::spawn(async move {
        let mut why = "connection closed".to_string();
        while let Some(ev) = events.next().await {
            match ev {
                Event::RecordStateChanged { active, state, path } => {
                    let s = match state { OutputState::Starting => "starting", OutputState::Started => "started", OutputState::Stopping => "stopping",
                        OutputState::Stopped => "stopped", OutputState::Paused => "paused", OutputState::Resumed => "resumed", _ => "other" };
                    let _ = app2.emit("obs-record-state", ObsRecordEvent { active, state: s.into(), path: path.clone() });
                    // OBS has finished writing the file -> UI auto-imports it
                    if matches!(state, OutputState::Stopped) { if let Some(p) = path { let _ = app2.emit("obs-recording-ready", p); } }
                }
                Event::CurrentProgramSceneChanged { id } => { let _ = app2.emit("obs-scene", id.name); }
                Event::ExitStarted => { why = "OBS is closing".into(); }
                _ => {}
            }
        }
        let _ = tx.send(why);
    });
    *st.client.lock().await = Some(client);
    Ok(rx)
}

/// Connect, then keep the connection alive: if OBS closes or crashes we retry with backoff
/// until it is back or the user disconnects.
pub async fn connect(app: AppHandle, st: Arc<ObsState>, host: String, port: u16, password: String) -> Result<ObsStatus, String> {
    let gen = st.generation.fetch_add(1, SeqCst) + 1;
    st.client.lock().await.take();
    let pw = (!password.is_empty()).then_some(password);
    let emit = |app: &AppHandle, state: &str, attempt, retry, msg: Option<String>| {
        let _ = app.emit("obs-conn", ObsConn { state: state.into(), attempt, retry_in_secs: retry, message: msg });
    };
    emit(&app, "connecting", 0, 0, None);
    let rx = match open(&app, &st, &host, port, &pw).await {
        Ok(rx) => rx,
        Err(e) => { emit(&app, "disconnected", 0, 0, Some(e.clone())); return Err(e) }
    };
    emit(&app, "connected", 0, 0, None);
    let (app2, st2) = (app.clone(), st.clone());
    tokio::spawn(async move {
        let mut rx = rx;
        loop {
            let why = rx.await.unwrap_or_else(|_| "connection closed".into());
            if st2.generation.load(SeqCst) != gen { return }
            st2.client.lock().await.take();
            let _ = app2.emit("obs-disconnected", why.clone());
            let mut attempt = 0u32;
            rx = loop {
                attempt += 1;
                let wait = backoff_secs(attempt);
                emit(&app2, "reconnecting", attempt, wait, Some(why.clone()));
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                if st2.generation.load(SeqCst) != gen { return }
                if let Ok(rx) = open(&app2, &st2, &host, port, &pw).await {
                    if st2.generation.load(SeqCst) != gen { st2.client.lock().await.take(); return }
                    emit(&app2, "connected", attempt, 0, None);
                    break rx;
                }
            };
        }
    });
    status(&st).await
}

async fn client(st: &ObsState) -> Result<Arc<obws::Client>, String> {
    st.client.lock().await.clone().ok_or_else(|| "Not connected to OBS".to_string())
}

pub async fn status(st: &ObsState) -> Result<ObsStatus, String> {
    let Ok(c) = client(st).await else {
        return Ok(ObsStatus { connected: false, recording: false, paused: false, duration_ms: 0, obs_version: None, message: None });
    };
    let ver = c.general().version().await.ok().map(|v| v.obs_studio_version.to_string());
    match c.recording().status().await {
        Ok(r) => Ok(ObsStatus { connected: true, recording: r.active, paused: r.paused, duration_ms: r.duration.whole_milliseconds() as u64, obs_version: ver, message: None }),
        Err(e) => { Ok(ObsStatus { connected: false, recording: false, paused: false, duration_ms: 0, obs_version: None, message: Some(e.to_string()) }) }
    }
}

pub async fn start_record(st: &ObsState) -> Result<(), String> { client(st).await?.recording().start().await.map_err(|e| e.to_string()) }
pub async fn stop_record(st: &ObsState) -> Result<String, String> { client(st).await?.recording().stop().await.map_err(|e| e.to_string()) }
/// Stops any reconnect loop and drops the client (the socket closes when the last handle goes).
pub async fn disconnect(app: &AppHandle, st: &ObsState) {
    st.generation.fetch_add(1, SeqCst);
    let c = st.client.lock().await.take();
    if let Some(c) = c { if let Ok(mut c) = Arc::try_unwrap(c) { c.disconnect().await; } }
    let _ = app.emit("obs-conn", ObsConn { state: "disconnected".into(), attempt: 0, retry_in_secs: 0, message: None });
}
pub async fn pause_record(st: &ObsState, pause: bool) -> Result<(), String> {
    let c = client(st).await?;
    if pause { c.recording().pause().await } else { c.recording().resume().await }.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn backoff_doubles_and_caps() {
        let v: Vec<u64> = (1..=8).map(backoff_secs).collect();
        assert_eq!(v, vec![1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(backoff_secs(0), 1);
        assert_eq!(backoff_secs(u32::MAX), 30);
    }
}
