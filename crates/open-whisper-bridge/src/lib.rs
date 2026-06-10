#[allow(dead_code)]
mod autostart;
#[allow(dead_code)]
mod dictation;
mod dictionary;
mod history_store;
#[allow(dead_code)]
mod llm_model_manager;
#[allow(dead_code)]
mod local_llm;
#[allow(dead_code)]
mod model_manager;
mod permission_diagnostics;
mod post_processing;
mod remote_models;
#[allow(dead_code)]
mod settings_store;
#[allow(dead_code)]
mod text_inserter;

use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use autostart::AutostartManager;
use dictation::{DictationController, DictationOutcome, MicSwitchEvent};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use llm_model_manager::LlmModelDownloadManager;
use model_manager::ModelDownloadManager;
use open_whisper_core::{
    AppSettings, CustomLlmSource, CustomLlmStatusDto, DeviceDto, DiagnosticsDto, HistoryEntry,
    LlmModelStatusDto, LlmPreset, ModelPreset, ModelStatusDto, RecordingLevelsDto,
    RemoteModelBackend, RemoteModelDto, RuntimeStatusDto,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

thread_local! {
    static RUNTIME: RefCell<BridgeRuntime> = RefCell::new(BridgeRuntime::new());
}

struct BridgeRuntime {
    autostart: AutostartManager,
    settings: AppSettings,
    dictation: DictationController,
    model_downloads: ModelDownloadManager,
    llm_downloads: LlmModelDownloadManager,
    hotkey: Option<HotKeyController>,
    post_processing_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    pending_post_processing: Option<PendingPostProcessing>,
    dictation_trigger_count: u64,
    last_status: String,
    last_transcript: String,
    cancelled: Arc<AtomicBool>,
    history: Vec<HistoryEntry>,
    history_revision: u64,
}

struct PendingPostProcessing {
    raw_transcript: String,
    mode_name: String,
    provider_label: String,
}

impl BridgeRuntime {
    fn new() -> Self {
        let mut last_status = "Ready".to_owned();
        let mut settings = settings_store::load().unwrap_or_else(|err| {
            last_status = format!("Settings could not be loaded: {err}");
            AppSettings::default()
        });
        settings.normalize();

        let mut autostart = AutostartManager::new();
        let mut dictation = DictationController::new();
        let mut model_downloads = ModelDownloadManager::new();
        let mut llm_downloads = LlmModelDownloadManager::new();

        for message in dictation.refresh_input_devices(&mut settings) {
            last_status = message;
        }

        if settings.local_model_path.trim().is_empty()
            && let Ok(path) = dictation.suggested_model_path(&settings)
        {
            settings.local_model_path = path.display().to_string();
        }

        model_downloads.refresh_local_state(&settings);

        match llm_model_manager::purge_legacy_llm_files() {
            Ok(removed) if !removed.is_empty() => {
                last_status = format!(
                    "Removed old language models ({} file(s)). Gemma 4 is now used.",
                    removed.len()
                );
            }
            Ok(_) => {}
            Err(err) => {
                last_status = err;
            }
        }

        llm_downloads.refresh_local_state(&settings);

        if let Ok(Some(message)) = autostart.sync_saved_settings(&settings) {
            last_status = message;
        }

        let mut hotkey = HotKeyController::new().ok();
        if let Some(controller) = &mut hotkey
            && let Err(err) = controller.apply_settings(&settings)
        {
            last_status = err;
        }

        Self {
            autostart,
            settings,
            dictation,
            model_downloads,
            llm_downloads,
            hotkey,
            post_processing_rx: None,
            pending_post_processing: None,
            dictation_trigger_count: 0,
            last_status,
            last_transcript: String::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            history: history_store::load().unwrap_or_default(),
            history_revision: 0,
        }
    }

    fn poll(&mut self) {
        for message in self.model_downloads.poll() {
            self.last_status = message;
        }

        for message in self.llm_downloads.poll() {
            self.last_status = message;
        }

        local_llm::maybe_unload_shared_runtime(self.settings.local_llm_auto_unload_secs);

        if let Some(rx) = &self.post_processing_rx {
            match rx.try_recv() {
                Ok(Ok(processed_text)) => {
                    self.post_processing_rx = None;
                    let was_cancelled = self.cancelled.load(Ordering::Relaxed);
                    let mode_name = self
                        .pending_post_processing
                        .as_ref()
                        .map(|pending| pending.mode_name.clone())
                        .unwrap_or_else(|| self.settings.active_mode_name().to_owned());
                    self.pending_post_processing = None;
                    self.finish_transcript(
                        processed_text,
                        &format!("Post-processing '{mode_name}' complete."),
                        was_cancelled,
                    );
                }
                Ok(Err(err)) => {
                    self.post_processing_rx = None;
                    let was_cancelled = self.cancelled.load(Ordering::Relaxed);
                    if let Some(pending) = self.pending_post_processing.take() {
                        let fallback_status = format!(
                            "Post-processing '{}' via {} failed. Using raw transcript. {err}",
                            pending.mode_name, pending.provider_label
                        );
                        self.finish_transcript(pending.raw_transcript, &fallback_status, was_cancelled);
                    } else if !was_cancelled {
                        self.last_status = err;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.post_processing_rx = None;
                    let was_cancelled = self.cancelled.load(Ordering::Relaxed);
                    if let Some(pending) = self.pending_post_processing.take() {
                        let fallback_status = format!(
                            "Post-processing '{}' stopped unexpectedly. Using raw transcript.",
                            pending.mode_name
                        );
                        self.finish_transcript(pending.raw_transcript, &fallback_status, was_cancelled);
                    } else if !was_cancelled {
                        self.last_status =
                            "Post-processing worker stopped unexpectedly.".to_owned();
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        let pending_actions = self
            .hotkey
            .as_mut()
            .map(HotKeyController::poll_actions)
            .unwrap_or_default();
        for action in pending_actions {
            match action {
                HotKeyAction::Pressed => {
                    self.dictation_trigger_count += 1;
                    if !self.dictation.is_recording() && !self.dictation.is_transcribing() {
                        self.reset_cancellation();
                    }
                    let outcomes = self.dictation.handle_hotkey(&self.settings, true);
                    self.apply_dictation_outcomes(outcomes);
                }
                HotKeyAction::Released => {
                    let outcomes = self.dictation.handle_hotkey(&self.settings, false);
                    self.apply_dictation_outcomes(outcomes);
                }
            }
        }

        let previous_input_device = self.settings.input_device_name.clone();
        let outcomes = self.dictation.poll(&mut self.settings);
        self.apply_dictation_outcomes(outcomes);
        if self.settings.input_device_name != previous_input_device {
            let _ = settings_store::save(&self.settings);
        }
    }

    fn apply_dictation_outcomes(&mut self, outcomes: Vec<DictationOutcome>) {
        for outcome in outcomes {
            match outcome {
                DictationOutcome::Status(message) => {
                    if !self.cancelled.load(Ordering::Relaxed) {
                        self.last_status = message;
                    }
                }
                DictationOutcome::TranscriptReady(transcript) => {
                    let was_cancelled = self.cancelled.load(Ordering::Relaxed);
                    let mode = self.settings.active_mode().clone();
                    let transcript = if mode.dictionary_enabled {
                        dictionary::apply(&self.settings.dictionary, &transcript)
                    } else {
                        transcript
                    };
                    if !was_cancelled && self.settings.active_mode_post_processing_enabled() {
                        let provider_label = self
                            .settings
                            .active_post_processing_backend
                            .label()
                            .to_owned();
                        let mode_name = mode.name.clone();
                        let raw_transcript = transcript.clone();
                        let settings = self.settings.clone();
                        let (tx, rx) = std::sync::mpsc::channel();
                        let cancelled = self.cancelled.clone();
                        std::thread::spawn(move || {
                            let result = post_processing::process_text(
                                &settings,
                                &raw_transcript,
                                &cancelled,
                            );
                            let _ = tx.send(result);
                        });
                        self.post_processing_rx = Some(rx);
                        self.pending_post_processing = Some(PendingPostProcessing {
                            raw_transcript: transcript,
                            mode_name: mode_name.clone(),
                            provider_label,
                        });
                        self.last_status = format!(
                            "Whisper transcript ready. Post-processing '{mode_name}' running."
                        );
                    } else {
                        self.finish_transcript(transcript, "Transcript ready.", was_cancelled);
                    }
                }
            }
        }
    }

    fn reset_cancellation(&mut self) {
        self.cancelled = Arc::new(AtomicBool::new(false));
    }

    fn cancel_dictation(&mut self) -> Result<String, String> {
        let was_recording = self.dictation.is_recording();
        let was_transcribing = self.dictation.is_transcribing();
        let was_post_processing = self.post_processing_rx.is_some();
        let was_blocked = self
            .dictation
            .is_blocked(std::time::Instant::now(), std::time::Duration::from_secs(6));

        if !was_recording && !was_transcribing && !was_post_processing && !was_blocked {
            return Ok(self.last_status.clone());
        }

        self.cancelled.store(true, Ordering::Relaxed);
        self.dictation.clear_blocked();

        if was_recording {
            // Abbruch während der Aufnahme: das bereits aufgenommene Audio NICHT
            // verwerfen. Es wird transkribiert und (als abgebrochen) in der
            // Historie abgelegt; der gesetzte cancelled-Flag verhindert das
            // Einfügen in andere Apps. So geht ein versehentlich mit Escape
            // beendetes Diktat nicht mehr verloren. Sehr kurze/leere Aufnahmen
            // erzeugen keinen Eintrag (Mindestlänge in stop_recording_and_transcribe,
            // leerer Text in record_history_entry).
            match self.dictation.stop_recording_and_transcribe(
                &self.settings,
                "abgebrochen",
                dictation::RecordingCue::Cancel,
            ) {
                Ok(outcomes) => self.apply_dictation_outcomes(outcomes),
                Err(_) => {
                    self.dictation.cancel_recording();
                    dictation::play_cancel_cue();
                }
            }
        } else if was_post_processing
            && let Some(pending) = self.pending_post_processing.take()
        {
            // Post-processing was running: we already have the raw Whisper
            // transcript. Save it to history now and skip the slow LLM result.
            self.post_processing_rx = None;
            self.finish_transcript(pending.raw_transcript, "Diktat abgebrochen.", true);
            dictation::play_cancel_cue();
        } else {
            // Whisper still transcribing (let it finish so TranscriptReady
            // arrives and is saved with was_cancelled = true), or blocked.
            dictation::play_cancel_cue();
        }

        if !self.last_status.starts_with("Diktat abgebrochen") {
            self.last_status = "Diktat abgebrochen.".to_owned();
        }
        Ok(self.last_status.clone())
    }

    fn finish_transcript(&mut self, transcript: String, ready_status: &str, was_cancelled: bool) {
        self.last_transcript = transcript.clone();
        self.record_history_entry(&transcript, was_cancelled);

        if was_cancelled {
            self.last_status = if self.settings.history_enabled && !transcript.trim().is_empty() {
                "Diktat abgebrochen — in Historie gespeichert.".to_owned()
            } else {
                "Diktat abgebrochen.".to_owned()
            };
            return;
        }

        if self.settings.insert_text_automatically {
            match text_inserter::insert_text_into_active_app(&transcript, &self.settings) {
                Ok(message) => {
                    if ready_status.is_empty() {
                        self.last_status = message;
                    } else {
                        self.last_status = format!("{ready_status} {message}");
                    }
                }
                Err(err) => match text_inserter::copy_to_clipboard(&transcript) {
                    Ok(()) => {
                        self.last_status =
                            "Einfuegen fehlgeschlagen – Text in Zwischenablage kopiert.".to_owned();
                    }
                    Err(clip_err) => {
                        self.last_status = format!("{err} Zwischenablage-Fallback: {clip_err}");
                    }
                },
            }
        } else {
            self.last_status = ready_status.to_owned();
        }
    }

    fn record_history_entry(&mut self, transcript: &str, was_cancelled: bool) {
        if !self.settings.history_enabled {
            return;
        }
        if transcript.trim().is_empty() {
            return;
        }

        let mode = self.settings.active_mode().clone();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("h-{}", d.as_nanos()))
            .unwrap_or_else(|_| format!("h-{}", self.history.len()));

        let entry = HistoryEntry {
            id,
            text: transcript.to_owned(),
            timestamp,
            mode_id: mode.id,
            mode_name: mode.name,
            was_cancelled,
        };

        history_store::append(
            &mut self.history,
            entry,
            self.settings.history_max_entries as usize,
        );
        self.history_revision = self.history_revision.wrapping_add(1);
        let _ = history_store::save(&self.history);
    }

    fn load_settings(&mut self) -> AppSettings {
        self.poll();
        self.settings.normalize();
        self.settings.clone()
    }

    fn save_settings(&mut self, mut next_settings: AppSettings) -> Result<String, String> {
        let previous_path = self.settings.local_model_path.clone();
        let previous_model = self.settings.local_model;
        let previous_input_device_name = self.settings.input_device_name.clone();
        next_settings.normalize();

        if next_settings.local_model_path.trim().is_empty()
            && let Ok(path) = self.dictation.suggested_model_path(&next_settings)
        {
            next_settings.local_model_path = path.display().to_string();
        }

        if next_settings.input_device_name != previous_input_device_name
            && !next_settings.input_device_name.trim().is_empty()
        {
            let now = current_unix_seconds();
            let chosen_name = next_settings.input_device_name.clone();
            next_settings.record_input_device_choice(&chosen_name, None, now);
            self.dictation.clear_mic_switch_message();
        }

        for message in self.dictation.refresh_input_devices(&mut next_settings) {
            self.last_status = message;
        }

        settings_store::save(&next_settings)
            .map_err(|err| format!("Settings could not be saved: {err}"))?;
        self.settings = next_settings;

        match self.autostart.sync_saved_settings(&self.settings) {
            Ok(Some(message)) => self.last_status = message,
            Ok(None) => {}
            Err(err) => self.last_status = err,
        }

        if let Some(hotkey) = &mut self.hotkey
            && let Err(err) = hotkey.apply_settings(&self.settings)
        {
            self.last_status = err;
        }

        self.model_downloads.refresh_local_state(&self.settings);

        if previous_path != self.settings.local_model_path
            || previous_model != self.settings.local_model
        {
            self.dictation.invalidate_model_cache();
        }

        Ok(self.last_status.clone())
    }

    fn list_input_devices(&mut self) -> Vec<DeviceDto> {
        self.poll();
        for message in self.dictation.refresh_input_devices(&mut self.settings) {
            self.last_status = message;
        }

        let active = self.dictation.active_input_device_name().to_owned();
        self.dictation
            .available_input_devices()
            .iter()
            .map(|device| DeviceDto {
                name: device.clone(),
                is_selected: *device == self.settings.input_device_name || *device == active,
                uid: None,
            })
            .collect()
    }

    fn reregister_hotkey(&mut self) -> Result<String, String> {
        if let Some(hotkey) = &mut self.hotkey {
            hotkey.force_reapply(&self.settings)?;
        }
        Ok(self.last_status.clone())
    }

    fn notify_device_change(&mut self) -> Option<MicSwitchEvent> {
        self.poll();
        let previous_input_device = self.settings.input_device_name.clone();
        let event = self.dictation.handle_device_change(&mut self.settings);
        if let Some(event) = &event {
            self.last_status = event.message.clone();
        }
        if self.settings.input_device_name != previous_input_device {
            let _ = settings_store::save(&self.settings);
        }
        event
    }

    fn model_status(&mut self) -> ModelStatusDto {
        self.poll();
        self.model_downloads.refresh_local_state(&self.settings);

        let path = model_manager::resolve_model_path(&self.settings)
            .ok()
            .map(|value| value.display().to_string())
            .unwrap_or_else(|| self.settings.local_model_path.clone());

        let is_downloaded = model_manager::resolve_model_path(&self.settings)
            .ok()
            .is_some_and(|value| value.exists());
        let progress_basis_points = self
            .model_downloads
            .progress_fraction()
            .map(|progress| (progress.clamp(0.0, 1.0) * 10_000.0).round() as u16);

        ModelStatusDto {
            preset_label: self.settings.local_model.display_label().to_owned(),
            backend_model_name: self.settings.local_model.whisper_model().to_owned(),
            path,
            summary: self.model_downloads.summary(&self.settings),
            is_downloaded,
            is_downloading: self.model_downloads.is_downloading(),
            progress_basis_points,
            expected_size_bytes: self.settings.local_model.download_size_bytes(),
        }
    }

    fn start_model_download(&mut self, preset: Option<ModelPreset>) -> Result<String, String> {
        self.poll();
        let target = preset.unwrap_or(self.settings.local_model);
        let message = self.model_downloads.start_download_for(target)?;
        self.last_status = message.clone();
        Ok(message)
    }

    fn delete_model(&mut self, preset: Option<ModelPreset>) -> Result<String, String> {
        self.poll();
        let target = preset.unwrap_or(self.settings.local_model);
        let message = self.model_downloads.delete_preset(target)?;
        self.last_status = message.clone();
        self.dictation.invalidate_model_cache();
        Ok(message)
    }

    fn model_status_list(&mut self) -> Vec<ModelStatusDto> {
        self.poll();
        self.model_downloads.refresh_local_state(&self.settings);

        let active_download = self.model_downloads.active_download_preset();
        let active_progress = self.model_downloads.progress_basis_points();

        ModelPreset::ALL
            .iter()
            .copied()
            .map(|preset| {
                let path = model_manager::default_model_path(preset)
                    .map(|value| value.display().to_string())
                    .unwrap_or_default();
                let is_downloaded = model_manager::default_model_path(preset)
                    .map(|value| value.exists())
                    .unwrap_or(false);
                let is_downloading = active_download == Some(preset);
                let progress_basis_points = if is_downloading {
                    active_progress
                } else {
                    None
                };
                let summary = if is_downloading {
                    format!("Download for {} in progress.", preset.display_label())
                } else if is_downloaded {
                    format!("{} ready.", preset.display_label())
                } else {
                    format!(
                        "{} ({}) not loaded yet.",
                        preset.display_label(),
                        model_manager::human_readable_size(preset.download_size_bytes()),
                    )
                };

                ModelStatusDto {
                    preset_label: preset.label().to_owned(),
                    backend_model_name: preset.whisper_model().to_owned(),
                    path,
                    summary,
                    is_downloaded,
                    is_downloading,
                    progress_basis_points,
                    expected_size_bytes: preset.download_size_bytes(),
                }
            })
            .collect()
    }

    fn llm_status_list(&mut self) -> Vec<LlmModelStatusDto> {
        self.poll();
        self.llm_downloads.refresh_local_state(&self.settings);

        let loaded_preset = local_llm::shared_runtime()
            .try_lock()
            .ok()
            .and_then(|runtime| runtime.loaded_preset());
        let active_download = self.llm_downloads.active_download_preset();
        let active_progress = self.llm_downloads.progress_basis_points();

        LlmPreset::ALL
            .iter()
            .copied()
            .map(|preset| {
                let path = llm_model_manager::default_llm_model_path(preset)
                    .map(|value| value.display().to_string())
                    .unwrap_or_default();
                let is_downloaded = llm_model_manager::default_llm_model_path(preset)
                    .map(|value| value.exists())
                    .unwrap_or(false);
                let is_downloading = active_download == Some(preset);
                let progress_basis_points = if is_downloading {
                    active_progress
                } else {
                    None
                };
                let summary = if is_downloading {
                    format!("Download for {} in progress.", preset.display_label())
                } else if is_downloaded {
                    format!("{} ready.", preset.display_label())
                } else {
                    format!("{} not loaded yet.", preset.display_label())
                };

                LlmModelStatusDto {
                    preset_label: preset.label().to_owned(),
                    display_label: preset.display_label().to_owned(),
                    path,
                    summary,
                    is_downloaded,
                    is_downloading,
                    is_loaded: loaded_preset == Some(preset),
                    progress_basis_points,
                    expected_size_bytes: preset.download_size_bytes(),
                }
            })
            .collect()
    }

    fn start_llm_download(&mut self, preset: LlmPreset) -> Result<String, String> {
        self.poll();
        let message = self.llm_downloads.start_download_for(preset)?;
        self.last_status = message.clone();
        Ok(message)
    }

    fn custom_llm_status_list(&mut self) -> Vec<CustomLlmStatusDto> {
        self.poll();

        let loaded_custom_id = local_llm::shared_runtime()
            .try_lock()
            .ok()
            .and_then(|runtime| runtime.loaded_custom_id());
        let active_custom_download = self.llm_downloads.active_download_custom_id();
        let active_progress = self.llm_downloads.progress_basis_points();

        self.settings
            .custom_llm_models
            .iter()
            .map(|entry| {
                let (path_buf, needs_download, source_label) = match &entry.source {
                    CustomLlmSource::LocalPath { path } => (
                        Some(std::path::PathBuf::from(path)),
                        false,
                        format!("Lokale Datei: {path}"),
                    ),
                    CustomLlmSource::DownloadUrl { url, .. } => (
                        llm_model_manager::default_custom_llm_path(&entry.id).ok(),
                        true,
                        format!("Download-URL: {url}"),
                    ),
                };
                let path_display = path_buf
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let is_downloaded = path_buf.as_ref().map(|p| p.exists()).unwrap_or(false);
                let is_downloading = active_custom_download.as_deref() == Some(entry.id.as_str());
                let progress_basis_points = if is_downloading {
                    active_progress
                } else {
                    None
                };

                CustomLlmStatusDto {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    source_label,
                    path: path_display,
                    is_downloaded,
                    is_downloading,
                    is_loaded: loaded_custom_id.as_deref() == Some(entry.id.as_str()),
                    needs_download,
                    progress_basis_points,
                }
            })
            .collect()
    }

    fn start_custom_llm_download(&mut self, id: &str) -> Result<String, String> {
        self.poll();
        let entry = self
            .settings
            .custom_llm_models
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("Custom language model '{id}' not found."))?
            .clone();
        let url = match &entry.source {
            CustomLlmSource::DownloadUrl { url, .. } => url.clone(),
            CustomLlmSource::LocalPath { .. } => {
                return Err(format!(
                    "'{}' is a locally selected model — no download needed.",
                    entry.name
                ));
            }
        };
        let message = self
            .llm_downloads
            .start_custom_download(&entry.id, &entry.name, &url)?;
        self.last_status = message.clone();
        Ok(message)
    }

    fn delete_custom_llm_download(&mut self, id: &str) -> Result<String, String> {
        self.poll();
        let Some(entry) = self
            .settings
            .custom_llm_models
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            return Err(format!("Custom language model '{id}' not found."));
        };
        match &entry.source {
            CustomLlmSource::DownloadUrl { .. } => {
                let message = self
                    .llm_downloads
                    .delete_custom_file(&entry.id, &entry.name)?;
                self.last_status = message.clone();
                Ok(message)
            }
            CustomLlmSource::LocalPath { .. } => Ok(format!(
                "'{}' is an external file and will not be deleted.",
                entry.name
            )),
        }
    }

    fn delete_llm_model(&mut self, preset: LlmPreset) -> Result<String, String> {
        self.poll();
        let message = self.llm_downloads.delete_preset(preset)?;
        self.last_status = message.clone();
        Ok(message)
    }

    fn list_remote_models(
        &mut self,
        backend: RemoteModelBackend,
    ) -> Result<Vec<RemoteModelDto>, String> {
        self.poll();
        let provider = match backend {
            RemoteModelBackend::Ollama => &self.settings.ollama,
            RemoteModelBackend::LmStudio => &self.settings.lm_studio,
        };
        remote_models::list_remote_models(backend, provider)
    }

    fn run_permission_diagnostics(&mut self) -> DiagnosticsDto {
        self.poll();
        permission_diagnostics::collect(
            &self.settings,
            &self.dictation,
            self.hotkey.as_ref(),
            self.autostart.summary(),
        )
    }

    fn start_dictation(&mut self) -> Result<String, String> {
        self.poll();
        self.reset_cancellation();
        let message = self.dictation.start_recording(&self.settings)?;
        self.last_status = message.clone();
        Ok(message)
    }

    fn stop_dictation(&mut self) -> Result<String, String> {
        self.poll();
        let outcomes = self
            .dictation
            .stop_recording_and_transcribe(&self.settings, "Menueleisten-Aktion", dictation::RecordingCue::Stop)?;
        self.apply_dictation_outcomes(outcomes);
        Ok(self.last_status.clone())
    }

    fn recording_levels(&mut self) -> RecordingLevelsDto {
        RecordingLevelsDto {
            levels: self.dictation.current_levels(),
        }
    }

    fn runtime_status(&mut self) -> RuntimeStatusDto {
        self.poll();

        let blocked_ttl = std::time::Duration::from_secs(6);
        let now = std::time::Instant::now();

        let mut is_blocked = self.dictation.is_blocked(now, blocked_ttl);
        if is_blocked {
            let preset = self.settings.local_model;
            if model_manager::default_model_path(preset)
                .map(|path| path.exists())
                .unwrap_or(false)
            {
                self.dictation.clear_blocked();
                is_blocked = false;
            }
        }

        let (blocked_label, blocked_is_downloading, blocked_progress) = if is_blocked {
            let preset = self.settings.local_model;
            let is_downloading = self.model_downloads.is_downloading_preset(preset);
            let progress = if is_downloading {
                self.model_downloads.progress_basis_points()
            } else {
                None
            };
            (preset.display_label().to_owned(), is_downloading, progress)
        } else {
            (String::new(), false, None)
        };

        RuntimeStatusDto {
            is_recording: self.dictation.is_recording(),
            is_transcribing: self.dictation.is_transcribing(),
            is_post_processing: self.post_processing_rx.is_some(),
            last_status: self.last_status.clone(),
            last_transcript: self.last_transcript.clone(),
            dictation_trigger_count: self.dictation_trigger_count,
            hotkey_registered: self
                .hotkey
                .as_ref()
                .is_some_and(HotKeyController::is_registered),
            hotkey_text: self
                .hotkey
                .as_ref()
                .and_then(HotKeyController::registered_text)
                .unwrap_or_else(|| self.settings.hotkey.clone()),
            startup_summary: self.autostart.summary().to_owned(),
            provider_summary: self.settings.active_provider_summary(),
            active_mode_name: self.settings.active_mode_name().to_owned(),
            onboarding_completed: self.settings.onboarding_completed,
            dictation_blocked_by_missing_model: is_blocked,
            blocked_model_label: blocked_label,
            blocked_model_is_downloading: blocked_is_downloading,
            blocked_model_progress_basis_points: blocked_progress,
            active_input_device_name: self.dictation.active_input_device_name().to_owned(),
            last_mic_switch_message: self.dictation.last_mic_switch_message().to_owned(),
            mic_switch_event_count: self.dictation.mic_switch_event_count(),
            history_revision: self.history_revision,
        }
    }

    fn load_history(&mut self) -> Vec<HistoryEntry> {
        self.history.clone()
    }

    fn delete_history_entry(&mut self, id: &str) -> Result<String, String> {
        if history_store::delete(&mut self.history, id) {
            self.history_revision = self.history_revision.wrapping_add(1);
            history_store::save(&self.history)
                .map_err(|err| format!("History could not be saved: {err}"))?;
            Ok(format!("History entry {id} deleted."))
        } else {
            Err(format!("History entry {id} not found."))
        }
    }

    fn clear_history(&mut self) -> Result<String, String> {
        history_store::clear(&mut self.history);
        self.history_revision = self.history_revision.wrapping_add(1);
        history_store::save(&self.history)
            .map_err(|err| format!("History could not be saved: {err}"))?;
        Ok("History cleared.".to_owned())
    }
}

fn current_unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

mod hotkey {
    use super::*;

    pub enum HotKeyAction {
        Pressed,
        Released,
    }

    pub struct HotKeyController {
        manager: GlobalHotKeyManager,
        registered_hotkey: Option<HotKey>,
        registered_text: Option<String>,
    }

    impl HotKeyController {
        pub fn new() -> Result<Self, String> {
            let manager = GlobalHotKeyManager::new().map_err(|err| err.to_string())?;
            Ok(Self {
                manager,
                registered_hotkey: None,
                registered_text: None,
            })
        }

        pub fn apply_settings(&mut self, settings: &AppSettings) -> Result<(), String> {
            if self.registered_text.as_deref() == Some(settings.hotkey.as_str()) {
                return Ok(());
            }

            if let Some(old) = self.registered_hotkey.take() {
                self.manager
                    .unregister(old)
                    .map_err(|err| format!("Previous hotkey could not be unregistered: {err}"))?;
            }

            let parsed: HotKey = settings
                .hotkey
                .parse()
                .map_err(|err| format!("Hotkey '{}' is invalid: {err}", settings.hotkey))?;

            self.manager.register(parsed).map_err(|err| {
                format!(
                    "Hotkey '{}' could not be registered: {err}",
                    settings.hotkey
                )
            })?;

            self.registered_hotkey = Some(parsed);
            self.registered_text = Some(settings.hotkey.clone());
            Ok(())
        }

        /// Re-registers the hotkey unconditionally — used when the keyboard
        /// hardware changes and the OS may have lost the prior registration.
        pub fn force_reapply(&mut self, settings: &AppSettings) -> Result<(), String> {
            self.registered_text = None;
            self.apply_settings(settings)
        }

        pub fn poll_actions(&mut self) -> Vec<HotKeyAction> {
            let mut actions = Vec::new();

            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if self
                    .registered_hotkey
                    .as_ref()
                    .is_some_and(|registered| registered.id() == event.id)
                {
                    match event.state {
                        HotKeyState::Pressed => actions.push(HotKeyAction::Pressed),
                        HotKeyState::Released => actions.push(HotKeyAction::Released),
                    }
                }
            }

            actions
        }

        pub fn is_registered(&self) -> bool {
            self.registered_hotkey.is_some()
        }

        pub fn registered_text(&self) -> Option<String> {
            self.registered_text.clone()
        }
    }
}

use hotkey::{HotKeyAction, HotKeyController};

#[derive(Serialize)]
struct BridgeResponse<T> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ModelPresetRequest {
    preset: Option<ModelPreset>,
}

#[derive(Deserialize)]
struct LlmPresetRequest {
    preset: LlmPreset,
}

#[derive(Deserialize)]
struct HotkeyValidationRequest {
    hotkey: String,
}

#[derive(Deserialize)]
struct RemoteBackendRequest {
    backend: RemoteModelBackend,
}

#[derive(Deserialize)]
struct CustomLlmIdRequest {
    id: String,
}

fn with_runtime<T>(f: impl FnOnce(&mut BridgeRuntime) -> Result<T, String>) -> Result<T, String> {
    RUNTIME.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        f(&mut runtime)
    })
}

fn with_runtime_value<T>(f: impl FnOnce(&mut BridgeRuntime) -> T) -> T {
    RUNTIME.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        f(&mut runtime)
    })
}

fn response_ok<T: Serialize>(value: T) -> *mut c_char {
    response_from_result::<T>(Ok(value))
}

fn response_from_result<T: Serialize>(result: Result<T, String>) -> *mut c_char {
    let payload = match result {
        Ok(value) => BridgeResponse {
            ok: true,
            value: Some(value),
            error: None,
        },
        Err(error) => BridgeResponse::<T> {
            ok: false,
            value: None,
            error: Some(error),
        },
    };

    let json = serde_json::to_string(&payload).unwrap_or_else(|err| {
        format!(
            "{{\"ok\":false,\"error\":\"Bridge-Serialisierung fehlgeschlagen: {}\"}}",
            err
        )
    });
    CString::new(json)
        .expect("bridge json must not contain interior nul bytes")
        .into_raw()
}

fn parse_json_arg<T: DeserializeOwned>(raw: *const c_char, label: &str) -> Result<T, String> {
    if raw.is_null() {
        return Err(format!("{label} fehlt."));
    }

    let text = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .map_err(|err| format!("{label} ist kein gueltiges UTF-8: {err}"))?;
    serde_json::from_str(text).map_err(|err| format!("{label} ist ungueltig: {err}"))
}

fn parse_optional_preset(raw: *const c_char) -> Result<Option<ModelPreset>, String> {
    if raw.is_null() {
        return Ok(None);
    }

    let request: ModelPresetRequest = parse_json_arg(raw, "ModelPresetRequest")?;
    Ok(request.preset)
}

fn validate_hotkey_text(raw_hotkey: &str) -> Result<String, String> {
    let hotkey = raw_hotkey.trim();
    if hotkey.is_empty() {
        return Err("Hotkey must not be empty.".to_owned());
    }

    let _: HotKey = hotkey.parse().map_err(|err| {
        let normalized = hotkey.to_ascii_lowercase();
        let tokens: Vec<_> = normalized.split('+').map(str::trim).collect();
        let modifier_only = !tokens.is_empty()
            && tokens.iter().all(|token| {
                matches!(
                    *token,
                    "shift"
                        | "ctrl"
                        | "control"
                        | "cmd"
                        | "command"
                        | "super"
                        | "option"
                        | "alt"
                        | "cmdorctrl"
                        | "commandorcontrol"
                        | "commandorctrl"
                        | "cmdorcontrol"
                )
            });

        if modifier_only {
            "Hotkey needs a real key like Space, R, or F8 in addition to modifier keys.".to_owned()
        } else {
            format!("Hotkey '{hotkey}' is invalid: {err}")
        }
    })?;

    Ok(hotkey.to_owned())
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_load_settings() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::load_settings))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_save_settings(settings_json: *const c_char) -> *mut c_char {
    let settings = match parse_json_arg::<AppSettings>(settings_json, "AppSettings") {
        Ok(settings) => settings,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.save_settings(settings)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_list_input_devices() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::list_input_devices))
}

#[derive(Serialize)]
struct MicSwitchEventDto {
    from: String,
    to: String,
    was_recording: bool,
    message: String,
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_notify_device_change() -> *mut c_char {
    let event = with_runtime_value(BridgeRuntime::notify_device_change);
    response_ok(event.map(|event| MicSwitchEventDto {
        from: event.from,
        to: event.to,
        was_recording: event.was_recording,
        message: event.message,
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_get_model_status() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::model_status))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_get_model_status_list() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::model_status_list))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_start_model_download(request_json: *const c_char) -> *mut c_char {
    let preset = match parse_optional_preset(request_json) {
        Ok(value) => value,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.start_model_download(preset)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_delete_model(request_json: *const c_char) -> *mut c_char {
    let preset = match parse_optional_preset(request_json) {
        Ok(value) => value,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.delete_model(preset)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_get_llm_status_list() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::llm_status_list))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_start_llm_download(request_json: *const c_char) -> *mut c_char {
    let preset = match parse_json_arg::<LlmPresetRequest>(request_json, "LlmPresetRequest") {
        Ok(request) => request.preset,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.start_llm_download(preset)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_delete_llm_model(request_json: *const c_char) -> *mut c_char {
    let preset = match parse_json_arg::<LlmPresetRequest>(request_json, "LlmPresetRequest") {
        Ok(request) => request.preset,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.delete_llm_model(preset)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_get_custom_llm_status_list() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::custom_llm_status_list))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_start_custom_llm_download(request_json: *const c_char) -> *mut c_char {
    let id = match parse_json_arg::<CustomLlmIdRequest>(request_json, "CustomLlmIdRequest") {
        Ok(request) => request.id,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| {
        runtime.start_custom_llm_download(&id)
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_delete_custom_llm_model(request_json: *const c_char) -> *mut c_char {
    let id = match parse_json_arg::<CustomLlmIdRequest>(request_json, "CustomLlmIdRequest") {
        Ok(request) => request.id,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| {
        runtime.delete_custom_llm_download(&id)
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_list_remote_models(request_json: *const c_char) -> *mut c_char {
    let backend = match parse_json_arg::<RemoteBackendRequest>(request_json, "RemoteBackendRequest")
    {
        Ok(request) => request.backend,
        Err(err) => return response_from_result::<Vec<RemoteModelDto>>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.list_remote_models(backend)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_run_permission_diagnostics() -> *mut c_char {
    response_ok(with_runtime_value(
        BridgeRuntime::run_permission_diagnostics,
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_start_dictation() -> *mut c_char {
    response_from_result(with_runtime(BridgeRuntime::start_dictation))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_stop_dictation() -> *mut c_char {
    response_from_result(with_runtime(BridgeRuntime::stop_dictation))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_cancel_dictation() -> *mut c_char {
    response_from_result(with_runtime(BridgeRuntime::cancel_dictation))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_get_runtime_status() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::runtime_status))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_get_recording_levels() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::recording_levels))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_validate_hotkey(request_json: *const c_char) -> *mut c_char {
    let request =
        match parse_json_arg::<HotkeyValidationRequest>(request_json, "HotkeyValidationRequest") {
            Ok(request) => request,
            Err(err) => return response_from_result::<String>(Err(err)),
        };

    response_from_result(validate_hotkey_text(&request.hotkey))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_reregister_hotkey() -> *mut c_char {
    response_from_result(with_runtime(BridgeRuntime::reregister_hotkey))
}

#[derive(Deserialize)]
struct HistoryIdRequest {
    id: String,
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_load_history() -> *mut c_char {
    response_ok(with_runtime_value(BridgeRuntime::load_history))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_delete_history_entry(request_json: *const c_char) -> *mut c_char {
    let id = match parse_json_arg::<HistoryIdRequest>(request_json, "HistoryIdRequest") {
        Ok(request) => request.id,
        Err(err) => return response_from_result::<String>(Err(err)),
    };

    response_from_result(with_runtime(|runtime| runtime.delete_history_entry(&id)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ow_clear_history() -> *mut c_char {
    response_from_result(with_runtime(BridgeRuntime::clear_history))
}

/// Frees a C string returned by any `ow_*` function.
///
/// # Safety
///
/// `raw` must either be null or a pointer previously returned by an `ow_*`
/// function in this crate (i.e. produced via `CString::into_raw`). The pointer
/// must not have been freed before and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ow_string_free(raw: *mut c_char) {
    if raw.is_null() {
        return;
    }

    let _ = unsafe { CString::from_raw(raw) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_whisper_core::DiagnosticStatus;

    #[test]
    fn bridge_response_serializes_success_shape() {
        let payload = BridgeResponse {
            ok: true,
            value: Some("ok"),
            error: None,
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"value\":\"ok\""));
    }

    #[test]
    fn model_preset_request_parses() {
        let parsed: ModelPresetRequest = serde_json::from_str("{\"preset\":\"quality\"}").unwrap();
        assert_eq!(parsed.preset, Some(ModelPreset::Quality));
    }

    #[test]
    fn diagnostics_status_is_stable_for_swift() {
        assert_eq!(DiagnosticStatus::Warning.label(), "Warning");
    }

    #[test]
    fn validate_hotkey_rejects_modifier_only_combo() {
        let error = validate_hotkey_text("Ctrl+Shift").unwrap_err();
        assert!(error.contains("real key") || error.contains("echte Taste"));
    }

    #[test]
    fn validate_hotkey_accepts_trimmed_combo() {
        let validated = validate_hotkey_text("  Cmd+Shift+Space  ").unwrap();
        assert_eq!(validated, "Cmd+Shift+Space");
    }

    #[test]
    fn validate_hotkey_accepts_single_key() {
        let validated = validate_hotkey_text("F8").unwrap();
        assert_eq!(validated, "F8");
    }
}
