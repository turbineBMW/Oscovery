//! A README's useful first part: prose without badges, HTML, images and
//! headings, cut to a size the agent can read for five thousand projects.

const MAX_CHARS: usize = 1500;

pub fn excerpt(readme: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut in_html_block = false;
    for line in readme.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if trimmed.starts_with("<details") || trimmed.starts_with("<picture") {
            in_html_block = true;
        }
        if in_html_block {
            if trimmed.contains("</details>") || trimmed.contains("</picture>") {
                in_html_block = false;
            }
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("[![")
            || trimmed.starts_with("![")
            || trimmed.starts_with('<')
            || trimmed.starts_with("|")
            || trimmed.starts_with("---")
            || trimmed.starts_with("===")
        {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            continue;
        }
        let cleaned = strip_markup(trimmed);
        if cleaned.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(&cleaned);
        if out.len() > MAX_CHARS {
            break;
        }
    }
    let mut text = out.trim().to_string();
    if text.len() > MAX_CHARS {
        let cut = text
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i <= MAX_CHARS)
            .last()
            .unwrap_or(0);
        text.truncate(cut);
        if let Some(end) = text.rfind(['.', '!', '?']) {
            text.truncate(end + 1);
        } else {
            text.push('…');
        }
    }
    text
}

/// The first sentence of an excerpt, as a summary, at most ~160 characters.
pub fn first_sentence(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    let sentence = match first.find(['.', '!', '?']) {
        Some(end) if end + 1 < first.len() => &first[..=end],
        _ => first,
    };
    let mut summary = sentence.to_string();
    if summary.len() > 160 {
        let cut = summary
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i <= 157)
            .last()
            .unwrap_or(0);
        summary.truncate(cut);
        summary.push('…');
    }
    summary
}

/// Links keep their text; inline HTML and emphasis markers go.
fn strip_markup(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[')
            && let Some(close) = after.find("](")
            && let Some(end) = after[close..].find(')')
        {
            out.push_str(&after[..close]);
            rest = &after[close + end + 1..];
        } else if let Some(after) = rest.strip_prefix('<')
            && let Some(end) = after.find('>')
        {
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix("**").or_else(|| rest.strip_prefix("__")) {
            rest = after;
        } else if let Some(after) = rest.strip_prefix('`') {
            rest = after;
        } else {
            let mut chars = rest.chars();
            out.push(chars.next().unwrap());
            rest = chars.as_str();
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_headings_code_and_html_go() {
        let readme = "# Tool\n\n[![ci](x)](y)\n\n<p align=center><img src=x></p>\n\nA **tool** for `things` with a [link](http://x).\n\n```sh\nmake\n```\n\n## Install\n\n| a | b |\n\nRun it.\n";
        assert_eq!(excerpt(readme), "A tool for things with a link.\nRun it.");
    }

    #[test]
    fn the_first_sentence_summarises() {
        assert_eq!(
            first_sentence("A fast client. It does things.\nMore."),
            "A fast client."
        );
        assert_eq!(first_sentence("No period here\nnext"), "No period here");
        assert!(first_sentence(&"word ".repeat(100)).ends_with('…'));
    }

    #[test]
    fn long_text_is_cut_at_a_sentence() {
        let long = "Sentence one. ".repeat(200);
        let text = excerpt(&long);
        assert!(text.len() <= MAX_CHARS + 1);
        assert!(text.ends_with('.'));
    }
}
