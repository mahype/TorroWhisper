//! Auto-correct stage (ROADMAP #4). v1 supports `off` and `llm`; `spell_check`
//! (NSSpellChecker, Swift-side) is a placeholder that skips for now.

use serde_json::json;
use torrowhisper_core::{AppSettings, LlmModelRef, STAGE_AUTO_CORRECT, STAGE_LLM};

use crate::llm;
use crate::pipeline::PipelineStage;
use crate::pipeline::context::{PipelineContext, StageError, StageOutcome};

const CLEANUP_PROMPT: &str = "Fix spelling, grammar and punctuation without changing the meaning. Return only the corrected text.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoCorrectMode {
    Off,
    SpellCheck,
    Llm,
}

impl AutoCorrectMode {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "spell_check" => Self::SpellCheck,
            "llm" => Self::Llm,
            _ => Self::Off,
        }
    }
}

pub(crate) struct AutoCorrectStage {
    mode: AutoCorrectMode,
    settings: AppSettings,
}

impl AutoCorrectStage {
    pub(crate) fn new(mode: AutoCorrectMode, settings: &AppSettings) -> Self {
        Self {
            mode,
            settings: settings.clone(),
        }
    }
}

impl PipelineStage for AutoCorrectStage {
    fn id(&self) -> &str {
        STAGE_AUTO_CORRECT
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<StageOutcome, StageError> {
        match self.mode {
            AutoCorrectMode::Off => Ok(StageOutcome::skipped("auto-correct off")),
            AutoCorrectMode::SpellCheck => {
                // NSSpellChecker is Swift-side; not yet wired through the bridge.
                Ok(StageOutcome::skipped("spell-check not yet implemented"))
            }
            AutoCorrectMode::Llm => {
                // Don't double-clean if an LLM stage already did so.
                if ctx.ran(STAGE_LLM)
                    && ctx.var("llm.did_cleanup").and_then(|v| v.as_bool()) == Some(true)
                {
                    return Ok(StageOutcome::skipped("LLM stage already cleaned the text"));
                }

                let mode = self.settings.active_mode();
                let model_ref = self
                    .settings
                    .active_post_processing_model
                    .clone()
                    .unwrap_or_else(|| {
                        LlmModelRef::from(self.settings.effective_post_processing_choice(mode))
                    });
                let provider = llm::provider_for(&model_ref, &self.settings)
                    .map_err(|err| StageError::new(STAGE_AUTO_CORRECT, err))?;
                let output = provider
                    .generate(CLEANUP_PROMPT, &ctx.text, ctx.cancel_flag())
                    .map_err(|err| StageError::new(STAGE_AUTO_CORRECT, err))?;
                let trimmed = output.trim();
                if trimmed.is_empty() {
                    return Err(StageError::new(
                        STAGE_AUTO_CORRECT,
                        "Auto-correct returned no text.",
                    ));
                }
                let changed = trimmed != ctx.text;
                ctx.text = trimmed.to_owned();
                ctx.set_var("auto_correct.applied", json!(true));
                Ok(StageOutcome::cont(changed, "auto-corrected (LLM)"))
            }
        }
    }
}
