use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use directories::ProjectDirs;
use open_whisper_core::{AppSettings, LlmPreset};
use reqwest::blocking::Client;

const USER_AGENT: &str = "open-whisper/0.1";
const DOWNLOAD_BUFFER_SIZE: usize = 256 * 1024;
const DOWNLOAD_PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// GGUF files start with the ASCII magic "GGUF" — `0x4655_4747` little-endian.
/// Mirrors the ggml magic check the whisper models use in
/// [`crate::model_manager::model_file_integrity`].
const GGUF_FILE_MAGIC: u32 = 0x4655_4747;

/// Result of verifying a language-model file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmModelIntegrity {
    Missing,
    Valid,
    Corrupt { reason: String },
}

/// Validates that `path` exists, matches `expected_size` when known and starts
/// with the GGUF magic header. The size is only known for bundled presets;
/// custom models pass `None` and are checked by magic only — the same split the
/// whisper integrity check uses for preset vs. custom paths.
pub fn gguf_file_integrity(path: &Path, expected_size: Option<u64>) -> LlmModelIntegrity {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return LlmModelIntegrity::Missing,
    };

    if let Some(expected) = expected_size
        && metadata.len() != expected
    {
        return LlmModelIntegrity::Corrupt {
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
            if u32::from_le_bytes(magic_bytes) != GGUF_FILE_MAGIC {
                return LlmModelIntegrity::Corrupt {
                    reason: "file does not start with the GGUF magic header".to_owned(),
                };
            }
        }
        Err(err) => {
            return LlmModelIntegrity::Corrupt {
                reason: format!("file header could not be read: {err}"),
            };
        }
    }

    LlmModelIntegrity::Valid
}

/// Removes stale `.part` files left behind by interrupted language-model
/// downloads, in both the preset and custom directories. Mirrors the whisper
/// [`crate::model_manager::cleanup_partial_downloads`] call at startup.
pub fn cleanup_partial_downloads() -> usize {
    let Ok(model_path) = default_llm_model_path(LlmPreset::default()) else {
        return 0;
    };
    let Some(dir) = model_path.parent() else {
        return 0;
    };
    let mut removed = crate::model_manager::cleanup_partial_downloads_in(dir);
    removed += crate::model_manager::cleanup_partial_downloads_in(&dir.join("custom"));
    removed
}

pub struct LlmModelDownloadManager {
    state: LlmDownloadState,
    download_rx: Option<Receiver<DownloadEvent>>,
}

impl LlmModelDownloadManager {
    pub fn new() -> Self {
        Self {
            state: LlmDownloadState::Idle,
            download_rx: None,
        }
    }

    pub fn start_download(&mut self, settings: &AppSettings) -> Result<String, String> {
        self.start_download_for(settings.local_llm)
    }

    pub fn start_download_for(&mut self, preset: LlmPreset) -> Result<String, String> {
        if self.is_downloading() {
            return Err("A language model download is already running.".to_owned());
        }

        let target_path = default_llm_model_path(preset)?;
        match gguf_file_integrity(&target_path, Some(preset.download_size_bytes())) {
            LlmModelIntegrity::Valid => {
                self.state = LlmDownloadState::Ready {
                    path: target_path.clone(),
                };
                return Ok(format!("{} is already present.", preset.display_label()));
            }
            LlmModelIntegrity::Corrupt { reason } => {
                log::warn!(
                    target: "models",
                    "{} on disk is corrupt ({reason}); re-downloading",
                    preset.display_label()
                );
                let _ = fs::remove_file(&target_path);
            }
            LlmModelIntegrity::Missing => {}
        }

        let download_url = preset.download_url().to_owned();
        let download_path = target_path.clone();
        let temp_path = temporary_download_path(&target_path);
        let (tx, rx) = mpsc::channel();

        self.download_rx = Some(rx);
        self.state = LlmDownloadState::Downloading {
            target: LlmDownloadTarget::Preset(preset),
            downloaded_bytes: 0,
            total_bytes: None,
            started_at: Instant::now(),
        };

        log::info!(
            target: "models",
            "llm download started: {} from {download_url} (expected {})",
            preset.default_filename(),
            human_readable_size(preset.download_size_bytes())
        );
        spawn_logged_download(download_url, download_path, temp_path, tx);

        Ok(format!("Download for {} started.", preset.display_label()))
    }

    pub fn start_custom_download(
        &mut self,
        id: &str,
        display_name: &str,
        url: &str,
    ) -> Result<String, String> {
        if self.is_downloading() {
            return Err("A language model download is already running.".to_owned());
        }

        let target_path = default_custom_llm_path(id)?;
        match gguf_file_integrity(&target_path, None) {
            LlmModelIntegrity::Valid => {
                self.state = LlmDownloadState::Ready {
                    path: target_path.clone(),
                };
                return Ok(format!("{} is already present.", display_name));
            }
            LlmModelIntegrity::Corrupt { reason } => {
                log::warn!(
                    target: "models",
                    "custom language model '{display_name}' on disk is corrupt ({reason}); re-downloading"
                );
                let _ = fs::remove_file(&target_path);
            }
            LlmModelIntegrity::Missing => {}
        }

        let download_url = url.trim().to_owned();
        if download_url.is_empty() {
            return Err("URL for custom language model is empty.".to_owned());
        }
        let download_path = target_path.clone();
        let temp_path = temporary_download_path(&target_path);
        let (tx, rx) = mpsc::channel();

        self.download_rx = Some(rx);
        self.state = LlmDownloadState::Downloading {
            target: LlmDownloadTarget::Custom {
                id: id.to_owned(),
                display_name: display_name.to_owned(),
            },
            downloaded_bytes: 0,
            total_bytes: None,
            started_at: Instant::now(),
        };

        log::info!(
            target: "models",
            "llm download started: custom '{display_name}' from {download_url}"
        );
        spawn_logged_download(download_url, download_path, temp_path, tx);

        Ok(format!("Download for {} started.", display_name))
    }

    pub fn delete_custom_file(&mut self, id: &str, display_name: &str) -> Result<String, String> {
        if self.is_downloading_custom(id) {
            return Err("A running download can't be deleted at the same time.".to_owned());
        }

        let path = default_custom_llm_path(id)?;
        if !path.exists() {
            return Ok(format!("{} was already not present locally.", display_name));
        }
        fs::remove_file(&path)
            .map_err(|err| format!("Language model could not be deleted: {err}"))?;

        if !self.is_downloading() {
            self.state = LlmDownloadState::Missing;
        }

        Ok(format!("{} was deleted locally.", display_name))
    }

    pub fn is_downloading_custom(&self, id: &str) -> bool {
        matches!(
            &self.state,
            LlmDownloadState::Downloading { target: LlmDownloadTarget::Custom { id: active, .. }, .. }
                if active == id
        )
    }

    pub fn active_download_custom_id(&self) -> Option<String> {
        if let LlmDownloadState::Downloading {
            target: LlmDownloadTarget::Custom { id, .. },
            ..
        } = &self.state
        {
            Some(id.clone())
        } else {
            None
        }
    }

    pub fn delete_downloaded_model(&mut self, settings: &AppSettings) -> Result<String, String> {
        self.delete_preset(settings.local_llm)
    }

    pub fn delete_preset(&mut self, preset: LlmPreset) -> Result<String, String> {
        if self.is_downloading_preset(preset) {
            return Err("A running download can't be deleted at the same time.".to_owned());
        }

        let path = default_llm_model_path(preset)?;
        if !path.exists() {
            return Ok(format!(
                "{} was already not present locally.",
                preset.display_label()
            ));
        }

        fs::remove_file(&path)
            .map_err(|err| format!("Language model could not be deleted: {err}"))?;

        if !self.is_downloading() {
            self.state = LlmDownloadState::Missing;
        }

        Ok(format!("{} was deleted locally.", preset.display_label()))
    }

    pub fn is_downloading_preset(&self, preset: LlmPreset) -> bool {
        matches!(
            &self.state,
            LlmDownloadState::Downloading { target: LlmDownloadTarget::Preset(active), .. }
                if *active == preset
        )
    }

    pub fn active_download_preset(&self) -> Option<LlmPreset> {
        if let LlmDownloadState::Downloading {
            target: LlmDownloadTarget::Preset(preset),
            ..
        } = &self.state
        {
            Some(*preset)
        } else {
            None
        }
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
                        if let LlmDownloadState::Downloading {
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
                        let label = llm_label_for_path(&path);
                        self.download_rx = None;
                        self.state = LlmDownloadState::Ready { path: path.clone() };
                        messages.push(format!(
                            "Language model loaded: {} ({})",
                            label,
                            human_readable_size(downloaded_bytes)
                        ));
                        break;
                    }
                    Ok(DownloadEvent::Failed(err)) => {
                        self.download_rx = None;
                        self.state = LlmDownloadState::Failed {
                            message: err.clone(),
                        };
                        messages.push(err);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.download_rx = None;
                        self.state = LlmDownloadState::Failed {
                            message: "Language model download worker stopped unexpectedly."
                                .to_owned(),
                        };
                        messages.push(
                            "Language model download worker stopped unexpectedly.".to_owned(),
                        );
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

        if let Ok(path) = resolve_llm_model_path(settings) {
            self.state = if path.exists() {
                LlmDownloadState::Ready { path }
            } else {
                LlmDownloadState::Missing
            };
        }
    }

    pub fn is_downloading(&self) -> bool {
        matches!(self.state, LlmDownloadState::Downloading { .. })
    }

    pub fn is_downloaded(&self, settings: &AppSettings) -> bool {
        match &self.state {
            LlmDownloadState::Ready { .. } => true,
            _ => resolve_llm_model_path(settings)
                .map(|path| path.exists())
                .unwrap_or(false),
        }
    }

    pub fn progress_fraction(&self) -> Option<f32> {
        match &self.state {
            LlmDownloadState::Downloading {
                downloaded_bytes,
                total_bytes: Some(total_bytes),
                ..
            } if *total_bytes > 0 => Some(*downloaded_bytes as f32 / *total_bytes as f32),
            _ => None,
        }
    }

    pub fn progress_basis_points(&self) -> Option<u16> {
        self.progress_fraction()
            .map(|fraction| (fraction.clamp(0.0, 1.0) * 10_000.0) as u16)
    }

    pub fn summary(&self, settings: &AppSettings) -> String {
        match &self.state {
            LlmDownloadState::Idle => summary_for_path(resolve_llm_model_path(settings).ok()),
            LlmDownloadState::Missing => format!(
                "{} has not been downloaded yet.",
                settings.local_llm.display_label()
            ),
            LlmDownloadState::Ready { path } => summary_for_existing_path(path),
            LlmDownloadState::Downloading {
                target,
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
                let label = match target {
                    LlmDownloadTarget::Preset(preset) => preset.display_label().to_owned(),
                    LlmDownloadTarget::Custom { display_name, .. } => display_name.clone(),
                };
                format!(
                    "Download for {} has been running for {} ({progress}).",
                    label,
                    human_readable_duration(started_at.elapsed())
                )
            }
            LlmDownloadState::Failed { message } => {
                format!("Last language model download failed: {message}")
            }
        }
    }
}

impl Default for LlmModelDownloadManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmDownloadTarget {
    Preset(LlmPreset),
    Custom { id: String, display_name: String },
}

enum LlmDownloadState {
    Idle,
    Missing,
    Ready {
        path: PathBuf,
    },
    Downloading {
        target: LlmDownloadTarget,
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

pub fn resolve_llm_model_path(settings: &AppSettings) -> Result<PathBuf, String> {
    if !settings.local_llm_path.trim().is_empty() {
        return Ok(PathBuf::from(settings.local_llm_path.trim()));
    }

    default_llm_model_path(settings.local_llm)
}

pub fn default_llm_model_path(preset: LlmPreset) -> Result<PathBuf, String> {
    let project_dirs = ProjectDirs::from("dev", "awesome", "open-whisper")
        .ok_or_else(|| "Config directory for language models not available.".to_owned())?;
    Ok(project_dirs
        .config_dir()
        .join("llm-models")
        .join(preset.default_filename()))
}

pub fn default_custom_llm_path(id: &str) -> Result<PathBuf, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("Custom language model has no ID.".to_owned());
    }
    let project_dirs = ProjectDirs::from("dev", "awesome", "open-whisper")
        .ok_or_else(|| "Config directory for language models not available.".to_owned())?;
    Ok(project_dirs
        .config_dir()
        .join("llm-models")
        .join("custom")
        .join(format!("{trimmed}.gguf")))
}

/// Runs the download on a worker thread and logs outcome plus duration/speed.
fn spawn_logged_download(
    download_url: String,
    download_path: PathBuf,
    temp_path: PathBuf,
    tx: mpsc::Sender<DownloadEvent>,
) {
    thread::spawn(move || {
        let started = Instant::now();
        let result = download_model_file(&download_url, &download_path, &temp_path, &tx);
        let elapsed = started.elapsed();
        match result {
            Ok(downloaded_bytes) => log::info!(
                target: "models",
                "llm download finished: {} ({} in {}, {}/s)",
                download_path.display(),
                human_readable_size(downloaded_bytes),
                human_readable_duration(elapsed),
                human_readable_size(per_second(downloaded_bytes, elapsed))
            ),
            Err(err) => {
                log::error!(
                    target: "models",
                    "llm download failed: {} after {} — {err}",
                    download_path.display(),
                    human_readable_duration(elapsed)
                );
                let _ = cleanup_temp_file(&temp_path);
                let _ = tx.send(DownloadEvent::Failed(err));
            }
        }
    });
}

/// Returns the number of downloaded bytes on success.
fn download_model_file(
    url: &str,
    target_path: &Path,
    temp_path: &Path,
    tx: &mpsc::Sender<DownloadEvent>,
) -> Result<u64, String> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Language model directory could not be created: {err}"))?;
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("HTTP client for language model download failed: {err}"))?;

    let mut response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Language model download failed: {err}"))?;

    let total_bytes = response.content_length();
    let mut file = fs::File::create(temp_path)
        .map_err(|err| format!("Temporary language model file could not be created: {err}"))?;
    let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
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
            .map_err(|err| format!("Language model could not be written to disk: {err}"))?;
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
        .map_err(|err| format!("Language model file could not be finalized: {err}"))?;
    drop(file);

    if let Some(total) = total_bytes
        && downloaded_bytes != total
    {
        return Err(format!(
            "Language model download was incomplete ({} of {}). Please try again.",
            human_readable_size(downloaded_bytes),
            human_readable_size(total)
        ));
    }

    // Verify the freshly downloaded file is a real GGUF before activating it, so
    // a truncated/HTML-error response never gets renamed into place and later
    // crashes the llama helper. Size was already checked against Content-Length
    // above, so a magic-only check is enough here.
    if let LlmModelIntegrity::Corrupt { reason } = gguf_file_integrity(temp_path, None) {
        let _ = fs::remove_file(temp_path);
        return Err(format!(
            "Downloaded language model failed verification ({reason}). Please try again."
        ));
    }

    fs::rename(temp_path, target_path)
        .map_err(|err| format!("Language model file could not be activated: {err}"))?;

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

/// Writes an inventory of local language model files to the log.
pub fn log_llm_inventory(settings: &AppSettings) {
    for preset in LlmPreset::ALL {
        let filename = preset.default_filename();
        let Ok(path) = default_llm_model_path(preset) else {
            continue;
        };
        match fs::metadata(&path) {
            Ok(metadata) if metadata.len() == preset.download_size_bytes() => {
                log::info!(
                    target: "models",
                    "llm inventory: {filename} — OK ({})",
                    human_readable_size(metadata.len())
                );
            }
            Ok(metadata) => log::warn!(
                target: "models",
                "llm inventory: {filename} — size {} but {} expected",
                human_readable_size(metadata.len()),
                human_readable_size(preset.download_size_bytes())
            ),
            Err(_) => {
                log::info!(target: "models", "llm inventory: {filename} — not downloaded");
            }
        }
    }

    for entry in &settings.custom_llm_models {
        let location = match &entry.source {
            open_whisper_core::CustomLlmSource::LocalPath { path } => {
                if Path::new(path).exists() {
                    format!("local file present ({path})")
                } else {
                    format!("local file MISSING ({path})")
                }
            }
            open_whisper_core::CustomLlmSource::DownloadUrl { .. } => {
                match default_custom_llm_path(&entry.id) {
                    Ok(path) if path.exists() => format!("downloaded ({})", path.display()),
                    Ok(_) => "not downloaded".to_owned(),
                    Err(err) => format!("path not resolvable: {err}"),
                }
            }
        };
        log::info!(
            target: "models",
            "llm inventory: custom '{}' — {location}",
            entry.name
        );
    }
}

fn temporary_download_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("model.gguf");
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
        Some(_) => "Local language model has not been downloaded yet.".to_owned(),
        None => "Local language model path is not currently resolvable.".to_owned(),
    }
}

fn summary_for_existing_path(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(metadata) => format!(
            "Local language model ready ({})",
            human_readable_size(metadata.len())
        ),
        Err(_) => "Local language model ready.".to_owned(),
    }
}

fn llm_label_for_path(path: &Path) -> &'static str {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("google_gemma-4-E2B-it-Q4_K_M.gguf") => "Gemma 4 E2B (small)",
        Some("google_gemma-4-E4B-it-Q4_K_M.gguf") => "Gemma 4 E4B (medium)",
        Some("google_gemma-4-26B-A4B-it-Q4_K_M.gguf") => "Gemma 4 26B (large)",
        _ => "local language model",
    }
}

pub fn purge_legacy_llm_files() -> Result<Vec<String>, String> {
    use open_whisper_core::LEGACY_LLM_FILENAMES;

    let Some(project_dirs) = ProjectDirs::from("dev", "awesome", "open-whisper") else {
        return Ok(Vec::new());
    };

    let dir = project_dirs.config_dir().join("llm-models");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut removed = Vec::new();
    for filename in LEGACY_LLM_FILENAMES {
        let candidate = dir.join(filename);
        if candidate.exists() {
            fs::remove_file(&candidate).map_err(|err| {
                format!(
                    "Old model file {} could not be removed: {err}",
                    candidate.display()
                )
            })?;
            removed.push((*filename).to_owned());
        }
    }

    Ok(removed)
}

fn human_readable_size(bytes: u64) -> String {
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
        let path = temporary_download_path(Path::new("/tmp/Qwen2.5-3B-Instruct-Q4_K_M.gguf"));
        assert!(path.ends_with("Qwen2.5-3B-Instruct-Q4_K_M.gguf.part"));
    }

    #[test]
    fn default_llm_path_is_under_llm_models_dir() {
        let path = default_llm_model_path(LlmPreset::Medium).unwrap();
        let as_str = path.to_string_lossy();
        assert!(as_str.contains("llm-models"));
        assert!(as_str.ends_with("google_gemma-4-E4B-it-Q4_K_M.gguf"));
    }

    #[test]
    fn progress_basis_points_scales_to_ten_thousand() {
        let mut manager = LlmModelDownloadManager::new();
        manager.state = LlmDownloadState::Downloading {
            target: LlmDownloadTarget::Preset(LlmPreset::Medium),
            downloaded_bytes: 500,
            total_bytes: Some(1_000),
            started_at: Instant::now(),
        };
        assert_eq!(manager.progress_basis_points(), Some(5_000));
    }

    #[test]
    fn gguf_integrity_missing_for_absent_file() {
        let path =
            std::env::temp_dir().join(format!("ow-gguf-missing-{}.gguf", std::process::id()));
        let _ = fs::remove_file(&path);
        assert_eq!(gguf_file_integrity(&path, None), LlmModelIntegrity::Missing);
    }

    #[test]
    fn gguf_integrity_valid_for_magic_header() {
        let path = std::env::temp_dir().join(format!("ow-gguf-valid-{}.gguf", std::process::id()));
        // "GGUF" magic followed by arbitrary padding.
        fs::write(&path, [0x47, 0x47, 0x55, 0x46, 0x00, 0x01, 0x02, 0x03]).unwrap();
        assert_eq!(gguf_file_integrity(&path, None), LlmModelIntegrity::Valid);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn gguf_integrity_corrupt_for_wrong_magic() {
        let path = std::env::temp_dir().join(format!("ow-gguf-bad-{}.gguf", std::process::id()));
        // An HTML error page, not a GGUF file.
        fs::write(&path, b"<html>error</html>").unwrap();
        assert!(matches!(
            gguf_file_integrity(&path, None),
            LlmModelIntegrity::Corrupt { .. }
        ));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn gguf_integrity_corrupt_for_size_mismatch() {
        let path = std::env::temp_dir().join(format!("ow-gguf-size-{}.gguf", std::process::id()));
        fs::write(&path, [0x47, 0x47, 0x55, 0x46, 0x00]).unwrap();
        assert!(matches!(
            gguf_file_integrity(&path, Some(999_999)),
            LlmModelIntegrity::Corrupt { .. }
        ));
        let _ = fs::remove_file(&path);
    }
}
