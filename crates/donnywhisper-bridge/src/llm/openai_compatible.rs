//! OpenAI-compatible Chat-Completions backend.
//!
//! One implementation for every vendor that speaks the OpenAI Chat-Completions
//! wire format — OpenAI, Mistral, DeepSeek and Grok (xAI). They differ only in
//! base URL and which Keychain key authenticates the `Authorization: Bearer`
//! header.

use std::sync::{Arc, atomic::AtomicBool};

use donnywhisper_core::OpenAiCompatibleProvider;
use serde_json::{Value, json};

use super::{LlmProvider, USER_AGENT, build_http_client, build_system_prompt, stream_chat_completion};

pub(super) struct OpenAiCompatibleProviderImpl {
    provider: OpenAiCompatibleProvider,
    model_name: String,
    api_key: String,
}

impl OpenAiCompatibleProviderImpl {
    pub(super) fn new(
        provider: OpenAiCompatibleProvider,
        model_name: String,
        api_key: String,
    ) -> Self {
        Self {
            provider,
            model_name,
            api_key,
        }
    }
}

/// Base URL (without the trailing `/chat/completions`) for each vendor.
fn base_url(provider: OpenAiCompatibleProvider) -> &'static str {
    match provider {
        OpenAiCompatibleProvider::OpenAi => "https://api.openai.com/v1",
        OpenAiCompatibleProvider::Mistral => "https://api.mistral.ai/v1",
        OpenAiCompatibleProvider::DeepSeek => "https://api.deepseek.com/v1",
        OpenAiCompatibleProvider::Grok => "https://api.x.ai/v1",
    }
}

impl OpenAiCompatibleProviderImpl {
    fn complete(
        &self,
        system_message: &str,
        user_text: &str,
        temperature: f32,
    ) -> Result<String, String> {
        let client = build_http_client()?;
        let url = format!("{}/chat/completions", base_url(self.provider));

        let response = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": self.model_name,
                "temperature": temperature,
                "messages": [
                    { "role": "system", "content": system_message },
                    { "role": "user", "content": user_text },
                ]
            }))
            .send()
            .map_err(|err| format!("Cloud request could not be started: {err}"))?;

        let status = response.status();
        let value: Value = response
            .json()
            .map_err(|err| format!("Cloud response could not be read: {err}"))?;
        if !status.is_success() {
            return Err(format!(
                "{} returned HTTP {status}.",
                self.provider.backend_kind().label()
            ));
        }

        parse_chat_completion(&value)
    }

    /// Streaming counterpart of [`complete`]: requests `stream: true` and
    /// forwards SSE deltas to `on_chunk`.
    fn complete_stream(
        &self,
        system_message: &str,
        user_text: &str,
        temperature: f32,
        cancelled: &Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let client = build_http_client()?;
        let url = format!("{}/chat/completions", base_url(self.provider));

        let response = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": self.model_name,
                "temperature": temperature,
                "stream": true,
                "messages": [
                    { "role": "system", "content": system_message },
                    { "role": "user", "content": user_text },
                ]
            }))
            .send()
            .map_err(|err| format!("Cloud request could not be started: {err}"))?;

        stream_chat_completion(
            response,
            self.provider.backend_kind().label(),
            cancelled,
            on_chunk,
        )
    }
}

impl LlmProvider for OpenAiCompatibleProviderImpl {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        // Low temperature: post-processing should be deterministic.
        self.complete(&build_system_prompt(role_prompt), user_text, 0.1)
    }

    fn chat(
        &self,
        system_prompt: &str,
        user_text: &str,
        _session_key: Option<&str>,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        // Higher temperature: chat should feel natural, not robotic.
        self.complete(system_prompt, user_text, 0.7)
    }

    fn chat_stream(
        &self,
        system_prompt: &str,
        user_text: &str,
        _session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.complete_stream(system_prompt, user_text, 0.7, cancelled, on_chunk)
    }
}

/// Extracts the assistant text from an OpenAI Chat-Completions response body.
fn parse_chat_completion(value: &Value) -> Result<String, String> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Cloud response contained no processed text.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_assistant_content() {
        let body = json!({
            "choices": [{ "message": { "role": "assistant", "content": "cleaned up text" } }]
        });
        assert_eq!(parse_chat_completion(&body).unwrap(), "cleaned up text");
    }

    #[test]
    fn errors_on_missing_content() {
        let body = json!({ "choices": [] });
        assert!(parse_chat_completion(&body).is_err());
    }

    #[test]
    fn base_urls_are_https() {
        for provider in [
            OpenAiCompatibleProvider::OpenAi,
            OpenAiCompatibleProvider::Mistral,
            OpenAiCompatibleProvider::DeepSeek,
            OpenAiCompatibleProvider::Grok,
        ] {
            assert!(base_url(provider).starts_with("https://"));
        }
    }
}
