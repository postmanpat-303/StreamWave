//! Opt-in latency tracing (`STREAMWAVE_PERF=1`): wall-clock marks on stderr, so the app's play/stop paths can be
//! measured end to end (UI press -> IPC -> render -> first audio callback). Off by default; costs one atomic load.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn on() -> bool { static ON: OnceLock<bool> = OnceLock::new(); *ON.get_or_init(|| std::env::var_os("STREAMWAVE_PERF").is_some()) }
/// Microseconds since the Unix epoch (same clock as JS `performance.timeOrigin + performance.now()`).
pub fn now_us() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0) }
pub fn mark(what: &str) { if on() { eprintln!("perf {:.3} {what}", now_us() as f64 / 1000.0) } }
pub fn mark_at(us: u64, what: &str) { if on() && us > 0 { eprintln!("perf {:.3} {what}", us as f64 / 1000.0) } }
/// Set once by the audio callback (first real samples / first silence after a stop), logged by the meter thread.
#[derive(Default)]
pub struct Stamp(AtomicU64);
impl Stamp {
    pub fn arm(&self) { self.0.store(1, Relaxed) }
    /// Called from the callback: records the time if armed (one clock read, no locks/allocation).
    pub fn hit(&self) { if self.0.load(Relaxed) == 1 { self.0.store(now_us(), Relaxed) } }
    pub fn take(&self) -> u64 { let v = self.0.load(Relaxed); if v > 1 { self.0.store(0, Relaxed); v } else { 0 } }
}
