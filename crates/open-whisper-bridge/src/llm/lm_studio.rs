//! LM Studio backend (`POST {endpoint}/v1/chat/completions`, OpenAI-compatible).

use std::sync::{Arc, atomic::AtomicBool};

use serde_json::{Value, json};

use super::{LlmProvider, USER_AGENT, build_http_client, build_system_prompt, join_base_url};

pub(super) struct LmStudioProvider {
    endpoint: String,
    model_name: String,
}

impl LmStudioProvider {
    pub(super) fn new(endpoint: String, model_name: String) -> Self {
        Self {
            endpoint,
            model_name,
        }
    }
}

impl LmStudioProvider {
    fn complete(&self, system_message: &str, user_text: &str) -> Result<String, String> {
        let client = build_http_client()?;
        let url = join_base_url(&self.endpoint, "/v1/chat/completions");

        let response = client
            .post(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .json(&json!({
                "model": self.model_name,
                "temperature": 0.1,
                "messages": [
                    { "role": "system", "content": system_message },
                    { "role": "user", "content": user_text },
                ]
            }))
            .send()
            .map_err(|err| format!("LM Studio request could not be started: {err}"))?;

        let status = response.status();
        let value: Value = response
            .json()
            .map_err(|err| format!("LM Studio response could not be read: {err}"))?;
        if !status.is_success() {
            return Err(format!("LM Studio returned HTTP {status}."));
        }

        value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "LM Studio response contained no text.".to_owned())
    }
}

impl LlmProvider for LmStudioProvider {
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
        _cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        self.complete(system_prompt, user_text)
    }
}
