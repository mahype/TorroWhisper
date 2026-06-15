//! Turns an assistant answer into text that sounds natural when read aloud.
//!
//! Models (and especially tool-using agents) often reply with markdown, bullet
//! points and abbreviations. That reads fine on screen but is jarring through
//! text-to-speech. [`normalize_for_speech`] strips the formatting, expands
//! common German/English abbreviations, drops URLs/code, and flattens lists into
//! flowing sentences — so the TTS speaks prose, not punctuation. The system
//! prompt asks the model for spoken prose in the first place; this is the
//! model-independent safety net applied right before synthesis.

use std::sync::OnceLock;

use regex::Regex;

/// Normalizes `input` for speech. Idempotent and cheap; safe to run per sentence
/// during streaming.
pub fn normalize_for_speech(input: &str) -> String {
    let mut text = input.replace("\r\n", "\n");

    // Drop fenced code blocks entirely — reading code aloud is useless.
    text = re_fenced().replace_all(&text, " ").into_owned();
    // Images before links: ![alt](url) → drop.
    text = re_image().replace_all(&text, " ").into_owned();
    // Links [label](url) → label.
    text = re_link().replace_all(&text, "$label").into_owned();
    // Bare URLs → drop (don't spell out http colon slash slash …).
    text = re_url().replace_all(&text, " ").into_owned();
    // Inline code `x` → x.
    text = text.replace('`', "");

    // Per line: strip block markers (headings, quotes, list bullets), flatten to
    // sentences.
    let mut sentences: Vec<String> = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim().to_owned();
        if line.is_empty() || is_horizontal_rule(&line) {
            continue;
        }
        // Heading markers.
        line = line.trim_start_matches('#').trim_start().to_owned();
        // Blockquote markers (possibly nested).
        while let Some(rest) = line.strip_prefix('>') {
            line = rest.trim_start().to_owned();
        }
        // List bullet / numbered marker at the start.
        line = re_list_marker().replace(&line, "").into_owned();
        // Table cell pipes → spaces.
        line = line.replace('|', " ");
        // Emphasis / leftover markdown punctuation.
        line = line
            .replace("**", "")
            .replace("__", "")
            .replace("~~", "")
            .replace('*', "")
            .replace('_', "")
            .replace('#', "");
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut sentence = trimmed.to_owned();
        // Each former line becomes its own spoken sentence.
        if !ends_with_sentence_punct(&sentence) {
            sentence.push('.');
        }
        sentences.push(sentence);
    }

    let mut joined = sentences.join(" ");
    joined = expand_abbreviations(&joined);
    re_spaces().replace_all(&joined, " ").trim().to_owned()
}

fn ends_with_sentence_punct(s: &str) -> bool {
    matches!(s.chars().last(), Some('.' | '!' | '?' | ':' | ';' | '…'))
}

fn is_horizontal_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// Expands common German + English abbreviations to their spoken form. Applied
/// case-insensitively with surrounding-context guards so it doesn't fire inside
/// words.
fn expand_abbreviations(text: &str) -> String {
    let mut out = text.to_owned();
    for (re, replacement) in abbreviations() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out
}

fn abbreviations() -> &'static [(Regex, &'static str)] {
    static ABBR: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    ABBR.get_or_init(|| {
        // `(?i)` case-insensitive; `\b` / explicit `\.` anchor the abbreviation.
        // Multi-token forms (with optional spaces) come first.
        let entries: &[(&str, &str)] = &[
            (r"(?i)\bz\.\s*B\.", "zum Beispiel"),
            (r"(?i)\bd\.\s*h\.", "das heißt"),
            (r"(?i)\bu\.\s*a\.", "unter anderem"),
            (r"(?i)\bu\.\s*U\.", "unter Umständen"),
            (r"(?i)\bi\.\s*d\.\s*R\.", "in der Regel"),
            (r"(?i)\bo\.\s*[ÄäAa]\.", "oder Ähnliches"),
            (r"(?i)\bz\.\s*T\.", "zum Teil"),
            (r"(?i)\bu\.\s*v\.\s*m\.", "und vieles mehr"),
            (r"(?i)\busw\.", "und so weiter"),
            (r"(?i)\bbzw\.", "beziehungsweise"),
            (r"(?i)\bca\.", "circa"),
            (r"(?i)\bvgl\.", "vergleiche"),
            (r"(?i)\bsog\.", "sogenannt"),
            (r"(?i)\bggf\.", "gegebenenfalls"),
            (r"(?i)\binkl\.", "inklusive"),
            (r"(?i)\bexkl\.", "exklusive"),
            (r"(?i)\bmax\.", "maximal"),
            (r"(?i)\bNr\.", "Nummer"),
            (r"(?i)\bMio\.", "Millionen"),
            (r"(?i)\bMrd\.", "Milliarden"),
            (r"(?i)\be\.\s*g\.", "for example"),
            (r"(?i)\bi\.\s*e\.", "that is"),
            (r"(?i)\betc\.", "and so on"),
            (r"(?i)\bvs\.", "versus"),
        ];
        entries
            .iter()
            .filter_map(|(pat, rep)| Regex::new(pat).ok().map(|re| (re, *rep)))
            .collect()
    })
}

fn re_fenced() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```.*?```").unwrap())
}

fn re_image() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap())
}

fn re_link() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[(?P<label>[^\]]*)\]\([^)]*\)").unwrap())
}

fn re_url() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(?:https?://|www\.)\S+").unwrap())
}

fn re_list_marker() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Leading "- ", "* ", "+ ", "1. ", "2) ", with optional indentation.
    RE.get_or_init(|| Regex::new(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)").unwrap())
}

fn re_spaces() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markdown_emphasis_and_headings() {
        let out = normalize_for_speech("# Titel\n\nDas ist **wichtig** und _kursiv_.");
        assert!(!out.contains('#'));
        assert!(!out.contains('*'));
        assert!(!out.contains('_'));
        assert!(out.contains("Das ist wichtig und kursiv."));
    }

    #[test]
    fn flattens_bullet_lists_into_sentences() {
        let out = normalize_for_speech("Vorteile:\n- schnell\n- lokal\n- gratis");
        assert!(!out.contains('-'));
        assert!(out.contains("schnell."));
        assert!(out.contains("lokal."));
        assert!(out.contains("gratis."));
    }

    #[test]
    fn expands_abbreviations() {
        let out = normalize_for_speech("Das geht z. B. lokal, d.h. ohne Netz, usw.");
        assert!(out.contains("zum Beispiel"));
        assert!(out.contains("das heißt"));
        assert!(out.contains("und so weiter"));
        assert!(!out.contains("z. B."));
    }

    #[test]
    fn drops_urls_and_keeps_link_label() {
        let out = normalize_for_speech("Siehe [die Doku](https://example.com/x) oder https://foo.bar/y hier.");
        assert!(out.contains("die Doku"));
        assert!(!out.to_lowercase().contains("http"));
        assert!(!out.contains("example.com"));
    }

    #[test]
    fn removes_code_blocks() {
        let out = normalize_for_speech("Hier:\n```\nlet x = 1;\n```\nFertig.");
        assert!(!out.contains("let x"));
        assert!(out.contains("Fertig."));
    }

    #[test]
    fn plain_prose_passes_through() {
        let input = "Hallo! Ich bin dein Assistent. Wie kann ich helfen?";
        assert_eq!(normalize_for_speech(input), input);
    }
}
