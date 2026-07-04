//! Hermes Agent (NousResearch) backend (#agent).
//!
//! Talks to a user-configured Hermes Agent API server over its OpenAI-compatible
//! `/v1/chat/completions` endpoint. Unlike the fixed-vendor [`super::openai_compatible`]
//! providers, each agent has its own base URL and an *optional* bearer token
//! (`API_SERVER_KEY`, stored in the Keychain). The active conversation's id is
//! forwarded as `X-Hermes-Session-Key` so the agent keeps long-term memory
//! scoped per chat (Honcho memory provider).

use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use serde_json::{Value, json};

use super::{
    LlmProvider, USER_AGENT, build_http_client_with_timeout, build_system_prompt, join_base_url,
    stream_chat_completion,
};

/// Agent tools (terminal, web search, file ops) can take a long time per turn,
/// so give Hermes far more headroom than a plain chat-completion request.
const HERMES_TIMEOUT: Duration = Duration::from_secs(300);

/// Max length of `X-Hermes-Session-Key` accepted by the server (control chars
/// are rejected). We sanitize defensively even though our session ids are safe.
const SESSION_KEY_MAX: usize = 256;

pub(super) struct HermesAgentProvider {
    base_url: String,
    model_name: String,
    /// `None` (or empty) → no `Authorization` header, for keyless local servers.
    api_key: Option<String>,
}

impl HermesAgentProvider {
    pub(super) fn new(base_url: String, model_name: String, api_key: Option<String>) -> Self {
        Self {
            base_url,
            model_name,
            api_key,
        }
    }

    fn complete(
        &self,
        system_message: &str,
        user_text: &str,
        session_key: Option<&str>,
        temperature: f32,
    ) -> Result<String, String> {
        let client = build_http_client_with_timeout(HERMES_TIMEOUT)?;
        let url = join_base_url(&self.base_url, "/v1/chat/completions");

        let mut request = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT);
        if let Some(key) = self.api_key.as_deref().filter(|key| !key.is_empty()) {
            request = request.bearer_auth(key);
        }
        if let Some(scope) = sanitize_session_key(session_key) {
            request = request.header("X-Hermes-Session-Key", scope);
        }

        let response = request
            .json(&json!({
                "model": self.model_name,
                "temperature": temperature,
                "stream": false,
                "messages": [
                    { "role": "system", "content": system_message },
                    { "role": "user", "content": user_text },
                ]
            }))
            .send()
            .map_err(|err| format!("Hermes request could not be started: {err}"))?;

        // Check the status before parsing: an unreachable/erroring agent (or a
        // proxy in front of it) may return a non-JSON body, and a JSON-parse
        // error would mask the real HTTP failure.
        let status = response.status();
        if !status.is_success() {
            return Err(format!("Hermes agent returned HTTP {status}."));
        }
        let value: Value = response
            .json()
            .map_err(|err| format!("Hermes response could not be read: {err}"))?;

        parse_chat_completion(&value)
    }

    /// Streaming counterpart of [`complete`]: requests `stream: true` and
    /// forwards SSE deltas to `on_chunk`.
    fn complete_stream(
        &self,
        system_message: &str,
        user_text: &str,
        session_key: Option<&str>,
        temperature: f32,
        cancelled: &std::sync::Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let client = build_http_client_with_timeout(HERMES_TIMEOUT)?;
        let url = join_base_url(&self.base_url, "/v1/chat/completions");

        let mut request = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT);
        if let Some(key) = self.api_key.as_deref().filter(|key| !key.is_empty()) {
            request = request.bearer_auth(key);
        }
        if let Some(scope) = sanitize_session_key(session_key) {
            request = request.header("X-Hermes-Session-Key", scope);
        }

        let response = request
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
            .map_err(|err| format!("Hermes request could not be started: {err}"))?;

        stream_chat_completion(response, "Hermes agent", cancelled, on_chunk)
    }
}

impl LlmProvider for HermesAgentProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        // Post-processing has no conversation, so no memory scope.
        self.complete(&build_system_prompt(role_prompt), user_text, None, 0.1)
    }

    fn chat(
        &self,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.complete(system_prompt, user_text, session_key, 0.7)
    }

    fn chat_stream(
        &self,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.complete_stream(system_prompt, user_text, session_key, 0.7, cancelled, on_chunk)
    }
}

/// Trims, strips control characters, and length-caps a session key. Returns
/// `None` when nothing usable remains (so no header is sent).
fn sanitize_session_key(session_key: Option<&str>) -> Option<String> {
    let raw = session_key?.trim();
    if raw.is_empty() {
        return None;
    }
    let cleaned: String = raw
        .chars()
        .filter(|ch| !ch.is_control())
        .take(SESSION_KEY_MAX)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Lightweight reachability/auth check for the "Test connection" button.
/// Does `GET {base}/v1/models` with the optional bearer — this validates the
/// address, port and key without running an agent turn. Returns a short,
/// human-readable status string.
pub(super) fn test_connection(base_url: &str, api_key: Option<&str>) -> Result<String, String> {
    let base = base_url.trim();
    if base.is_empty() {
        return Err("No address configured.".to_owned());
    }
    let client = build_http_client_with_timeout(Duration::from_secs(15))?;
    let url = join_base_url(base, "/v1/models");

    let mut request = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT);
    if let Some(key) = api_key.filter(|key| !key.is_empty()) {
        request = request.bearer_auth(key);
    }

    let response = request
        .send()
        .map_err(|err| format!("Not reachable: {err}"))?;
    let status = response.status();
    if status.is_success() {
        let count = response
            .json::<Value>()
            .ok()
            .and_then(|body| body.get("data").and_then(Value::as_array).map(|models| models.len()));
        return Ok(match count {
            Some(n) => format!("Connected — {n} model(s) available."),
            None => "Connected.".to_owned(),
        });
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(format!(
            "Reachable, but authentication failed (HTTP {status}). Check the API key."
        ));
    }
    Err(format!("Server returned HTTP {status}."))
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
        .ok_or_else(|| "Hermes response contained no answer text.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_assistant_content() {
        let body = json!({
            "choices": [{ "message": { "role": "assistant", "content": "hi from hermes" } }]
        });
        assert_eq!(parse_chat_completion(&body).unwrap(), "hi from hermes");
    }

    #[test]
    fn missing_content_is_an_error() {
        assert!(parse_chat_completion(&json!({ "choices": [] })).is_err());
    }

    #[test]
    fn session_key_is_trimmed_and_control_stripped() {
        assert_eq!(
            sanitize_session_key(Some("  session-42\n ")),
            Some("session-42".to_owned())
        );
        assert_eq!(sanitize_session_key(Some("   ")), None);
        assert_eq!(sanitize_session_key(None), None);
    }

    #[test]
    fn session_key_is_length_capped() {
        let long = "a".repeat(SESSION_KEY_MAX + 50);
        assert_eq!(
            sanitize_session_key(Some(&long)).map(|s| s.len()),
            Some(SESSION_KEY_MAX)
        );
    }
}
