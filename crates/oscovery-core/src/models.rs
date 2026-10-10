//! The catalog: one [`Entry`] per Omarchy plugin or app, as the index
//! publishes it and the client stores it. Every field but the identity ones
//! is optional on the wire, so an older client reads a newer index.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// The index file: what `oscovery-index build` writes and the app fetches.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Index {
    /// Bumped when the shape changes incompatibly; the client refuses newer.
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<Entry>,
}

impl Index {
    pub const VERSION: u32 = 1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An Omarchy shell plugin: a bar widget, panel, overlay, menu or service.
    Plugin,
    /// A desktop or terminal application.
    App,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Plugin => "plugin",
            Kind::App => "app",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "plugin" => Some(Kind::Plugin),
            "app" => Some(Kind::App),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMethod {
    /// `omarchy plugin add <repo> --enable`: a git clone, no code run, no sudo.
    OmarchyPlugin,
    /// A plugin that ships with Omarchy: `omarchy plugin enable <id>`.
    OmarchyBuiltin,
    /// An AUR package, built and installed with yay in a terminal.
    Aur,
    /// An Arch repository package: `omarchy pkg add <name>`.
    Pacman,
    /// A GitHub release asset.
    GithubRelease,
    /// A source checkout with an `install.sh`.
    InstallScript,
    /// Instructions only; the project page links to them.
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
    pub method: InstallMethod,
    /// The exact command, shown to the user before anything runs.
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// A plugin's manifest id (`io.github.user.name`), `aur:<name>` or
    /// `gh:<owner>/<repo>`.
    pub id: String,
    pub kind: Kind,
    /// Finer than `kind`: the marketplace's "Bar widget", "Service", or an
    /// app's "aur", "github-release".
    #[serde(default)]
    pub subkind: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// A README excerpt, a few paragraphs at most.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// What groups "more by this developer": the GitHub owner, or the AUR
    /// maintainer, lower-case.
    #[serde(default)]
    pub author_key: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub homepage: String,
    /// The project's page on the marketplace or the AUR.
    #[serde(default)]
    pub listing_url: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub stars: i64,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub install: Option<Install>,
    /// Full-size preview images.
    #[serde(default)]
    pub previews: Vec<String>,
    /// A card-sized image, when there is one.
    #[serde(default)]
    pub thumbnail: String,
    /// The day the index first saw it: the Timeline's axis.
    pub first_seen: NaiveDate,
    /// When the upstream listing says it was added, when it says.
    #[serde(default)]
    pub upstream_added: Option<NaiveDate>,
    /// The last upstream push or release.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// The author asked for help: an `omarchy-wants-contributors` topic, an
    /// `.oscovery.toml`, or the submission form.
    #[serde(default)]
    pub wants_contributors: bool,
    /// Which crawl produced it: `marketplace`, `github`, `aur`, `submitted`.
    #[serde(default)]
    pub source: String,
    /// The listing's own status word, when it has one ("Manual setup").
    #[serde(default)]
    pub status: String,
    /// Reviewed by the upstream listing's maintainers.
    #[serde(default)]
    pub verified: bool,
}

impl Entry {
    /// A plugin's id doubles as what `omarchy plugin` commands take.
    pub fn is_plugin(&self) -> bool {
        self.kind == Kind::Plugin
    }

    /// `owner/repo` when the project is on GitHub.
    pub fn github_slug(&self) -> Option<String> {
        github_slug(&self.repo)
    }
}

/// `owner/repo` out of any GitHub URL, without a `.git` suffix.
pub fn github_slug(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .strip_prefix("https://github.com/")
        .or_else(|| url.trim().strip_prefix("http://github.com/"))
        .or_else(|| url.trim().strip_prefix("git@github.com:"))?;
    let mut parts = rest.trim_end_matches('/').splitn(3, '/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    Some(format!("{owner}/{repo}"))
}

/// The GitHub owner, lower-case, as an `author_key`.
pub fn github_owner(url: &str) -> Option<String> {
    github_slug(url).map(|slug| slug.split('/').next().unwrap_or("").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_urls_resolve_to_slugs() {
        assert_eq!(
            github_slug("https://github.com/gardnmi/omarchy-boomux.git"),
            Some("gardnmi/omarchy-boomux".into())
        );
        assert_eq!(
            github_slug("https://github.com/Owner/Repo/tree/main/plugins/x"),
            Some("Owner/Repo".into())
        );
        assert_eq!(
            github_owner("https://github.com/Owner/Repo"),
            Some("owner".into())
        );
        assert_eq!(github_slug("https://codeberg.org/a/b"), None);
        assert_eq!(github_slug("https://github.com/only-owner"), None);
    }

    #[test]
    fn an_old_client_reads_a_newer_entry() {
        let json =
            r#"{"id":"x","kind":"app","name":"X","first_seen":"2026-10-10","brand_new_field":1}"#;
        let entry: Entry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.kind, Kind::App);
        assert!(entry.install.is_none());
    }
}
