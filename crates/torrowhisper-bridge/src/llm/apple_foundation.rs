//! Apple's system-managed on-device language model.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::LlmProvider;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::build_system_prompt;

pub(crate) struct AppleFoundationProvider;

pub(crate) fn is_available() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        foundation_models::SystemLanguageModel::is_available()
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

pub(crate) fn availability_detail() -> String {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        if is_available() {
            "Provided by macOS · ready".to_owned()
        } else {
            format!(
                "Provided by macOS · unavailable ({:?})",
                foundation_models::SystemLanguageModel::availability()
            )
        }
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        "Requires Apple Silicon, macOS 26, and Apple Intelligence".to_owned()
    }
}

impl LlmProvider for AppleFoundationProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Post-processing was cancelled.".to_owned());
        }

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if !is_available() {
                return Err(format!(
                    "Apple Foundation Models is unavailable: {:?}",
                    foundation_models::SystemLanguageModel::availability()
                ));
            }
            let instructions = build_system_prompt(role_prompt);
            let session = foundation_models::LanguageModelSession::try_new(Some(&instructions))
                .ok_or_else(|| {
                    "Apple Foundation Models session could not be created.".to_owned()
                })?;
            let prompt = format!(
                "Edit the dictated text below. Preserve its meaning, facts, numbers, names, URLs, and code exactly. Return only the final text.\n\n<dictated_text>\n{user_text}\n</dictated_text>"
            );
            let output = session
                .respond(&prompt)
                .map_err(|err| format!("Apple Foundation Models failed: {err}"))?;
            if cancelled.load(Ordering::Relaxed) {
                return Err("Post-processing was cancelled.".to_owned());
            }
            Ok(output)
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = (role_prompt, user_text);
            Err(
                "Apple Foundation Models requires Apple Silicon, macOS 26, and Apple Intelligence."
                    .to_owned(),
            )
        }
    }

    fn chat(
        &self,
        system_prompt: &str,
        user_text: &str,
        _session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Generation was cancelled.".to_owned());
        }

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if !is_available() {
                return Err("Apple Foundation Models is unavailable.".to_owned());
            }
            let session = foundation_models::LanguageModelSession::try_new(Some(system_prompt))
                .ok_or_else(|| {
                    "Apple Foundation Models session could not be created.".to_owned()
                })?;
            session
                .respond(user_text)
                .map_err(|err| format!("Apple Foundation Models failed: {err}"))
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = (system_prompt, user_text);
            Err(
                "Apple Foundation Models requires Apple Silicon, macOS 26, and Apple Intelligence."
                    .to_owned(),
            )
        }
    }
}
