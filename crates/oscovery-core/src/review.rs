//! The security review: a shallow clone of the project, the agent reading
//! it with read-only tools, and a verdict with findings. Nothing here
//! installs; the page shows the verdict next to the Install button.

use crate::error::{Error, Result};
use crate::models::{Entry, InstallMethod};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// What gets cloned for a review: the project's git repository, or the
/// AUR package's PKGBUILD repository.
pub fn source_url(entry: &Entry) -> Option<String> {
    match entry.install.as_ref().map(|i| i.method) {
        Some(InstallMethod::Aur) => entry
            .id
            .strip_prefix("aur:")
            .map(|name| format!("https://aur.archlinux.org/{name}.git")),
        _ => (!entry.repo.is_empty()).then(|| entry.repo.clone()),
    }
}

/// Where a project's review clone lives under the cache directory.
pub fn clone_dir(cache_dir: &Path, entry: &Entry) -> PathBuf {
    let safe: String = entry
        .id
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    cache_dir.join("review").join(safe)
}

/// A fresh shallow clone (an old one is replaced, so the review reads
/// today's code).
pub async fn clone(url: &str, dir: &Path) -> Result<()> {
    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_dir_all(dir).await;
    let output = Command::new("git")
        .args(["clone", "--depth", "1", "--", url])
        .arg(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| Error::message(format!("could not run git: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(Error::message(
        stderr
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or("git clone failed")
            .to_string(),
    ))
}

/// What the agent is asked to do with the clone.
pub fn prompt(entry: &Entry) -> String {
    let what = match entry.install.as_ref().map(|i| i.method) {
        Some(InstallMethod::OmarchyPlugin | InstallMethod::OmarchyBuiltin) => {
            "an Omarchy shell plugin (QML and JavaScript run inside the user's shell with the \
             user's privileges; `manifest.json` names the entry points; a `service` entry runs \
             in the background)"
        }
        Some(InstallMethod::Aur) => {
            "an AUR package (the PKGBUILD runs at build time; check its `source=` URLs, \
             checksums, and the install/post-install hooks it declares)"
        }
        _ => "a desktop application or tool",
    };
    format!(
        "You are reviewing {name} for someone about to install it on their Omarchy desktop. \
         It is {what}. The current directory is a fresh clone of {repo}.\n\n\
         Read the code and answer: is it safe to install? Look for anything that reaches \
         outside what the project needs: network calls and where they go, downloads or \
         `curl | sh`, anything run with sudo or pkexec, reads of credentials, keys, tokens, \
         browser or SSH data, obfuscated or minified code, encoded payloads, writes outside \
         its own directory, post-install hooks, telemetry, and whether the README's claims \
         match what the code does. Also note the useful facts: what permissions it needs, \
         which external services it talks to, and what it installs.\n\n\
         Answer for a person in plain text (no headings): a verdict in one sentence, then \
         the findings that matter, one line each, worst first, then the useful facts. Keep it \
         short; a clean project gets a short answer.\n\n\
         Then end with exactly one fenced block tagged `oscovery-review` holding JSON:\n\
         ```oscovery-review\n\
         {{\"verdict\": \"safe\" | \"caution\" | \"avoid\", \"findings\": [{{\"severity\": \
         \"high\" | \"medium\" | \"low\" | \"info\", \"title\": \"<one line>\", \"file\": \
         \"<path or empty>\"}}]}}\n\
         ```\n\
         \"safe\" means nothing unexpected; \"caution\" means something a careful person \
         should know before installing; \"avoid\" means it does something harmful or \
         deceptive. Only read this directory. Do not run, build, install or change anything.",
        name = entry.name,
        repo = source_url(entry).unwrap_or_default(),
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verdict {
    /// The agent gave no structured verdict (a refusal, an error).
    #[default]
    Unknown,
    Safe,
    Caution,
    Avoid,
}

impl Verdict {
    pub fn parse(text: &str) -> Self {
        match text.trim().to_lowercase().as_str() {
            "safe" => Verdict::Safe,
            "caution" => Verdict::Caution,
            "avoid" => Verdict::Avoid,
            _ => Verdict::Unknown,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub file: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Review {
    pub text: String,
    pub verdict: Verdict,
    pub findings: Vec<Finding>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(default)]
    verdict: String,
    #[serde(default)]
    findings: Vec<Finding>,
}

/// Split the agent's answer into prose and the verdict block.
pub fn parse(raw: &str) -> Review {
    let mut review = Review {
        text: raw.trim().to_string(),
        ..Default::default()
    };
    let Some((before, rest)) = raw.rsplit_once("```oscovery-review") else {
        return review;
    };
    let (json, after) = rest.split_once("```").unwrap_or((rest, ""));
    if let Ok(block) = serde_json::from_str::<Block>(json.trim()) {
        review.verdict = Verdict::parse(&block.verdict);
        review.findings = block.findings;
        review.text = format!("{}\n{}", before.trim_end(), after.trim())
            .trim()
            .to_string();
    }
    review
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verdict_comes_apart_from_the_prose() {
        let raw = "Looks fine.\n\n```oscovery-review\n{\"verdict\":\"caution\",\"findings\":[{\"severity\":\"medium\",\"title\":\"Fetches weather from api.example.com\",\"file\":\"Service.qml\"}]}\n```";
        let review = parse(raw);
        assert_eq!(review.text, "Looks fine.");
        assert_eq!(review.verdict, Verdict::Caution);
        assert_eq!(review.findings[0].file, "Service.qml");
        assert_eq!(parse("I can't help with that.").verdict, Verdict::Unknown);
    }

    #[test]
    fn clone_dirs_are_safe_names() {
        let mut entry = crate::models::Entry {
            id: "gh:owner/repo".into(),
            kind: crate::models::Kind::App,
            subkind: String::new(),
            name: "R".into(),
            summary: String::new(),
            description: String::new(),
            author: String::new(),
            author_key: String::new(),
            repo: "https://github.com/owner/repo".into(),
            homepage: String::new(),
            listing_url: String::new(),
            license: String::new(),
            stars: 0,
            category: String::new(),
            tags: Vec::new(),
            version: String::new(),
            install: None,
            previews: Vec::new(),
            thumbnail: String::new(),
            first_seen: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            upstream_added: None,
            updated_at: None,
            wants_contributors: false,
            source: String::new(),
            status: String::new(),
            verified: false,
        };
        assert_eq!(
            clone_dir(Path::new("/c"), &entry),
            PathBuf::from("/c/review/gh_owner_repo")
        );
        assert_eq!(
            source_url(&entry).as_deref(),
            Some("https://github.com/owner/repo")
        );
        entry.id = "aur:foo-bin".into();
        entry.install = Some(crate::models::Install {
            method: InstallMethod::Aur,
            command: String::new(),
            note: String::new(),
        });
        assert_eq!(
            source_url(&entry).as_deref(),
            Some("https://aur.archlinux.org/foo-bin.git")
        );
    }
}
