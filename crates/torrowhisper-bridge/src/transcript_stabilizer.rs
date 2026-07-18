//! Turns the stream of full-buffer Whisper hypotheses produced during a live
//! recording into stable display text (#41).
//!
//! Every streaming pass re-transcribes the whole take, so consecutive
//! hypotheses mostly agree but keep rewriting their tail while Whisper settles.
//! The stabilizer splits each hypothesis into `committed` text — words that
//! stayed identical across enough consecutive passes — and a `pending` tail
//! that is rendered dimmed. `committed` is append-only within a session: once
//! shown as stable it never changes or shrinks, even if a later hypothesis
//! disagrees (the final post-stop pass overrides the whole display anyway).

use std::ops::Range;

pub(crate) struct StabilizedText {
    pub committed: String,
    pub pending: String,
}

pub(crate) struct TranscriptStabilizer {
    committed: String,
    committed_tokens: Vec<String>,
    /// Agreement state for the hypothesis tail beyond the committed prefix:
    /// each entry is (token, consecutive passes it was observed unchanged at
    /// this position). A change at one position resets every later entry,
    /// because a shifted word re-times everything after it.
    tail_agreement: Vec<(String, u32)>,
    /// The last N tokens of a hypothesis never commit, no matter how stable —
    /// Whisper keeps revising the most recent words the longest.
    holdback: usize,
    /// Consecutive observations required before a token may commit.
    required_passes: u32,
}

impl TranscriptStabilizer {
    pub(crate) fn new(holdback: usize, required_passes: u32) -> Self {
        Self {
            committed: String::new(),
            committed_tokens: Vec::new(),
            tail_agreement: Vec::new(),
            holdback,
            required_passes: required_passes.max(1),
        }
    }

    /// Adjusts the holdback while a session runs. The worker drops it to 0
    /// during trailing silence — once the user stops speaking there is
    /// nothing left that Whisper would still revise, and the remaining
    /// dimmed tail should finish committing (#41 feedback).
    pub(crate) fn set_holdback(&mut self, holdback: usize) {
        self.holdback = holdback;
    }

    pub(crate) fn observe(&mut self, hypothesis: &str) -> StabilizedText {
        let spans = tokenize_spans(hypothesis);
        let tokens: Vec<&str> = spans.iter().map(|span| &hypothesis[span.clone()]).collect();

        let prefix_len = self
            .committed_tokens
            .iter()
            .zip(tokens.iter())
            .take_while(|(committed, token)| committed.as_str() == **token)
            .count();

        if prefix_len < self.committed_tokens.len() {
            // The hypothesis contradicts already-committed words. Committed
            // stays frozen (monotonicity guarantee); everything after the
            // agreeing prefix becomes pending and stability tracking restarts.
            self.tail_agreement = tokens[prefix_len..]
                .iter()
                .map(|token| ((*token).to_owned(), 1))
                .collect();
            let pending = spans
                .get(prefix_len)
                .map(|span| hypothesis[span.start..].trim_end().to_owned())
                .unwrap_or_default();
            return self.result(pending);
        }

        let tail = &tokens[self.committed_tokens.len()..];
        let tail_spans = &spans[self.committed_tokens.len()..];

        // Positional agreement update: identical token extends its streak; the
        // first difference resets that position and every one after it.
        let mut invalidated = false;
        for (index, token) in tail.iter().enumerate() {
            let matches = !invalidated
                && self
                    .tail_agreement
                    .get(index)
                    .is_some_and(|(seen, _)| seen == token);
            if matches {
                self.tail_agreement[index].1 += 1;
            } else {
                invalidated = true;
                let entry = ((*token).to_owned(), 1);
                if index < self.tail_agreement.len() {
                    self.tail_agreement[index] = entry;
                } else {
                    self.tail_agreement.push(entry);
                }
            }
        }
        self.tail_agreement.truncate(tail.len());

        let stable = self
            .tail_agreement
            .iter()
            .take_while(|(_, count)| *count >= self.required_passes)
            .count();
        let commit_count = stable.min(tail.len().saturating_sub(self.holdback));

        if commit_count > 0 {
            // Append the original byte slice (not re-joined tokens) so the
            // hypothesis' own spacing and punctuation survive verbatim.
            let start = tail_spans[0].start;
            let end = tail_spans[commit_count - 1].end;
            append_with_join(&mut self.committed, &hypothesis[start..end]);
            self.committed_tokens
                .extend(tail[..commit_count].iter().map(|token| (*token).to_owned()));
            self.tail_agreement.drain(..commit_count);
        }

        let pending = tail_spans
            .get(commit_count)
            .map(|span| hypothesis[span.start..].trim_end().to_owned())
            .unwrap_or_default();
        self.result(pending)
    }

    fn result(&self, pending: String) -> StabilizedText {
        StabilizedText {
            committed: self.committed.clone(),
            pending,
        }
    }
}

/// Appends `slice` to `committed`, inserting a single space unless the join is
/// CJK-to-CJK (scripts written without word spacing).
fn append_with_join(committed: &mut String, slice: &str) {
    if committed.is_empty() {
        committed.push_str(slice);
        return;
    }
    let cjk_join = committed.chars().next_back().is_some_and(is_cjk)
        && slice.chars().next().is_some_and(is_cjk);
    if !cjk_join {
        committed.push(' ');
    }
    committed.push_str(slice);
}

/// Byte ranges of the stabilizer's tokens within `text`: whitespace-separated
/// runs, except that every CJK character is its own token so scripts without
/// word spacing still stabilize at a useful granularity instead of forming one
/// giant token that never settles.
fn tokenize_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(run_start) = start.take() {
                spans.push(run_start..index);
            }
            continue;
        }
        if is_cjk(ch) {
            if let Some(run_start) = start.take() {
                spans.push(run_start..index);
            }
            spans.push(index..index + ch.len_utf8());
            continue;
        }
        if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(run_start) = start {
        spans.push(run_start..text.len());
    }
    spans
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{AC00}'..='\u{D7AF}' // Hangul syllables
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLDBACK: usize = 4;
    const REQUIRED: u32 = 2;

    fn stabilizer() -> TranscriptStabilizer {
        TranscriptStabilizer::new(HOLDBACK, REQUIRED)
    }

    #[test]
    fn first_pass_commits_nothing() {
        let mut stab = stabilizer();
        let out = stab.observe("one two three four five six");
        assert_eq!(out.committed, "");
        assert_eq!(out.pending, "one two three four five six");
    }

    #[test]
    fn steady_growth_commits_all_but_holdback() {
        let mut stab = stabilizer();
        let hypothesis = "one two three four five six seven eight";
        stab.observe(hypothesis);
        let out = stab.observe(hypothesis);
        assert_eq!(out.committed, "one two three four");
        assert_eq!(out.pending, "five six seven eight");
    }

    #[test]
    fn holdback_survives_many_stable_passes() {
        let mut stab = stabilizer();
        let hypothesis = "one two three four five six seven eight";
        let mut out = stab.observe(hypothesis);
        for _ in 0..10 {
            out = stab.observe(hypothesis);
        }
        // The last HOLDBACK words never commit while the text stops growing.
        assert_eq!(out.committed, "one two three four");
        assert_eq!(out.pending, "five six seven eight");
    }

    #[test]
    fn flickering_tail_word_never_commits() {
        let mut stab = stabilizer();
        for _ in 0..4 {
            stab.observe("one two three four five fo");
            stab.observe("one two three four five fox");
        }
        let out = stab.observe("one two three four five fox");
        assert!(!out.committed.contains("fo"));
        assert!(out.pending.ends_with("fox"));
    }

    #[test]
    fn correction_before_stability_wins() {
        let mut stab = stabilizer();
        stab.observe("a b c d e f six");
        let out = stab.observe("a b c d e f seven eight");
        // "six" was only ever seen once and is replaced by the correction.
        assert_eq!(out.committed, "a b c d");
        assert_eq!(out.pending, "e f seven eight");
    }

    #[test]
    fn mid_tail_change_invalidates_followers() {
        let mut stab = stabilizer();
        stab.observe("a b c d e f g h");
        let out = stab.observe("a b x d e f g h");
        // The change at position 2 resets d..h too, so nothing beyond the
        // stable prefix commits even though d..h textually matched pass 1.
        assert_eq!(out.committed, "a b");
        let out = stab.observe("a b x d e f g h");
        assert_eq!(out.committed, "a b x d");
    }

    #[test]
    fn contradiction_freezes_committed() {
        let mut stab = stabilizer();
        let hypothesis = "the quick brown fox jumps over the dog";
        stab.observe(hypothesis);
        let before = stab.observe(hypothesis).committed;
        assert_eq!(before, "the quick brown fox");

        let out = stab.observe("a quick brown fox jumps over the dog");
        assert_eq!(out.committed, before, "committed must never change");
        assert_eq!(out.pending, "a quick brown fox jumps over the dog");
    }

    #[test]
    fn committed_is_monotonic_prefix_across_noisy_sequence() {
        let script = [
            "hello",
            "hello world",
            "hello world this",
            "hello world this is",
            "hello world this is a longer test",
            "hello world this is a longer test sentence",
            "hello word this is a longer test sentence",
            "hello world this is a longer test sentence for you",
            "hello world this is a longer test sentence for you today",
        ];
        let mut stab = stabilizer();
        let mut previous = String::new();
        for hypothesis in script {
            let out = stab.observe(hypothesis);
            assert!(
                out.committed.starts_with(&previous),
                "'{}' is not a prefix of '{}'",
                previous,
                out.committed
            );
            previous = out.committed;
        }
    }

    #[test]
    fn empty_hypothesis_keeps_committed() {
        let mut stab = stabilizer();
        let hypothesis = "one two three four five six seven eight";
        stab.observe(hypothesis);
        let before = stab.observe(hypothesis).committed;

        let out = stab.observe("");
        assert_eq!(out.committed, before);
        assert_eq!(out.pending, "");
    }

    #[test]
    fn punctuation_flip_resets_streak() {
        let mut stab = stabilizer();
        for _ in 0..4 {
            stab.observe("so we go there now quickly");
            stab.observe("So, we go there now quickly");
        }
        // The first token alternates between "so" and "So," — its streak never
        // reaches REQUIRED, so nothing behind it can commit either.
        let out = stab.observe("so we go there now quickly");
        assert_eq!(out.committed, "");
    }

    #[test]
    fn tokenizer_handles_extra_whitespace() {
        let spans = tokenize_spans("  hello   world \n");
        let text = "  hello   world \n";
        let tokens: Vec<&str> = spans.iter().map(|s| &text[s.clone()]).collect();
        assert_eq!(tokens, vec!["hello", "world"]);
    }

    #[test]
    fn tokenizer_splits_cjk_per_character() {
        let text = "今日はhello天気";
        let spans = tokenize_spans(text);
        let tokens: Vec<&str> = spans.iter().map(|s| &text[s.clone()]).collect();
        assert_eq!(tokens, vec!["今", "日", "は", "hello", "天", "気"]);
    }

    #[test]
    fn cjk_commits_join_without_spaces() {
        let mut stab = stabilizer();
        let hypothesis = "今日は天気がいい";
        stab.observe(hypothesis);
        stab.observe(hypothesis);
        // 8 CJK tokens, holdback 4 → first 4 characters commit as one run.
        let out = stab.observe(hypothesis);
        assert_eq!(out.committed, "今日は天");
        assert!(!out.committed.contains(' '));
    }

    #[test]
    fn zero_holdback_commits_the_stable_tail() {
        let mut stab = stabilizer();
        let hypothesis = "one two three four five six seven eight";
        stab.observe(hypothesis);
        let out = stab.observe(hypothesis);
        assert_eq!(out.pending, "five six seven eight");

        // Trailing silence: the worker lifts the holdback; the already-stable
        // tail commits on the next pass and nothing stays dimmed.
        stab.set_holdback(0);
        let out = stab.observe(hypothesis);
        assert_eq!(out.committed, hypothesis);
        assert_eq!(out.pending, "");
    }

    #[test]
    fn append_with_join_rules() {
        let mut committed = String::new();
        append_with_join(&mut committed, "hello");
        assert_eq!(committed, "hello");
        append_with_join(&mut committed, "world");
        assert_eq!(committed, "hello world");

        let mut cjk = String::from("今");
        append_with_join(&mut cjk, "日");
        assert_eq!(cjk, "今日");
        append_with_join(&mut cjk, "hello");
        assert_eq!(cjk, "今日 hello");

        let mut latin = String::from("hello");
        append_with_join(&mut latin, "今");
        assert_eq!(latin, "hello 今");
    }
}
