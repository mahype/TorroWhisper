use std::time::Duration;

use torrowhisper_core::{ExternalProviderSettings, RemoteModelBackend, RemoteModelDto};
use reqwest::blocking::Client;
use serde::Deserialize;

const USER_AGENT: &str = "torrowhisper-bridge/0.1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Deserialize)]
struct OllamaModelEntry {
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModelEntry>,
}

#[derive(Debug, Deserialize)]
struct LmStudioModelEntry {
    id: String,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    arch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LmStudioModelsResponse {
    #[serde(default)]
    data: Vec<LmStudioModelEntry>,
}

pub fn list_remote_models(
    backend: RemoteModelBackend,
    provider: &ExternalProviderSettings,
) -> Result<Vec<RemoteModelDto>, String> {
    let endpoint = provider.endpoint.trim();
    if endpoint.is_empty() {
        return Err(format!(
            "{}-Endpoint ist leer. Bitte Endpoint konfigurieren.",
            backend.label()
        ));
    }

    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|err| format!("HTTP client could not be created: {err}"))?;

    match backend {
        RemoteModelBackend::Ollama => fetch_ollama(&client, endpoint),
        RemoteModelBackend::LmStudio => fetch_lm_studio(&client, endpoint),
    }
}

fn fetch_ollama(client: &Client, endpoint: &str) -> Result<Vec<RemoteModelDto>, String> {
    let url = join_base_url(endpoint, "/api/tags");
    let response = client
        .get(&url)
        .send()
        .map_err(|err| format!("Ollama endpoint {} not reachable: {err}", endpoint))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Ollama returned HTTP {} at {}.", status, url));
    }
    let payload: OllamaTagsResponse = response
        .json()
        .map_err(|err| format!("Ollama response could not be read: {err}"))?;

    Ok(payload
        .models
        .into_iter()
        .map(|entry| {
            let mut parts: Vec<String> = Vec::new();
            if let Some(details) = entry.details.as_ref() {
                if let Some(param_size) = details.parameter_size.as_deref() {
                    let trimmed = param_size.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_owned());
                    }
                }
                if let Some(quant) = details.quantization_level.as_deref() {
                    let trimmed = quant.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_owned());
                    }
                }
            }
            if let Some(bytes) = entry.size {
                parts.push(human_bytes(bytes));
            }
            let summary = if parts.is_empty() {
                "Ollama model".to_owned()
            } else {
                parts.join(" · ")
            };
            RemoteModelDto {
                backend: RemoteModelBackend::Ollama,
                name: entry.name,
                summary,
            }
        })
        .collect())
}

fn fetch_lm_studio(client: &Client, endpoint: &str) -> Result<Vec<RemoteModelDto>, String> {
    let url = join_base_url(endpoint, "/v1/models");
    let response = client
        .get(&url)
        .send()
        .map_err(|err| format!("LM Studio endpoint {} not reachable: {err}", endpoint))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("LM Studio returned HTTP {} at {}.", status, url));
    }
    let payload: LmStudioModelsResponse = response
        .json()
        .map_err(|err| format!("LM Studio response could not be read: {err}"))?;

    Ok(payload
        .data
        .into_iter()
        .map(|entry| {
            let mut parts: Vec<String> = Vec::new();
            if let Some(publisher) = entry.publisher.as_deref() {
                let trimmed = publisher.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_owned());
                }
            }
            if let Some(arch) = entry.arch.as_deref() {
                let trimmed = arch.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_owned());
                }
            }
            let summary = if parts.is_empty() {
                "LM Studio model".to_owned()
            } else {
                parts.join(" · ")
            };
            RemoteModelDto {
                backend: RemoteModelBackend::LmStudio,
                name: entry.id,
                summary,
            }
        })
        .collect())
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

fn human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.0} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_base_url_avoids_duplicate_api_prefix() {
        assert_eq!(
            join_base_url("http://127.0.0.1:11434/api", "/api/tags"),
            "http://127.0.0.1:11434/api/tags"
        );
    }

    #[test]
    fn join_base_url_adds_missing_segment() {
        assert_eq!(
            join_base_url("http://127.0.0.1:1234", "/v1/models"),
            "http://127.0.0.1:1234/v1/models"
        );
    }

    #[test]
    fn human_bytes_formats_gigabytes() {
        let result = human_bytes(4_500_000_000);
        assert!(result.contains("GB"));
    }
}
