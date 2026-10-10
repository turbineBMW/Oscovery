//! `oscovery-index build`: the catalog index, from the Omarchy plugin
//! marketplace, GitHub's `omarchy*` topics and the AUR. Runs daily in
//! GitHub Actions and writes a static site: `index.json` (the whole
//! catalog), `timeline.json` (day -> ids), `manifest.json` (counts, hash)
//! and `first_seen.json` (the state, published so the next stateless build
//! can fetch it).
//!
//! `state/first_seen.json` remembers the day each id first appeared, so the
//! Timeline is stable across rebuilds. On the very first build each
//! source's own "added" date seeds it, so the timeline has history on day
//! one.

mod aur;
mod github;
mod marketplace;
mod readme;

use chrono::{NaiveDate, Utc};
use oscovery_core::models::{Entry, Index, Kind};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct Args {
    out: PathBuf,
    state: PathBuf,
    marketplace: String,
    github: bool,
    github_limit: usize,
    aur: bool,
    token: Option<String>,
    sources: PathBuf,
}

fn usage() -> ! {
    eprintln!(
        "usage: oscovery-index build --out <dir> [--state <dir>] [--marketplace <url|path>]\n\
         \x20                           [--no-github] [--github-limit <n>] [--no-aur] [--token <t>]\n\
         \n  --marketplace   the marketplace catalog.json (default: {})\n  --state         where first_seen.json lives (default: <out>/../state)\n  --sources       a directory of submitted projects, one TOML each (default: ./sources)\n  --token         a GitHub token (default: $GITHUB_TOKEN, else `gh auth token`)",
        marketplace::CATALOG
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut args = Args {
        marketplace: marketplace::CATALOG.into(),
        github: true,
        aur: true,
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
            "--github-limit" => args.github_limit = value().parse().unwrap_or_else(|_| usage()),
            "--token" => args.token = Some(value()),
            "--sources" => args.sources = PathBuf::from(value()),
            "--no-github" => args.github = false,
            "--no-aur" => args.aur = false,
            _ => usage(),
        }
    }
    if args.out.as_os_str().is_empty() {
        usage();
    }
    if args.state.as_os_str().is_empty() {
        args.state = args.out.parent().unwrap_or(Path::new(".")).join("state");
    }
    if args.sources.as_os_str().is_empty() {
        args.sources = PathBuf::from("sources");
    }
    args
}

/// A submitted project: `sources/<name>.toml`.
#[derive(Default, serde::Deserialize)]
struct Submission {
    repo: String,
    #[serde(default)]
    wants_contributors: bool,
    #[serde(default)]
    category: String,
}

fn load_submissions(dir: &Path) -> Vec<Submission> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut submissions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str::<Submission>(&text).map_err(|e| e.to_string()))
        {
            Ok(submission) if !submission.repo.trim().is_empty() => submissions.push(submission),
            Ok(_) => eprintln!("{}: no repo", path.display()),
            Err(error) => eprintln!("{}: {error}", path.display()),
        }
    }
    submissions
}

/// One HTTP client for every source, with the GitHub token when there is
/// one and a pause when GitHub asks for it.
pub struct Http {
    client: reqwest::Client,
    token: Option<String>,
}

impl Http {
    fn new(token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(oscovery_core::index::USER_AGENT)
                .timeout(Duration::from_secs(60))
                .build()
                .expect("a client"),
            token,
        }
    }

    async fn get(&self, url: &str) -> Option<reqwest::Response> {
        for attempt in 0..3 {
            let mut request = self.client.get(url);
            if url.starts_with("https://api.github.com/")
                && let Some(token) = &self.token
            {
                request = request
                    .bearer_auth(token)
                    .header("Accept", "application/vnd.github+json");
            }
            match request.send().await {
                Ok(response) if response.status().is_success() => return Some(response),
                Ok(response) if response.status() == 403 || response.status() == 429 => {
                    let wait = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(30 * (attempt + 1));
                    eprintln!("rate limited on {url}; waiting {wait}s");
                    tokio::time::sleep(Duration::from_secs(wait.min(120))).await;
                }
                Ok(response) => {
                    if response.status() != 404 {
                        eprintln!("{url}: {}", response.status());
                    }
                    return None;
                }
                Err(error) => {
                    eprintln!("{url}: {error}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        None
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Option<T> {
        let text = self.get(url).await?.text().await.ok()?;
        match serde_json::from_str(&text) {
            Ok(value) => Some(value),
            Err(error) => {
                eprintln!("{url}: {error}");
                None
            }
        }
    }

    pub async fn get_text(&self, url: &str) -> Option<String> {
        self.get(url).await?.text().await.ok()
    }
}

fn github_token(explicit: Option<String>) -> Option<String> {
    explicit
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("gh")
                .args(["auth", "token"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|t| !t.is_empty())
        })
}

/// A repository URL as a key: `owner/repo`, lower-case.
fn repo_key(url: &str) -> Option<String> {
    oscovery_core::models::github_slug(url).map(|slug| slug.to_lowercase())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    let today = Utc::now().date_naive();
    let http = Arc::new(Http::new(if args.github {
        github_token(args.token.clone())
    } else {
        None
    }));
    if args.github && http.token.is_none() {
        eprintln!("no GitHub token: the crawl will be slow and shallow");
    }

    // 1. The marketplace: every listed plugin.
    let marketplace = marketplace::fetch(&http, &args.marketplace).await?;
    let mut entries: Vec<Entry> = marketplace
        .iter()
        .filter_map(|plugin| marketplace::entry(plugin, today))
        .collect();
    eprintln!("marketplace: {} plugins", entries.len());
    let mut known_repos: HashMap<String, usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| repo_key(&e.repo).map(|k| (k, i)))
        .collect();

    // 2. GitHub: plugins the marketplace missed, and apps.
    if args.github {
        let repos = github::search(&http, args.github_limit).await;
        eprintln!("github: {} repositories under the topics", repos.len());
        let now = Utc::now();
        let candidates: Vec<_> = repos
            .into_iter()
            .filter(|repo| github::passes_floor(repo, now))
            .filter(|repo| !known_repos.contains_key(&repo.full_name.to_lowercase()))
            .collect();
        eprintln!("github: {} pass the floor and are new", candidates.len());
        let semaphore = Arc::new(tokio::sync::Semaphore::new(6));
        let mut tasks = tokio::task::JoinSet::new();
        for repo in candidates {
            let http = http.clone();
            let permit = semaphore.clone().acquire_owned().await?;
            tasks.spawn(async move {
                let classified = github::classify(&http, &repo, today).await;
                drop(permit);
                classified
            });
        }
        let (mut plugins, mut apps) = (0, 0);
        while let Some(result) = tasks.join_next().await {
            match result? {
                github::Classified::Plugin(entry) => {
                    if !entries.iter().any(|e| e.id == entry.id) {
                        plugins += 1;
                        if let Some(key) = repo_key(&entry.repo) {
                            known_repos.insert(key, entries.len());
                        }
                        entries.push(entry);
                    }
                }
                github::Classified::App(entry) => {
                    apps += 1;
                    if let Some(key) = repo_key(&entry.repo) {
                        known_repos.insert(key, entries.len());
                    }
                    entries.push(entry);
                }
                github::Classified::Skip => {}
            }
        }
        eprintln!("github: {plugins} plugins, {apps} apps added");
    }

    // 2b. Submitted projects: fetched by name, past the floor, classified
    // like the rest.
    let submissions = load_submissions(&args.sources);
    if !submissions.is_empty() {
        let mut added = 0;
        for submission in &submissions {
            let Some(slug) = oscovery_core::models::github_slug(&submission.repo) else {
                eprintln!("submission {}: not a GitHub repository", submission.repo);
                continue;
            };
            if known_repos.contains_key(&slug.to_lowercase()) {
                continue;
            }
            let url = format!("https://api.github.com/repos/{slug}");
            let Some(repo) = http.get_json::<github::Repo>(&url).await else {
                eprintln!("submission {slug}: not found");
                continue;
            };
            if repo.archived {
                continue;
            }
            let entry = match github::classify(&http, &repo, today).await {
                github::Classified::Plugin(entry) | github::Classified::App(entry) => entry,
                github::Classified::Skip => {
                    eprintln!("submission {slug}: nothing to classify (no README?)");
                    continue;
                }
            };
            let mut entry = entry;
            entry.source = "submitted".into();
            entry.wants_contributors |= submission.wants_contributors;
            if !submission.category.is_empty() {
                entry.category = submission.category.clone();
            }
            if let Some(key) = repo_key(&entry.repo) {
                known_repos.insert(key, entries.len());
            }
            entries.push(entry);
            added += 1;
        }
        eprintln!("submissions: {} files, {added} added", submissions.len());
    }

    // 3. The AUR: packages, merged onto their GitHub project when both exist.
    if args.aur {
        let packages = aur::search(&http).await;
        let (mut added, mut merged) = (0, 0);
        for package in &packages {
            let Some(entry) = aur::entry(package, today) else {
                continue;
            };
            match repo_key(&entry.repo).and_then(|key| known_repos.get(&key).copied()) {
                Some(index) if entries[index].kind == Kind::App => {
                    let app = &mut entries[index];
                    app.install = entry.install.clone();
                    app.subkind = "aur".into();
                    app.listing_url = entry.listing_url.clone();
                    if app.version.is_empty() {
                        app.version = entry.version.clone();
                    }
                    merged += 1;
                }
                Some(_) => {}
                None => {
                    entries.push(entry);
                    added += 1;
                }
            }
        }
        eprintln!(
            "aur: {} packages, {added} added, {merged} merged",
            packages.len()
        );
    }

    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| a.id == b.id);

    // first_seen: remembered per id; a fresh state takes each source's own
    // date so the first build doesn't put everything on one day.
    let mut first_seen = load_first_seen(&args.state.join("first_seen.json"));
    let fresh_state = first_seen.is_empty();
    for entry in &mut entries {
        let seen =
            first_seen
                .entry(entry.id.clone())
                .or_insert_with(|| match entry.upstream_added {
                    Some(day) if fresh_state => day,
                    _ => today,
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
    let first_seen_json = serde_json::to_vec_pretty(&first_seen)?;
    std::fs::write(args.state.join("first_seen.json"), &first_seen_json)?;
    std::fs::write(args.out.join("first_seen.json"), &first_seen_json)?;
    eprintln!(
        "wrote {} entries ({} plugins, {} apps) to {}",
        index.entries.len(),
        plugins,
        index.entries.len() - plugins,
        args.out.display()
    );
    Ok(())
}

fn load_first_seen(path: &Path) -> BTreeMap<String, NaiveDate> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}
