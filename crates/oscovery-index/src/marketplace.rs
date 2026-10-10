//! plugins.omarchy.org: its public `catalog.json`, one entry per listed
//! plugin, already normalised with install commands and previews.

use crate::Http;
use chrono::{DateTime, NaiveDate, Utc};
use oscovery_core::models::{Entry, Install, InstallMethod, Kind, github_owner};
use serde::Deserialize;

pub const CATALOG: &str = "https://plugins.omarchy.org/catalog.json";
const SITE: &str = "https://plugins.omarchy.org/";

#[derive(Deserialize)]
struct Catalog {
    plugins: Vec<Plugin>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    source_url: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    stars: i64,
    #[serde(default)]
    added_at: Option<NaiveDate>,
    #[serde(default)]
    repository_updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    built_in: bool,
    #[serde(default)]
    install_available: bool,
    #[serde(default)]
    install_command: String,
    #[serde(default)]
    install_note: String,
    #[serde(default)]
    official_command: String,
    #[serde(default)]
    verification_status: String,
    #[serde(default)]
    preview_image: String,
    #[serde(default)]
    preview_thumbnail: String,
}

pub async fn fetch(http: &Http, source: &str) -> Result<Vec<Plugin>, Box<dyn std::error::Error>> {
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        http.get_text(source)
            .await
            .ok_or("could not fetch the marketplace catalog")?
    } else {
        std::fs::read_to_string(source)?
    };
    Ok(serde_json::from_str::<Catalog>(&text)?.plugins)
}

pub fn entry(plugin: &Plugin, today: NaiveDate) -> Option<Entry> {
    if plugin.id.is_empty() || plugin.name.is_empty() {
        return None;
    }
    let repo = if plugin.repo.is_empty() {
        plugin.source_url.clone()
    } else {
        plugin.repo.clone()
    };
    let install = if plugin.built_in {
        Some(Install {
            method: InstallMethod::OmarchyBuiltin,
            command: if plugin.official_command.is_empty() {
                format!("omarchy plugin enable {}", plugin.id)
            } else {
                plugin.official_command.clone()
            },
            note: String::new(),
        })
    } else if plugin.install_available && !plugin.install_command.is_empty() {
        Some(Install {
            method: InstallMethod::OmarchyPlugin,
            command: plugin.install_command.clone(),
            note: plugin.install_note.clone(),
        })
    } else if !repo.is_empty() {
        Some(Install {
            method: InstallMethod::Manual,
            command: String::new(),
            note: if plugin.install_note.is_empty() {
                format!("{}; see the project's README.", plugin.status)
            } else {
                plugin.install_note.clone()
            },
        })
    } else {
        None
    };
    let author_key = if plugin.built_in {
        "omarchy".to_string()
    } else {
        github_owner(&repo).unwrap_or_else(|| slug(&plugin.author))
    };
    let site = |path: &str| {
        if path.is_empty() {
            String::new()
        } else {
            format!("{SITE}{path}")
        }
    };
    Some(Entry {
        id: plugin.id.clone(),
        kind: Kind::Plugin,
        subkind: plugin.kind.to_lowercase().replace(' ', "-"),
        name: plugin.name.clone(),
        summary: plugin.description.trim().to_string(),
        description: String::new(),
        author: if plugin.built_in && plugin.author.is_empty() {
            "Omarchy".into()
        } else {
            plugin.author.clone()
        },
        author_key,
        repo: repo.clone(),
        homepage: repo,
        listing_url: format!("{SITE}plugin.html?id={}", plugin.id),
        license: plugin.license.clone(),
        stars: plugin.stars,
        category: plugin.category.clone(),
        tags: plugin.tags.clone(),
        version: plugin.version.clone(),
        install,
        previews: [site(&plugin.preview_image)]
            .into_iter()
            .filter(|url| !url.is_empty())
            .collect(),
        thumbnail: site(&plugin.preview_thumbnail),
        first_seen: today,
        upstream_added: plugin.added_at,
        updated_at: plugin.repository_updated_at,
        wants_contributors: false,
        source: "marketplace".into(),
        status: plugin.status.clone(),
        verified: plugin.verification_status == "verified",
    })
}

fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
