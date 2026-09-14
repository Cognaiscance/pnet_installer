//! One store card (owned). Shared by catalog load, cache, and UI.

use serde::{Deserialize, Serialize};

use crate::sources::SourceEntry;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub placement: String,
    pub os: String,
    pub github_url: String,
    pub crate_name: String,
    pub install_cmd: String,
    pub fabric_alias: String,
    pub web_slug: Option<String>,
    pub notes: String,
    pub source_file: String,
    pub status: String,
}

impl CatalogApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        name: &str,
        summary: &str,
        placement: &str,
        os: &str,
        src: &SourceEntry,
        crate_name: &str,
        fabric_alias: &str,
        web_slug: Option<&str>,
        notes: &str,
        status: &str,
    ) -> Self {
        CatalogApp {
            id: id.to_string(),
            name: name.to_string(),
            summary: summary.to_string(),
            placement: placement.to_string(),
            os: os.to_string(),
            github_url: src.url.clone(),
            crate_name: crate_name.to_string(),
            install_cmd: install_cmd(&src.url, crate_name, web_slug),
            fabric_alias: fabric_alias.to_string(),
            web_slug: web_slug.map(|s| s.to_string()),
            notes: notes.to_string(),
            source_file: src.source_file.clone(),
            status: status.to_string(),
        }
    }
}

pub fn install_cmd(url: &str, crate_name: &str, web_slug: Option<&str>) -> String {
    let mut s = format!("git clone {url}\ncd {crate_name}\ncargo run\n");
    if let Some(slug) = web_slug {
        s.push_str(&format!("# UI: /apps/{slug}/\n"));
    }
    s
}

/// Catalog ids are lowercase `[a-z0-9-]+` (same idea as portal slugs).
pub fn valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    id.bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// `pnet_filesync` / `pnet-timesheets` → `filesync` / `timesheets`.
pub fn id_from_repo(repo: &str) -> String {
    let mut s = repo.to_ascii_lowercase();
    if let Some(r) = s.strip_prefix("pnet_") {
        s = r.to_string();
    } else if let Some(r) = s.strip_prefix("pnet-") {
        s = r.to_string();
    }
    s = s.replace('_', "-");
    let s: String = s
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .take(32)
        .collect();
    if valid_id(&s) {
        s
    } else {
        "app".into()
    }
}
