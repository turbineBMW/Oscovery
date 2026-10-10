//! The directory the agent searches: the whole catalog as files it can grep
//! and read with its own tools, nothing else. `INDEX.md` has one line per
//! project; `catalog/<kind>/<id>.md` has each project in full. Rebuilt when
//! the index changes (a stamp file remembers which build it holds).

use crate::error::Result;
use crate::models::Entry;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const STAMP: &str = ".index-stamp";

/// Whether `root` already holds `stamp`'s build, so the caller can skip
/// loading the entries at all.
pub fn is_current(root: &Path, stamp: &str) -> bool {
    std::fs::read_to_string(root.join(STAMP)).is_ok_and(|s| s == stamp)
}

/// Write the catalog under `root` unless it already holds `stamp`'s build.
/// Blocking: call it off the main thread.
pub fn prepare(root: &Path, stamp: &str, entries: &[Entry]) -> Result<PathBuf> {
    if is_current(root, stamp) {
        return Ok(root.to_path_buf());
    }
    let staging = root.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("catalog"))?;

    let mut index = String::from(
        "# Oscovery catalog\n\nOne line per project: `id — name [kind, category]: summary`. \
         Each project's full page is `catalog/<kind>/<id>.md`.\n\n",
    );
    for entry in entries {
        let dir = staging.join("catalog").join(entry.kind.id());
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{}.md", safe_name(&entry.id))),
            page(entry),
        )?;
        let _ = writeln!(
            index,
            "- {} — {} [{}, {}]: {}",
            entry.id,
            entry.name,
            if entry.subkind.is_empty() {
                entry.kind.id()
            } else {
                &entry.subkind
            },
            entry.category,
            one_line(&entry.summary)
        );
    }
    std::fs::write(staging.join("INDEX.md"), index)?;
    std::fs::write(staging.join("README.md"), README)?;
    std::fs::write(staging.join(STAMP), stamp)?;
    let _ = std::fs::remove_dir_all(root);
    std::fs::rename(&staging, root)?;
    Ok(root.to_path_buf())
}

/// A project's page for the agent.
pub fn page(entry: &Entry) -> String {
    let mut text = format!("# {}\n\n", entry.name);
    let _ = writeln!(text, "- id: {}", entry.id);
    let _ = writeln!(
        text,
        "- kind: {}{}",
        entry.kind.id(),
        if entry.subkind.is_empty() {
            String::new()
        } else {
            format!(" ({})", entry.subkind)
        }
    );
    if !entry.category.is_empty() {
        let _ = writeln!(text, "- category: {}", entry.category);
    }
    if !entry.tags.is_empty() {
        let _ = writeln!(text, "- tags: {}", entry.tags.join(", "));
    }
    if !entry.author.is_empty() {
        let _ = writeln!(text, "- author: {}", entry.author);
    }
    if !entry.repo.is_empty() {
        let _ = writeln!(text, "- repository: {}", entry.repo);
    }
    if !entry.license.is_empty() {
        let _ = writeln!(text, "- license: {}", entry.license);
    }
    let _ = writeln!(text, "- stars: {}", entry.stars);
    if let Some(updated) = entry.updated_at {
        let _ = writeln!(text, "- updated: {}", updated.format("%Y-%m-%d"));
    }
    if !entry.status.is_empty() {
        let _ = writeln!(text, "- status: {}", entry.status);
    }
    if let Some(install) = &entry.install
        && !install.command.is_empty()
    {
        let _ = writeln!(text, "- install: `{}`", install.command);
    }
    if entry.wants_contributors {
        text.push_str("- the author is looking for contributors\n");
    }
    if !entry.summary.is_empty() {
        let _ = write!(text, "\n{}\n", entry.summary.trim());
    }
    if !entry.description.is_empty() {
        let _ = write!(text, "\n{}\n", entry.description.trim());
    }
    text
}

/// The shortlist a keyword search produced, inlined into the prompt.
pub fn shortlist(entries: &[Entry]) -> String {
    let mut text = String::new();
    for entry in entries {
        let _ = writeln!(
            text,
            "- {} — {} [{}, {}]: {}",
            entry.id,
            entry.name,
            if entry.subkind.is_empty() {
                entry.kind.id()
            } else {
                &entry.subkind
            },
            entry.category,
            one_line(&entry.summary)
        );
    }
    text
}

fn safe_name(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

const README: &str = "\
# Oscovery catalog

This directory is a snapshot of the Oscovery catalog of Omarchy plugins and apps, laid
out for searching with grep and reading file by file:

- `INDEX.md`: one line per project (`id — name [kind, category]: summary`).
- `catalog/plugin/<id>.md`, `catalog/app/<id>.md`: each project in full.

It is read-only reference material; nothing here should be edited or run.
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Kind;

    fn entry(id: &str, name: &str) -> Entry {
        Entry {
            id: id.into(),
            kind: Kind::Plugin,
            subkind: "bar-widget".into(),
            name: name.into(),
            summary: "Shows  things\nin the bar".into(),
            description: String::new(),
            author: "A".into(),
            author_key: "a".into(),
            repo: "https://github.com/a/b".into(),
            homepage: String::new(),
            listing_url: String::new(),
            license: "MIT".into(),
            stars: 3,
            category: "Widgets".into(),
            tags: vec!["bar".into()],
            version: String::new(),
            install: None,
            previews: Vec::new(),
            thumbnail: String::new(),
            first_seen: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            upstream_added: None,
            updated_at: None,
            wants_contributors: true,
            source: String::new(),
            status: String::new(),
            verified: false,
        }
    }

    #[test]
    fn the_catalog_is_written_once_per_build() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("agent");
        let entries = [entry("a.b", "Bee"), entry("c/d", "Dee")];
        prepare(&root, "build-1", &entries).unwrap();
        let index = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(index.contains("- a.b — Bee [bar-widget, Widgets]: Shows things in the bar"));
        assert!(root.join("catalog/plugin/a.b.md").is_file());
        assert!(root.join("catalog/plugin/c_d.md").is_file());
        let page = std::fs::read_to_string(root.join("catalog/plugin/a.b.md")).unwrap();
        assert!(page.contains("looking for contributors"));

        // Same build: untouched. New build: rewritten without the old file.
        std::fs::write(root.join("extra"), "x").unwrap();
        prepare(&root, "build-1", &entries).unwrap();
        assert!(root.join("extra").exists());
        prepare(&root, "build-2", &entries[..1]).unwrap();
        assert!(!root.join("extra").exists());
        assert!(!root.join("catalog/plugin/c_d.md").exists());
    }
}
