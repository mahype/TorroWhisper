//! Parakeet transcription through FluidAudio/Core ML.
//!
//! Parakeet Ultra is the recommended model: moondream's post-training of
//! NVIDIA Parakeet TDT v3 with the same 25 languages, architecture and speed
//! but lower word error rates (FLEURS German 4.13 % → 3.61 %, English 4.25 %
//! → 3.55 %). Redux and v2 are experimental, v3 is deprecated but stays usable
//! so installations from before 0.11 keep working and are only offered the
//! upgrade (#67).
//!
//! The models are owned and cached by FluidAudio. TorroWhisper only tracks the
//! coarse preparation state because FluidAudio intentionally exposes model
//! preparation as one operation (download, Core ML compile, load), without
//! byte-level progress callbacks.
//!
//! Switching models never interrupts dictation: the new model is prepared in
//! a second FluidAudio instance while the active one keeps transcribing, and
//! takes over only once it is ready.

use std::sync::{Arc, Mutex};

use torrowhisper_core::{ParakeetModel, ParakeetModelInfoDto, ParakeetModelStatusDto};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
type Engine = fluidaudio_rs::FluidAudio;

#[derive(Default)]
struct State {
    /// The loaded model transcription uses.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    active: Option<(ParakeetModel, Arc<Engine>)>,
    /// A model being downloaded and prepared in the background.
    preparing: Option<ParakeetModel>,
    /// The last preparation failure, cleared by the next attempt.
    error: Option<String>,
}

impl State {
    fn active_model(&self) -> Option<ParakeetModel> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            self.active.as_ref().map(|(model, _)| *model)
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            None
        }
    }
}

/// Process-lifetime Parakeet runtime. Cloning it only clones the shared state,
/// so transcription workers and the UI status endpoint agree.
#[derive(Clone, Default)]
pub struct ParakeetRuntime {
    state: Arc<Mutex<State>>,
}

impl ParakeetRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Makes `target` the active model, downloading it on first use. Runs in
    /// the background; while it runs, a previously active model keeps
    /// transcribing. Safe to call repeatedly: the active model or one already
    /// being prepared is a no-op, and a failure can be retried.
    pub fn prepare(&self, target: ParakeetModel) {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let mut state = self.lock();
            if state.active_model() == Some(target) || state.preparing == Some(target) {
                return;
            }
            state.preparing = Some(target);
            state.error = None;
            drop(state);

            let shared = self.state.clone();
            std::thread::spawn(move || {
                log::info!(
                    target: "models",
                    "preparing {} (download on first run)",
                    target.display_label()
                );
                let result = Engine::new()
                    .map_err(|err| err.to_string())
                    .and_then(|engine| {
                        engine
                            .init_asr_with_version(fluid_version(target))
                            .map(|()| engine)
                            .map_err(|err| err.to_string())
                    });
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                if state.preparing != Some(target) {
                    // A newer request replaced this one; its result wins.
                    return;
                }
                state.preparing = None;
                match result {
                    Ok(engine) => {
                        log::info!(target: "models", "{} is ready", target.display_label());
                        let previous = state.active.replace((target, Arc::new(engine)));
                        drop(state);
                        if previous.is_some_and(|(model, _)| model == ParakeetModel::V3)
                            && target != ParakeetModel::V3
                        {
                            remove_cached_model(ParakeetModel::V3);
                        }
                    }
                    Err(err) => {
                        log::error!(
                            target: "models",
                            "{} preparation failed: {err}",
                            target.display_label()
                        );
                        state.error = Some(err);
                    }
                }
            });
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = target;
        }
    }

    pub fn is_ready(&self) -> bool {
        self.lock().active_model().is_some()
    }

    /// `selected` is the model the settings resolve to; see
    /// [`ParakeetModel::effective`] and [`effective_model`].
    pub fn status(&self, selected: ParakeetModel) -> ParakeetModelStatusDto {
        let supported = cfg!(all(target_os = "macos", target_arch = "aarch64"));
        let installed = installed_models();
        let state = self.lock();
        let active = state.active_model();
        let preparing = state.preparing;
        let shown = preparing.or(active).unwrap_or(selected);
        let (summary, error) = if !supported {
            ("Parakeet requires an Apple-Silicon Mac.".to_owned(), None)
        } else if let Some(model) = preparing {
            let summary = if active.is_some() {
                format!(
                    "Downloading and preparing {} … dictation keeps using the current model.",
                    model.display_label()
                )
            } else {
                "Downloading and preparing Parakeet for Apple Neural Engine…".to_owned()
            };
            (summary, None)
        } else if let Some(message) = &state.error {
            (
                "Parakeet could not be prepared. Retry from Settings.".to_owned(),
                Some(message.clone()),
            )
        } else if active.is_some() {
            ("Parakeet is ready.".to_owned(), None)
        } else {
            (
                "Parakeet will be downloaded and prepared automatically.".to_owned(),
                None,
            )
        };

        ParakeetModelStatusDto {
            display_label: shown.display_label().to_owned(),
            active_model: active,
            preparing_model: preparing,
            selected_model: selected,
            installed_models: installed.clone(),
            models: ParakeetModel::ALL
                .into_iter()
                .map(|model| ParakeetModelInfoDto {
                    model,
                    display_label: model.display_label().to_owned(),
                    tier: model.tier(),
                    successor: model.successor(),
                    approx_size_bytes: model.approx_size_bytes(),
                    min_macos_major: model.min_macos_major(),
                    is_installed: installed.contains(&model),
                })
                .collect(),
            summary,
            is_supported: supported,
            is_ready: active.is_some(),
            is_preparing: preparing.is_some(),
            error,
            expected_size_bytes: shown.approx_size_bytes(),
        }
    }

    pub fn transcribe(&self, samples_16khz: &[f32]) -> Result<String, String> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let engine = self
                .lock()
                .active
                .as_ref()
                .map(|(_, engine)| engine.clone())
                .ok_or_else(|| {
                    "Parakeet is not ready yet. Check the model status in Settings.".to_owned()
                })?;
            let result = engine
                .transcribe_samples(samples_16khz)
                .map_err(|err| format!("Parakeet transcription failed: {err}"))?;
            let text = normalize_transcript(&result.text);
            if text.is_empty() {
                return Err("Parakeet recognized no text.".to_owned());
            }
            Ok(text)
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = samples_16khz;
            Err("Parakeet requires an Apple-Silicon Mac.".to_owned())
        }
    }
}

/// The Parakeet model the settings resolve to on this Mac.
pub fn effective_model(settings: &torrowhisper_core::AppSettings) -> ParakeetModel {
    ParakeetModel::effective(
        settings.parakeet_model,
        installed_models().contains(&ParakeetModel::V3),
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn fluid_version(model: ParakeetModel) -> fluidaudio_rs::AsrModelVersion {
    match model {
        ParakeetModel::Ultra => fluidaudio_rs::AsrModelVersion::Ultra,
        ParakeetModel::Redux => fluidaudio_rs::AsrModelVersion::Redux,
        ParakeetModel::V2 => fluidaudio_rs::AsrModelVersion::V2,
        ParakeetModel::V3 => fluidaudio_rs::AsrModelVersion::V3,
    }
}

/// FluidAudio caches each model in its own directory below this one.
fn models_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::Path::new(&home).join("Library/Application Support/FluidAudio/Models"))
}

fn installed_models() -> Vec<ParakeetModel> {
    let Some(dir) = models_dir() else {
        return Vec::new();
    };
    ParakeetModel::ALL
        .into_iter()
        .filter(|model| dir.join(model.cache_dir_name()).is_dir())
        .collect()
}

/// Deletes a model FluidAudio cached. Used once a user has moved from the
/// deprecated v3 to its successor, so ~460 MB do not stay on disk for good;
/// called only after the successor is ready.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn remove_cached_model(model: ParakeetModel) {
    let Some(dir) = models_dir().map(|dir| dir.join(model.cache_dir_name())) else {
        return;
    };
    if !dir.is_dir() {
        return;
    }
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => log::info!(target: "models", "removed {}", model.display_label()),
        Err(err) => log::warn!(
            target: "models",
            "could not remove {}: {err}",
            model.display_label()
        ),
    }
}

/// Removes a punctuation-restoration artifact observed in Parakeet output:
/// some otherwise valid transcripts start with a standalone period and space.
/// Keep this deliberately narrow so ellipses, decimals, dotfiles, and quoted
/// sentence openings remain untouched.
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn normalize_transcript(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(remainder) = trimmed.strip_prefix(". ")
        && remainder.chars().next().is_some_and(char::is_alphanumeric)
    {
        return remainder.to_owned();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::normalize_transcript;

    #[test]
    fn removes_spurious_standalone_leading_period() {
        assert_eq!(
            normalize_transcript(". Das ist der eigentliche Text."),
            "Das ist der eigentliche Text."
        );
        assert_eq!(
            normalize_transcript("  . 42 ist die Antwort.  "),
            "42 ist die Antwort."
        );
    }

    #[test]
    fn preserves_meaningful_leading_punctuation() {
        for text in [
            ".gitignore bleibt unverändert.",
            ".5 ist kleiner als eins.",
            "... und dann ging es weiter.",
            ". \"Ein Zitat beginnt.\"",
        ] {
            assert_eq!(normalize_transcript(text), text);
        }
    }

    #[test]
    fn still_trims_outer_whitespace() {
        assert_eq!(
            normalize_transcript("  Normaler Text. \n"),
            "Normaler Text."
        );
    }
}
