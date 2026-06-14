//! LLM post-processing stage — runs the active mode's prompt over the text
//! through the shared provider layer (#14). Replaces the old
//! `post_processing::process_text` LLM dispatch.

use std::time::Instant;

use open_whisper_core::{AppSettings, LlmModelRef, STAGE_LLM};
use serde_json::json;

use crate::pipeline::PipelineStage;
use crate::pipeline::context::{PipelineContext, StageError, StageOutcome};
use crate::plugin_api::{BridgeHost, PluginHost};

pub(crate) struct LlmStage {
    settings: AppSettings,
    role_prompt: String,
    model_ref: LlmModelRef,
}

impl LlmStage {
    /// Resolves the effective model: a registry-selected model takes precedence,
    /// else the legacy `PostProcessingChoice` resolution.
    pub(crate) fn new(settings: &AppSettings, role_prompt: String) -> Self {
        let mode = settings.active_mode();
        let model_ref = settings
            .active_post_processing_model
            .clone()
            .unwrap_or_else(|| LlmModelRef::from(settings.effective_post_processing_choice(mode)));
        Self {
            settings: settings.clone(),
            role_prompt,
            model_ref,
        }
    }
}

impl PipelineStage for LlmStage {
    fn id(&self) -> &str {
        STAGE_LLM
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<StageOutcome, StageError> {
        let backend_label = self.model_ref.backend_kind().label();
        let started = Instant::now();
        log::info!(
            target: "post_processing",
            "llm stage via {backend_label} ({} chars in)",
            ctx.text.chars().count()
        );

        // Route through the shared plugin host so post-processing and plugins
        // resolve + run models the same way.
        let host = BridgeHost::new("post_processing", self.settings.clone());
        let output = host
            .generate(
                &self.model_ref,
                &self.role_prompt,
                &ctx.text,
                ctx.cancel_flag(),
            )
            .map_err(|err| StageError::new(STAGE_LLM, err))?;

        let trimmed = output.trim();
        if trimmed.is_empty() {
            return Err(StageError::new(
                STAGE_LLM,
                "Post-processing returned no text.",
            ));
        }

        log::info!(
            target: "post_processing",
            "llm stage via {backend_label} done in {:.1}s ({} chars out)",
            started.elapsed().as_secs_f32(),
            trimmed.chars().count()
        );

        ctx.set_var("llm.model_used", json!(backend_label));
        ctx.set_var("llm.did_cleanup", json!(true));
        ctx.text = trimmed.to_owned();
        Ok(StageOutcome::cont(true, "post-processed"))
    }
}
