//! Turning what the user typed into an FTS5 query, and the shortlist the
//! agent starts from.

/// An FTS5 `MATCH` expression for free text: every word a quoted prefix
/// term, all required. `None` when nothing searchable was typed. Quoting
/// keeps FTS5 syntax (`AND`, `NEAR`, `*`, colons) from being interpreted.
pub fn fts_match(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{}\"*", word.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// A looser expression for a sentence: any word may match, so a description
/// ("something that shows my printer's progress in the bar") still finds
/// candidates. Stop words are dropped; the rest are quoted prefixes joined
/// with OR.
pub fn fts_any(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|word| word.len() > 2 && !is_stop_word(&word.to_lowercase()))
        .map(|word| format!("\"{}\"*", word.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

fn is_stop_word(word: &str) -> bool {
    const STOP: &[&str] = &[
        "the",
        "and",
        "for",
        "that",
        "with",
        "this",
        "from",
        "have",
        "want",
        "need",
        "like",
        "something",
        "some",
        "which",
        "what",
        "there",
        "any",
        "can",
        "does",
        "into",
        "show",
        "shows",
        "get",
        "lets",
        "let",
        "app",
        "apps",
        "plugin",
        "plugins",
        "omarchy",
        "one",
        "would",
        "could",
        "should",
        "when",
        "where",
        "about",
        "just",
        "also",
        "but",
        "not",
        "you",
        "your",
        "its",
        "are",
        "was",
        "has",
        "all",
        "more",
        "than",
    ];
    STOP.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_required_prefix_terms() {
        assert_eq!(
            fts_match("  battery  bar "),
            Some("\"battery\"* \"bar\"*".into())
        );
        assert_eq!(
            fts_match("a:b NEAR \"c\""),
            Some("\"a\"* \"b\"* \"NEAR\"* \"c\"*".into())
        );
        assert_eq!(fts_match(" - "), None);
    }

    #[test]
    fn sentences_become_optional_terms_without_stop_words() {
        assert_eq!(
            fts_any("I want something that shows my 3d printer progress in the bar"),
            Some("\"printer\"* OR \"progress\"* OR \"bar\"*".into())
        );
        assert_eq!(fts_any("the and for"), None);
    }
}
