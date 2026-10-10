//! `oscovery-index build`: the catalog index, from the Omarchy plugin
//! marketplace, GitHub repositories that opted in (an `oscovery.toml`, or
//! the `omarchy-plugin` / `omarchy-app` topic) and the AUR. Any of them can
//! opt out with `listed = false`, or be named in `sources/removed.txt`. Runs daily in
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
use std::collections::{BTreeMap, HashMap, HashSet};
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

/// A submitted project: `sources/<name>.toml`. Listed when the person who
/// submitted it owns the repository, or the repository opted in itself.
#[derive(Default, serde::Deserialize)]
struct Submission {
    repo: String,
    /// The GitHub login that asked for it (the issue's author).
    #[serde(default)]
    submitted_by: String,
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
                    let header = |name: &str| {
                        response
                            .headers()
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                    };
                    // The hourly allowance is spent: wait for it to come back
                    // rather than give up on the request.
                    let wait = if header("x-ratelimit-remaining") == Some(0)
                        && let Some(reset) = header("x-ratelimit-reset")
                    {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs());
                        reset.saturating_sub(now).saturating_add(2).min(3700)
                    } else {
                        header("retry-after").unwrap_or(30 * (attempt + 1)).min(120)
                    };
                    eprintln!("rate limited on {url}; waiting {wait}s");
                    tokio::time::sleep(Duration::from_secs(wait)).await;
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

    // 2. GitHub: plugins the marketplace missed, and apps, from repositories
    // that opted in. One with an `oscovery.toml` asked to be listed, so only
    // the topic ones have to pass the quality floor.
    if args.github {
        let opted_in = github::opted_in(&http).await;
        eprintln!(
            "github: {} repositories with an {}",
            opted_in.len(),
            github::MANIFEST
        );
        let opted: HashSet<String> = opted_in
            .iter()
            .map(|repo| repo.full_name.to_lowercase())
            .collect();
        let tagged = github::search(&http, args.github_limit).await;
        eprintln!("github: {} repositories under the topics", tagged.len());
        let now = Utc::now();
        let candidates: Vec<_> = opted_in
            .into_iter()
            .filter(|repo| !repo.archived)
            .chain(tagged.into_iter().filter(|repo| {
                !opted.contains(&repo.full_name.to_lowercase()) && github::passes_floor(repo, now)
            }))
            .filter(|repo| !known_repos.contains_key(&repo.full_name.to_lowercase()))
            .collect();
        eprintln!("github: {} to classify", candidates.len());
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
            let consented = submission
                .submitted_by
                .eq_ignore_ascii_case(&repo.owner.login)
                || repo
                    .topics
                    .iter()
                    .any(|t| github::OPT_IN_TOPICS.contains(&t.as_str()))
                || github::manifest(&http, &repo.full_name, &repo.default_branch)
                    .await
                    .is_some();
            if !consented {
                eprintln!(
                    "submission {slug}: submitted by {:?}, not the owner, and the repository \
                     has no {} or opt-in topic; left out",
                    submission.submitted_by,
                    github::MANIFEST
                );
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

    // 4. Opt-outs. The GitHub crawl read its repositories' manifests
    // already; a marketplace or AUR project on GitHub can say
    // `listed = false` (or ask for contributors) in one too. Then whatever
    // sources/removed.txt names.
    let unread: Vec<(usize, String)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.source != "github" && e.source != "submitted")
        .filter_map(|(i, e)| oscovery_core::models::github_slug(&e.repo).map(|slug| (i, slug)))
        .collect();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    for (index, slug) in unread {
        let http = http.clone();
        let permit = semaphore.clone().acquire_owned().await?;
        tasks.spawn(async move {
            let manifest = github::manifest(&http, &slug, "HEAD").await;
            drop(permit);
            (index, manifest)
        });
    }
    let mut unlisted = HashSet::new();
    while let Some(result) = tasks.join_next().await {
        if let (index, Some(manifest)) = result? {
            if manifest.listed {
                entries[index].wants_contributors |= manifest.wants_contributors;
            } else {
                unlisted.insert(entries[index].id.clone());
            }
        }
    }
    let removed = load_removed(&args.sources.join("removed.txt"));
    let before = entries.len();
    entries.retain(|e| !unlisted.contains(&e.id) && !is_removed(e, &removed));
    eprintln!(
        "opt-outs: {} by manifest, {} in all",
        unlisted.len(),
        before - entries.len()
    );

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

/// `sources/removed.txt`: one project per line, as a repository URL,
/// `owner/repo`, `owner/*` (everything of theirs) or a catalog id
/// (`aur:name`, a plugin id). `#` starts a comment.
fn load_removed(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(|line| {
            oscovery_core::models::github_slug(line)
                .unwrap_or_else(|| line.to_string())
                .to_lowercase()
        })
        .collect()
}

fn is_removed(entry: &Entry, removed: &[String]) -> bool {
    let id = entry.id.to_lowercase();
    let key = repo_key(&entry.repo);
    let owner = key.as_deref().and_then(|k| k.split('/').next());
    removed.iter().any(|line| {
        *line == id
            || Some(line.as_str()) == key.as_deref()
            || line
                .strip_suffix("/*")
                .is_some_and(|wild| Some(wild) == owner)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_matches_ids_repositories_and_owners() {
        let dir = std::env::temp_dir().join(format!("oscovery-removed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("removed.txt");
        std::fs::write(
            &path,
            "# asked on #12\nhttps://github.com/Some/Tool.git\naur:thing  # theirs\nquiet/*\n\n",
        )
        .unwrap();
        let removed = load_removed(&path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(removed, ["some/tool", "aur:thing", "quiet/*"]);

        let entry = |id: &str, repo: &str| Entry {
            id: id.into(),
            repo: repo.into(),
            ..serde_json::from_value(serde_json::json!({
                "id": "", "kind": "app", "name": "", "first_seen": "2026-01-01"
            }))
            .unwrap()
        };
        assert!(is_removed(
            &entry("gh:some/tool", "https://github.com/some/tool"),
            &removed
        ));
        assert!(is_removed(&entry("aur:thing", ""), &removed));
        assert!(is_removed(
            &entry("x.y.z", "https://github.com/Quiet/plugin"),
            &removed
        ));
        assert!(!is_removed(
            &entry("gh:some/tool2", "https://github.com/some/tool2"),
            &removed
        ));
        assert!(!is_removed(&entry("aur:thing-bin", ""), &removed));
    }
}
