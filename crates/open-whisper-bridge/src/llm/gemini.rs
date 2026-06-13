//! Google Gemini backend (`generateContent`).
//!
//! The system prompt goes in `systemInstruction`; the user turn in `contents`.
//! Auth is the `x-goog-api-key` header (preferred over `?key=` so the key stays
//! out of URLs/logs). The response is `candidates[].content.parts[].text`.

use std::sync::{Arc, atomic::AtomicBool};

use serde_json::{Value, json};

use super::{LlmProvider, USER_AGENT, build_http_client, build_system_prompt};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

pub(super) struct GeminiProvider {
    model_name: String,
    api_key: String,
}

impl GeminiProvider {
    pub(super) fn new(model_name: String, api_key: String) -> Self {
        Self {
            model_name,
            api_key,
        }
    }
}

impl LlmProvider for GeminiProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        let client = build_http_client()?;
        let system_prompt = build_system_prompt(role_prompt);
        let url = format!("{API_BASE}/{}:generateContent", self.model_name);

        let response = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header("x-goog-api-key", &self.api_key)
            .json(&json!({
                "systemInstruction": { "parts": [{ "text": system_prompt }] },
                "contents": [
                    { "role": "user", "parts": [{ "text": user_text }] },
                ]
            }))
            .send()
            .map_err(|err| format!("Gemini post-processing could not be started: {err}"))?;

        let status = response.status();
        let value: Value = response
            .json()
            .map_err(|err| format!("Gemini response could not be read: {err}"))?;
        if !status.is_success() {
            return Err(format!(
                "Gemini returned HTTP {status} during post-processing."
            ));
        }

        parse_generate_content(&value)
    }
}

/// Concatenates the text parts from the first candidate of a Gemini response.
fn parse_generate_content(value: &Value) -> Result<String, String> {
    let parts = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Gemini response contained no content.".to_owned())?;

    let text: String = parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect();

    if text.trim().is_empty() {
        Err("Gemini response contained no processed text.".to_owned())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_candidate_parts() {
        let body = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "cleaned " }, { "text": "text" }] }
            }]
        });
        assert_eq!(parse_generate_content(&body).unwrap(), "cleaned text");
    }

    #[test]
    fn errors_on_missing_candidates() {
        let body = json!({ "candidates": [] });
        assert!(parse_generate_content(&body).is_err());
    }
}
