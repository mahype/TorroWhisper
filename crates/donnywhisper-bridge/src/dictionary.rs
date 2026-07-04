use donnywhisper_core::DictionaryEntry;
use regex::RegexBuilder;

pub fn apply(entries: &[DictionaryEntry], text: &str) -> String {
    let mut result = text.to_owned();
    for entry in entries {
        if entry.pattern.is_empty() {
            continue;
        }
        result = apply_entry(entry, &result);
    }
    result
}

fn apply_entry(entry: &DictionaryEntry, text: &str) -> String {
    let escaped = regex::escape(&entry.pattern);
    let pattern = if entry.whole_word {
        let leading = entry
            .pattern
            .chars()
            .next()
            .map(is_word_char)
            .unwrap_or(false);
        let trailing = entry
            .pattern
            .chars()
            .next_back()
            .map(is_word_char)
            .unwrap_or(false);
        match (leading, trailing) {
            (true, true) => format!(r"\b{escaped}\b"),
            (true, false) => format!(r"\b{escaped}"),
            (false, true) => format!(r"{escaped}\b"),
            (false, false) => escaped,
        }
    } else {
        escaped
    };
    match RegexBuilder::new(&pattern)
        .case_insensitive(!entry.case_sensitive)
        .build()
    {
        Ok(re) => re
            .replace_all(text, entry.replacement.as_str())
            .into_owned(),
        Err(_) => text.to_owned(),
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pattern: &str, replacement: &str) -> DictionaryEntry {
        DictionaryEntry {
            id: "test".to_owned(),
            pattern: pattern.to_owned(),
            replacement: replacement.to_owned(),
            case_sensitive: false,
            whole_word: true,
        }
    }

    #[test]
    fn case_insensitive_replaces_capitalized_match() {
        let entries = vec![entry("komm bitte", "committe")];
        let result = apply(&entries, "Komm Bitte mach das");
        assert_eq!(result, "committe mach das");
    }

    #[test]
    fn case_sensitive_skips_capitalized_match() {
        let mut e = entry("komm bitte", "committe");
        e.case_sensitive = true;
        let result = apply(&[e.clone()], "Komm Bitte mach das");
        assert_eq!(result, "Komm Bitte mach das");
        let result = apply(&[e], "komm bitte mach das");
        assert_eq!(result, "committe mach das");
    }

    #[test]
    fn whole_word_does_not_match_inside_word() {
        let entries = vec![entry("is", "IS")];
        let result = apply(&entries, "this is it");
        assert_eq!(result, "this IS it");
    }

    #[test]
    fn substring_match_when_whole_word_off() {
        let mut e = entry("is", "IS");
        e.whole_word = false;
        let result = apply(&[e], "this is it");
        assert_eq!(result, "thIS IS it");
    }

    #[test]
    fn regex_meta_characters_in_pattern_are_escaped() {
        let entries = vec![entry("c++", "Cpp")];
        let result = apply(&entries, "ich mag c++ sehr");
        assert_eq!(result, "ich mag Cpp sehr");
    }

    #[test]
    fn dot_in_pattern_does_not_match_arbitrary_char() {
        let entries = vec![entry("a.b", "ANSWER")];
        let result = apply(&entries, "axb a.b acb");
        assert_eq!(result, "axb ANSWER acb");
    }

    #[test]
    fn empty_pattern_is_skipped() {
        let entries = vec![entry("", "ignored")];
        let result = apply(&entries, "hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn empty_input_returns_empty() {
        let entries = vec![entry("foo", "bar")];
        let result = apply(&entries, "");
        assert_eq!(result, "");
    }

    #[test]
    fn multiple_entries_apply_in_listed_order() {
        let entries = vec![entry("foo", "bar"), entry("bar", "baz")];
        let result = apply(&entries, "foo");
        assert_eq!(result, "baz");
    }

    #[test]
    fn no_entries_passes_text_through() {
        let result = apply(&[], "hello world");
        assert_eq!(result, "hello world");
    }
}
