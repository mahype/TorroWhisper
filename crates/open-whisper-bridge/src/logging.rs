//! File logging for the bridge and the host app.
//!
//! Writes to `~/Library/Logs/OpenWhisper/open-whisper.log` on macOS (the
//! standard location picked up by Console.app) and to the platform data
//! directory on other systems. The file is rotated once it exceeds
//! [`MAX_LOG_BYTES`]; one rotated file (`open-whisper.log.1`) is kept.
//!
//! A panic hook mirrors panics from worker threads (transcription,
//! post-processing, downloads) into the log so they no longer disappear
//! silently.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, Once},
    time::{SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const LOG_FILE_NAME: &str = "open-whisper.log";

pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let path = log_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }

        let logger: &'static FileLogger = Box::leak(Box::new(FileLogger {
            state: Mutex::new(None),
            path,
        }));

        if log::set_logger(logger).is_ok() {
            log::set_max_level(max_level_from_env());
        }

        install_panic_hook();

        log::info!(
            target: "bridge",
            "logging started (bridge {})",
            env!("CARGO_PKG_VERSION")
        );
    });
}

pub fn log_path() -> PathBuf {
    log_dir().join(LOG_FILE_NAME)
}

fn log_dir() -> PathBuf {
    // Override for tests and debugging.
    if let Ok(dir) = std::env::var("OW_LOG_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    if cfg!(target_os = "macos") {
        if let Some(base) = directories::BaseDirs::new() {
            return base.home_dir().join("Library/Logs/OpenWhisper");
        }
    }
    if let Some(dirs) = directories::ProjectDirs::from("", "", "OpenWhisper") {
        return dirs.data_local_dir().join("logs");
    }
    std::env::temp_dir().join("open-whisper-logs")
}

fn max_level_from_env() -> LevelFilter {
    match std::env::var("OW_LOG").ok().as_deref() {
        Some("trace") => LevelFilter::Trace,
        Some("debug") => LevelFilter::Debug,
        Some("warn") => LevelFilter::Warn,
        Some("error") => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let location = info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown location".to_owned());
        let message = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_owned()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "non-string panic payload".to_owned()
        };
        log::error!(
            target: "panic",
            "thread '{}' panicked at {location}: {message}",
            thread.name().unwrap_or("<unnamed>")
        );
        previous(info);
    }));
}

struct FileLogger {
    /// Open file plus its current size; lazily (re-)opened on demand.
    state: Mutex<Option<(File, u64)>>,
    path: PathBuf,
}

impl FileLogger {
    fn write_line(&self, line: &str) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };

        if guard.is_none() {
            *guard = self.open_file();
        }

        let needs_rotation = guard
            .as_ref()
            .is_some_and(|(_, size)| *size >= MAX_LOG_BYTES);
        if needs_rotation {
            *guard = None;
            let rotated = self.path.with_extension("log.1");
            let _ = fs::rename(&self.path, rotated);
            *guard = self.open_file();
        }

        if let Some((file, size)) = guard.as_mut() {
            let bytes = line.as_bytes();
            if file.write_all(bytes).is_ok() {
                let _ = file.write_all(b"\n");
                *size += bytes.len() as u64 + 1;
            }
        }
    }

    fn open_file(&self) -> Option<(File, u64)> {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok()?;
        let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Some((file, size))
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // Keep third-party crates (reqwest, hyper, ...) at warn+ so the log
        // stays readable; our own targets pass through at the global level.
        if metadata.level() <= Level::Warn {
            return true;
        }
        let target = metadata.target();
        target.starts_with("open_whisper")
            || matches!(target, "bridge" | "dictation" | "app" | "panic")
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:5} [{}] {}",
            format_utc_timestamp(SystemTime::now()),
            record.level(),
            record.target(),
            record.args()
        );
        self.write_line(&line);
    }

    fn flush(&self) {}
}

/// Formats a `SystemTime` as `YYYY-MM-DDTHH:MM:SS.mmmZ` without pulling in a
/// date-time crate. Uses the days-to-civil algorithm by Howard Hinnant.
fn format_utc_timestamp(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_epoch() {
        assert_eq!(format_utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn writes_log_line_to_file() {
        let dir = std::env::temp_dir().join(format!("ow-log-test-{}", std::process::id()));
        // SAFETY: tests in this binary run before any other thread touches
        // the environment-dependent logger (init is Once-guarded and only
        // this test triggers it).
        unsafe { std::env::set_var("OW_LOG_DIR", &dir) };
        init();
        log::info!(target: "bridge", "test line");
        let content = fs::read_to_string(dir.join(LOG_FILE_NAME)).expect("log file must exist");
        assert!(content.contains("logging started"));
        assert!(content.contains("test line"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn formats_known_date() {
        // 2026-06-10 12:34:56.789 UTC
        let time = UNIX_EPOCH + Duration::from_millis(1_781_094_896_789);
        assert_eq!(format_utc_timestamp(time), "2026-06-10T12:34:56.789Z");
    }
}
