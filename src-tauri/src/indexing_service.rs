//! Scan-owned Android foreground service, independent of playback and the webview.
//!
//! Construct only on indexing workers: synchronous mobile plugin calls must not
//! block the Android main thread. Plugin registration precedes library startup.
//! Rejected starts cancel scanning. Lost service protection cancels at the next
//! throttled worker checkpoint without stale-row cleanup or a completion event.
//! Already indexed rows remain durable; a later explicit scan resumes the work.

#[cfg(target_os = "android")]
mod android {
    use serde_json::{json, Value};
    use std::sync::{atomic::{AtomicU64, Ordering}, OnceLock};
    use std::time::{Duration, Instant};

    static MOBILE: OnceLock<tauri::plugin::PluginHandle<tauri::Wry>> = OnceLock::new();
    static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
    const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

    pub fn mobile_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
        tauri::plugin::Builder::new("player-indexing")
            .setup(|_app, api| {
                let handle = api.register_android_plugin("dev.verncat.player", "IndexingPlugin")?;
                MOBILE.set(handle).map_err(|_| {
                    std::io::Error::other("Android indexing bridge is already initialized")
                })?;
                Ok(())
            })
            .build()
    }

    fn call(method: &str, args: Value) -> Result<(), String> {
        let handle = MOBILE.get().ok_or("Android indexing bridge is not initialized")?;
        handle.run_mobile_plugin::<Value>(method, args)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub struct IndexingSession {
        session_id: String,
        last_progress: Instant,
        started: bool,
    }

    impl IndexingSession {
        pub fn begin() -> Option<Self> {
            let session_id = format!("{}-{}", std::process::id(), NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
            let started = match call("beginIndexing", json!({ "sessionId": &session_id })) {
                Ok(()) => true,
                Err(error) => {
                    tracing::error!(%error, %session_id, "Indexing foreground service failed to start; cancelling scan");
                    false
                }
            };
            let session = Self { session_id, last_progress: Instant::now(), started };
            if started { Some(session) } else { None }
        }

        pub fn progress(&mut self, current: usize, total: usize) -> bool {
            if !self.started || self.last_progress.elapsed() < PROGRESS_INTERVAL {
                return true;
            }
            self.last_progress = Instant::now();
            if let Err(error) = call("updateIndexing", json!({
                "sessionId": &self.session_id,
                "current": current.min(i32::MAX as usize) as i32,
                "total": total.min(i32::MAX as usize) as i32,
            })) {
                tracing::error!(%error, session_id = %self.session_id, "Indexing foreground service unavailable; cancelling scan");
                return false;
            }
            true
        }
    }

    impl Drop for IndexingSession {
        fn drop(&mut self) {
            // Attempt cleanup even if begin failed after partially starting the service.
            if let Err(error) = call("endIndexing", json!({ "sessionId": &self.session_id })) {
                tracing::error!(%error, session_id = %self.session_id, "Failed to end indexing foreground service");
            }
        }
    }
}

#[cfg(target_os = "android")]
pub use android::{mobile_plugin, IndexingSession};

#[cfg(not(target_os = "android"))]
pub struct IndexingSession;

#[cfg(not(target_os = "android"))]
impl IndexingSession {
    pub fn begin() -> Option<Self> { Some(Self) }
    pub fn progress(&mut self, _current: usize, _total: usize) -> bool { true }
}
