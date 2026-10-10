//! `oscovery-index build`: the catalog index, from the Omarchy plugin
//! marketplace today and GitHub, the AUR and submissions next. Runs daily in
//! GitHub Actions and writes a static site: `index.json` (the whole catalog),
//! `timeline.json` (day -> ids) and `manifest.json` (counts, hash).
//!
//! `state/first_seen.json` remembers the day each id first appeared, so the
//! Timeline is stable across rebuilds. On the very first build the
//! marketplace's own `addedAt` seeds it, so the timeline has history on day
//! one.

use chrono::{DateTime, NaiveDate, Utc};
use oscovery_core::models::{Entry, Index, Install, InstallMethod, Kind, github_owner};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MARKETPLACE_CATALOG: &str = "https://plugins.omarchy.org/catalog.json";
const MARKETPLACE_SITE: &str = "https://plugins.omarchy.org/";

#[derive(Default)]
struct Args {
    out: PathBuf,
    state: PathBuf,
    marketplace: String,
}

fn usage() -> ! {
    eprintln!(
        "usage: oscovery-index build --out <dir> [--state <dir>] [--marketplace <url|path>]\n\
         \n  --marketplace  the marketplace catalog.json (default: {MARKETPLACE_CATALOG})\n  --state        where first_seen.json lives (default: <out>/../state)"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut args = Args {
        marketplace: MARKETPLACE_CATALOG.into(),
        ..Default::default()
    };
    let mut argv = std::env::args().skip(1);
    if argv.next().as_deref() != Some("build") {
        usage();
    }
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().unwrap_or_else(|| usage());
        match flag.as_str() {
            "--out" => args.out = PathBuf::from(value()),
            "--state" => args.state = PathBuf::from(value()),
            "--marketplace" => args.marketplace = value(),
            _ => usage(),
        }
    }
    if args.out.as_os_str().is_empty() {
        usage();
    }
    if args.state.as_os_str().is_empty() {
        args.state = args.out.parent().unwrap_or(Path::new(".")).join("state");
    }
    args
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    let today = Utc::now().date_naive();

    let marketplace = fetch_text(&args.marketplace).await?;
    let marketplace: MarketplaceCatalog = serde_json::from_str(&marketplace)?;
    eprintln!("marketplace: {} plugins", marketplace.plugins.len());

    let mut first_seen = load_first_seen(&args.state.join("first_seen.json"));
    let fresh_state = first_seen.is_empty();

    let mut entries: Vec<Entry> = marketplace
        .plugins
        .iter()
        .filter_map(|plugin| marketplace_entry(plugin, today))
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| a.id == b.id);

    for entry in &mut entries {
        let seen = first_seen.entry(entry.id.clone()).or_insert_with(|| {
            // A fresh state file takes the upstream date, so the first build
            // doesn't put five thousand plugins on one day.
            match entry.upstream_added {
                Some(day) if fresh_state => day,
                _ => today,
            }
        });
        entry.first_seen = *seen;
    }

    let index = Index {
        version: Index::VERSION,
        generated_at: Utc::now(),
        entries,
    };
    std::fs::create_dir_all(&args.out)?;
    std::fs::create_dir_all(&args.state)?;
    let json = serde_json::to_vec(&index)?;
    std::fs::write(args.out.join("index.json"), &json)?;

    let mut timeline: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &index.entries {
        timeline
            .entry(entry.first_seen.to_string())
            .or_default()
            .push(entry.id.clone());
    }
    std::fs::write(
        args.out.join("timeline.json"),
        serde_json::to_vec(&timeline)?,
    )?;

    let plugins = index
        .entries
        .iter()
        .filter(|e| e.kind == Kind::Plugin)
        .count();
    let manifest = serde_json::json!({
        "version": Index::VERSION,
        "generated_at": index.generated_at,
        "entries": index.entries.len(),
        "plugins": plugins,
        "apps": index.entries.len() - plugins,
        "sha256": hex::encode(Sha256::digest(&json)),
        "bytes": json.len(),
    });
    std::fs::write(
        args.out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    // The state goes out with the site too, so a stateless build (GitHub
    // Actions) can fetch yesterday's from the published site.
    let first_seen_json = serde_json::to_vec_pretty(&first_seen)?;
    std::fs::write(args.state.join("first_seen.json"), &first_seen_json)?;
    std::fs::write(args.out.join("first_seen.json"), &first_seen_json)?;
    eprintln!(
        "wrote {} entries ({} plugins) to {}",
        index.entries.len(),
        plugins,
        args.out.display()
    );
    Ok(())
}

async fn fetch_text(source: &str) -> Result<String, Box<dyn std::error::Error>> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::Client::builder()
            .user_agent(oscovery_core::index::USER_AGENT)
            .build()?;
        Ok(client
            .get(source)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    } else {
        Ok(std::fs::read_to_string(source)?)
    }
}

fn load_first_seen(path: &Path) -> BTreeMap<String, NaiveDate> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

// --- the marketplace's catalog.json -------------------------------------

#[derive(Deserialize)]
struct MarketplaceCatalog {
    plugins: Vec<MarketplacePlugin>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplacePlugin {
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

fn marketplace_entry(plugin: &MarketplacePlugin, today: NaiveDate) -> Option<Entry> {
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
            format!("{MARKETPLACE_SITE}{path}")
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
        listing_url: format!("{MARKETPLACE_SITE}plugin.html?id={}", plugin.id),
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
