//! Turns the stream of Whisper hypotheses produced during a live recording
//! into stable display text (#41).
//!
//! Every streaming pass re-transcribes the current decode window — the audio
//! since the last cut of the sliding window (#89) — so consecutive hypotheses
//! mostly agree but keep rewriting their tail while Whisper settles. The
//! stabilizer splits each hypothesis into `committed` text — words that stayed
//! identical across enough consecutive passes — and a `pending` tail that is
//! rendered dimmed. `committed` is append-only within a session: once shown as
//! stable it never changes or shrinks, even if a later hypothesis disagrees
//! (the final post-stop pass overrides the whole display anyway).
//!
//! When the worker moves its window past committed words, it archives them
//! ([`TranscriptStabilizer::archive_window`]): they stay in `committed`, but
//! later hypotheses, which no longer cover their audio, are only compared
//! against the committed words that are still inside the window.
//!
//! A lasting revision (#93): when Whisper keeps decoding committed window
//! words differently ("das" → "dass", "20" → "zwanzig"), the worker accepts
//! the new reading after a few contradicting passes
//! ([`TranscriptStabilizer::accept_revision`]). The display stays
//! append-only — the revised words keep their old form on screen until the
//! final pass — but later hypotheses are compared against the new reading,
//! so committing and window cuts continue.

use std::ops::Range;

/// Most leading tokens an overlapping re-decode after a segment cut may carry
/// before the committed window words. The worker overlaps by ~0.8 s, but
/// Whisper's segment end can be late by seconds, so the overlap may hold
/// several words more.
const MAX_OVERLAP_TOKENS: usize = 24;
/// Most committed window words a re-decode after a segment cut may lack at
/// its start (the cut clipped them despite the overlap). They are already on
/// screen and are archived instead of contradicting forever.
const MAX_CLIPPED_TOKENS: usize = 3;
/// Committed words that must line up for an alignment to count.
const MIN_ALIGNED_TOKENS: usize = 2;

pub(crate) struct StabilizedText {
    pub committed: String,
    pub pending: String,
    /// The hypothesis disagreed with committed words of the current window.
    /// The worker never moves its window in such a pass.
    pub contradicted: bool,
    /// Leading hypothesis tokens skipped as overlap with archived text; the
    /// worker discounts them when it maps segments to committed words.
    pub overlap: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct WindowState {
    base: usize,
    overlaps: bool,
}

pub(crate) struct TranscriptStabilizer {
    committed: String,
    /// The basis hypotheses are compared against. Mirrors the tokens of
    /// `committed` until a revision is accepted; from then on its window
    /// part follows Whisper's new reading and may differ from the text on
    /// screen.
    committed_tokens: Vec<String>,
    /// Committed tokens before this index belong to audio the worker already
    /// cut off; hypotheses start at `committed_tokens[window_base]`.
    window_base: usize,
    /// The window starts at a segment cut and re-decodes some archived words
    /// first; only then are leading hypothesis tokens skipped by alignment.
    window_overlaps: bool,
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
    /// The last pass contradicted the committed window words: the agreeing
    /// prefix length and the hypothesis tokens that replace the committed
    /// window words behind it, should the worker accept the revision.
    revision: Option<(usize, Vec<String>)>,
}

impl TranscriptStabilizer {
    pub(crate) fn new(holdback: usize, required_passes: u32) -> Self {
        Self {
            committed: String::new(),
            committed_tokens: Vec::new(),
            window_base: 0,
            window_overlaps: false,
            tail_agreement: Vec::new(),
            holdback,
            required_passes: required_passes.max(1),
            revision: None,
        }
    }

    /// Adjusts the holdback while a session runs. The worker drops it to 0
    /// during trailing silence — once the user stops speaking there is
    /// nothing left that Whisper would still revise, and the remaining
    /// dimmed tail should finish committing (#41 feedback).
    pub(crate) fn set_holdback(&mut self, holdback: usize) {
        self.holdback = holdback;
    }

    /// Committed tokens inside the current decode window.
    pub(crate) fn window_committed_count(&self) -> usize {
        self.committed_tokens.len() - self.window_base
    }

    /// The worker cut its decode window behind the first `consumed` committed
    /// tokens of the window: later hypotheses start after them. Never archives
    /// more than is committed. `overlapping` marks a window that starts before
    /// the cut and re-decodes archived words first (segment cut).
    pub(crate) fn archive_window(&mut self, consumed: usize, overlapping: bool) {
        self.window_base += consumed.min(self.window_committed_count());
        self.window_overlaps = overlapping;
    }

    /// Opaque window position, so the worker can undo a cut.
    pub(crate) fn window_state(&self) -> WindowState {
        WindowState {
            base: self.window_base,
            overlaps: self.window_overlaps,
        }
    }

    /// Undoes cuts back to `state`: the words archived since are compared
    /// against hypotheses again. Committed text is unaffected.
    pub(crate) fn restore_window(&mut self, state: WindowState) {
        self.window_base = state.base.min(self.window_base);
        self.window_overlaps = state.overlaps;
        // Streaks and a revision were recorded against the cut window.
        self.tail_agreement.clear();
        self.revision = None;
    }

    /// Accepts the reading of the last contradicting pass (#93): the
    /// committed window words behind the agreeing prefix are replaced by the
    /// hypothesis tokens that took their place, so later hypotheses are
    /// compared against the new reading and can commit again. `committed`,
    /// the text on screen, is untouched and stays append-only. Returns
    /// `false` without changing the basis when the last pass did not
    /// contradict or left the revised part open (e.g. an empty or too short
    /// hypothesis); the revision is consumed either way.
    pub(crate) fn accept_revision(&mut self) -> bool {
        let Some((prefix, replacement)) = self.revision.take() else {
            return false;
        };
        log::debug!(
            target: "dictation",
            "streaming revision: {} → {}",
            self.committed_tokens[self.window_base + prefix..].join(" "),
            replacement.join(" ")
        );
        self.committed_tokens.truncate(self.window_base + prefix);
        self.committed_tokens.extend(replacement);
        self.tail_agreement.clear();
        true
    }

    /// The last archived and first remaining window words around a cut of
    /// `consumed` words, e.g. `"… lange diktiert, | möchte trotzdem"` — for
    /// the worker's cut log.
    pub(crate) fn window_boundary_words(&self, consumed: usize) -> String {
        let window = &self.committed_tokens[self.window_base..];
        let cut = consumed.min(window.len());
        let before = &window[cut.saturating_sub(3)..cut];
        let after = &window[cut..(cut + 3).min(window.len())];
        format!("… {} | {}", before.join(" "), after.join(" "))
    }

    pub(crate) fn observe(&mut self, hypothesis: &str) -> StabilizedText {
        let all_spans = tokenize_spans(hypothesis);
        let all_tokens: Vec<&str> = all_spans
            .iter()
            .map(|span| &hypothesis[span.clone()])
            .collect();
        // A window that starts at a segment cut overlaps the archived text;
        // skip those leading tokens when the committed window words line up
        // behind them. Committed words the cut clipped are archived.
        let (clipped, lead) = align_window(
            &self.committed_tokens[self.window_base..],
            &all_tokens,
            self.window_overlaps,
        );
        self.window_base += clipped;
        let window_committed = &self.committed_tokens[self.window_base..];
        let spans = &all_spans[lead..];
        let tokens = &all_tokens[lead..];

        // Tolerant: a re-decode that starts at a window cut may capitalize or
        // punctuate the same words differently ("Mit" vs "mit,"); that is
        // agreement, not a contradiction. The tail streaks stay exact.
        let prefix_len = window_committed
            .iter()
            .zip(tokens.iter())
            .take_while(|(committed, token)| same_word(committed, token))
            .count();

        if prefix_len < window_committed.len() {
            // The hypothesis contradicts already-committed words. Committed
            // stays frozen (monotonicity guarantee); everything after the
            // agreeing prefix becomes pending and stability tracking restarts.
            // The revised reading is kept in case the worker accepts it.
            self.revision = revision_end(window_committed, tokens, prefix_len).map(|end| {
                let replacement = tokens[prefix_len..end]
                    .iter()
                    .map(|token| (*token).to_owned())
                    .collect();
                (prefix_len, replacement)
            });
            self.tail_agreement = tokens[prefix_len..]
                .iter()
                .map(|token| ((*token).to_owned(), 1))
                .collect();
            let pending = spans
                .get(prefix_len)
                .map(|span| hypothesis[span.start..].trim_end().to_owned())
                .unwrap_or_default();
            return self.result(pending, true, lead);
        }

        self.revision = None;
        let in_window = window_committed.len();
        let tail = &tokens[in_window..];
        let tail_spans = &spans[in_window..];

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
        self.result(pending, false, lead)
    }

    fn result(&self, pending: String, contradicted: bool, overlap: usize) -> StabilizedText {
        StabilizedText {
            committed: self.committed.clone(),
            pending,
            contradicted,
            overlap,
        }
    }
}

/// How a hypothesis after a segment cut lines up with the committed window
/// words: `(clipped, lead)` — skip the first `clipped` committed words (the
/// cut lost their audio) and the first `lead` hypothesis tokens (overlap with
/// archived text), so that every remaining committed word (at least
/// [`MIN_ALIGNED_TOKENS`]) matches. Prefers losing nothing, then the shortest
/// skip. When nothing lines up (a real contradiction), `lead` still skips to
/// the longest partial match, so the pending text does not repeat archived
/// words. `(0, 0)` for a window without overlap.
fn align_window(window_committed: &[String], tokens: &[&str], overlaps: bool) -> (usize, usize) {
    if !overlaps || window_committed.len() < MIN_ALIGNED_TOKENS {
        return (0, 0);
    }
    let matched = |anchor: &[String], lead: usize| {
        anchor
            .iter()
            .zip(&tokens[lead..])
            .take_while(|(committed, token)| same_word(committed, token))
            .count()
    };
    let max_lead = MAX_OVERLAP_TOKENS.min(tokens.len());
    for clipped in 0..=MAX_CLIPPED_TOKENS.min(window_committed.len()) {
        let anchor = &window_committed[clipped..];
        if anchor.len() < MIN_ALIGNED_TOKENS {
            break;
        }
        if let Some(lead) = (0..=max_lead).find(|&lead| matched(anchor, lead) == anchor.len()) {
            return (clipped, lead);
        }
    }
    let partial_lead = (0..=max_lead)
        .map(|lead| (matched(window_committed, lead), lead))
        .filter(|&(count, _)| count >= MIN_ALIGNED_TOKENS)
        .max_by_key(|&(count, lead)| (count, std::cmp::Reverse(lead)))
        .map_or(0, |(_, lead)| lead);
    (0, partial_lead)
}

/// Exclusive end of the hypothesis part that took the place of the committed
/// window words in a contradicting pass, behind the agreeing `prefix`. Whisper
/// may have revised a word, or inserted or dropped some, so the end is found
/// at the last two committed words: the match nearest to the committed length
/// (the earlier one on a tie), at most [`MAX_OVERLAP_TOKENS`] away. Without
/// such a match:
/// - `None` while the hypothesis is shorter than the committed words: it
///   cannot tell yet what replaced them, and guessing would let words that
///   are on screen already commit a second time;
/// - `prefix` when two or more committed words remain and none of them
///   occurs in their place: Whisper dropped them (e.g. the start of a window
///   after a cut), and the words that follow are new;
/// - otherwise the committed length (a revised word, "20" → "zwanzig").
fn revision_end(window_committed: &[String], tokens: &[&str], prefix: usize) -> Option<usize> {
    let committed = window_committed.len();
    if committed >= MIN_ALIGNED_TOKENS {
        let anchor = &window_committed[committed - MIN_ALIGNED_TOKENS..];
        let anchored = ((prefix + 1).max(MIN_ALIGNED_TOKENS)..=tokens.len())
            .filter(|&end| end.abs_diff(committed) <= MAX_OVERLAP_TOKENS)
            .filter(|&end| {
                anchor
                    .iter()
                    .zip(&tokens[end - MIN_ALIGNED_TOKENS..end])
                    .all(|(committed, token)| same_word(committed, token))
            })
            .min_by_key(|&end| (end.abs_diff(committed), end));
        if anchored.is_some() {
            return anchored;
        }
    }
    if tokens.len() < committed {
        return None;
    }
    let revised = &window_committed[prefix..];
    let in_their_place = &tokens[prefix..committed];
    let dropped = revised.len() >= MIN_ALIGNED_TOKENS
        && !revised
            .iter()
            .any(|word| in_their_place.iter().any(|token| same_word(word, token)));
    Some(if dropped { prefix } else { committed })
}

/// Number of stabilizer tokens in `text` — what one segment of a hypothesis
/// contributes, so the worker can map segment boundaries to token counts.
pub(crate) fn token_count(text: &str) -> usize {
    tokenize_spans(text).len()
}

/// Word equality for the committed prefix: case-insensitive and ignoring
/// punctuation at either end; a token that is only punctuation compares raw.
fn same_word(committed: &str, token: &str) -> bool {
    committed == token || normalized_word(committed) == normalized_word(token)
}

fn normalized_word(token: &str) -> String {
    let trimmed = token.trim_matches(|ch: char| ch.is_ascii_punctuation() || is_unicode_quote(ch));
    if trimmed.is_empty() {
        token.to_owned()
    } else {
        trimmed.to_lowercase()
    }
}

fn is_unicode_quote(ch: char) -> bool {
    matches!(
        ch,
        '„' | '“' | '”' | '‚' | '‘' | '’' | '«' | '»' | '…' | '–' | '—' | '¿' | '¡'
    )
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
    fn archived_window_continues_seamlessly() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        let first = "one two three four";
        stab.observe(first);
        let out = stab.observe(first);
        assert_eq!(out.committed, first);
        assert_eq!(stab.window_committed_count(), 4);

        // Pause cut: the next window holds only the new audio.
        stab.archive_window(4, false);
        assert_eq!(stab.window_committed_count(), 0);
        stab.observe("five six");
        let out = stab.observe("five six");
        assert!(!out.contradicted);
        assert_eq!(out.committed, "one two three four five six");
        assert_eq!(out.pending, "");
    }

    #[test]
    fn partial_archive_keeps_the_rest_of_the_window() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        stab.observe("one two. three four");
        stab.observe("one two. three four");
        // Segment cut after "two.": "three four" stay committed inside the
        // window and the next hypothesis starts with them.
        stab.archive_window(2, true);
        assert_eq!(stab.window_committed_count(), 2);
        stab.observe("three four five");
        let out = stab.observe("three four five");
        assert!(!out.contradicted);
        assert_eq!(out.committed, "one two. three four five");
    }

    #[test]
    fn archive_never_exceeds_committed() {
        let mut stab = stabilizer();
        stab.archive_window(10, false);
        assert_eq!(stab.window_committed_count(), 0);
        let out = stab.observe("a b c d e");
        assert!(!out.contradicted);
        assert_eq!(out.pending, "a b c d e");
    }

    #[test]
    fn case_and_comma_change_at_window_start_is_no_contradiction() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        stab.observe("hello world. mit lokaler");
        stab.observe("hello world. mit lokaler");
        stab.archive_window(2, true);
        // The re-decode from the cut capitalizes and punctuates differently.
        let out = stab.observe("Mit, lokaler Verarbeitung");
        assert!(!out.contradicted);
        assert_eq!(out.committed, "hello world. mit lokaler");
        assert_eq!(out.pending, "Verarbeitung");
    }

    #[test]
    fn overlapping_redecode_after_a_cut_is_aligned() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        let first = "wer lange diktiert, möchte trotzdem eine Vorschau";
        stab.observe(first);
        stab.observe(first);
        // Segment cut after "diktiert,"; the new window starts ~0.8 s early
        // and re-decodes "lange diktiert," (plus a fragment) before the
        // committed words that stay in the window.
        stab.archive_window(3, true);
        let overlapping = "ge lange diktiert, möchte trotzdem eine Vorschau sehen";
        let out = stab.observe(overlapping);
        assert!(!out.contradicted);
        assert_eq!(out.overlap, 3);
        assert_eq!(out.pending, "sehen");
        let out = stab.observe(overlapping);
        assert_eq!(out.committed, format!("{first} sehen"));
        assert_eq!(out.pending, "");
    }

    #[test]
    fn alignment_needs_a_cut_and_a_full_match() {
        let committed = vec![
            "möchte".to_owned(),
            "trotzdem".to_owned(),
            "eine".to_owned(),
        ];
        let hypothesis = ["x", "möchte", "trotzdem", "eine", "neu"];
        assert_eq!(align_window(&committed, &hypothesis, true), (0, 1));
        assert_eq!(align_window(&committed, &hypothesis, false), (0, 0));
        // No full match: the lead skips to a partial match of two words.
        assert_eq!(
            align_window(&committed, &["x", "möchte", "trotzdem"], true),
            (0, 1)
        );
        assert_eq!(align_window(&committed, &["x", "möchte"], true), (0, 0));
        assert_eq!(align_window(&[], &["x", "y"], true), (0, 0));
        let mut far = vec!["w"; MAX_OVERLAP_TOKENS + 1];
        far.extend(["möchte", "trotzdem", "eine"]);
        assert_eq!(
            align_window(&committed, &far, true),
            (0, 0),
            "beyond the overlap"
        );
    }

    #[test]
    fn alignment_archives_words_the_cut_clipped() {
        let committed = vec![
            "diktiert,".to_owned(),
            "möchte".to_owned(),
            "trotzdem".to_owned(),
        ];
        // The re-decode lost "diktiert," — the rest still lines up.
        assert_eq!(
            align_window(&committed, &["möchte", "trotzdem", "eine"], true),
            (1, 0)
        );
        // Never down to a single matching word.
        let short = vec!["a".to_owned(), "b".to_owned()];
        assert_eq!(align_window(&short, &["b", "c"], true), (0, 0));
    }

    #[test]
    fn clipped_words_after_a_cut_stay_on_screen() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        let first = "wer lange diktiert, möchte trotzdem eine Vorschau";
        stab.observe(first);
        stab.observe(first);
        stab.archive_window(2, true);
        let clipped = "möchte trotzdem eine Vorschau sehen";
        let out = stab.observe(clipped);
        assert!(!out.contradicted);
        assert_eq!(stab.window_committed_count(), 4);
        let out = stab.observe(clipped);
        assert_eq!(out.committed, format!("{first} sehen"));
    }

    #[test]
    fn contradiction_after_a_segment_cut_does_not_repeat_archived_words() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        let first = "wer lange diktiert, möchte trotzdem eine Vorschau";
        stab.observe(first);
        stab.observe(first);
        stab.archive_window(3, true);
        let out = stab.observe("lange diktiert, möchte trotzdem keine Vorschau sehen");
        assert!(out.contradicted);
        assert_eq!(out.committed, first);
        // Without the partial alignment, "lange diktiert," would show twice.
        assert_eq!(out.pending, "keine Vorschau sehen");
    }

    #[test]
    fn revision_after_a_pause_cut_is_a_contradiction_not_an_overlap() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        stab.observe("eins zwei");
        stab.observe("eins zwei");
        stab.archive_window(2, false);
        stab.observe("die Daten");
        stab.observe("die Daten");
        // Whisper revises the window start: the new words must stay visible.
        let out = stab.observe("Sie haben die Daten gesichert");
        assert!(out.contradicted);
        assert_eq!(out.committed, "eins zwei die Daten");
        assert_eq!(out.pending, "Sie haben die Daten gesichert");
    }

    #[test]
    fn restore_window_undoes_a_cut() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        stab.observe("a b c d e f");
        stab.observe("a b c d e f");
        let before = stab.window_state();
        stab.archive_window(2, true);
        assert_eq!(stab.window_committed_count(), 4);
        stab.restore_window(before);
        assert_eq!(stab.window_committed_count(), 6);
        let out = stab.observe("a b c d e f g");
        assert!(!out.contradicted);
        assert_eq!(out.overlap, 0);
    }

    #[test]
    fn real_disagreement_in_the_window_is_reported() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        stab.observe("one two three");
        stab.observe("one two three");
        let out = stab.observe("one too three");
        assert!(out.contradicted);
        assert_eq!(out.committed, "one two three");
    }

    fn owned(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn revision_end_finds_the_revised_part() {
        // A revised word: the last two committed words line up behind it.
        assert_eq!(
            revision_end(
                &owned(&["x", "das", "ist", "gut"]),
                &["x", "dass", "ist", "gut", "und"],
                1
            ),
            Some(4)
        );
        // The revised word is one of the last two: the committed length.
        assert_eq!(
            revision_end(&owned(&["x", "y", "20"]), &["x", "y", "zwanzig", "Euro"], 2),
            Some(3)
        );
        // Words inserted before the committed ones belong to the revision.
        assert_eq!(
            revision_end(
                &owned(&["die", "Daten"]),
                &["Sie", "haben", "die", "Daten", "gesichert"],
                0
            ),
            Some(4)
        );
        // An anchor match too far away falls back: here the committed
        // words are gone from their place, so Whisper dropped them.
        let mut far = vec!["w"; MAX_OVERLAP_TOKENS + 1];
        far.extend(["die", "Daten"]);
        assert_eq!(revision_end(&owned(&["die", "Daten"]), &far, 0), Some(0));
        // A shorter hypothesis leaves the revised part open.
        assert_eq!(revision_end(&owned(&["a", "b", "c"]), &["a"], 1), None);
        assert_eq!(
            revision_end(
                &owned(&["x", "das", "ist", "gut", "und", "dann"]),
                &["x", "dass", "ist"],
                1
            ),
            None
        );
        // A partly revised stretch keeps the committed length ...
        assert_eq!(
            revision_end(
                &owned(&["x", "das", "ist", "gut"]),
                &["x", "dass", "ist", "guten", "Tag"],
                1
            ),
            Some(4)
        );
        // ... words missing from their place were dropped by Whisper.
        assert_eq!(
            revision_end(
                &owned(&["die", "direkt", "auf", "dem", "Rechner"]),
                &["die", "Sprache", "wird", "nicht", "gesendet", "auf", "dem"],
                1
            ),
            Some(1)
        );
    }

    #[test]
    fn accepted_revision_lets_new_words_commit() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        let first = "x das ist gut";
        stab.observe(first);
        stab.observe(first);
        let revised = "x dass ist gut und weiter";
        let out = stab.observe(revised);
        assert!(out.contradicted);
        assert!(stab.accept_revision());
        assert_eq!(stab.window_committed_count(), 4);
        // Taken: a second call has nothing left to accept.
        assert!(!stab.accept_revision());

        let out = stab.observe(revised);
        assert!(!out.contradicted);
        assert_eq!(out.committed, first, "the screen keeps the old reading");
        assert_eq!(out.pending, "und weiter");
        let out = stab.observe(revised);
        assert_eq!(out.committed, format!("{first} und weiter"));
        assert_eq!(out.pending, "");
    }

    #[test]
    fn nothing_to_accept_without_a_revised_reading() {
        let mut stab = stabilizer();
        stab.set_holdback(0);
        stab.observe("one two three");
        stab.observe("one two three");
        // An empty hypothesis contradicts but leaves the revision open.
        assert!(stab.observe("").contradicted);
        assert!(!stab.accept_revision());
        assert_eq!(stab.window_committed_count(), 3);

        // An agreeing pass clears the revision of the pass before.
        assert!(stab.observe("one too three").contradicted);
        assert!(!stab.observe("one two three four").contradicted);
        assert!(!stab.accept_revision());

        // So does undoing a cut.
        let before = stab.window_state();
        assert!(stab.observe("one too three four").contradicted);
        stab.restore_window(before);
        assert!(!stab.accept_revision());
        let out = stab.observe("one two three four");
        assert!(!out.contradicted);
    }

    #[test]
    fn token_count_matches_the_tokenizer() {
        assert_eq!(token_count("  Hallo, Welt!  "), 2);
        assert_eq!(token_count("今日は"), 3);
        assert_eq!(token_count(""), 0);
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
