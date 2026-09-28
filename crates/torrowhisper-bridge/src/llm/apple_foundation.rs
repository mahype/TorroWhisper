//! Apple's system-managed on-device language model.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::LlmProvider;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::build_system_prompt;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::context_budget::{self, Chunk};

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
            let chunks = plan_chunks(&instructions, user_text);
            if chunks.len() > 1 {
                log::info!(
                    "Apple Foundation Models: dictation exceeds the context window, post-processing it in {} parts",
                    chunks.len()
                );
            }
            let mut output = String::new();
            for chunk in chunks {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Post-processing was cancelled.".to_owned());
                }
                if chunk.fits {
                    output.push_str(&edit_chunk(&instructions, &chunk.text)?);
                } else {
                    log::warn!(
                        "Apple Foundation Models: a single sentence of {} characters exceeds the context window and is kept unedited",
                        chunk.text.chars().count()
                    );
                    output.push_str(&chunk.text);
                }
                output.push_str(&chunk.separator);
            }
            if cancelled.load(Ordering::Relaxed) {
                return Err("Post-processing was cancelled.".to_owned());
            }
            Ok(output.trim_end().to_owned())
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
            let session = foundation_models::LanguageModelSession::with_instructions(system_prompt)
                .map_err(|err| {
                    format!("Apple Foundation Models session could not be created: {err}")
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

/// Wraps one piece of the dictation in the editing prompt.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn edit_prompt(text: &str) -> String {
    format!(
        "Edit the dictated text below. Preserve its meaning, facts, numbers, names, URLs, and code exactly. Return only the final text.\n\n<dictated_text>\n{text}\n</dictated_text>"
    )
}

/// Post-processes one chunk in a fresh session, so earlier chunks never
/// occupy the context window of later ones.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn edit_chunk(instructions: &str, text: &str) -> Result<String, String> {
    let session = foundation_models::LanguageModelSession::with_instructions(instructions)
        .map_err(|err| format!("Apple Foundation Models session could not be created: {err}"))?;
    session
        .respond(&edit_prompt(text))
        .map(|output| output.trim().to_owned())
        .map_err(|err| format!("Apple Foundation Models failed: {err}"))
}

/// Splits the dictation so every request fits the model's context window.
///
/// Budget per chunk: context size minus instructions and the prompt frame,
/// halved to leave the same room again for the answer. Falls back to a single
/// request when macOS reports no context size (before 26.4) or cannot count
/// tokens — the pre-0.11 behavior.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn plan_chunks(instructions: &str, user_text: &str) -> Vec<Chunk> {
    let whole = || {
        vec![Chunk {
            text: user_text.trim_end().to_owned(),
            separator: String::new(),
            fits: true,
        }]
    };
    let context = foundation_models::SystemLanguageModel::context_size();
    if context == 0 {
        return whole();
    }
    let count = |text: &str| -> Option<usize> {
        pollster::block_on(foundation_models::SystemLanguageModel::token_count(text)).ok()
    };
    let (Some(instruction_tokens), Some(frame_tokens)) =
        (count(instructions), count(&edit_prompt("")))
    else {
        return whole();
    };
    let budget = context.saturating_sub(instruction_tokens + frame_tokens) / 2;
    if budget == 0 {
        return whole();
    }
    context_budget::split_to_fit(user_text, budget, count).unwrap_or_else(whole)
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;

    /// Needs Apple Intelligence on this Mac, so it only runs on request:
    /// `cargo test -p torrowhisper-bridge apple_foundation -- --ignored --nocapture`
    #[test]
    #[ignore = "requires Apple Intelligence on the host"]
    fn long_dictation_is_split_and_short_one_is_edited() {
        if !is_available() {
            eprintln!("Apple Foundation Models unavailable, skipping");
            return;
        }
        let context = foundation_models::SystemLanguageModel::context_size();
        eprintln!("context size: {context}");
        let instructions = build_system_prompt("Fix punctuation and capitalization.");
        let sentence = "das ist ein ziemlich langer diktierter satz ohne satzzeichen der immer weiter geht und kein ende findet. ";
        let long = sentence.repeat(600);
        let chunks = plan_chunks(&instructions, &long);
        eprintln!(
            "long dictation: {} characters, {} chunks",
            long.len(),
            chunks.len()
        );
        if context > 0 {
            assert!(
                chunks.len() > 1,
                "a dictation far beyond the context must be split"
            );
        }

        let edited = AppleFoundationProvider
            .generate(
                "Fix punctuation and capitalization.",
                "hallo zusammen wie geht es euch heute",
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("short dictation is edited");
        eprintln!("edited: {edited}");
        assert!(!edited.is_empty());
    }

    /// End to end: a dictation just over the context window is edited in
    /// parts and comes back complete. Takes about a minute.
    #[test]
    #[ignore = "requires Apple Intelligence on the host"]
    fn dictation_over_the_context_window_is_edited_completely() {
        if !is_available() {
            eprintln!("Apple Foundation Models unavailable, skipping");
            return;
        }
        // Distinct, numbered sentences: identical ones get merged by the
        // model, which would say nothing about the split.
        let dictation: String = (1..=160)
            .map(|n| format!("punkt {n} auf der liste ist erledigt und kann abgehakt werden. "))
            .collect();
        let instructions = build_system_prompt("Fix punctuation and capitalization.");
        let parts = plan_chunks(&instructions, &dictation).len();
        let edited = AppleFoundationProvider
            .generate(
                "Fix punctuation and capitalization.",
                &dictation,
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("long dictation is edited instead of failing");
        let kept = (1..=160)
            .filter(|n| edited.contains(&format!(" {n} ")))
            .count();
        eprintln!(
            "parts: {parts}, numbered sentences kept: {kept}/160, chars in/out: {}/{}",
            dictation.len(),
            edited.len()
        );
        assert!(parts > 1, "the dictation must exceed one context window");
        assert!(kept >= 150, "the edited text must stay complete");
    }
}
