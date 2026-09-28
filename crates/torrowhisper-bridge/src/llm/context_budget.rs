//! Splits a dictation into pieces that each fit a language model's context.
//!
//! Apple's on-device model has a small context window (8192 tokens on
//! macOS 26.4). A long dictation plus the mode prompt and room for the answer
//! can exceed it, and the request then fails outright. Instead we cut the
//! transcript at sentence boundaries into chunks that fit and post-process
//! them one by one. Pure logic with an injected token counter, so it is
//! testable without the model.

/// One piece of the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// The text to post-process, without its trailing whitespace.
    pub text: String,
    /// The whitespace that followed the text in the transcript. Appended
    /// verbatim after the processed text so paragraphs and line breaks
    /// survive the split.
    pub separator: String,
    /// False when a single sentence alone exceeds the budget. Such a chunk is
    /// passed through unchanged rather than sent to the model.
    pub fits: bool,
}

/// Splits `text` into chunks whose token count stays within `budget`.
///
/// `count` returns the token count of a piece of text, or `None` when it
/// cannot be determined; any `None` aborts the split and the caller falls
/// back to a single request. Sentence counts are summed rather than
/// recounted as a whole, which slightly overestimates and so errs on the safe
/// side.
pub(crate) fn split_to_fit(
    text: &str,
    budget: usize,
    mut count: impl FnMut(&str) -> Option<usize>,
) -> Option<Vec<Chunk>> {
    let trimmed = text.trim_end();
    let trailing = &text[trimmed.len()..];
    if count(trimmed)? <= budget {
        return Some(vec![Chunk {
            text: trimmed.to_owned(),
            separator: trailing.to_owned(),
            fits: true,
        }]);
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0;
    for sentence in sentences(text) {
        let tokens = count(sentence.trim_end())?;
        if tokens > budget {
            flush(&mut chunks, &mut current, &mut current_tokens);
            let body = sentence.trim_end();
            chunks.push(Chunk {
                text: body.to_owned(),
                separator: sentence[body.len()..].to_owned(),
                fits: false,
            });
            continue;
        }
        if !current.is_empty() && current_tokens + tokens > budget {
            flush(&mut chunks, &mut current, &mut current_tokens);
        }
        current.push_str(sentence);
        current_tokens += tokens;
    }
    flush(&mut chunks, &mut current, &mut current_tokens);
    Some(chunks)
}

fn flush(chunks: &mut Vec<Chunk>, current: &mut String, current_tokens: &mut usize) {
    if current.is_empty() {
        return;
    }
    let body = current.trim_end();
    chunks.push(Chunk {
        text: body.to_owned(),
        separator: current[body.len()..].to_owned(),
        fits: true,
    });
    current.clear();
    *current_tokens = 0;
}

/// Splits after sentence-ending punctuation (`.`, `!`, `?`, `…`) followed by
/// whitespace, and at line breaks. Each piece keeps its trailing whitespace,
/// so the pieces concatenate back to the original text.
fn sentences(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let is_break = ch == '\n'
            || (matches!(ch, '.' | '!' | '?' | '…')
                && chars.peek().is_some_and(|(_, next)| next.is_whitespace()));
        if !is_break {
            continue;
        }
        let mut end = index + ch.len_utf8();
        while let Some(&(next_index, next)) = chars.peek() {
            if !next.is_whitespace() {
                break;
            }
            end = next_index + next.len_utf8();
            chars.next();
        }
        pieces.push(&text[start..end]);
        start = end;
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One token per word keeps the expectations readable.
    fn words(text: &str) -> Option<usize> {
        Some(text.split_whitespace().count())
    }

    fn joined(chunks: &[Chunk]) -> String {
        chunks
            .iter()
            .map(|chunk| format!("{}{}", chunk.text, chunk.separator))
            .collect()
    }

    #[test]
    fn text_within_budget_stays_one_chunk() {
        let chunks = split_to_fit("Hallo Welt. Wie geht es dir?\n", 10, words).unwrap();
        assert_eq!(
            chunks,
            vec![Chunk {
                text: "Hallo Welt. Wie geht es dir?".to_owned(),
                separator: "\n".to_owned(),
                fits: true,
            }]
        );
    }

    #[test]
    fn long_text_is_split_at_sentence_boundaries() {
        let text = "Eins zwei drei. Vier fünf sechs! Sieben acht neun? Zehn elf.";
        let chunks = split_to_fit(text, 6, words).unwrap();
        let texts: Vec<_> = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Eins zwei drei. Vier fünf sechs!",
                "Sieben acht neun? Zehn elf.",
            ]
        );
        assert!(chunks.iter().all(|chunk| chunk.fits));
        assert_eq!(joined(&chunks), text);
    }

    #[test]
    fn paragraphs_survive_the_split() {
        let text = "Erster Absatz hier.\n\nZweiter Absatz folgt.";
        let chunks = split_to_fit(text, 3, words).unwrap();
        assert_eq!(chunks[0].text, "Erster Absatz hier.");
        assert_eq!(chunks[0].separator, "\n\n");
        assert_eq!(joined(&chunks), text);
    }

    #[test]
    fn oversized_sentence_is_passed_through() {
        let text = "Kurz. Dieser Satz ist viel zu lang für das Budget. Auch kurz.";
        let chunks = split_to_fit(text, 3, words).unwrap();
        let flags: Vec<_> = chunks
            .iter()
            .map(|chunk| (chunk.text.as_str(), chunk.fits))
            .collect();
        assert_eq!(
            flags,
            vec![
                ("Kurz.", true),
                ("Dieser Satz ist viel zu lang für das Budget.", false),
                ("Auch kurz.", true),
            ]
        );
        assert_eq!(joined(&chunks), text);
    }

    #[test]
    fn decimal_points_and_abbreviations_without_space_do_not_split() {
        assert_eq!(
            sentences("Version 0.10.0 ist da."),
            vec!["Version 0.10.0 ist da."]
        );
    }

    #[test]
    fn unknown_token_count_aborts_the_split() {
        assert_eq!(split_to_fit("Eins. Zwei.", 1, |_| None), None);
    }
}
