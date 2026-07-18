//! Runtime diagnostics that make silent deaths debuggable from the log
//! file alone (#39).
//!
//! Two independent pieces live here:
//!
//! * A **heartbeat** thread ([`start_heartbeat`]) writes one `[diag]` line
//!   per minute with uptime, resident memory and thread count. It turns the
//!   log into a timeline: the last heartbeat pins the moment of death to
//!   ±60s instead of "somewhere between the last activity and the next
//!   launch", and a climbing RSS right before the gap is the fingerprint of
//!   a jetsam / out-of-memory kill.
//!
//! * A **crash-report collector** ([`report_previous_crash`]) runs on the
//!   next launch after an abnormal end and folds the relevant fields of the
//!   matching macOS `.ips` crash report into the log, so the signal and the
//!   faulting frame are right there in the file the user already sends —
//!   no more asking them to hunt down a second file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// Prefix shared by all crash-report files this app produces.
const CRASH_REPORT_PREFIX: &str = "TorroWhisper-";
/// Extension macOS uses for the JSON crash reports.
const CRASH_REPORT_EXT: &str = "ips";
/// How often the heartbeat writes a line.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Part B — heartbeat
// ---------------------------------------------------------------------------

/// Starts the background heartbeat thread exactly once for the process.
///
/// Called from `ow_session_started`; safe to call more than once (only the
/// first call spawns a thread). The thread is a daemon: it is never joined
/// and simply dies with the process.
pub fn start_heartbeat() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let start = Instant::now();
        thread::Builder::new()
            .name("ow-diag-heartbeat".to_owned())
            .spawn(move || heartbeat_loop(start))
            .ok();
    });
}

fn heartbeat_loop(start: Instant) -> ! {
    loop {
        thread::sleep(HEARTBEAT_INTERVAL);
        let uptime = start.elapsed().as_secs();
        match process_stats() {
            Some(stats) => log::info!(
                target: "diag",
                "heartbeat: uptime {uptime}s, rss {}, threads {}",
                format_mib(stats.resident_bytes),
                stats.thread_count
            ),
            None => log::info!(target: "diag", "heartbeat: uptime {uptime}s"),
        }
    }
}

/// Resident memory and live thread count for the current process.
struct ProcessStats {
    resident_bytes: u64,
    thread_count: i32,
}

#[cfg(target_os = "macos")]
fn process_stats() -> Option<ProcessStats> {
    // One `proc_pidinfo(PROC_PIDTASKINFO)` call yields both resident size and
    // thread count in a single fixed-size struct — no allocation to free,
    // unlike `task_threads`. libc is already in the dependency tree.
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if written == size {
        Some(ProcessStats {
            resident_bytes: info.pti_resident_size,
            thread_count: info.pti_threadnum,
        })
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn process_stats() -> Option<ProcessStats> {
    None
}

fn format_mib(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

// ---------------------------------------------------------------------------
// Part C — crash-report collector
// ---------------------------------------------------------------------------

/// Looks for the macOS crash report belonging to a session that just ended
/// abnormally and folds its key fields into the log.
///
/// `session_start` is the moment the dead session began (the marker file's
/// mtime); it disambiguates the fresh crash from older `.ips` files lying
/// around. `None` means the start time is unknown — then the newest report
/// is used unconditionally.
///
/// Silent when the crash-report directory does not exist (e.g. under an
/// `OW_LOG_DIR` test override) — it never panics and never errors.
pub fn report_previous_crash(session_start: Option<SystemTime>) {
    let Some(dir) = crash_report_dir() else {
        return;
    };
    if !dir.is_dir() {
        return;
    }

    match newest_matching_report(&dir, session_start) {
        Some(path) => match fs::read_to_string(&path) {
            Ok(content) => match parse_crash_report(&content) {
                Some(info) => log::warn!(
                    target: "diag",
                    "previous crash report found: {} — {} [path: {}]",
                    file_name(&path),
                    info.summary(),
                    path.display()
                ),
                None => log::warn!(
                    target: "diag",
                    "previous crash report found but could not be parsed: {}",
                    path.display()
                ),
            },
            Err(err) => log::warn!(
                target: "diag",
                "previous crash report found but could not be read ({err}): {}",
                path.display()
            ),
        },
        None => log::warn!(
            target: "diag",
            "previous session ended abnormally but no matching crash report found \
             (likely SIGKILL/jetsam/power-loss)"
        ),
    }
}

/// `~/Library/Logs/DiagnosticReports`, the sibling of our own log directory.
fn crash_report_dir() -> Option<PathBuf> {
    crate::logging::log_dir()
        .parent()
        .map(|parent| parent.join("DiagnosticReports"))
}

/// Newest `TorroWhisper-*.ips` whose mtime is at or after `session_start`.
fn newest_matching_report(dir: &Path, session_start: Option<SystemTime>) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_crash_report(&path) {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        // Only reports produced during (or after) the dead session count.
        if let Some(start) = session_start
            && modified < start
        {
            continue;
        }
        if best.as_ref().is_none_or(|(best_mtime, _)| modified > *best_mtime) {
            best = Some((modified, path));
        }
    }

    best.map(|(_, path)| path)
}

fn is_crash_report(path: &Path) -> bool {
    let is_ips = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(CRASH_REPORT_EXT));
    is_ips
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(CRASH_REPORT_PREFIX))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
        .to_owned()
}

/// The handful of fields we lift out of an `.ips` — everything else is
/// ignored. Any field may be missing for a given crash type.
#[derive(Debug, Default, PartialEq)]
struct CrashInfo {
    exception_type: Option<String>,
    signal: Option<String>,
    indicator: Option<String>,
    faulting_thread: Option<i64>,
    faulting_queue: Option<String>,
    top_frame: Option<String>,
}

impl CrashInfo {
    /// One-line human summary, skipping the fields that are absent.
    fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        match (&self.exception_type, &self.signal) {
            (Some(exc), Some(sig)) => parts.push(format!("{exc} ({sig})")),
            (Some(exc), None) => parts.push(exc.clone()),
            (None, Some(sig)) => parts.push(sig.clone()),
            (None, None) => {}
        }
        if let Some(indicator) = &self.indicator {
            parts.push(indicator.clone());
        }
        if let Some(thread) = self.faulting_thread {
            match &self.faulting_queue {
                Some(queue) => parts.push(format!("faulting thread {thread} ({queue})")),
                None => parts.push(format!("faulting thread {thread}")),
            }
        }
        if let Some(frame) = &self.top_frame {
            parts.push(format!("top frame: {frame}"));
        }
        if parts.is_empty() {
            "crash details unavailable".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

/// Extracts the reportable fields from an `.ips` file's contents.
///
/// The format is line 1 = a small header JSON object, then the remaining
/// lines = one large JSON object. We only need the big object; the payload
/// may span multiple lines, so everything after the first newline is parsed
/// as one value. Returns `None` only when that payload is not valid JSON.
fn parse_crash_report(content: &str) -> Option<CrashInfo> {
    let payload = content.split_once('\n').map(|(_, rest)| rest)?;
    let root: serde_json::Value = serde_json::from_str(payload).ok()?;

    let mut info = CrashInfo {
        exception_type: string_at(&root, &["exception", "type"]),
        signal: string_at(&root, &["exception", "signal"]),
        indicator: string_at(&root, &["termination", "indicator"]),
        faulting_thread: root.get("faultingThread").and_then(serde_json::Value::as_i64),
        ..CrashInfo::default()
    };

    if let Some(index) = info.faulting_thread
        && let Some(thread) = root
            .get("threads")
            .and_then(serde_json::Value::as_array)
            .and_then(|threads| usize::try_from(index).ok().and_then(|i| threads.get(i)))
    {
        info.faulting_queue = thread
            .get("queue")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        info.top_frame = thread
            .get("frames")
            .and_then(serde_json::Value::as_array)
            .and_then(|frames| frames.first())
            .and_then(|frame| frame.get("symbol"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
    }

    Some(info)
}

/// Reads a nested string field by a path of object keys.
fn string_at(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real incident from the issue, trimmed to the fields we read, in
    /// the two-line `.ips` layout (header line + payload object).
    const SAMPLE_IPS: &str = concat!(
        r#"{"app_name":"TorroWhisper","timestamp":"2026-07-15 07:51:02.00 +0200","app_version":"0.5.0","os_version":"macOS 26.5.2 (25F84)","incident_id":"02D6A6A5-0000-0000-0000-000000000000"}"#,
        "\n",
        r#"{"exception":{"type":"EXC_BREAKPOINT","signal":"SIGTRAP"},"termination":{"indicator":"Trace/BPT trap: 5"},"faultingThread":5,"threads":[{},{},{},{},{},{"queue":"com.apple.root.utility-qos","frames":[{"symbol":"closure #1 in AudioDeviceMonitor.start()"},{"symbol":"partial apply"}]}]}"#
    );

    #[test]
    fn parses_real_incident_fields() {
        let info = parse_crash_report(SAMPLE_IPS).expect("payload is valid JSON");
        assert_eq!(info.exception_type.as_deref(), Some("EXC_BREAKPOINT"));
        assert_eq!(info.signal.as_deref(), Some("SIGTRAP"));
        assert_eq!(info.indicator.as_deref(), Some("Trace/BPT trap: 5"));
        assert_eq!(info.faulting_thread, Some(5));
        assert_eq!(info.faulting_queue.as_deref(), Some("com.apple.root.utility-qos"));
        assert_eq!(
            info.top_frame.as_deref(),
            Some("closure #1 in AudioDeviceMonitor.start()")
        );
    }

    #[test]
    fn summary_reads_like_a_sentence() {
        let info = parse_crash_report(SAMPLE_IPS).unwrap();
        assert_eq!(
            info.summary(),
            "EXC_BREAKPOINT (SIGTRAP), Trace/BPT trap: 5, \
             faulting thread 5 (com.apple.root.utility-qos), \
             top frame: closure #1 in AudioDeviceMonitor.start()"
        );
    }

    #[test]
    fn missing_fields_are_tolerated() {
        // A crash with no exception/thread info (e.g. some resource limits).
        let content = "{\"header\":true}\n{\"termination\":{\"indicator\":\"Namespace SIGNAL\"}}";
        let info = parse_crash_report(content).expect("still valid JSON");
        assert_eq!(info.exception_type, None);
        assert_eq!(info.faulting_thread, None);
        assert_eq!(info.summary(), "Namespace SIGNAL");
    }

    #[test]
    fn invalid_payload_returns_none() {
        let content = "{\"header\":true}\nnot json at all";
        assert_eq!(parse_crash_report(content), None);
    }

    #[test]
    fn empty_summary_falls_back() {
        let info = CrashInfo::default();
        assert_eq!(info.summary(), "crash details unavailable");
    }

    #[test]
    fn report_dir_is_diagnostic_reports_sibling() {
        // Under OW_LOG_DIR the sibling directory simply won't exist; the
        // resolver must still produce the right path and not panic.
        let dir = crash_report_dir().expect("log dir has a parent");
        assert!(dir.ends_with("DiagnosticReports"));
    }

    #[test]
    fn is_crash_report_matches_prefix_and_ext() {
        assert!(is_crash_report(Path::new("/x/TorroWhisper-2026-07-15-075102.ips")));
        assert!(!is_crash_report(Path::new("/x/OtherApp-2026.ips")));
        assert!(!is_crash_report(Path::new("/x/TorroWhisper-2026.crash")));
    }
}
