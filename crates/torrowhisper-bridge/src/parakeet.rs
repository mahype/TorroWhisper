//! Parakeet Ultra through FluidAudio/Core ML.
//!
//! Parakeet Ultra is moondream's post-training of NVIDIA Parakeet TDT v3:
//! the same 25 languages, architecture and speed, with lower word error
//! rates (FLEURS German 4.13 % → 3.61 %, English 4.25 % → 3.55 %).
//!
//! The model is owned and cached by FluidAudio. TorroWhisper only tracks the
//! coarse preparation state because FluidAudio intentionally exposes model
//! preparation as one operation (download, Core ML compile, load), without
//! byte-level progress callbacks.

use std::sync::{Arc, Mutex};

use torrowhisper_core::ParakeetModelStatusDto;

pub const DISPLAY_LABEL: &str = "Parakeet Ultra";
pub const EXPECTED_SIZE_BYTES: u64 = 630_000_000;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Intel builds retain only the explicit unsupported state.
enum PreparationState {
    Idle,
    Preparing,
    Ready,
    Failed(String),
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
type Engine = fluidaudio_rs::FluidAudio;

/// Process-lifetime Parakeet engine. Cloning it only clones the shared engine
/// and state, so transcription workers and the UI status endpoint agree.
#[derive(Clone)]
pub struct ParakeetRuntime {
    state: Arc<Mutex<PreparationState>>,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    engine: Option<Arc<Engine>>,
}

impl ParakeetRuntime {
    pub fn new() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let state = Arc::new(Mutex::new(PreparationState::Idle));
            let engine = match Engine::new() {
                Ok(engine) => Some(Arc::new(engine)),
                Err(err) => {
                    *state.lock().unwrap_or_else(|p| p.into_inner()) =
                        PreparationState::Failed(err.to_string());
                    None
                }
            };
            Self { state, engine }
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            Self {
                state: Arc::new(Mutex::new(PreparationState::Failed(
                    "Parakeet requires an Apple-Silicon Mac.".to_owned(),
                ))),
            }
        }
    }

    /// Starts the first-run download/compile/load operation in the background.
    /// Safe to call repeatedly; a failed preparation can be retried from
    /// Settings, while ready/in-flight states are no-ops.
    pub fn prepare(&self) {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let Some(engine) = self.engine.clone() else {
                return;
            };
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if matches!(
                *state,
                PreparationState::Preparing | PreparationState::Ready
            ) {
                return;
            }
            *state = PreparationState::Preparing;
            drop(state);

            let state = self.state.clone();
            std::thread::spawn(move || {
                log::info!(
                    target: "models",
                    "preparing Parakeet/Core ML (download on first run)"
                );
                let next = match engine.init_asr_with_version(fluidaudio_rs::AsrModelVersion::Ultra)
                {
                    Ok(()) => {
                        log::info!(target: "models", "Parakeet Ultra/Core ML is ready");
                        remove_superseded_v3_model();
                        PreparationState::Ready
                    }
                    Err(err) => {
                        log::error!(target: "models", "Parakeet preparation failed: {err}");
                        PreparationState::Failed(err.to_string())
                    }
                };
                *state.lock().unwrap_or_else(|p| p.into_inner()) = next;
            });
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(
            *self.state.lock().unwrap_or_else(|p| p.into_inner()),
            PreparationState::Ready
        )
    }

    pub fn status(&self) -> ParakeetModelStatusDto {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let (summary, is_ready, is_preparing, error) = match &*state {
            PreparationState::Idle => (
                "Parakeet will be downloaded and prepared automatically.".to_owned(),
                false,
                false,
                None,
            ),
            PreparationState::Preparing => (
                "Downloading and preparing Parakeet for Apple Neural Engine…".to_owned(),
                false,
                true,
                None,
            ),
            PreparationState::Ready => ("Parakeet is ready.".to_owned(), true, false, None),
            PreparationState::Failed(message) => (
                "Parakeet could not be prepared. Retry from Settings.".to_owned(),
                false,
                false,
                Some(message.clone()),
            ),
        };

        ParakeetModelStatusDto {
            display_label: DISPLAY_LABEL.to_owned(),
            summary,
            is_supported: cfg!(all(target_os = "macos", target_arch = "aarch64")),
            is_ready,
            is_preparing,
            error,
            expected_size_bytes: EXPECTED_SIZE_BYTES,
        }
    }

    pub fn transcribe(&self, samples_16khz: &[f32]) -> Result<String, String> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if !self.is_ready() {
                return Err(
                    "Parakeet is not ready yet. Check the model status in Settings.".to_owned(),
                );
            }
            let engine = self
                .engine
                .as_ref()
                .ok_or_else(|| "FluidAudio bridge is unavailable.".to_owned())?;
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

/// Deletes the Parakeet TDT v3 model that TorroWhisper up to 0.10 used.
///
/// Parakeet Ultra replaces it, and nothing else in the app loads v3, so the
/// ~460 MB would otherwise stay on disk for good. Called only after Ultra is
/// ready, so a failed Ultra download never leaves the user without a model
/// they had. FluidAudio caches models per repo under this fixed directory.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn remove_superseded_v3_model() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = std::path::Path::new(&home)
        .join("Library/Application Support/FluidAudio/Models/parakeet-tdt-0.6b-v3");
    if !dir.is_dir() {
        return;
    }
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => log::info!(target: "models", "removed superseded Parakeet v3 model"),
        Err(err) => {
            log::warn!(target: "models", "could not remove superseded Parakeet v3 model: {err}")
        }
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

impl Default for ParakeetRuntime {
    fn default() -> Self {
        Self::new()
    }
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
