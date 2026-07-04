//! Backend-independent language-model provider layer.
//!
//! [`provider_for`] is the single place that turns a backend-agnostic
//! [`LlmModelRef`] into a runnable [`LlmProvider`]. Post-processing (and, later,
//! the chat plugin) go through here instead of dispatching on backend types
//! themselves. The local GGUF backend runs in the existing `donnywhisper-llm-helper`
//! subprocess (ggml symbol-collision workaround); remote backends are blocking
//! HTTP calls.

mod anthropic;
mod gemini;
mod hermes;
pub(crate) mod keychain;
mod lm_studio;
mod ollama;
mod openai_compatible;
pub(crate) mod registry;

use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::Value;

use donnywhisper_core::{AppSettings, CustomLlmSource, LlmBackendKind, LlmModelRef, LlmPreset};
use reqwest::blocking::Client;

use crate::{llm_model_manager, local_llm};

/// User-Agent sent on remote/cloud provider HTTP calls.
pub(crate) const USER_AGENT: &str = "donnywhisper-bridge/0.1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// A runnable language-model backend.
///
/// `role_prompt` is the active mode's raw prompt (its "role"). Each backend
/// applies its own system-prompt convention: the local GGUF backend forwards it
/// straight to the llama helper (which builds the chat format), while remote and
/// cloud backends wrap it via [`build_system_prompt`].
pub trait LlmProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String>;

    /// Conversational generation for the chat plugin (#17). Unlike [`generate`]
    /// — which frames `role_prompt` as a "revise this dictated text"
    /// instruction — `chat` uses `system_prompt` directly as the assistant's
    /// system message and `user_text` as the user's turn, so the model answers
    /// the question instead of rewriting it.
    ///
    /// `session_key` is a stable per-conversation identifier. Backends that
    /// support long-term memory (the Hermes agent, via `X-Hermes-Session-Key`)
    /// scope it to this conversation; all other backends ignore it.
    fn chat(
        &self,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String>;

    /// Streaming variant of [`chat`]: `on_chunk` is called with each text delta
    /// as it arrives, and the full accumulated answer is returned. The default
    /// produces the whole answer up front and emits it as a single chunk — so
    /// non-streaming backends still work through the streaming path; SSE-capable
    /// backends (Hermes, OpenAI-compatible) override this.
    fn chat_stream(
        &self,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let full = self.chat(system_prompt, user_text, session_key, cancelled)?;
        if !full.is_empty() {
            on_chunk(&full);
        }
        Ok(full)
    }
}

/// Resolves a [`LlmModelRef`] into a runnable provider. The single dispatch
/// point shared by post-processing and chat.
pub fn provider_for(
    model: &LlmModelRef,
    settings: &AppSettings,
) -> Result<Box<dyn LlmProvider>, String> {
    match model {
        LlmModelRef::LocalPreset { preset } => Ok(Box::new(LocalGgufProvider::Preset(*preset))),
        LlmModelRef::LocalCustom { id } => {
            let custom = settings
                .custom_llm_models
                .iter()
                .find(|entry| &entry.id == id)
                .ok_or_else(|| format!("Custom language model '{id}' is not known in settings."))?;
            let path = match &custom.source {
                CustomLlmSource::LocalPath { path } => PathBuf::from(path),
                CustomLlmSource::DownloadUrl { .. } => {
                    let path = llm_model_manager::default_custom_llm_path(&custom.id)?;
                    if !path.exists() {
                        return Err(format!(
                            "Custom language model '{}' has not been downloaded yet.",
                            custom.name
                        ));
                    }
                    path
                }
            };
            Ok(Box::new(LocalGgufProvider::Custom {
                id: custom.id.clone(),
                name: custom.name.clone(),
                path,
            }))
        }
        LlmModelRef::Ollama { model_name } => Ok(Box::new(ollama::OllamaProvider::new(
            settings.ollama.endpoint.clone(),
            model_name.clone(),
        ))),
        LlmModelRef::LmStudio { model_name } => Ok(Box::new(lm_studio::LmStudioProvider::new(
            settings.lm_studio.endpoint.clone(),
            model_name.clone(),
        ))),
        LlmModelRef::OpenAiCompatible {
            provider,
            model_name,
        } => {
            let api_key = require_api_key(provider.backend_kind())?;
            Ok(Box::new(
                openai_compatible::OpenAiCompatibleProviderImpl::new(
                    *provider,
                    model_name.clone(),
                    api_key,
                ),
            ))
        }
        LlmModelRef::Anthropic { model_name } => {
            let api_key = require_api_key(LlmBackendKind::Anthropic)?;
            Ok(Box::new(anthropic::AnthropicProvider::new(
                model_name.clone(),
                api_key,
            )))
        }
        LlmModelRef::Gemini { model_name } => {
            let api_key = require_api_key(LlmBackendKind::Gemini)?;
            Ok(Box::new(gemini::GeminiProvider::new(
                model_name.clone(),
                api_key,
            )))
        }
        LlmModelRef::Hermes { id } => {
            let agent = settings
                .hermes_agents
                .iter()
                .find(|agent| &agent.id == id)
                .ok_or_else(|| format!("Hermes agent '{id}' is not configured."))?;
            if agent.base_url.trim().is_empty() {
                return Err(format!(
                    "Hermes agent '{}' has no address configured.",
                    agent.name
                ));
            }
            // The bearer token is optional — a local Hermes server may run
            // without `API_SERVER_KEY`.
            let api_key = keychain::get_hermes_api_key(id);
            Ok(Box::new(hermes::HermesAgentProvider::new(
                agent.base_url.clone(),
                agent.model_name.clone(),
                api_key,
            )))
        }
    }
}

/// Connection test for a Hermes agent (the settings "Test connection" button).
/// Thin wrapper so the FFI layer can reach the otherwise-private hermes module.
pub(crate) fn test_hermes_connection(
    base_url: &str,
    api_key: Option<&str>,
) -> Result<String, String> {
    hermes::test_connection(base_url, api_key)
}

/// Fetches a cloud backend's Keychain API key, or a clear error if unset.
fn require_api_key(kind: LlmBackendKind) -> Result<String, String> {
    keychain::get_api_key(kind)
        .ok_or_else(|| format!("API key for {} is not configured.", kind.label()))
}

/// Local GGUF backend, backed by the shared llama-helper subprocess.
enum LocalGgufProvider {
    Preset(LlmPreset),
    Custom {
        id: String,
        name: String,
        path: PathBuf,
    },
}

impl LlmProvider for LocalGgufProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        match self {
            LocalGgufProvider::Preset(preset) => {
                local_llm::generate_with_shared_runtime(*preset, role_prompt, user_text, cancelled)
            }
            LocalGgufProvider::Custom { id, name, path } => local_llm::generate_with_custom_path(
                id,
                name,
                path,
                role_prompt,
                user_text,
                cancelled,
            ),
        }
    }

    fn chat(
        &self,
        system_prompt: &str,
        user_text: &str,
        _session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        match self {
            LocalGgufProvider::Preset(preset) => {
                local_llm::chat_with_shared_runtime(*preset, system_prompt, user_text, cancelled)
            }
            LocalGgufProvider::Custom { id, name, path } => local_llm::chat_with_custom_path(
                id,
                name,
                path,
                system_prompt,
                user_text,
                cancelled,
            ),
        }
    }
}

/// Shared blocking HTTP client for remote/cloud providers.
pub(crate) fn build_http_client() -> Result<Client, String> {
    build_http_client_with_timeout(REQUEST_TIMEOUT)
}

/// Blocking HTTP client with a custom request timeout. The Hermes agent runs a
/// full toolset (terminal, web, files) per turn, so it needs far more headroom
/// than a plain chat-completion call.
pub(crate) fn build_http_client_with_timeout(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| format!("HTTP client for the language model could not be created: {err}"))
}

/// Reads an OpenAI-style streaming Chat-Completions response (`text/event-stream`
/// with `data: {chunk}` lines, terminated by `data: [DONE]`), forwarding each
/// `choices[0].delta.content` delta to `on_chunk` and returning the full
/// accumulated text. Shared by the Hermes and OpenAI-compatible backends.
pub(crate) fn stream_chat_completion(
    response: reqwest::blocking::Response,
    label: &str,
    cancelled: &Arc<AtomicBool>,
    on_chunk: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{label} returned HTTP {status}."));
    }

    let mut full = String::new();
    let reader = BufReader::new(response);
    for line in reader.lines() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let line = line.map_err(|err| format!("{label} stream could not be read: {err}"))?;
        let Some(data) = line.strip_prefix("data:") else {
            continue; // event:, id:, comments, blank separators
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            continue; // tolerate non-JSON keep-alive / progress events
        };
        if let Some(delta) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
        {
            if !delta.is_empty() {
                full.push_str(delta);
                on_chunk(delta);
            }
        }
    }

    if full.trim().is_empty() {
        return Err(format!("{label} stream contained no answer text."));
    }
    Ok(full)
}

/// Wraps the mode's role prompt into a system prompt for chat-style APIs that
/// take a system message (Ollama, LM Studio, OpenAI-compatible, ...).
pub(crate) fn build_system_prompt(mode_prompt: &str) -> String {
    let base = "You edit dictated text according to a configured role. Return only the final text — no explanations or meta comments.";
    let trimmed = mode_prompt.trim();
    if trimmed.is_empty() {
        base.to_owned()
    } else {
        format!("{base}\n\nRole context:\n{trimmed}")
    }
}

/// Joins a base endpoint with an API path, avoiding a duplicated `/v1` or `/api`
/// segment when the user already included it in the endpoint.
pub(crate) fn join_base_url(endpoint: &str, suffix: &str) -> String {
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

    #[test]
    fn empty_prompt_gets_safe_default_instruction() {
        let prompt = build_system_prompt("");
        assert!(prompt.contains("Return only the final text"));
    }

    #[test]
    fn role_prompt_is_appended_as_context() {
        let prompt = build_system_prompt("Use developer tone.");
        assert!(prompt.contains("Role context:"));
        assert!(prompt.contains("Use developer tone."));
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
}
