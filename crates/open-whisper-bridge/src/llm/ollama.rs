//! Ollama backend (`POST {endpoint}/api/chat`, non-streaming).

use std::sync::{Arc, atomic::AtomicBool};

use serde_json::{Value, json};

use super::{LlmProvider, USER_AGENT, build_http_client, build_system_prompt, join_base_url};

pub(super) struct OllamaProvider {
    endpoint: String,
    model_name: String,
}

impl OllamaProvider {
    pub(super) fn new(endpoint: String, model_name: String) -> Self {
        Self {
            endpoint,
            model_name,
        }
    }
}

impl LlmProvider for OllamaProvider {
    fn generate(
        &self,
        role_prompt: &str,
        user_text: &str,
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        let client = build_http_client()?;
        let system_prompt = build_system_prompt(role_prompt);
        let url = join_base_url(&self.endpoint, "/api/chat");

        let response = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .json(&json!({
                "model": self.model_name,
                "stream": false,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": user_text },
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
                "Ollama returned HTTP {status} during post-processing."
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
}
