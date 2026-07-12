//! Unified model registry.
//!
//! Builds one flat list of every locally-available and cloud model so the UI
//! and (later) the chat plugin select from a single source. A model that is
//! already on disk is reported once with its real availability and reused —
//! never offered for a duplicate download.
//!
//! Local presets, custom GGUF and cloud entries are cheap/synchronous and built
//! here. Remote Ollama / LM Studio models stay behind the on-demand
//! `ow_list_remote_models` path (a network call) and are merged in the UI, so
//! the registry never blocks the poll loop.

use std::path::PathBuf;

use torrowhisper_core::{
    AppSettings, CustomLlmSource, LlmAvailability, LlmBackendKind, LlmModelRef, LlmPreset,
    LlmRegistryEntryDto, OpenAiCompatibleProvider,
};

use super::keychain;
use crate::llm_model_manager::{self, LlmModelDownloadManager, LlmModelIntegrity};

/// Assembles the local + cloud registry. Cheap and synchronous — safe to call
/// from the FFI/poll path. Reflects live download progress.
pub(crate) fn build(
    settings: &AppSettings,
    downloads: &LlmModelDownloadManager,
) -> Vec<LlmRegistryEntryDto> {
    build_inner(settings, Some(downloads))
}

/// Same registry without a live download manager: nothing is reported as
/// `Downloading`, everything else (Ready / Downloadable / Corrupt / NeedsApiKey)
/// is resolved from settings + disk. Dependency-free variant the plugin host
/// hands to plugins ([`crate::plugin_api`]).
pub(crate) fn build_static(settings: &AppSettings) -> Vec<LlmRegistryEntryDto> {
    build_inner(settings, None)
}

fn build_inner(
    settings: &AppSettings,
    downloads: Option<&LlmModelDownloadManager>,
) -> Vec<LlmRegistryEntryDto> {
    let mut entries = Vec::new();

    for preset in LlmPreset::ALL {
        let (availability, progress) = match downloads {
            Some(d) if d.is_downloading_preset(preset) => {
                (LlmAvailability::Downloading, d.progress_basis_points())
            }
            _ => {
                let Ok(path) = llm_model_manager::default_llm_model_path(preset) else {
                    continue;
                };
                (
                    integrity_to_availability(llm_model_manager::gguf_file_integrity(
                        &path,
                        Some(preset.download_size_bytes()),
                    )),
                    None,
                )
            }
        };
        entries.push(entry(
            LlmModelRef::LocalPreset { preset },
            LlmBackendKind::LocalGguf,
            preset.display_label().to_owned(),
            preset.approx_size_label().to_owned(),
            availability,
            progress,
        ));
    }

    for custom in &settings.custom_llm_models {
        let (availability, progress) = match downloads {
            Some(d) if d.is_downloading_custom(&custom.id) => {
                (LlmAvailability::Downloading, d.progress_basis_points())
            }
            _ => {
                let path = match &custom.source {
                    CustomLlmSource::LocalPath { path } => PathBuf::from(path),
                    CustomLlmSource::DownloadUrl { .. } => {
                        match llm_model_manager::default_custom_llm_path(&custom.id) {
                            Ok(path) => path,
                            Err(_) => continue,
                        }
                    }
                };
                (
                    integrity_to_availability(llm_model_manager::gguf_file_integrity(&path, None)),
                    None,
                )
            }
        };
        entries.push(entry(
            LlmModelRef::LocalCustom {
                id: custom.id.clone(),
            },
            LlmBackendKind::LocalGguf,
            custom.name.clone(),
            "Custom GGUF".to_owned(),
            availability,
            progress,
        ));
    }

    for (model_ref, display_name) in cloud_catalog() {
        let kind = model_ref.backend_kind();
        let availability = if keychain::has_api_key(kind) {
            LlmAvailability::Ready
        } else {
            LlmAvailability::NeedsApiKey
        };
        let detail = match availability {
            LlmAvailability::NeedsApiKey => "Cloud · needs API key".to_owned(),
            _ => format!("Cloud · {}", kind.label()),
        };
        entries.push(entry(
            model_ref,
            kind,
            display_name,
            detail,
            availability,
            None,
        ));
    }

    // Remote Ollama / LM Studio models exist in the registry *only* once the user
    // enabled them — there is no static catalog to discover them from offline.
    // Reconstruct each enabled remote id into an entry without a network call (the
    // live fetch is just a discovery aid in the management UI). Reachability isn't
    // probed here, so a configured model is reported `Ready`; a failed request
    // surfaces its own error. `stable_id()` round-trips the id exactly, so these
    // never duplicate a catalog entry.
    for id in &settings.enabled_model_ids {
        let (model_ref, backend_kind, detail) = if let Some(name) = id.strip_prefix("ollama:") {
            (
                LlmModelRef::Ollama {
                    model_name: name.to_owned(),
                },
                LlmBackendKind::Ollama,
                "Ollama · remote",
            )
        } else if let Some(name) = id.strip_prefix("lmstudio:") {
            (
                LlmModelRef::LmStudio {
                    model_name: name.to_owned(),
                },
                LlmBackendKind::LmStudio,
                "LM Studio · remote",
            )
        } else {
            continue;
        };
        let display_name = match &model_ref {
            LlmModelRef::Ollama { model_name } | LlmModelRef::LmStudio { model_name } => {
                model_name.clone()
            }
            _ => unreachable!("only ollama/lmstudio refs are built above"),
        };
        entries.push(entry(
            model_ref,
            backend_kind,
            display_name,
            detail.to_owned(),
            LlmAvailability::Ready,
            None,
        ));
    }

    // One final pass stamps the app-wide enable state onto every entry: literal
    // membership of `enabled_model_ids` (the "empty = show all" fallback lives in
    // the pickers).
    for item in &mut entries {
        item.enabled = settings
            .enabled_model_ids
            .iter()
            .any(|id| id == &item.stable_id);
    }

    entries
}

fn integrity_to_availability(integrity: LlmModelIntegrity) -> LlmAvailability {
    match integrity {
        LlmModelIntegrity::Valid => LlmAvailability::Ready,
        LlmModelIntegrity::Corrupt { .. } => LlmAvailability::Corrupt,
        LlmModelIntegrity::Missing => LlmAvailability::Downloadable,
    }
}

fn entry(
    model_ref: LlmModelRef,
    backend_kind: LlmBackendKind,
    display_name: String,
    detail: String,
    availability: LlmAvailability,
    progress_basis_points: Option<u16>,
) -> LlmRegistryEntryDto {
    LlmRegistryEntryDto {
        stable_id: model_ref.stable_id(),
        model_ref,
        backend_kind,
        display_name,
        detail,
        availability,
        // Filled in one final pass in `build_inner` from `enabled_model_ids`.
        enabled: false,
        progress_basis_points,
    }
}

/// Curated default cloud models per provider. Users can still point a provider
/// at any model name (free-text) elsewhere; this is the discoverable default
/// list. Anthropic IDs verified via the `claude-api` reference.
fn cloud_catalog() -> Vec<(LlmModelRef, String)> {
    let openai_compat = |provider, model: &str, label: &str| {
        (
            LlmModelRef::OpenAiCompatible {
                provider,
                model_name: model.to_owned(),
            },
            label.to_owned(),
        )
    };
    let anthropic = |model: &str, label: &str| {
        (
            LlmModelRef::Anthropic {
                model_name: model.to_owned(),
            },
            label.to_owned(),
        )
    };
    let gemini = |model: &str, label: &str| {
        (
            LlmModelRef::Gemini {
                model_name: model.to_owned(),
            },
            label.to_owned(),
        )
    };

    vec![
        anthropic("claude-opus-4-8", "Claude Opus 4.8"),
        anthropic("claude-sonnet-4-6", "Claude Sonnet 4.6"),
        anthropic("claude-haiku-4-5", "Claude Haiku 4.5"),
        openai_compat(OpenAiCompatibleProvider::OpenAi, "gpt-4o", "GPT-4o"),
        openai_compat(
            OpenAiCompatibleProvider::OpenAi,
            "gpt-4o-mini",
            "GPT-4o mini",
        ),
        openai_compat(
            OpenAiCompatibleProvider::Mistral,
            "mistral-large-latest",
            "Mistral Large",
        ),
        openai_compat(
            OpenAiCompatibleProvider::Mistral,
            "mistral-small-latest",
            "Mistral Small",
        ),
        openai_compat(
            OpenAiCompatibleProvider::DeepSeek,
            "deepseek-chat",
            "DeepSeek Chat",
        ),
        openai_compat(OpenAiCompatibleProvider::Grok, "grok-2-latest", "Grok 2"),
        gemini("gemini-2.5-flash", "Gemini 2.5 Flash"),
        gemini("gemini-2.5-pro", "Gemini 2.5 Pro"),
    ]
}

#[cfg(test)]
mod tests {
    // Tests stay Keychain-free on purpose: reading an existing item can prompt
    // for access from a non-app test binary on macOS. `build()` touches the
    // Keychain for cloud availability, so it is exercised via the FFI/app, not
    // here — these cover the pure pieces.
    use super::*;

    #[test]
    fn cloud_catalog_entries_are_all_cloud() {
        let catalog = cloud_catalog();
        assert!(
            catalog
                .iter()
                .any(|(r, _)| r.stable_id() == "anthropic:claude-opus-4-8")
        );
        assert!(catalog.iter().all(|(r, _)| r.is_cloud()));
    }

    #[test]
    fn integrity_maps_to_availability() {
        assert_eq!(
            integrity_to_availability(LlmModelIntegrity::Valid),
            LlmAvailability::Ready
        );
        assert_eq!(
            integrity_to_availability(LlmModelIntegrity::Missing),
            LlmAvailability::Downloadable
        );
        assert!(matches!(
            integrity_to_availability(LlmModelIntegrity::Corrupt {
                reason: "bad".to_owned()
            }),
            LlmAvailability::Corrupt
        ));
    }
}
