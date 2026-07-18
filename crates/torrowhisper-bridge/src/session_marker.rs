//! Detects sessions that ended without a clean shutdown.
//!
//! The host app calls [`session_started`] once at launch and
//! [`session_ended_cleanly`] when it terminates normally. A marker file is
//! written on start and removed on clean shutdown, so a marker that is
//! already present at launch means the previous session died without
//! reaching the shutdown path — crash, `abort()` below the panic hook
//! (e.g. a GGML assertion in whisper.cpp), SIGKILL, or power loss. That
//! case is logged as a warning so silent deaths become visible in the log
//! even when the dying process could not write anything itself.
//!
//! The marker lives next to the log file and honours the `OW_LOG_DIR`
//! override, so tests never touch the real user log directory.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

const MARKER_FILE_NAME: &str = "torrowhisper.session";

/// Records the start of a session. Returns `true` if the previous session
/// ended abnormally (its marker was never cleaned up).
pub fn session_started() -> bool {
    let path = marker_path();
    // The marker's mtime is the previous session's start time: it lets the
    // crash-report collector tell this crash's `.ips` from older ones.
    let previous_start = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    let previous = match fs::read_to_string(&path) {
        Ok(contents) => {
            log::warn!(
                target: "bridge",
                "previous session ended abnormally — no clean shutdown recorded (last session: {})",
                contents.trim()
            );
            // Fold the matching macOS crash report into the log so the
            // signal and faulting frame land in the file the user sends
            // (#39, Part C). A missing report is itself logged as a signal
            // that the process was killed rather than crashed.
            crate::diagnostics::report_previous_crash(previous_start);
            true
        }
        Err(_) => false,
    };

    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let contents = format!(
        "started {} (bridge {}, pid {})",
        crate::logging::format_utc_timestamp(SystemTime::now()),
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    if let Err(err) = fs::write(&path, contents) {
        log::warn!(
            target: "bridge",
            "session marker could not be written to {}: {err}",
            path.display()
        );
    }

    previous
}

/// Records a clean shutdown: logs it and removes the session marker so the
/// next launch does not report an abnormal end.
pub fn session_ended_cleanly() {
    log::info!(target: "bridge", "app quitting (clean shutdown)");
    let _ = fs::remove_file(marker_path());
}

fn marker_path() -> PathBuf {
    crate::logging::log_dir().join(MARKER_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests in this module: they all mutate the same
    /// process-wide `OW_LOG_DIR` variable.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_temp_log_dir(test: impl FnOnce()) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "ow-session-test-{}-{:p}",
            std::process::id(),
            &guard as *const _
        ));
        let _ = fs::remove_dir_all(&dir);
        // SAFETY: ENV_LOCK serializes all mutations of OW_LOG_DIR in this
        // test binary; logging::init is Once-guarded and reads it earlier.
        unsafe { std::env::set_var("OW_LOG_DIR", &dir) };
        test();
        unsafe { std::env::remove_var("OW_LOG_DIR") };
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_start_reports_clean_and_writes_marker() {
        with_temp_log_dir(|| {
            assert!(!session_started());
            assert!(marker_path().exists());
        });
    }

    #[test]
    fn clean_shutdown_removes_marker() {
        with_temp_log_dir(|| {
            session_started();
            session_ended_cleanly();
            assert!(!marker_path().exists());
            assert!(!session_started());
        });
    }

    #[test]
    fn stale_marker_reports_abnormal_end() {
        with_temp_log_dir(|| {
            session_started();
            // No clean shutdown in between — the next start must flag it.
            assert!(session_started());
        });
    }
}
