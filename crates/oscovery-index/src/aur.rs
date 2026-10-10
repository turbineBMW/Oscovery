//! The AUR: packages whose name or description mentions Omarchy.

use crate::Http;
use chrono::{DateTime, NaiveDate};
use oscovery_core::models::{Entry, Install, InstallMethod, Kind, github_owner};
use serde::Deserialize;

const SEARCH: &str = "https://aur.archlinux.org/rpc/v5/search/omarchy?by=name-desc";

#[derive(Deserialize)]
struct Results {
    #[serde(default)]
    results: Vec<Package>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub maintainer: Option<String>,
    #[serde(default, rename = "URL")]
    pub url: Option<String>,
    #[serde(default)]
    pub num_votes: i64,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub first_submitted: i64,
    #[serde(default)]
    pub last_modified: i64,
    #[serde(default)]
    pub out_of_date: Option<i64>,
}

/// The packages, with `foo`, `foo-bin` and `foo-git` folded into one: the
/// plain one when it exists, else `-bin`, else `-git`.
pub async fn search(http: &Http) -> Vec<Package> {
    let mut packages = http
        .get_json::<Results>(SEARCH)
        .await
        .map(|r| r.results)
        .unwrap_or_default();
    packages.sort_by_key(|p| (variant_rank(&p.name), p.name.clone()));
    let mut seen = std::collections::HashSet::new();
    packages.retain(|p| seen.insert(base_name(&p.name).to_string()));
    packages
}

pub fn base_name(name: &str) -> &str {
    name.strip_suffix("-bin")
        .or_else(|| name.strip_suffix("-git"))
        .unwrap_or(name)
}

fn variant_rank(name: &str) -> u8 {
    if name.ends_with("-bin") {
        1
    } else if name.ends_with("-git") {
        2
    } else {
        0
    }
}

pub fn entry(package: &Package, today: NaiveDate) -> Option<Entry> {
    let maintainer = package.maintainer.clone()?;
    let url = package.url.clone().unwrap_or_default();
    let author_key = github_owner(&url).unwrap_or_else(|| maintainer.to_lowercase());
    let modified = DateTime::from_timestamp(package.last_modified, 0);
    Some(Entry {
        id: format!("aur:{}", package.name),
        kind: Kind::App,
        subkind: "aur".into(),
        name: base_name(&package.name).to_string(),
        summary: package.description.clone().unwrap_or_default(),
        description: String::new(),
        author: maintainer,
        author_key,
        repo: if url.contains("github.com") {
            url.clone()
        } else {
            String::new()
        },
        homepage: url,
        listing_url: format!("https://aur.archlinux.org/packages/{}", package.name),
        license: String::new(),
        stars: package.num_votes,
        category: "Apps".into(),
        tags: Vec::new(),
        version: package.version.clone(),
        install: Some(Install {
            method: InstallMethod::Aur,
            command: format!("yay -S --needed {}", package.name),
            note: String::new(),
        }),
        previews: Vec::new(),
        thumbnail: String::new(),
        first_seen: today,
        upstream_added: DateTime::from_timestamp(package.first_submitted, 0)
            .map(|t| t.date_naive()),
        updated_at: modified,
        wants_contributors: false,
        source: "aur".into(),
        status: if package.out_of_date.is_some() {
            "Flagged out of date".into()
        } else {
            String::new()
        },
        verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_fold_onto_the_base() {
        assert_eq!(base_name("flea-bin"), "flea");
        assert_eq!(base_name("flea-git"), "flea");
        assert_eq!(base_name("flea"), "flea");
        assert!(variant_rank("flea") < variant_rank("flea-bin"));
        assert!(variant_rank("flea-bin") < variant_rank("flea-git"));
    }
}
