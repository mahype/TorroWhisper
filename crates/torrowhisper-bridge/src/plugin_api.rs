//! Stable host→plugin API surface (#15 / plugin host).
//!
//! [`PluginHost`] is the single contract the app hands to every plugin — the
//! built-in chat plugin today, third-party plugins later. It bundles the three
//! things a plugin needs from the host:
//!
//! 1. **Discover** the available language models ([`PluginHost::available_models`]),
//!    each with its [`LlmAvailability`] — the same unified registry the settings
//!    UI shows (local presets, custom GGUF, cloud).
//! 2. **Run** a model, either conversationally ([`PluginHost::chat`]) or as an
//!    instruction/rewrite ([`PluginHost::generate`]).
//! 3. **Log** under a consistent `plugin:<id>` target ([`PluginHost::log`]).
//!
//! Plugins must reach the host *only* through this trait, never past it into
//! bridge internals. That keeps the boundary versioned: bump
//! [`PLUGIN_API_VERSION`] on any breaking change so a plugin can guard against
//! an incompatible host.
//!
//! The concrete [`BridgeHost`] owns a settings snapshot, so it is `Send` and can
//! move into a plugin's worker thread (e.g. chat generation runs off the main
//! loop). See `docs/PLUGIN_API.md` for the full guide.

use std::sync::{Arc, atomic::AtomicBool};

use torrowhisper_core::{AppSettings, LlmAvailability, LlmModelRef, LlmRegistryEntryDto};

use crate::{llm, plugin_log};

/// Current version of the plugin host API. Bump on breaking changes.
pub const PLUGIN_API_VERSION: u32 = 1;

/// Severity for [`PluginHost::log`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// The stable surface every plugin receives from the host.
pub trait PluginHost {
    /// API version this host implements ([`PLUGIN_API_VERSION`]).
    fn api_version(&self) -> u32 {
        PLUGIN_API_VERSION
    }

    /// Every model in the unified registry (local presets, custom GGUF, cloud),
    /// each carrying its [`LlmAvailability`]. Same list the settings UI shows.
    fn available_models(&self) -> Vec<LlmRegistryEntryDto>;

    /// Convenience filter: only models ready to run right now — local files
    /// present + valid, or a cloud key configured.
    fn ready_models(&self) -> Vec<LlmRegistryEntryDto> {
        self.available_models()
            .into_iter()
            .filter(|m| m.availability == LlmAvailability::Ready)
            .collect()
    }

    /// Conversational generation: `system_prompt` is used verbatim as the
    /// assistant's system message and `user_text` as the user's turn, so the
    /// model answers rather than rewrites. `session_key` is a stable
    /// per-conversation id; memory-capable backends (Hermes) scope it, the rest
    /// ignore it.
    fn chat(
        &self,
        model: &LlmModelRef,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String>;

    /// Streaming variant of [`chat`]: `on_chunk` receives text deltas as they
    /// arrive; returns the full accumulated answer.
    fn chat_stream(
        &self,
        model: &LlmModelRef,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String>;

    /// Instructional generation: `role_prompt` frames a transform/rewrite task
    /// over `user_text` (post-processing style).
    fn generate(
        &self,
        model: &LlmModelRef,
        role_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String>;

    /// Logs `message` under this plugin's `plugin:<id>` target.
    fn log(&self, level: LogLevel, message: &str);
}

/// Concrete [`PluginHost`] backed by the bridge.
///
/// Owns a settings snapshot taken when the host is built, so it can move into a
/// plugin's worker thread. Model resolution and execution route through the same
/// [`llm::provider_for`] dispatch post-processing uses, so plugins reach exactly
/// the backends the user configured (local GGUF helper, Ollama, LM Studio, cloud).
pub struct BridgeHost {
    plugin_id: String,
    settings: AppSettings,
}

impl BridgeHost {
    /// Builds a host for `plugin_id` from a settings snapshot.
    pub fn new(plugin_id: impl Into<String>, settings: AppSettings) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            settings,
        }
    }
}

impl PluginHost for BridgeHost {
    fn available_models(&self) -> Vec<LlmRegistryEntryDto> {
        llm::registry::build_static(&self.settings)
    }

    fn chat(
        &self,
        model: &LlmModelRef,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        llm::provider_for(model, &self.settings)?.chat(
            system_prompt,
            user_text,
            session_key,
            cancelled,
        )
    }

    fn chat_stream(
        &self,
        model: &LlmModelRef,
        system_prompt: &str,
        user_text: &str,
        session_key: Option<&str>,
        cancelled: &Arc<AtomicBool>,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        llm::provider_for(model, &self.settings)?.chat_stream(
            system_prompt,
            user_text,
            session_key,
            cancelled,
            on_chunk,
        )
    }

    fn generate(
        &self,
        model: &LlmModelRef,
        role_prompt: &str,
        user_text: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<String, String> {
        llm::provider_for(model, &self.settings)?.generate(role_prompt, user_text, cancelled)
    }

    fn log(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Debug => plugin_log::debug(&self.plugin_id, message),
            LogLevel::Info => plugin_log::info(&self.plugin_id, message),
            LogLevel::Warn => plugin_log::warn(&self.plugin_id, message),
            LogLevel::Error => plugin_log::error(&self.plugin_id, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_reports_current_api_version() {
        // Stays Keychain-free on purpose (like the registry tests): construct a
        // host and check the version constant without calling `available_models`,
        // which would touch the Keychain for cloud entries.
        let host = BridgeHost::new("test", AppSettings::default());
        assert_eq!(host.api_version(), PLUGIN_API_VERSION);
    }
}
