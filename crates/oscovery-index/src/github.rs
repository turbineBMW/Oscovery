//! GitHub: repositories under the `omarchy`, `omarchy-plugin` and
//! `omarchy-app` topics, filtered by a quality floor, classified by what
//! their root holds, with a README excerpt.

use crate::{Http, readme};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use oscovery_core::models::{Entry, Install, InstallMethod, Kind};
use serde::Deserialize;
use std::collections::HashSet;

const QUERIES: [&str; 3] = ["topic:omarchy", "topic:omarchy-plugin", "topic:omarchy-app"];
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

/// Every repository the topic searches return, deduplicated, at most
/// `limit` (0 for no limit beyond GitHub's own).
pub async fn search(http: &Http, limit: usize) -> Vec<Repo> {
    let mut seen = HashSet::new();
    let mut repos = Vec::new();
    for query in QUERIES {
        for sort in ["updated", "stars"] {
            let mut page = 1;
            loop {
                if limit > 0 && repos.len() >= limit {
                    return repos;
                }
                let url = format!(
                    "https://api.github.com/search/repositories?q={query}&sort={sort}&per_page=100&page={page}"
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
    let readme_text = match paths
        .iter()
        .find(|p| p.eq_ignore_ascii_case("README.md") || p.eq_ignore_ascii_case("README"))
    {
        Some(file) => http
            .get_text(&raw(file))
            .await
            .map(|text| readme::excerpt(&text))
            .unwrap_or_default(),
        None => return Classified::Skip,
    };

    let license = repo
        .license
        .as_ref()
        .and_then(|l| l.spdx_id.clone())
        .unwrap_or_default();
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
        summary: repo
            .description
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string(),
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
    if !looks_like_software {
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
}
