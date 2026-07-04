//! Anthropic Messages API backend.
//!
//! Differs from the OpenAI-compatible shape: the system prompt is a top-level
//! `system` field (not a message), auth is `x-api-key` plus the
//! `anthropic-version` header, and the response is a `content` array of typed
//! blocks. `max_tokens` is required. Default model is `claude-opus-4-8`; per the
//! claude-api guidance, sampling params and `thinking` are omitted (Opus 4.8
//! rejects them) and the system prompt already asks for a final-answer-only
//! reply.

use std::sync::{Arc, atomic::AtomicBool};

use serde_json::{Value, json};

use super::{LlmProvider, USER_AGENT, build_http_client, build_system_prompt};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 4096;

pub(super) struct AnthropicProvider {
    model_name: String,
    api_key: String,
}

impl AnthropicProvider {
    pub(super) fn new(model_name: String, api_key: String) -> Self {
        Self {
            model_name,
            api_key,
        }
    }
}

impl AnthropicProvider {
    fn complete(&self, system_message: &str, user_text: &str) -> Result<String, String> {
        let client = build_http_client()?;

        let response = client
            .post(API_URL)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&json!({
                "model": self.model_name,
                "max_tokens": MAX_TOKENS,
                "system": system_message,
                "messages": [
                    { "role": "user", "content": user_text },
                ]
            }))
            .send()
            .map_err(|err| format!("Anthropic request could not be started: {err}"))?;

        let status = response.status();
        let value: Value = response
            .json()
            .map_err(|err| format!("Anthropic response could not be read: {err}"))?;
        if !status.is_success() {
            return Err(format!("Anthropic returned HTTP {status}."));
        }

        parse_messages_response(&value)
    }
}

impl LlmProvider for AnthropicProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.complete(&build_system_prompt(role_prompt), user_text)
    }

    fn chat(
        &self,
        system_prompt: &str,
        user_text: &str,
        _session_key: Option<&str>,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.complete(system_prompt, user_text)
    }
}

/// Concatenates the `text` blocks from an Anthropic Messages response body.
fn parse_messages_response(value: &Value) -> Result<String, String> {
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "Anthropic response contained no content.".to_owned())?;

    let text: String = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();

    if text.trim().is_empty() {
        Err("Anthropic response contained no processed text.".to_owned())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_text_blocks() {
        let body = json!({
            "content": [
                { "type": "text", "text": "Hello " },
                { "type": "text", "text": "world" },
            ]
        });
        assert_eq!(parse_messages_response(&body).unwrap(), "Hello world");
    }

    #[test]
    fn ignores_non_text_blocks() {
        let body = json!({
            "content": [
                { "type": "thinking", "thinking": "..." },
                { "type": "text", "text": "answer" },
            ]
        });
        assert_eq!(parse_messages_response(&body).unwrap(), "answer");
    }

    #[test]
    fn errors_on_empty_content() {
        let body = json!({ "content": [] });
        assert!(parse_messages_response(&body).is_err());
    }
}
