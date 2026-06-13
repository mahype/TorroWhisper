use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use open_whisper_core::{AppSettings, LlmModelRef};

use crate::llm;

/// Runs the active mode's post-processing on a raw transcript. Resolves the
/// configured backend into a [`LlmModelRef`] and dispatches through the shared
/// provider layer ([`crate::llm::provider_for`]).
pub fn process_text(
    settings: &AppSettings,
    raw_transcript: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    if !settings.active_mode_post_processing_enabled() {
        return Ok(raw_transcript.to_owned());
    }

    if cancelled.load(Ordering::Relaxed) {
        return Err("Post-processing cancelled.".to_owned());
    }

    let mode = settings.active_mode();
    // A registry-selected model (incl. cloud) takes precedence; otherwise fall
    // back to the legacy PostProcessingChoice resolution.
    let model_ref = settings
        .active_post_processing_model
        .clone()
        .unwrap_or_else(|| LlmModelRef::from(settings.effective_post_processing_choice(mode)));
    let backend_label = model_ref.backend_kind().label();

    let started = Instant::now();
    log::info!(
        target: "post_processing",
        "post-processing '{}' via {backend_label} ({} chars in)",
        mode.name,
        raw_transcript.chars().count()
    );

    let provider = llm::provider_for(&model_ref, settings)?;
    let text = provider.generate(&mode.prompt, raw_transcript, cancelled)?;

    if cancelled.load(Ordering::Relaxed) {
        return Err("Post-processing cancelled.".to_owned());
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Post-processing returned no text.".to_owned());
    }

    log::info!(
        target: "post_processing",
        "post-processing via {backend_label} done in {:.1}s ({} chars out)",
        started.elapsed().as_secs_f32(),
        trimmed.chars().count()
    );

    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_whisper_core::{
        AppSettings, LlmPreset, PostProcessingBackend, PostProcessingChoice, ProcessingMode,
    };

    #[test]
    fn disabled_mode_returns_original_text() {
        let settings = AppSettings::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        let result = process_text(&settings, "roher text", &cancelled).unwrap();
        assert_eq!(result, "roher text");
    }

    #[test]
    fn active_backend_reflects_global_setting() {
        let mut settings = AppSettings {
            active_post_processing_backend: PostProcessingBackend::Ollama,
            post_processing_enabled: true,
            ..AppSettings::default()
        };
        settings.modes.push(ProcessingMode {
            id: "dev".to_owned(),
            name: "Entwickler".to_owned(),
            prompt: "Nutze Entwickler-Sprache.".to_owned(),
            post_processing_choice: None,
            dictionary_enabled: true,
        });
        settings.active_mode_id = "dev".to_owned();

        assert!(settings.active_mode_post_processing_enabled());
        assert_eq!(
            settings.active_post_processing_backend,
            PostProcessingBackend::Ollama
        );
    }

    #[test]
    fn profile_override_beats_global_choice() {
        let mut settings = AppSettings {
            active_post_processing_backend: PostProcessingBackend::Local,
            local_llm: LlmPreset::Small,
            post_processing_enabled: true,
            ..AppSettings::default()
        };
        settings.modes.push(ProcessingMode {
            id: "email".to_owned(),
            name: "E-Mail".to_owned(),
            prompt: "Formatiere als E-Mail.".to_owned(),
            post_processing_choice: Some(PostProcessingChoice::Ollama {
                model_name: "llama3.1".to_owned(),
            }),
            dictionary_enabled: true,
        });
        settings.active_mode_id = "email".to_owned();

        let mode = settings.active_mode();
        assert_eq!(
            settings.effective_post_processing_choice(mode),
            PostProcessingChoice::Ollama {
                model_name: "llama3.1".to_owned(),
            }
        );
    }

    #[test]
    fn missing_profile_override_falls_back_to_global_choice() {
        let settings = AppSettings {
            active_post_processing_backend: PostProcessingBackend::Local,
            local_llm: LlmPreset::Medium,
            ..AppSettings::default()
        };

        let mode = settings.active_mode();
        assert!(mode.post_processing_choice.is_none());
        assert_eq!(
            settings.effective_post_processing_choice(mode),
            PostProcessingChoice::LocalPreset {
                preset: LlmPreset::Medium,
            }
        );
    }

    #[test]
    fn legacy_processing_mode_without_choice_deserializes() {
        let json = r#"{"id":"foo","name":"Foo","prompt":"bar"}"#;
        let mode: ProcessingMode = serde_json::from_str(json).unwrap();
        assert!(mode.post_processing_choice.is_none());
    }

    #[test]
    fn post_processing_choice_maps_to_model_ref() {
        assert_eq!(
            LlmModelRef::from(PostProcessingChoice::LmStudio {
                model_name: "qwen".to_owned(),
            }),
            LlmModelRef::LmStudio {
                model_name: "qwen".to_owned(),
            }
        );
    }
}
