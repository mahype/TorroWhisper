use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use directories::ProjectDirs;
use torrowhisper_core::{AppSettings, ModelPreset};
use reqwest::blocking::Client;

const USER_AGENT: &str = "torrowhisper/0.1";
const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;
const DOWNLOAD_PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

/// whisper.cpp ggml model files start with this little-endian magic ("ggml").
const GGML_FILE_MAGIC: u32 = 0x6767_6d6c;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelIntegrity {
    Missing,
    Valid,
    Corrupt { reason: String },
}

/// Checks a model file on disk: existence, expected byte size (when known)
/// and the ggml magic header. Cheap enough to run on every status poll.
pub fn model_file_integrity(path: &Path, expected_size: Option<u64>) -> ModelIntegrity {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return ModelIntegrity::Missing,
    };

    if let Some(expected) = expected_size
        && metadata.len() != expected
    {
        return ModelIntegrity::Corrupt {
            reason: format!(
                "file size is {} but {} was expected",
                human_readable_size(metadata.len()),
                human_readable_size(expected)
            ),
        };
    }

    let mut magic_bytes = [0_u8; 4];
    match fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic_bytes)) {
        Ok(()) => {
            if u32::from_le_bytes(magic_bytes) != GGML_FILE_MAGIC {
                return ModelIntegrity::Corrupt {
                    reason: "file does not start with the ggml magic header".to_owned(),
                };
            }
        }
        Err(err) => {
            return ModelIntegrity::Corrupt {
                reason: format!("file header could not be read: {err}"),
            };
        }
    }

    ModelIntegrity::Valid
}

/// Integrity of the file at a preset's default download location.
pub fn preset_model_integrity(preset: ModelPreset) -> ModelIntegrity {
    match default_model_path(preset) {
        Ok(path) => model_file_integrity(&path, Some(preset.download_size_bytes())),
        Err(_) => ModelIntegrity::Missing,
    }
}

/// Resolves the model path for the active settings and validates the file.
/// Used by dictation before recording and before loading the whisper context,
/// so both ends of the pipeline agree on what "downloaded" means.
pub fn validated_model_path(settings: &AppSettings) -> Result<PathBuf, String> {
    let path = resolve_model_path(settings)?;
    let default_path = default_model_path(settings.local_model).ok();
    let expected_size = match &default_path {
        Some(default_path) if *default_path == path => {
            Some(settings.local_model.download_size_bytes())
        }
        // Custom model file chosen by the user: size is unknown to us.
        _ => None,
    };

    let override_path = settings.local_model_path.trim();
    let override_info = if override_path.is_empty() {
        "no".to_owned()
    } else {
        format!("yes ('{override_path}')")
    };

    match model_file_integrity(&path, expected_size) {
        ModelIntegrity::Valid => Ok(path),
        ModelIntegrity::Missing => {
            log::warn!(
                target: "models",
                "model not usable: preset '{}', resolved '{}', exists: false, override set: {override_info}",
                settings.local_model.display_label(),
                path.display()
            );
            Err(format!(
                "{} has not been downloaded yet. Download it in Settings first.",
                settings.local_model.display_label()
            ))
        }
        ModelIntegrity::Corrupt { reason } => {
            log::warn!(
                target: "models",
                "model not usable: preset '{}', resolved '{}', failed verification ({reason}), override set: {override_info}",
                settings.local_model.display_label(),
                path.display()
            );
            Err(format!(
                "{} is damaged or incomplete. Please download it again.",
                settings.local_model.display_label()
            ))
        }
    }
}

/// Removes leftover `*.part` files from interrupted downloads in `dir`.
pub fn cleanup_partial_downloads_in(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    let mut removed = 0_usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_partial = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("part"));
        if is_partial && fs::remove_file(&path).is_ok() {
            log::info!(
                target: "models",
                "removed stale partial download {}",
                path.display()
            );
            removed += 1;
        }
    }
    removed
}

/// Cleans up interrupted whisper model downloads from the default models dir.
pub fn cleanup_partial_downloads() -> usize {
    match default_model_path(ModelPreset::default()) {
        Ok(path) => path.parent().map(cleanup_partial_downloads_in).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Free space on the volume holding `path` (longest matching mount point).
pub fn free_disk_space_for(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(sysinfo::Disk::available_space)
}

/// Writes a full whisper model inventory to the log: which preset is active,
/// which file the runtime will actually load, and the verification state of
/// every preset file on disk. Runs at startup and on diagnostics request so
/// logs from the field answer "what is really on this machine".
pub fn log_model_inventory(settings: &AppSettings) {
    let override_path = settings.local_model_path.trim();
    match resolve_model_path(settings) {
        Ok(path) => log::info!(
            target: "models",
            "inventory: active preset '{}', resolved path '{}' (source: {})",
            settings.local_model.display_label(),
            path.display(),
            if override_path.is_empty() {
                "preset default"
            } else {
                "custom override"
            }
        ),
        Err(err) => log::warn!(
            target: "models",
            "inventory: active model path not resolvable: {err}"
        ),
    }

    let mut ready = 0_usize;
    let mut damaged = 0_usize;
    let mut missing = 0_usize;
    for preset in ModelPreset::ALL {
        let filename = preset.default_filename();
        let Ok(path) = default_model_path(preset) else {
            continue;
        };
        match model_file_integrity(&path, Some(preset.download_size_bytes())) {
            ModelIntegrity::Valid => {
                ready += 1;
                let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                log::info!(
                    target: "models",
                    "inventory: {filename} — OK ({}, header verified)",
                    human_readable_size(size)
                );
            }
            ModelIntegrity::Missing => {
                missing += 1;
                log::info!(target: "models", "inventory: {filename} — not downloaded");
            }
            ModelIntegrity::Corrupt { reason } => {
                damaged += 1;
                log::warn!(target: "models", "inventory: {filename} — CORRUPT ({reason})");
            }
        }
    }

    let free_disk = default_model_path(ModelPreset::default())
        .ok()
        .and_then(|path| free_disk_space_for(&path))
        .map(|bytes| format!(", free disk: {}", human_readable_size(bytes)))
        .unwrap_or_default();
    log::info!(
        target: "models",
        "inventory: {ready} ready, {damaged} damaged, {missing} not downloaded{free_disk}"
    );
}

pub struct ModelDownloadManager {
    state: ModelDownloadState,
    download_rx: Option<Receiver<DownloadEvent>>,
}

impl ModelDownloadManager {
    pub fn new() -> Self {
        Self {
            state: ModelDownloadState::Idle,
            download_rx: None,
        }
    }

    pub fn start_download(&mut self, settings: &AppSettings) -> Result<String, String> {
        self.start_download_for(settings.local_model)
    }

    pub fn start_download_for(&mut self, preset: ModelPreset) -> Result<String, String> {
        if self.is_downloading() {
            return Err("A model download is already running.".to_owned());
        }

        let target_path = default_model_path(preset)?;
        match model_file_integrity(&target_path, Some(preset.download_size_bytes())) {
            ModelIntegrity::Valid => {
                self.state = ModelDownloadState::Ready {
                    path: target_path.clone(),
                };
                return Ok(format!("{} is already present.", preset.display_label()));
            }
            ModelIntegrity::Corrupt { reason } => {
                log::warn!(
                    target: "models",
                    "replacing damaged model file {} ({reason})",
                    target_path.display()
                );
                fs::remove_file(&target_path)
                    .map_err(|err| format!("Model could not be deleted: {err}"))?;
            }
            ModelIntegrity::Missing => {}
        }

        let download_url = preset.download_url().to_owned();
        let download_path = target_path.clone();
        let expected_size = preset.download_size_bytes();
        let temp_path = temporary_download_path(&target_path);
        let (tx, rx) = mpsc::channel();

        self.download_rx = Some(rx);
        self.state = ModelDownloadState::Downloading {
            preset,
            downloaded_bytes: 0,
            total_bytes: None,
            started_at: Instant::now(),
        };

        log::info!(
            target: "models",
            "download started: {} from {download_url} (expected {})",
            preset.default_filename(),
            human_readable_size(expected_size)
        );

        thread::spawn(move || {
            let started = Instant::now();
            let result = download_model_file(
                &download_url,
                &download_path,
                &temp_path,
                Some(expected_size),
                &tx,
            );
            let elapsed = started.elapsed();
            match result {
                Ok(downloaded_bytes) => log::info!(
                    target: "models",
                    "download finished: {} ({} in {}, {}/s), verification OK",
                    download_path.display(),
                    human_readable_size(downloaded_bytes),
                    human_readable_duration(elapsed),
                    human_readable_size(per_second(downloaded_bytes, elapsed))
                ),
                Err(err) => {
                    log::error!(
                        target: "models",
                        "download failed: {} after {} — {err}",
                        download_path.display(),
                        human_readable_duration(elapsed)
                    );
                    let _ = cleanup_temp_file(&temp_path);
                    let _ = tx.send(DownloadEvent::Failed(err));
                }
            }
        });

        Ok(format!("Download for {} started.", preset.display_label()))
    }

    pub fn delete_downloaded_model(&mut self, settings: &AppSettings) -> Result<String, String> {
        self.delete_preset(settings.local_model)
    }

    pub fn delete_preset(&mut self, preset: ModelPreset) -> Result<String, String> {
        if self.is_downloading_preset(preset) {
            return Err("A running download can't be deleted at the same time.".to_owned());
        }

        let path = default_model_path(preset)?;
        if !path.exists() {
            return Ok(format!(
                "{} was already not present locally.",
                preset.display_label()
            ));
        }

        fs::remove_file(&path).map_err(|err| format!("Model could not be deleted: {err}"))?;

        if !self.is_downloading() {
            self.state = ModelDownloadState::Missing;
        }

        Ok(format!("{} was deleted locally.", preset.display_label()))
    }

    pub fn poll(&mut self) -> Vec<String> {
        let mut messages = Vec::new();

        if let Some(rx) = &self.download_rx {
            loop {
                match rx.try_recv() {
                    Ok(DownloadEvent::Progress {
                        downloaded_bytes,
                        total_bytes,
                    }) => {
                        if let ModelDownloadState::Downloading {
                            downloaded_bytes: current_downloaded,
                            total_bytes: current_total,
                            ..
                        } = &mut self.state
                        {
                            *current_downloaded = downloaded_bytes;
                            *current_total = total_bytes;
                        }
                    }
                    Ok(DownloadEvent::Completed {
                        path,
                        downloaded_bytes,
                    }) => {
                        let label = model_label_for_path(&path);
                        self.download_rx = None;
                        self.state = ModelDownloadState::Ready { path: path.clone() };
                        messages.push(format!(
                            "Download complete: {} ({})",
                            label,
                            human_readable_size(downloaded_bytes)
                        ));
                        break;
                    }
                    Ok(DownloadEvent::Failed(err)) => {
                        self.download_rx = None;
                        self.state = ModelDownloadState::Failed {
                            message: err.clone(),
                        };
                        messages.push(err);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.download_rx = None;
                        self.state = ModelDownloadState::Failed {
                            message: "Download worker stopped unexpectedly.".to_owned(),
                        };
                        messages.push("Download worker stopped unexpectedly.".to_owned());
                        break;
                    }
                }
            }
        }

        messages
    }

    pub fn refresh_local_state(&mut self, settings: &AppSettings) {
        if self.is_downloading() {
            return;
        }

        if let Ok(path) = resolve_model_path(settings) {
            let expected_size = default_model_path(settings.local_model)
                .ok()
                .filter(|default_path| *default_path == path)
                .map(|_| settings.local_model.download_size_bytes());
            self.state = match model_file_integrity(&path, expected_size) {
                ModelIntegrity::Valid => ModelDownloadState::Ready { path },
                ModelIntegrity::Missing => ModelDownloadState::Missing,
                ModelIntegrity::Corrupt { .. } => ModelDownloadState::Corrupt,
            };
        }
    }

    pub fn is_corrupt(&self) -> bool {
        matches!(self.state, ModelDownloadState::Corrupt)
    }

    pub fn is_downloading(&self) -> bool {
        matches!(self.state, ModelDownloadState::Downloading { .. })
    }

    pub fn is_downloading_preset(&self, preset: ModelPreset) -> bool {
        matches!(self.state, ModelDownloadState::Downloading { preset: active, .. } if active == preset)
    }

    pub fn active_download_preset(&self) -> Option<ModelPreset> {
        if let ModelDownloadState::Downloading { preset, .. } = &self.state {
            Some(*preset)
        } else {
            None
        }
    }

    pub fn progress_fraction(&self) -> Option<f32> {
        match &self.state {
            ModelDownloadState::Downloading {
                downloaded_bytes,
                total_bytes: Some(total_bytes),
                ..
            } if *total_bytes > 0 => Some(*downloaded_bytes as f32 / *total_bytes as f32),
            _ => None,
        }
    }

    pub fn progress_basis_points(&self) -> Option<u16> {
        self.progress_fraction()
            .map(|fraction| (fraction.clamp(0.0, 1.0) * 10_000.0).round() as u16)
    }

    pub fn summary(&self, settings: &AppSettings) -> String {
        match &self.state {
            ModelDownloadState::Idle => summary_for_path(resolve_model_path(settings).ok()),
            ModelDownloadState::Missing => {
                format!(
                    "{} has not been downloaded yet.",
                    settings.local_model.display_label()
                )
            }
            ModelDownloadState::Corrupt => {
                format!(
                    "{} is damaged or incomplete. Please download it again.",
                    settings.local_model.display_label()
                )
            }
            ModelDownloadState::Ready { path } => summary_for_existing_path(path),
            ModelDownloadState::Downloading {
                preset,
                downloaded_bytes,
                total_bytes,
                started_at,
            } => {
                let progress = match total_bytes {
                    Some(total_bytes) if *total_bytes > 0 => format!(
                        "{} of {}",
                        human_readable_size(*downloaded_bytes),
                        human_readable_size(*total_bytes)
                    ),
                    _ => format!("{} downloaded", human_readable_size(*downloaded_bytes)),
                };
                format!(
                    "Download for {} has been running for {} ({progress}).",
                    preset.display_label(),
                    human_readable_duration(started_at.elapsed())
                )
            }
            ModelDownloadState::Failed { message } => {
                format!("Last model download failed: {message}")
            }
        }
    }
}

enum ModelDownloadState {
    Idle,
    Missing,
    Corrupt,
    Ready {
        path: PathBuf,
    },
    Downloading {
        preset: ModelPreset,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        started_at: Instant,
    },
    Failed {
        message: String,
    },
}

enum DownloadEvent {
    Progress {
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Completed {
        path: PathBuf,
        downloaded_bytes: u64,
    },
    Failed(String),
}

pub fn resolve_model_path(settings: &AppSettings) -> Result<PathBuf, String> {
    if !settings.local_model_path.trim().is_empty() {
        return Ok(PathBuf::from(settings.local_model_path.trim()));
    }

    default_model_path(settings.local_model)
}

pub fn default_model_path(preset: ModelPreset) -> Result<PathBuf, String> {
    let project_dirs = ProjectDirs::from("com", "gettorro", "TorroWhisper")
        .ok_or_else(|| "Config directory for models not available.".to_owned())?;
    Ok(project_dirs
        .config_dir()
        .join("models")
        .join(preset.default_filename()))
}

/// Returns the number of downloaded bytes on success.
fn download_model_file(
    url: &str,
    target_path: &Path,
    temp_path: &Path,
    expected_size: Option<u64>,
    tx: &mpsc::Sender<DownloadEvent>,
) -> Result<u64, String> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Model directory could not be created: {err}"))?;
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("HTTP client for model download failed: {err}"))?;

    let mut response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Model download failed: {err}"))?;

    let total_bytes = response.content_length();
    let mut file = fs::File::create(temp_path)
        .map_err(|err| format!("Temporary model file could not be created: {err}"))?;
    let mut buffer = [0_u8; DOWNLOAD_BUFFER_SIZE];
    let mut downloaded_bytes = 0_u64;
    let mut last_progress = Instant::now() - DOWNLOAD_PROGRESS_INTERVAL;

    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|err| format!("Read error during download: {err}"))?;
        if read == 0 {
            break;
        }

        file.write_all(&buffer[..read])
            .map_err(|err| format!("Model could not be written to disk: {err}"))?;
        downloaded_bytes += read as u64;

        if last_progress.elapsed() >= DOWNLOAD_PROGRESS_INTERVAL {
            let _ = tx.send(DownloadEvent::Progress {
                downloaded_bytes,
                total_bytes,
            });
            last_progress = Instant::now();
        }
    }

    file.sync_all()
        .map_err(|err| format!("Model file could not be finalized: {err}"))?;
    drop(file);

    if let Some(total) = total_bytes
        && downloaded_bytes != total
    {
        return Err(format!(
            "Model download was incomplete ({} of {}). Please try again.",
            human_readable_size(downloaded_bytes),
            human_readable_size(total)
        ));
    }

    match model_file_integrity(temp_path, expected_size) {
        ModelIntegrity::Valid => {}
        ModelIntegrity::Missing => {
            return Err("Downloaded model file disappeared before verification.".to_owned());
        }
        ModelIntegrity::Corrupt { reason } => {
            log::warn!(
                target: "models",
                "downloaded model failed verification ({}): {reason}",
                temp_path.display()
            );
            return Err(format!("Downloaded model failed verification: {reason}"));
        }
    }

    fs::rename(temp_path, target_path)
        .map_err(|err| format!("Model file could not be activated: {err}"))?;

    let _ = tx.send(DownloadEvent::Completed {
        path: target_path.to_path_buf(),
        downloaded_bytes,
    });

    Ok(downloaded_bytes)
}

/// Average bytes per second; saturates to the total on sub-second downloads.
fn per_second(bytes: u64, elapsed: Duration) -> u64 {
    let secs = elapsed.as_secs_f64();
    if secs < 0.001 {
        bytes
    } else {
        (bytes as f64 / secs) as u64
    }
}

fn temporary_download_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("model.bin");
    target_path.with_file_name(format!("{file_name}.part"))
}

fn cleanup_temp_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|err| format!("Temp file could not be removed: {err}"))?;
    }

    Ok(())
}

fn summary_for_path(path: Option<PathBuf>) -> String {
    match path {
        Some(path) if path.exists() => summary_for_existing_path(&path),
        Some(_) => "Local model has not been downloaded yet.".to_owned(),
        None => "Local model path is not currently resolvable.".to_owned(),
    }
}

fn summary_for_existing_path(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(metadata) => format!(
            "Local model ready ({})",
            human_readable_size(metadata.len())
        ),
        Err(_) => "Local model ready.".to_owned(),
    }
}

fn model_label_for_path(path: &Path) -> &'static str {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("ggml-tiny.bin") => "Whisper Tiny",
        Some("ggml-base.bin") => "Whisper Base",
        Some("ggml-small.bin") => "Whisper Small",
        Some("ggml-large-v3-turbo-q5_0.bin") => "Whisper Large v3 Turbo (compact)",
        Some("ggml-medium.bin") => "Whisper Medium",
        Some("ggml-large-v3-turbo.bin") => "Whisper Large v3 Turbo",
        Some("ggml-large-v3.bin") => "Whisper Large v3",
        _ => "local model",
    }
}

pub fn human_readable_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];

    let mut value = bytes as f64;
    let mut unit_index = 0_usize;
    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{bytes} {}", UNITS[unit_index])
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}

fn human_readable_duration(duration: Duration) -> String {
    if duration.as_secs() < 60 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}m {}s", duration.as_secs() / 60, duration.as_secs() % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_download_path_keeps_original_name() {
        let path = temporary_download_path(Path::new("/tmp/ggml-small.bin"));
        assert!(path.ends_with("ggml-small.bin.part"));
    }

    #[test]
    fn human_readable_size_uses_expected_units() {
        assert_eq!(human_readable_size(900), "900 B");
        assert_eq!(human_readable_size(2_048), "2.0 KB");
    }

    fn write_temp_model(name: &str, contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("torrowhisper-test-{name}"));
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn integrity_missing_for_absent_file() {
        let path = std::env::temp_dir().join("torrowhisper-test-does-not-exist.bin");
        assert_eq!(model_file_integrity(&path, None), ModelIntegrity::Missing);
    }

    #[test]
    fn integrity_valid_for_ggml_file_with_expected_size() {
        let mut contents = GGML_FILE_MAGIC.to_le_bytes().to_vec();
        contents.extend_from_slice(&[0_u8; 12]);
        let path = write_temp_model("valid.bin", &contents);
        assert_eq!(
            model_file_integrity(&path, Some(contents.len() as u64)),
            ModelIntegrity::Valid
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn integrity_corrupt_on_size_mismatch() {
        let mut contents = GGML_FILE_MAGIC.to_le_bytes().to_vec();
        contents.extend_from_slice(&[0_u8; 12]);
        let path = write_temp_model("truncated.bin", &contents);
        assert!(matches!(
            model_file_integrity(&path, Some(contents.len() as u64 + 1)),
            ModelIntegrity::Corrupt { .. }
        ));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn integrity_corrupt_on_bad_magic() {
        let contents = b"<html>not a model</html>".to_vec();
        let path = write_temp_model("bad-magic.bin", &contents);
        assert!(matches!(
            model_file_integrity(&path, Some(contents.len() as u64)),
            ModelIntegrity::Corrupt { .. }
        ));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn cleanup_removes_only_part_files() {
        let dir = std::env::temp_dir().join("torrowhisper-test-partials");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ggml-base.bin.part"), b"partial").unwrap();
        fs::write(dir.join("ggml-base.bin"), b"keep").unwrap();
        assert_eq!(cleanup_partial_downloads_in(&dir), 1);
        assert!(!dir.join("ggml-base.bin.part").exists());
        assert!(dir.join("ggml-base.bin").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
