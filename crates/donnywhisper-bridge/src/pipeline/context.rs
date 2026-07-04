//! The pipeline "traveler" and the stage outcome/record types.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use donnywhisper_core::ModeKind;
use serde_json::Value;

/// What a stage decided to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcomeKind {
    /// Ran (or no-op'd) and the pipeline continues.
    Continue,
    /// Ran and the pipeline should stop (short-circuit).
    Stop,
    /// Deliberately did nothing (e.g. disabled or already handled upstream).
    Skipped,
}

pub struct StageOutcome {
    pub kind: StageOutcomeKind,
    pub changed: bool,
    pub note: String,
}

impl StageOutcome {
    pub fn cont(changed: bool, note: impl Into<String>) -> Self {
        Self {
            kind: StageOutcomeKind::Continue,
            changed,
            note: note.into(),
        }
    }

    pub fn stop(note: impl Into<String>) -> Self {
        Self {
            kind: StageOutcomeKind::Stop,
            changed: false,
            note: note.into(),
        }
    }

    pub fn skipped(note: impl Into<String>) -> Self {
        Self {
            kind: StageOutcomeKind::Skipped,
            changed: false,
            note: note.into(),
        }
    }
}

#[derive(Debug)]
pub struct StageError {
    pub stage_id: String,
    pub message: String,
}

impl StageError {
    pub fn new(stage_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            stage_id: stage_id.into(),
            message: message.into(),
        }
    }
}

/// Diagnostic record of what a stage did, kept in `ctx.history` so later stages
/// can introspect (and the app can surface) the pipeline run.
#[derive(Debug, Clone)]
pub struct StageRecord {
    pub stage_id: String,
    pub outcome: StageOutcomeKind,
    pub changed: bool,
    pub note: String,
}

/// The object that travels through the pipeline. `text` is the single mutable
/// working buffer and is the final result; `original_transcript` is the frozen
/// fallback. `vars`/`history` are sidecar metadata, never inserted.
pub struct PipelineContext {
    pub original_transcript: String,
    pub text: String,
    pub language: String,
    pub mode_id: String,
    pub kind: ModeKind,
    pub vars: HashMap<String, Value>,
    pub history: Vec<StageRecord>,
    stop: bool,
    cancelled: Arc<AtomicBool>,
}

impl PipelineContext {
    pub fn new(
        transcript: String,
        language: String,
        mode_id: String,
        kind: ModeKind,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            original_transcript: transcript.clone(),
            text: transcript,
            language,
            mode_id,
            kind,
            vars: HashMap::new(),
            history: Vec::new(),
            stop: false,
            cancelled,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancel_flag(&self) -> &Arc<AtomicBool> {
        &self.cancelled
    }

    pub fn request_stop(&mut self) {
        self.stop = true;
    }

    pub fn should_stop(&self) -> bool {
        self.stop
    }

    /// True if a prior stage with this id ran and was not skipped.
    pub fn ran(&self, stage_id: &str) -> bool {
        self.history.iter().any(|record| {
            record.stage_id == stage_id && record.outcome != StageOutcomeKind::Skipped
        })
    }

    pub fn var(&self, key: &str) -> Option<&Value> {
        self.vars.get(key)
    }

    pub fn set_var(&mut self, key: impl Into<String>, value: Value) {
        self.vars.insert(key.into(), value);
    }
}
