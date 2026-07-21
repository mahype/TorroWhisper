//! NVIDIA Parakeet TDT v3 through FluidAudio/Core ML.
//!
//! The model is owned and cached by FluidAudio. TorroWhisper only tracks the
//! coarse preparation state because FluidAudio intentionally exposes model
//! preparation as one operation (download, Core ML compile, load), without
//! byte-level progress callbacks.

use std::sync::{Arc, Mutex};

use torrowhisper_core::ParakeetModelStatusDto;

pub const DISPLAY_LABEL: &str = "NVIDIA Parakeet TDT v3";
pub const EXPECTED_SIZE_BYTES: u64 = 600_000_000;

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
                let next = match engine.init_asr() {
                    Ok(()) => {
                        log::info!(target: "models", "Parakeet/Core ML is ready");
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
