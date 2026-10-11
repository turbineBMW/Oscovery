//! GitHub: repositories that opted in, either with an `oscovery.toml` at
//! their root or with the `omarchy-plugin` / `omarchy-app` topic (the topic
//! ones also pass a quality floor), classified by what their root holds,
//! with a README excerpt. Repositories that merely mention Omarchy are only
//! looked at for the file, never listed without it: plenty of them are
//! personal setups that were never meant for a catalog.

use crate::{Http, readme};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use oscovery_core::models::{Entry, Install, InstallMethod, Kind};
use serde::Deserialize;
use std::collections::{BTreeSet, HashSet};

/// Topics that mean "find this": a repository under one is listed unless it
/// opts out.
pub const OPT_IN_TOPICS: [&str; 2] = ["omarchy-plugin", "omarchy-app"];
/// The file at a repository's root that opts it in (and, with
/// `listed = false`, out).
pub const MANIFEST: &str = "oscovery.toml";
/// Searches for repositories that mention Omarchy, the ones looked at for
/// an `oscovery.toml`.
const MENTION_QUERIES: [&str; 2] = ["omarchy+in:name,description,readme", "topic:omarchy"];
/// GitHub search returns at most this many per query.
const SEARCH_CAP: usize = 1000;
/// Pushed longer ago than this and a project is left out.
const STALE_MONTHS: i64 = 18;
/// What an Omarchy shell plugin's manifest may declare; a `manifest.json`
/// from some other system has none of these.
const PLUGIN_KINDS: [&str; 6] = ["bar-widget", "panel", "overlay", "menu", "service", "bar"];

#[derive(Clone, Debug, Deserialize)]
pub struct Repo {
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub stargazers_count: i64,
    #[serde(default)]
    pub license: Option<License>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub pushed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub fork: bool,
    pub default_branch: String,
    pub html_url: String,
    pub owner: Owner,
}

#[derive(Clone, Debug, Deserialize)]
pub struct License {
    #[serde(default)]
    pub spdx_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Owner {
    pub login: String,
}

#[derive(Deserialize)]
struct SearchPage {
    #[serde(default)]
    total_count: usize,
    items: Vec<Repo>,
}

#[derive(Deserialize)]
struct Tree {
    #[serde(default)]
    tree: Vec<TreeEntry>,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default, rename = "schemaVersion")]
    schema_version: Option<serde_json::Value>,
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    kinds: Vec<String>,
}

/// `oscovery.toml`: being there opts a repository in. Every field is
/// optional and corrects what the crawl would otherwise guess.
#[derive(Debug, Deserialize)]
pub struct OscoveryToml {
    /// `false` keeps the project out of the catalog, whichever source found it.
    #[serde(default = "listed_by_default")]
    pub listed: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub wants_contributors: bool,
    /// Full-size images (https), the first doubling as the card's.
    #[serde(default)]
    pub previews: Vec<String>,
    /// An app's install method: `install-script`, `github-release` or
    /// `manual`. Only the method: the app builds the command itself.
    #[serde(default)]
    pub install: Option<InstallMethod>,
    #[serde(default)]
    pub install_note: String,
}

fn listed_by_default() -> bool {
    true
}

/// At most this many previews from a manifest.
const MAX_PREVIEWS: usize = 8;

impl OscoveryToml {
    /// A manifest that doesn't parse keeps the project out: it may well
    /// have said `listed = false`.
    pub fn parse(text: &str, repo: &str) -> Self {
        toml::from_str(text).unwrap_or_else(|error| {
            eprintln!("{repo}: {MANIFEST}: {error}; leaving it out");
            Self {
                listed: false,
                ..toml::from_str("").expect("an empty manifest")
            }
        })
    }

    pub fn apply(&self, entry: &mut Entry) {
        if !self.name.trim().is_empty() {
            entry.name = self.name.trim().to_string();
        }
        if !self.summary.trim().is_empty() {
            entry.summary = self.summary.trim().to_string();
        }
        if !self.category.trim().is_empty() {
            entry.category = self.category.trim().to_string();
        }
        for tag in &self.tags {
            let tag = tag.trim().to_lowercase();
            if !tag.is_empty() && !entry.tags.contains(&tag) {
                entry.tags.push(tag);
            }
        }
        entry.wants_contributors |= self.wants_contributors;
        let previews: Vec<String> = self
            .previews
            .iter()
            .filter(|url| url.starts_with("https://"))
            .take(MAX_PREVIEWS)
            .cloned()
            .collect();
        if let Some(first) = previews.first() {
            entry.thumbnail = first.clone();
            entry.previews = previews;
        }
        if entry.kind != Kind::App {
            return;
        }
        match self.install {
            Some(
                method @ (InstallMethod::InstallScript
                | InstallMethod::GithubRelease
                | InstallMethod::Manual),
            ) => {
                entry.subkind = match method {
                    InstallMethod::GithubRelease => "github-release",
                    _ => "source",
                }
                .into();
                entry.install = Some(Install {
                    method,
                    command: match method {
                        InstallMethod::InstallScript => {
                            format!("git clone {} && sh install.sh", entry.repo)
                        }
                        _ => String::new(),
                    },
                    note: self.install_note.trim().to_string(),
                });
            }
            Some(other) => eprintln!(
                "{}: {MANIFEST}: install = {other:?} is not taken from a manifest",
                entry.id
            ),
            None => {
                if let Some(install) = &mut entry.install
                    && !self.install_note.trim().is_empty()
                {
                    install.note = self.install_note.trim().to_string();
                }
            }
        }
    }
}

/// A repository's `oscovery.toml`, when it has one.
pub async fn manifest(http: &Http, full_name: &str, branch: &str) -> Option<OscoveryToml> {
    let url = format!("https://raw.githubusercontent.com/{full_name}/{branch}/{MANIFEST}");
    let text = http.get_text(&url).await?;
    Some(OscoveryToml::parse(&text, full_name))
}

#[derive(Deserialize)]
struct CodePage {
    items: Vec<CodeHit>,
}

#[derive(Deserialize)]
struct CodeHit {
    path: String,
    repository: CodeRepo,
}

#[derive(Deserialize)]
struct CodeRepo {
    full_name: String,
}

/// Repositories code search says have an `oscovery.toml` at their root.
/// Its index leaves out many repositories, so this only adds to
/// `pushed_between`.
pub async fn code_search(http: &Http) -> Vec<String> {
    let mut names = BTreeSet::new();
    for page in 1..=SEARCH_CAP / 100 {
        let url = format!(
            "https://api.github.com/search/code?q=filename:{MANIFEST}&per_page=100&page={page}"
        );
        let Some(found) = http.get_json::<CodePage>(&url).await else {
            break;
        };
        let count = found.items.len();
        names.extend(
            found
                .items
                .into_iter()
                .filter(|hit| hit.path == MANIFEST)
                .map(|hit| hit.repository.full_name),
        );
        if count < 100 {
            break;
        }
    }
    names.into_iter().collect()
}

/// What can have just gained an `oscovery.toml` (adding it is a push):
/// repositories that mention Omarchy in their name, description, README or
/// topics, pushed between `since` and `until`. A search returns at most
/// 1000, so a busy window is halved until each half fits. `false` when part
/// of the window couldn't be read.
pub async fn pushed_between(
    http: &Http,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> (Vec<Repo>, bool) {
    let stamp = |at: DateTime<Utc>| at.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut seen = HashSet::new();
    let mut repos = Vec::new();
    let mut complete = true;
    for query in MENTION_QUERIES {
        let mut windows = vec![(since, until)];
        while let Some((from, to)) = windows.pop() {
            let mut page = 1;
            loop {
                let url = format!(
                    "https://api.github.com/search/repositories?q={query}+pushed:{}..{}&per_page=100&page={page}",
                    stamp(from),
                    stamp(to)
                );
                let Some(found) = http.get_json::<SearchPage>(&url).await else {
                    complete = false;
                    break;
                };
                if page == 1 && found.total_count > SEARCH_CAP {
                    if to - from > Duration::minutes(1) {
                        let middle = from + (to - from) / 2;
                        windows.push((from, middle));
                        windows.push((middle, to));
                        break;
                    }
                    eprintln!(
                        "github: over {SEARCH_CAP} pushed within a minute at {}",
                        stamp(from)
                    );
                    complete = false;
                }
                let count = found.items.len();
                for repo in found.items {
                    if seen.insert(repo.full_name.to_lowercase()) {
                        repos.push(repo);
                    }
                }
                if count < 100 || page * 100 >= SEARCH_CAP {
                    break;
                }
                page += 1;
            }
        }
    }
    (repos, complete)
}

/// One repository by name.
pub async fn repo(http: &Http, full_name: &str) -> Option<Repo> {
    http.get_json(&format!("https://api.github.com/repos/{full_name}"))
        .await
}

/// Every repository the topic searches return, deduplicated, at most
/// `limit` (0 for no limit beyond GitHub's own).
pub async fn search(http: &Http, limit: usize) -> Vec<Repo> {
    let mut seen = HashSet::new();
    let mut repos = Vec::new();
    for topic in OPT_IN_TOPICS {
        for sort in ["updated", "stars"] {
            let mut page = 1;
            loop {
                if limit > 0 && repos.len() >= limit {
                    return repos;
                }
                let url = format!(
                    "https://api.github.com/search/repositories?q=topic:{topic}&sort={sort}&per_page=100&page={page}"
                );
                let Some(found) = http.get_json::<SearchPage>(&url).await else {
                    break;
                };
                let count = found.items.len();
                for repo in found.items {
                    if seen.insert(repo.full_name.to_lowercase()) {
                        repos.push(repo);
                    }
                }
                if count < 100 || page * 100 >= SEARCH_CAP {
                    break;
                }
                page += 1;
            }
        }
    }
    repos
}

/// The quality floor: alive, original, described, licensed, not a theme
/// or a dotfiles dump.
pub fn passes_floor(repo: &Repo, today: DateTime<Utc>) -> bool {
    let name = repo.full_name.to_lowercase();
    let description = repo.description.as_deref().unwrap_or("").trim();
    !repo.archived
        && !repo.fork
        && !description.is_empty()
        && repo
            .license
            .as_ref()
            .and_then(|l| l.spdx_id.as_deref())
            .is_some_and(|id| id != "NOASSERTION")
        && today - repo.pushed_at < Duration::days(STALE_MONTHS * 30)
        && !repo.topics.iter().any(|t| t == "omarchy-theme")
        && !["dotfiles", "-theme", "theme-", "/themes", "/dots"]
            .iter()
            .any(|word| name.contains(word))
}

/// What the repository is, from its root files: a plugin (a valid
/// `manifest.json`), or an app with a way to install it.
pub enum Classified {
    Plugin(Entry),
    App(Entry),
    Skip,
}

pub async fn classify(http: &Http, repo: &Repo, today: NaiveDate) -> Classified {
    let tree_url = format!(
        "https://api.github.com/repos/{}/git/trees/{}",
        repo.full_name, repo.default_branch
    );
    let paths: Vec<String> = http
        .get_json::<Tree>(&tree_url)
        .await
        .map(|tree| tree.tree.into_iter().map(|e| e.path).collect())
        .unwrap_or_default();
    let has = |file: &str| paths.iter().any(|p| p == file);
    let raw = |file: &str| {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{file}",
            repo.full_name, repo.default_branch
        )
    };
    let listing = if has(MANIFEST) {
        manifest(http, &repo.full_name, &repo.default_branch).await
    } else {
        None
    };
    if listing.as_ref().is_some_and(|m| !m.listed) {
        return Classified::Skip;
    }
    let readme_text = match paths
        .iter()
        .find(|p| p.eq_ignore_ascii_case("README.md") || p.eq_ignore_ascii_case("README"))
    {
        Some(file) => http
            .get_text(&raw(file))
            .await
            .map(|text| readme::excerpt(&text))
            .unwrap_or_default(),
        None if listing.is_some() => String::new(),
        None => return Classified::Skip,
    };

    let license = repo
        .license
        .as_ref()
        .and_then(|l| l.spdx_id.clone())
        .unwrap_or_default();
    // A repository without a description: its README's first sentence.
    let summary = match repo.description.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => text.to_string(),
        _ => readme::first_sentence(&readme_text),
    };
    let base = Entry {
        id: format!("gh:{}", repo.full_name.to_lowercase()),
        kind: Kind::App,
        subkind: String::new(),
        name: repo
            .full_name
            .rsplit('/')
            .next()
            .unwrap_or(&repo.full_name)
            .to_string(),
        summary,
        description: readme_text,
        author: repo.owner.login.clone(),
        author_key: repo.owner.login.to_lowercase(),
        repo: repo.html_url.clone(),
        homepage: repo.html_url.clone(),
        listing_url: String::new(),
        license,
        stars: repo.stargazers_count,
        category: String::new(),
        tags: repo
            .topics
            .iter()
            .filter(|t| !t.starts_with("omarchy"))
            .cloned()
            .collect(),
        version: String::new(),
        install: None,
        previews: Vec::new(),
        thumbnail: format!(
            "https://opengraph.githubusercontent.com/1/{}",
            repo.full_name
        ),
        first_seen: today,
        upstream_added: Some(repo.created_at.date_naive()),
        updated_at: Some(repo.pushed_at),
        wants_contributors: repo
            .topics
            .iter()
            .any(|t| t == "omarchy-wants-contributors" || t == "help-wanted"),
        source: "github".into(),
        status: String::new(),
        verified: false,
    };

    if has("manifest.json")
        && let Some(manifest) = http.get_json::<Manifest>(&raw("manifest.json")).await
        && manifest.schema_version.as_ref().and_then(|v| v.as_u64()) == Some(1)
        && !manifest.id.is_empty()
        && manifest
            .kinds
            .iter()
            .any(|k| PLUGIN_KINDS.contains(&k.as_str()))
    {
        let mut entry = base;
        entry.id = manifest.id;
        entry.kind = Kind::Plugin;
        entry.subkind = manifest
            .kinds
            .iter()
            .find(|k| PLUGIN_KINDS.contains(&k.as_str()))
            .cloned()
            .unwrap_or_default();
        if !manifest.name.is_empty() {
            entry.name = manifest.name;
        }
        if !manifest.description.is_empty() {
            entry.summary = manifest.description;
        }
        if !manifest.author.is_empty() {
            entry.author = manifest.author;
        }
        if !manifest.license.is_empty() && entry.license.is_empty() {
            entry.license = manifest.license;
        }
        entry.version = manifest.version;
        entry.category = "Widgets".into();
        entry.install = Some(Install {
            method: InstallMethod::OmarchyPlugin,
            command: format!("omarchy plugin add {}.git --enable", repo.html_url),
            note: "Not listed on plugins.omarchy.org; found by its manifest on GitHub.".into(),
        });
        if let Some(listing) = &listing {
            listing.apply(&mut entry);
        }
        return Classified::Plugin(entry);
    }

    let mut entry = base;
    let looks_like_software = paths.iter().any(|p| {
        p.ends_with(".desktop")
            || matches!(
                p.as_str(),
                "Cargo.toml"
                    | "package.json"
                    | "go.mod"
                    | "pyproject.toml"
                    | "setup.py"
                    | "Makefile"
                    | "meson.build"
                    | "CMakeLists.txt"
                    | "PKGBUILD"
                    | "install.sh"
                    | "omastore.toml"
                    | "main.go"
                    | "bin"
                    | "src"
            )
    });
    if !looks_like_software && listing.is_none() {
        return Classified::Skip;
    }
    entry.category = "Apps".into();
    entry.install = Some(if has("install.sh") {
        entry.subkind = "source".into();
        Install {
            method: InstallMethod::InstallScript,
            command: format!("git clone {} && sh install.sh", repo.html_url),
            note: String::new(),
        }
    } else if has("omastore.toml") {
        entry.subkind = "github-release".into();
        Install {
            method: InstallMethod::GithubRelease,
            command: String::new(),
            note: "Ships releases on GitHub; see the project's releases page.".into(),
        }
    } else {
        entry.subkind = "source".into();
        Install {
            method: InstallMethod::Manual,
            command: String::new(),
            note: "See the README for how to install.".into(),
        }
    });
    if let Some(listing) = &listing {
        listing.apply(&mut entry);
    }
    Classified::App(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, license: Option<&str>, pushed_days_ago: i64) -> Repo {
        Repo {
            full_name: name.into(),
            description: Some("A thing".into()),
            stargazers_count: 1,
            license: license.map(|id| License {
                spdx_id: Some(id.into()),
            }),
            topics: vec!["omarchy".into()],
            pushed_at: Utc::now() - Duration::days(pushed_days_ago),
            created_at: Utc::now(),
            archived: false,
            fork: false,
            default_branch: "main".into(),
            html_url: format!("https://github.com/{name}"),
            owner: Owner {
                login: name.split('/').next().unwrap().into(),
            },
        }
    }

    #[test]
    fn the_floor_keeps_live_licensed_projects() {
        let now = Utc::now();
        assert!(passes_floor(&repo("a/tool", Some("MIT"), 10), now));
        assert!(!passes_floor(&repo("a/tool", None, 10), now));
        assert!(!passes_floor(&repo("a/tool", Some("NOASSERTION"), 10), now));
        assert!(!passes_floor(&repo("a/tool", Some("MIT"), 700), now));
        assert!(!passes_floor(
            &repo("a/omarchy-dotfiles", Some("MIT"), 1),
            now
        ));
        assert!(!passes_floor(
            &repo("a/omarchy-rose-theme", Some("MIT"), 1),
            now
        ));
    }

    fn app() -> Entry {
        Entry {
            id: "gh:a/tool".into(),
            kind: Kind::App,
            subkind: "source".into(),
            name: "tool".into(),
            summary: "A thing".into(),
            description: String::new(),
            author: "a".into(),
            author_key: "a".into(),
            repo: "https://github.com/a/tool".into(),
            homepage: String::new(),
            listing_url: String::new(),
            license: "MIT".into(),
            stars: 0,
            category: "Apps".into(),
            tags: vec!["rust".into()],
            version: String::new(),
            install: Some(Install {
                method: InstallMethod::Manual,
                command: String::new(),
                note: "See the README for how to install.".into(),
            }),
            previews: Vec::new(),
            thumbnail: "https://opengraph.githubusercontent.com/1/a/tool".into(),
            first_seen: Utc::now().date_naive(),
            upstream_added: None,
            updated_at: None,
            wants_contributors: false,
            source: "github".into(),
            status: String::new(),
            verified: false,
        }
    }

    #[test]
    fn an_empty_manifest_opts_in_and_changes_nothing() {
        let manifest = OscoveryToml::parse("", "a/tool");
        assert!(manifest.listed);
        let mut entry = app();
        manifest.apply(&mut entry);
        assert_eq!(entry, app());
    }

    #[test]
    fn a_manifest_can_opt_out_and_a_broken_one_does() {
        assert!(!OscoveryToml::parse("listed = false", "a/tool").listed);
        assert!(!OscoveryToml::parse("listed = \"yes", "a/tool").listed);
    }

    #[test]
    fn a_manifest_corrects_the_guesses() {
        let manifest = OscoveryToml::parse(
            r#"
            summary = "Better words"
            category = "Media"
            tags = ["Audio", "rust"]
            wants_contributors = true
            previews = ["http://insecure.png", "https://example.com/shot.png"]
            install = "github-release"
            install_note = "Grab the tarball."
            some_future_field = 1
            "#,
            "a/tool",
        );
        let mut entry = app();
        manifest.apply(&mut entry);
        assert_eq!(entry.summary, "Better words");
        assert_eq!(entry.category, "Media");
        assert_eq!(entry.tags, ["rust", "audio"]);
        assert!(entry.wants_contributors);
        assert_eq!(entry.previews, ["https://example.com/shot.png"]);
        assert_eq!(entry.thumbnail, "https://example.com/shot.png");
        let install = entry.install.unwrap();
        assert_eq!(install.method, InstallMethod::GithubRelease);
        assert_eq!(install.note, "Grab the tarball.");
        assert_eq!(entry.subkind, "github-release");
    }

    #[test]
    fn a_manifest_cannot_pick_a_method_that_runs_something_else() {
        let manifest = OscoveryToml::parse(r#"install = "omarchy-plugin""#, "a/tool");
        let mut entry = app();
        manifest.apply(&mut entry);
        assert_eq!(entry.install, app().install);
    }
}
