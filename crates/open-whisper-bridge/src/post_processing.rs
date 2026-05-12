use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use open_whisper_core::{AppSettings, CustomLlmSource, PostProcessingChoice};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::{llm_model_manager, local_llm};

const USER_AGENT: &str = "open-whisper-bridge/0.1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

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
    let choice = settings.effective_post_processing_choice(mode);

    let text = match choice {
        PostProcessingChoice::LocalPreset { preset } => local_llm::generate_with_shared_runtime(
            preset,
            &mode.prompt,
            raw_transcript,
            cancelled,
        )?,
        PostProcessingChoice::LocalCustom { id } => {
            let custom = settings
                .custom_llm_models
                .iter()
                .find(|entry| entry.id == id)
                .ok_or_else(|| format!("Custom language model '{id}' is not known in settings."))?;
            match &custom.source {
                CustomLlmSource::LocalPath { path } => local_llm::generate_with_custom_path(
                    &custom.id,
                    &custom.name,
                    Path::new(path),
                    &mode.prompt,
                    raw_transcript,
                    cancelled,
                )?,
                CustomLlmSource::DownloadUrl { .. } => {
                    let path = llm_model_manager::default_custom_llm_path(&custom.id)?;
                    if !path.exists() {
                        return Err(format!(
                            "Custom language model '{}' has not been downloaded yet.",
                            custom.name
                        ));
                    }
                    local_llm::generate_with_custom_path(
                        &custom.id,
                        &custom.name,
                        &path,
                        &mode.prompt,
                        raw_transcript,
                        cancelled,
                    )?
                }
            }
        }
        PostProcessingChoice::Ollama { model_name } => {
            let client = build_http_client()?;
            let system_prompt = build_system_prompt(&mode.prompt);
            request_ollama(
                &client,
                &settings.ollama.endpoint,
                &model_name,
                &system_prompt,
                raw_transcript,
            )?
        }
        PostProcessingChoice::LmStudio { model_name } => {
            let client = build_http_client()?;
            let system_prompt = build_system_prompt(&mode.prompt);
            request_lm_studio(
                &client,
                &settings.lm_studio.endpoint,
                &model_name,
                &system_prompt,
                raw_transcript,
            )?
        }
    };

    if cancelled.load(Ordering::Relaxed) {
        return Err("Post-processing cancelled.".to_owned());
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Post-processing returned no text.".to_owned());
    }

    Ok(trimmed.to_owned())
}

fn build_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| format!("HTTP client for post-processing could not be created: {err}"))
}

fn build_system_prompt(mode_prompt: &str) -> String {
    let base = "You edit dictated text according to a configured role. Return only the final text — no explanations or meta comments.";
    let trimmed = mode_prompt.trim();
    if trimmed.is_empty() {
        base.to_owned()
    } else {
        format!("{base}\n\nRole context:\n{trimmed}")
    }
}

fn request_ollama(
    client: &Client,
    endpoint: &str,
    model_name: &str,
    system_prompt: &str,
    raw_transcript: &str,
) -> Result<String, String> {
    let url = join_base_url(endpoint, "/api/chat");
    let response = client
        .post(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(&json!({
            "model": model_name,
            "stream": false,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt,
                },
                {
                    "role": "user",
                    "content": raw_transcript,
                }
            ]
        }))
        .send()
        .map_err(|err| format!("Ollama post-processing could not be started: {err}"))?;

    let status = response.status();
    let value: Value = response
        .json()
        .map_err(|err| format!("Ollama response could not be read: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Ollama returned HTTP {} during post-processing.",
            status
        ));
    }

    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("response")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| "Ollama response contained no processed text.".to_owned())
}

fn request_lm_studio(
    client: &Client,
    endpoint: &str,
    model_name: &str,
    system_prompt: &str,
    raw_transcript: &str,
) -> Result<String, String> {
    let url = join_base_url(endpoint, "/v1/chat/completions");
    let response = client
        .post(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(&json!({
            "model": model_name,
            "temperature": 0.1,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt,
                },
                {
                    "role": "user",
                    "content": raw_transcript,
                }
            ]
        }))
        .send()
        .map_err(|err| format!("LM Studio post-processing could not be started: {err}"))?;

    let status = response.status();
    let value: Value = response
        .json()
        .map_err(|err| format!("LM Studio response could not be read: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "LM Studio returned HTTP {} during post-processing.",
            status
        ));
    }

    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "LM Studio response contained no processed text.".to_owned())
}

fn join_base_url(endpoint: &str, suffix: &str) -> String {
    let base = endpoint.trim().trim_end_matches('/');
    if suffix.starts_with("/v1/") && base.ends_with("/v1") {
        return format!("{base}{}", &suffix[3..]);
    }
    if suffix.starts_with("/api/") && base.ends_with("/api") {
        return format!("{base}{}", &suffix[4..]);
    }
    format!("{base}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_whisper_core::{
        AppSettings, LlmPreset, PostProcessingBackend, PostProcessingChoice, ProcessingMode,
    };

    #[test]
    fn empty_prompt_gets_safe_default_instruction() {
        let prompt = build_system_prompt("");
        assert!(prompt.contains("Return only the final text"));
    }

    #[test]
    fn disabled_mode_returns_original_text() {
        let settings = AppSettings::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        let result = process_text(&settings, "roher text", &cancelled).unwrap();
        assert_eq!(result, "roher text");
    }

    #[test]
    fn join_base_url_trims_trailing_slash() {
        assert_eq!(
            join_base_url("http://127.0.0.1:11434/", "/api/chat"),
            "http://127.0.0.1:11434/api/chat"
        );
    }

    #[test]
    fn join_base_url_avoids_duplicate_version_prefix() {
        assert_eq!(
            join_base_url("http://127.0.0.1:1234/v1", "/v1/chat/completions"),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
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
}
