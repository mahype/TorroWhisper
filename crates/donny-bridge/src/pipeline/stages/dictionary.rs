//! Dictionary replacement stage — wraps `crate::dictionary::apply`.

use donny_core::{DictionaryEntry, STAGE_DICTIONARY};
use serde_json::json;

use crate::pipeline::PipelineStage;
use crate::pipeline::context::{PipelineContext, StageError, StageOutcome};

pub(crate) struct DictionaryStage {
    entries: Vec<DictionaryEntry>,
}

impl DictionaryStage {
    pub(crate) fn new(entries: Vec<DictionaryEntry>) -> Self {
        Self { entries }
    }
}

impl PipelineStage for DictionaryStage {
    fn id(&self) -> &str {
        STAGE_DICTIONARY
    }

    fn run(&self, ctx: &mut PipelineContext) -> Result<StageOutcome, StageError> {
        if self.entries.is_empty() {
            return Ok(StageOutcome::skipped("no dictionary entries"));
        }
        let before = ctx.text.clone();
        ctx.text = crate::dictionary::apply(&self.entries, &ctx.text);
        let changed = before != ctx.text;
        ctx.set_var("dictionary.changed", json!(changed));
        Ok(StageOutcome::cont(
            changed,
            if changed {
                "replacements applied"
            } else {
                "no match"
            },
        ))
    }
}
