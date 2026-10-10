//! Fetching the published index: one JSON file behind a CDN, with an ETag so
//! the daily check is a 304 most days. A path or `file://` URL reads a local
//! build, for development and for the `oscovery import` command.

use crate::error::{Error, Result};
use crate::models::Index;
use std::path::Path;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("Oscovery/", env!("CARGO_PKG_VERSION"));

pub enum Fetched {
    /// The server's copy is the one we have.
    NotModified,
    Index {
        index: Index,
        etag: Option<String>,
    },
}

/// Fetch `url` unless it still matches `etag`.
pub async fn fetch(url: &str, etag: Option<&str>) -> Result<Fetched> {
    if let Some(path) = local_path(url) {
        return Ok(Fetched::Index {
            index: read_file(Path::new(&path))?,
            etag: None,
        });
    }
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut request = client.get(url);
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    let response = response.error_for_status()?;
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = response.bytes().await?;
    let index = parse(&bytes)?;
    Ok(Fetched::Index { index, etag })
}

pub fn read_file(path: &Path) -> Result<Index> {
    parse(&std::fs::read(path)?)
}

fn parse(bytes: &[u8]) -> Result<Index> {
    let index: Index = serde_json::from_slice(bytes)?;
    if index.version > Index::VERSION {
        return Err(Error::IndexVersion(index.version, Index::VERSION));
    }
    Ok(index)
}

/// A plain path or a `file://` URL, as a path.
fn local_path(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(path) = url.strip_prefix("file://") {
        return Some(path.to_string());
    }
    (url.starts_with('/') || url.starts_with("~/")).then(|| {
        url.strip_prefix("~/")
            .and_then(|rest| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| format!("{home}/{rest}"))
            })
            .unwrap_or_else(|| url.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_urls_are_paths() {
        assert_eq!(
            local_path("file:///tmp/index.json"),
            Some("/tmp/index.json".into())
        );
        assert_eq!(
            local_path("/tmp/index.json"),
            Some("/tmp/index.json".into())
        );
        assert_eq!(local_path("https://example.org/index.json"), None);
    }

    #[test]
    fn a_newer_index_is_refused() {
        let json = format!(
            r#"{{"version":{},"generated_at":"2026-10-10T00:00:00Z","entries":[]}}"#,
            Index::VERSION + 1
        );
        assert!(matches!(
            parse(json.as_bytes()),
            Err(Error::IndexVersion(..))
        ));
    }
}
