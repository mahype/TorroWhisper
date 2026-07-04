//! Ordered post-processing pipeline (Issue #16).
//!
//! Replaces the fixed "dictionary → single LLM step" with a configurable,
//! ordered chain of stages. A [`PipelineContext`] travels through the stages;
//! each does one thing and records what it did, so later stages can introspect
//! and short-circuit. Plugins register extra stages via
//! [`StageRegistry::register`] (Issue #15).

pub(crate) mod context;
mod stages;

use std::sync::{Arc, atomic::AtomicBool};

use donny_core::{
    AppSettings, ProcessingMode, STAGE_AUTO_CORRECT, STAGE_DICTIONARY, STAGE_LLM,
    StageCatalogEntryDto,
};
use serde_json::Value;

use self::context::{PipelineContext, StageError, StageOutcome, StageOutcomeKind};
use self::stages::{
    auto_correct::{AutoCorrectMode, AutoCorrectStage},
    dictionary::DictionaryStage,
    llm::LlmStage,
};

/// A runnable pipeline step. `run` mutates `ctx.text` and returns what it did.
pub trait PipelineStage: Send {
    fn id(&self) -> &str;
    fn run(&self, ctx: &mut PipelineContext) -> Result<StageOutcome, StageError>;
}

/// Sequential runner. Honors cancellation and the short-circuit `stop` flag,
/// and records every stage into `ctx.history`.
struct Pipeline {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl Pipeline {
    fn process(&self, ctx: &mut PipelineContext) -> Result<(), StageError> {
        for stage in &self.stages {
            if ctx.is_cancelled() {
                return Err(StageError::new(stage.id(), "Pipeline cancelled."));
            }
            if ctx.should_stop() {
                break;
            }
            let outcome = stage.run(ctx)?;
            let stop = outcome.kind == StageOutcomeKind::Stop;
            ctx.history.push(context::StageRecord {
                stage_id: stage.id().to_owned(),
                outcome: outcome.kind,
                changed: outcome.changed,
                note: outcome.note,
            });
            if stop {
                ctx.request_stop();
                break;
            }
        }
        Ok(())
    }
}

/// Context handed to a [`StageFactory`] when instantiating a stage for a run.
pub struct BuildCx<'a> {
    pub settings: &'a AppSettings,
    pub mode: &'a ProcessingMode,
    pub config: &'a Value,
}

/// Produces a stage instance bound to a run. Built-in factories plus, later,
/// plugin-provided ones.
pub trait StageFactory: Send + Sync {
    fn stage_id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn is_configurable(&self) -> bool {
        false
    }
    fn is_plugin(&self) -> bool {
        false
    }
    fn build(&self, cx: &BuildCx) -> Result<Box<dyn PipelineStage>, StageError>;
}

/// Holds the built-in stage factories plus any registered by plugins (#15).
pub struct StageRegistry {
    factories: Vec<Box<dyn StageFactory>>,
}

impl StageRegistry {
    pub fn with_builtins() -> Self {
        Self {
            factories: vec![
                Box::new(DictionaryFactory),
                Box::new(AutoCorrectFactory),
                Box::new(LlmFactory),
            ],
        }
    }

    /// Adds a stage factory (used by the plugin host in #15).
    pub fn register(&mut self, factory: Box<dyn StageFactory>) {
        self.factories.push(factory);
    }

    fn resolve(&self, stage_id: &str) -> Option<&dyn StageFactory> {
        self.factories
            .iter()
            .find(|factory| factory.stage_id() == stage_id)
            .map(|factory| factory.as_ref())
    }

    pub fn catalog(&self) -> Vec<StageCatalogEntryDto> {
        self.factories
            .iter()
            .map(|factory| StageCatalogEntryDto {
                stage_id: factory.stage_id().to_owned(),
                display_name: factory.display_name().to_owned(),
                is_configurable: factory.is_configurable(),
                is_plugin: factory.is_plugin(),
            })
            .collect()
    }
}

/// Runs the active mode's pipeline over `transcript`, returning the final text
/// (or an error so the caller can fall back to the raw transcript).
pub fn run(
    registry: &StageRegistry,
    settings: &AppSettings,
    transcript: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mode = settings.active_mode();
    let mut ctx = PipelineContext::new(
        transcript.to_owned(),
        settings.transcription_language.clone(),
        mode.id.clone(),
        mode.kind,
        cancelled.clone(),
    );
    let pipeline = build_pipeline_for_mode(registry, settings, mode);
    pipeline.process(&mut ctx).map_err(|err| err.message)?;

    let trimmed = ctx.text.trim();
    if trimmed.is_empty() {
        return Err("Pipeline produced no text.".to_owned());
    }
    Ok(trimmed.to_owned())
}

/// Builds the ordered, enabled stages for a mode. Falls back to the synthesized
/// legacy order when the mode has no explicit pipeline; unknown stage ids
/// (e.g. an uninstalled plugin) are skipped with a log line, never a hard error.
fn build_pipeline_for_mode(
    registry: &StageRegistry,
    settings: &AppSettings,
    mode: &ProcessingMode,
) -> Pipeline {
    let steps = if mode.pipeline.is_empty() {
        mode.synthesized_pipeline(settings.post_processing_enabled)
    } else {
        mode.pipeline.clone()
    };

    let mut stages: Vec<Box<dyn PipelineStage>> = Vec::new();
    for step in &steps {
        if !step.enabled {
            continue;
        }
        match registry.resolve(&step.stage_id) {
            Some(factory) => {
                let cx = BuildCx {
                    settings,
                    mode,
                    config: &step.config,
                };
                match factory.build(&cx) {
                    Ok(stage) => stages.push(stage),
                    Err(err) => log::warn!(
                        target: "post_processing",
                        "pipeline stage '{}' could not be built: {}",
                        step.stage_id, err.message
                    ),
                }
            }
            None => log::warn!(
                target: "post_processing",
                "unknown pipeline stage '{}' skipped",
                step.stage_id
            ),
        }
    }
    Pipeline { stages }
}

// --- built-in factories ------------------------------------------------------

struct DictionaryFactory;
impl StageFactory for DictionaryFactory {
    fn stage_id(&self) -> &str {
        STAGE_DICTIONARY
    }
    fn display_name(&self) -> &str {
        "Dictionary"
    }
    fn build(&self, cx: &BuildCx) -> Result<Box<dyn PipelineStage>, StageError> {
        Ok(Box::new(DictionaryStage::new(
            cx.settings.dictionary.clone(),
        )))
    }
}

struct AutoCorrectFactory;
impl StageFactory for AutoCorrectFactory {
    fn stage_id(&self) -> &str {
        STAGE_AUTO_CORRECT
    }
    fn display_name(&self) -> &str {
        "Auto-correct"
    }
    fn is_configurable(&self) -> bool {
        true
    }
    fn build(&self, cx: &BuildCx) -> Result<Box<dyn PipelineStage>, StageError> {
        let mode = cx
            .config
            .get("mode")
            .and_then(Value::as_str)
            .map(AutoCorrectMode::from_str)
            .unwrap_or(AutoCorrectMode::Off);
        Ok(Box::new(AutoCorrectStage::new(mode, cx.settings)))
    }
}

struct LlmFactory;
impl StageFactory for LlmFactory {
    fn stage_id(&self) -> &str {
        STAGE_LLM
    }
    fn display_name(&self) -> &str {
        "LLM post-processing"
    }
    fn build(&self, cx: &BuildCx) -> Result<Box<dyn PipelineStage>, StageError> {
        Ok(Box::new(LlmStage::new(cx.settings, cx.mode.prompt.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn empty_pipeline_returns_transcript_unchanged() {
        // Default settings: post-processing off → only the (no-op) dictionary
        // step is synthesized, so the text passes through unchanged.
        let settings = AppSettings::default();
        let registry = StageRegistry::with_builtins();
        let cancelled = Arc::new(AtomicBool::new(false));
        let out = run(&registry, &settings, "hallo welt", &cancelled).unwrap();
        assert_eq!(out, "hallo welt");
    }

    #[test]
    fn catalog_lists_builtin_stages() {
        let ids: Vec<String> = StageRegistry::with_builtins()
            .catalog()
            .into_iter()
            .map(|entry| entry.stage_id)
            .collect();
        assert_eq!(ids, vec![STAGE_DICTIONARY, STAGE_AUTO_CORRECT, STAGE_LLM]);
    }

    #[test]
    fn cancelled_pipeline_errors() {
        let mut settings = AppSettings::default();
        settings.post_processing_enabled = true; // would run the LLM step
        let registry = StageRegistry::with_builtins();
        let cancelled = Arc::new(AtomicBool::new(true));
        assert!(run(&registry, &settings, "text", &cancelled).is_err());
    }
}
