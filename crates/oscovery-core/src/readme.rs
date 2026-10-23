//! A project's README, fetched from GitHub when its page opens. GitHub's API
//! allows 60 anonymous requests an hour, so each README is cached on disk
//! with its ETag: within an hour it isn't asked for again, after that a
//! conditional request answers 304 (which the limit doesn't count) unless it
//! changed, and when GitHub can't be reached the cached copy is shown.

use crate::error::{Error, Result};
use crate::index::USER_AGENT;
use crate::markdown::Base;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a cached README is shown without asking GitHub.
const FRESH_FOR: chrono::TimeDelta = chrono::TimeDelta::hours(1);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Readme {
    /// `README.md`, as the repository names it.
    pub name: String,
    pub text: String,
    /// The README on github.com.
    pub html_url: String,
    pub base: Base,
}

impl Readme {
    /// Markdown, or something to show as plain text (`.rst`, `.txt`, `.org`).
    pub fn is_markdown(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        !name.contains('.')
            || [".md", ".markdown", ".mdown", ".mkd"]
                .iter()
                .any(|ext| name.ends_with(ext))
    }
}

#[derive(Serialize, Deserialize)]
struct Cached {
    etag: Option<String>,
    checked: DateTime<Utc>,
    /// `None`: the repository has no README.
    readme: Option<Readme>,
}

/// The API's answer for `GET /repos/{slug}/readme`.
#[derive(Deserialize)]
struct Contents {
    name: String,
    path: String,
    content: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    download_url: Option<String>,
}

/// The README of the GitHub repository `slug` (`owner/repo`); `None` when it
/// has none. `cache_dir` keeps one file per repository.
pub async fn fetch(slug: &str, cache_dir: &Path) -> Result<Option<Readme>> {
    let path = cache_path(cache_dir, slug);
    let cached: Option<Cached> = tokio::fs::read(&path)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if let Some(cached) = &cached
        && Utc::now() - cached.checked < FRESH_FOR
    {
        return Ok(cached.readme.clone());
    }
    match ask(slug, cached.as_ref().and_then(|c| c.etag.as_deref())).await {
        Ok(Answer::NotModified) => {
            let Some(mut cached) = cached else {
                return Err(Error::message("GitHub answered 304 for nothing cached"));
            };
            cached.checked = Utc::now();
            store(&path, &cached).await;
            Ok(cached.readme)
        }
        Ok(Answer::Readme { readme, etag }) => {
            let fresh = Cached {
                etag,
                checked: Utc::now(),
                readme,
            };
            store(&path, &fresh).await;
            Ok(fresh.readme)
        }
        Err(error) => match cached {
            Some(cached) => {
                tracing::debug!("readme {slug}: {error}; showing the cached copy");
                Ok(cached.readme)
            }
            None => Err(error),
        },
    }
}

enum Answer {
    NotModified,
    Readme {
        readme: Option<Readme>,
        etag: Option<String>,
    },
}

async fn ask(slug: &str, etag: Option<&str>) -> Result<Answer> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut request = client
        .get(format!("https://api.github.com/repos/{slug}/readme"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Answer::NotModified);
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(Answer::Readme { readme: None, etag });
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        let reset = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(|secs| DateTime::from_timestamp(secs, 0));
        return Err(Error::message(match reset {
            Some(reset) => format!(
                "GitHub's limit on anonymous requests is used up until {}",
                reset.with_timezone(&chrono::Local).format("%H:%M")
            ),
            None => format!("GitHub refused the request ({status})"),
        }));
    }
    let contents: Contents = response.error_for_status()?.json().await?;
    Ok(Answer::Readme {
        readme: Some(readme_from(slug, contents)?),
        etag,
    })
}

fn readme_from(slug: &str, contents: Contents) -> Result<Readme> {
    let packed: String = contents
        .content
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(packed)
        .map_err(|error| Error::message(format!("the README did not decode: {error}")))?;
    Ok(Readme {
        base: base_for(slug, &contents),
        name: contents.name,
        text: String::from_utf8_lossy(&bytes).into_owned(),
        html_url: contents
            .html_url
            .unwrap_or_else(|| format!("https://github.com/{slug}")),
    })
}

/// The README's URLs without its own path are the repository's root at the
/// README's ref, whatever the ref looks like.
fn base_for(slug: &str, contents: &Contents) -> Base {
    let root = |url: &Option<String>| {
        url.as_deref()
            .and_then(|url| url.strip_suffix(contents.path.as_str()))
            .map(str::to_string)
    };
    let fallback = Base::for_repo(slug);
    let dir = match contents.path.rfind('/') {
        Some(at) => contents.path[..=at].to_string(),
        None => String::new(),
    };
    Base {
        html: root(&contents.html_url).unwrap_or(fallback.html),
        raw: root(&contents.download_url).unwrap_or(fallback.raw),
        dir,
    }
}

fn cache_path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{}.json", slug.to_lowercase().replace('/', "__")))
}

async fn store(path: &Path, cached: &Cached) {
    let result = async {
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let partial = path.with_extension("part");
        tokio::fs::write(&partial, serde_json::to_vec(cached)?).await?;
        tokio::fs::rename(&partial, path).await?;
        Ok::<_, Error>(())
    }
    .await;
    if let Err(error) = result {
        tracing::debug!("could not cache {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(path: &str) -> Contents {
        Contents {
            name: "README.md".into(),
            path: path.into(),
            // "# Hi\n", wrapped the way the API wraps it.
            content: "IyBI\naQo=\n".into(),
            html_url: Some(format!("https://github.com/o/r/blob/feat/x/{path}")),
            download_url: Some(format!(
                "https://raw.githubusercontent.com/o/r/refs/heads/feat/x/{path}"
            )),
        }
    }

    #[test]
    fn the_api_answer_becomes_a_readme() {
        let readme = readme_from("o/r", contents("README.md")).unwrap();
        assert_eq!(readme.text, "# Hi\n");
        assert_eq!(readme.base.html, "https://github.com/o/r/blob/feat/x/");
        assert_eq!(
            readme.base.raw,
            "https://raw.githubusercontent.com/o/r/refs/heads/feat/x/"
        );
        assert_eq!(readme.base.dir, "");
        assert!(readme.is_markdown());

        let nested = readme_from("o/r", contents("docs/README.md")).unwrap();
        assert_eq!(nested.base.html, "https://github.com/o/r/blob/feat/x/");
        assert_eq!(nested.base.dir, "docs/");
    }

    #[tokio::test]
    async fn a_fresh_cache_answers_without_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let readme = readme_from("Some/Repo", contents("README.md")).unwrap();
        let cached = Cached {
            etag: Some("\"x\"".into()),
            checked: Utc::now(),
            readme: Some(readme.clone()),
        };
        store(&cache_path(dir.path(), "Some/Repo"), &cached).await;
        assert_eq!(fetch("some/repo", dir.path()).await.unwrap(), Some(readme));
    }
}
