//! macOS Keychain storage for cloud-provider API keys.
//!
//! Keys never touch `settings.json`. Rust reads them directly at request time
//! (cloud providers live here), which keeps secrets out of the FFI/Swift call
//! frames and works for the background dictation/chat paths. Only an explicit
//! `set` ever moves a secret across the FFI boundary; status checks return
//! booleans only.

use torrowhisper_core::LlmBackendKind;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain service name. Matches the `ProjectDirs` qualifier/org/app.
const SERVICE: &str = "com.gettorro.TorroWhisper.llm";

/// The Keychain account (per-provider key slot), or `None` for non-cloud
/// backends that don't use API keys.
fn account_for(kind: LlmBackendKind) -> Option<&'static str> {
    match kind {
        LlmBackendKind::OpenAi => Some("openai_api_key"),
        LlmBackendKind::Mistral => Some("mistral_api_key"),
        LlmBackendKind::DeepSeek => Some("deepseek_api_key"),
        LlmBackendKind::Grok => Some("grok_api_key"),
        LlmBackendKind::Anthropic => Some("anthropic_api_key"),
        LlmBackendKind::Gemini => Some("gemini_api_key"),
        // Hermes agents use per-agent accounts (see the Hermes helpers below),
        // not this single-slot-per-backend table.
        LlmBackendKind::LocalGguf
        | LlmBackendKind::Ollama
        | LlmBackendKind::LmStudio
        | LlmBackendKind::Hermes => None,
    }
}

/// Returns the stored API key for a cloud backend, or `None` if absent/empty.
pub fn get_api_key(kind: LlmBackendKind) -> Option<String> {
    let account = account_for(kind)?;
    let bytes = get_generic_password(SERVICE, account).ok()?;
    let key = String::from_utf8(bytes).ok()?.trim().to_owned();
    if key.is_empty() { None } else { Some(key) }
}

/// Stores (or replaces) the API key for a cloud backend.
pub fn set_api_key(kind: LlmBackendKind, key: &str) -> Result<(), String> {
    let account =
        account_for(kind).ok_or_else(|| format!("{} does not use an API key.", kind.label()))?;
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("API key is empty.".to_owned());
    }
    set_generic_password(SERVICE, account, trimmed.as_bytes())
        .map_err(|err| format!("API key could not be saved to the Keychain: {err}"))
}

/// Deletes the stored key for a cloud backend. A missing key is not an error.
pub fn delete_api_key(kind: LlmBackendKind) -> Result<(), String> {
    let account =
        account_for(kind).ok_or_else(|| format!("{} does not use an API key.", kind.label()))?;
    // A "not found" result is success — the desired end state (no key) holds.
    let _ = delete_generic_password(SERVICE, account);
    Ok(())
}

/// True if a non-empty key is stored for this backend.
pub fn has_api_key(kind: LlmBackendKind) -> bool {
    get_api_key(kind).is_some()
}

// --- Hermes Agents (#agent) ---
//
// Unlike the fixed cloud vendors above (one key slot per backend), each Hermes
// agent has its own bearer token, keyed by the agent id. Same Keychain service,
// account `hermes_agent:<id>`. The token is optional — a local Hermes server can
// run without `API_SERVER_KEY` — so absence is never an error.

/// Keychain account for a Hermes agent's bearer token.
fn hermes_account(id: &str) -> String {
    format!("hermes_agent:{id}")
}

/// Returns the stored bearer token for a Hermes agent, or `None` if absent/empty.
pub fn get_hermes_api_key(id: &str) -> Option<String> {
    let bytes = get_generic_password(SERVICE, &hermes_account(id)).ok()?;
    let key = String::from_utf8(bytes).ok()?.trim().to_owned();
    if key.is_empty() { None } else { Some(key) }
}

/// Stores (or replaces) a Hermes agent's bearer token.
pub fn set_hermes_api_key(id: &str, key: &str) -> Result<(), String> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("API key is empty.".to_owned());
    }
    set_generic_password(SERVICE, &hermes_account(id), trimmed.as_bytes())
        .map_err(|err| format!("API key could not be saved to the Keychain: {err}"))
}

/// Deletes a Hermes agent's stored token. A missing token is not an error.
pub fn delete_hermes_api_key(id: &str) -> Result<(), String> {
    let _ = delete_generic_password(SERVICE, &hermes_account(id));
    Ok(())
}

/// True if a non-empty bearer token is stored for this Hermes agent.
pub fn has_hermes_api_key(id: &str) -> bool {
    get_hermes_api_key(id).is_some()
}
