//! Asking the agent: the catalog directory it searches, the keyword
//! shortlist, and the blocking run moved onto the tokio pool. Used by the
//! Discover page and by `oscovery ask`.

use crate::config::DIR_NAME;
use crate::rt;
use crate::settings as keys;
use gtk::gio;
use gtk::prelude::*;
use oscovery_core::db::{Database, Query};
use oscovery_core::harness::{self, Answer, Harness, Turn};
use oscovery_core::models::Entry;
use oscovery_core::retrieve::fts_any;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// How many keyword candidates go into the prompt.
const SHORTLIST: usize = 40;

/// The agent and model Preferences name.
pub fn configured(settings: &gio::Settings) -> (Option<Harness>, String) {
    (
        harness::pick(&settings.string(keys::HARNESS)),
        settings.string(keys::MODEL).to_string(),
    )
}

/// Where the catalog directory lives.
pub fn workdir_root() -> PathBuf {
    glib::user_cache_dir().join(DIR_NAME).join("agent")
}

/// Which build of the catalog the directory should hold.
fn stamp(db: &Database) -> String {
    format!(
        "{}:{}",
        db.meta("index-generated-at")
            .ok()
            .flatten()
            .unwrap_or_default(),
        db.count().unwrap_or(0)
    )
}

/// The entries the directory needs writing with, or none when it is current.
pub fn entries_for_workdir(db: &Database) -> Option<(String, Vec<Entry>)> {
    let stamp = stamp(db);
    if oscovery_core::workdir::is_current(&workdir_root(), &stamp) {
        return None;
    }
    let entries = db
        .search(&Query {
            limit: usize::MAX / 2,
            ..Default::default()
        })
        .unwrap_or_default();
    Some((stamp, entries))
}

/// The keyword candidates for a question, inlined into the prompt.
pub fn shortlist(db: &Database, question: &str) -> Vec<Entry> {
    let Some(fts) = fts_any(question) else {
        return Vec::new();
    };
    db.search(&Query {
        fts: Some(fts),
        limit: SHORTLIST,
        ..Default::default()
    })
    .unwrap_or_default()
}

/// One question, answered: the whole job off the main thread.
pub struct Job {
    pub harness: Harness,
    pub model: String,
    pub prompt: String,
    /// `Some` when the catalog directory needs (re)writing first.
    pub catalog: Option<(String, Vec<Entry>)>,
}

pub async fn run(job: Job) -> Result<String, String> {
    let root = workdir_root();
    tokio::task::spawn_blocking(move || {
        if let Some((stamp, entries)) = job.catalog {
            oscovery_core::workdir::prepare(&root, &stamp, &entries)
                .map_err(|error| format!("could not write the catalog directory: {error}"))?;
        }
        harness::ask(job.harness, &job.model, &job.prompt, &root)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Resolve an answer's matched ids to entries, in the agent's order.
pub fn resolve(db: &Database, answer: &Answer) -> Vec<Entry> {
    answer
        .matches
        .iter()
        .filter_map(|m| db.entry(&m.id).ok().flatten())
        .collect()
}

/// Ask from the GTK side: builds the job from the database on the main
/// thread, runs it, and hands back the parsed answer with its entries.
pub fn ask(
    db: Rc<RefCell<Database>>,
    harness: Harness,
    model: String,
    turns: Vec<Turn>,
    question: String,
    on_done: impl FnOnce(Result<(Answer, Vec<Entry>), String>) + 'static,
) {
    let (catalog, prompt) = {
        let db = db.borrow();
        let shortlist = oscovery_core::workdir::shortlist(&shortlist(&db, &question));
        (
            entries_for_workdir(&db),
            harness::search_prompt(&shortlist, &turns, &question),
        )
    };
    let job = Job {
        harness,
        model,
        prompt,
        catalog,
    };
    rt::run(run(job), move |result| {
        on_done(result.map(|raw| {
            let answer = harness::parse_answer(&raw);
            let entries = resolve(&db.borrow(), &answer);
            (answer, entries)
        }))
    });
}

/// Start the agent interactively in a terminal, in `dir`, on `prompt`.
pub async fn hand_off(
    harness: Harness,
    model: String,
    dir: PathBuf,
    prompt: String,
) -> Result<(), String> {
    let program = harness
        .program()
        .ok_or_else(|| format!("{} is not installed", harness.label()))?;
    let mut command = tokio::process::Command::new("omarchy-launch-terminal");
    command
        .arg("--")
        .arg(&program)
        .args(harness.interactive_args(&model, &prompt))
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let status = command
        .status()
        .await
        .map_err(|error| format!("could not open a terminal: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("the terminal did not open".into())
    }
}

/// `git clone` into the projects directory (or update an existing clone's
/// path), returning where it landed.
pub async fn clone_into(repo: String, projects_dir: PathBuf) -> Result<PathBuf, String> {
    let name = repo
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .map(|n| n.strip_suffix(".git").unwrap_or(n))
        .filter(|n| !n.is_empty() && !n.starts_with('.'))
        .ok_or_else(|| "the repository URL has no name".to_string())?;
    let dir = projects_dir.join(name);
    if dir.join(".git").is_dir() {
        return Ok(dir);
    }
    tokio::fs::create_dir_all(&projects_dir)
        .await
        .map_err(|error| error.to_string())?;
    let output = tokio::process::Command::new("git")
        .args(["clone", "--", &repo])
        .arg(&dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|error| format!("could not run git: {error}"))?;
    if output.status.success() {
        Ok(dir)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or("git clone failed")
            .to_string())
    }
}

/// Where clones go: the setting, else ~/Projects.
pub fn projects_dir(settings: &gio::Settings) -> PathBuf {
    let configured = settings.string(keys::PROJECTS_DIR);
    if configured.trim().is_empty() {
        glib::home_dir().join("Projects")
    } else {
        PathBuf::from(configured.as_str())
    }
}
