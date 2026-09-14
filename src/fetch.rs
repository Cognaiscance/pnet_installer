//! Fetch store cards from GitHub and keep an on-disk cache.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::card::{self, CatalogApp};
use crate::sources::SourceEntry;

pub const CACHE_DIR: &str = "catalog-cache";
pub const CACHE_TTL_SECS: u64 = 6 * 3600;

pub trait Fetcher: Send + Sync {
    fn get_text(&self, url: &str) -> Result<String, String>;
}

pub struct UreqFetcher;

impl Fetcher for UreqFetcher {
    fn get_text(&self, url: &str) -> Result<String, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(8))
            .user_agent("pnet-installer/0.1 (+https://github.com/Cognaiscance/pnet_installer)")
            .build();
        let resp = agent.get(url).call().map_err(|e| e.to_string())?;
        resp.into_string().map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub fetched_at: u64,
    pub source: String,
    pub app: CatalogApp,
}

pub fn cache_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(CACHE_DIR)
}

pub fn cache_path(state_dir: &Path, owner: &str, repo: &str) -> PathBuf {
    cache_dir(state_dir).join(format!("{owner}--{repo}.json"))
}

pub fn load_cache(state_dir: &Path, owner: &str, repo: &str) -> Option<CacheEntry> {
    let text = fs::read_to_string(cache_path(state_dir, owner, repo)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_cache(state_dir: &Path, entry: &CacheEntry) -> Result<(), String> {
    let dir = cache_dir(state_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let owner_repo = entry
        .url
        .trim_start_matches("https://github.com/")
        .replace('/', "--");
    let path = dir.join(format!("{owner_repo}.json"));
    let text = serde_json::to_string_pretty(entry).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| e.to_string())
}

pub fn cache_fresh(entry: &CacheEntry, now: u64) -> bool {
    now.saturating_sub(entry.fetched_at) < CACHE_TTL_SECS
}

#[derive(Deserialize)]
struct Manifest {
    id: Option<String>,
    name: Option<String>,
    summary: Option<String>,
    placement: Option<String>,
    os: Option<String>,
    fabric_alias: Option<String>,
    web_slug: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct GhRepo {
    name: Option<String>,
    description: Option<String>,
}

/// Try `pnet-app.json` at HEAD, then the GitHub repo API.
pub fn fetch_listing(
    src: &SourceEntry,
    fetcher: &dyn Fetcher,
) -> Result<(CatalogApp, &'static str), String> {
    let manifest_url = format!(
        "https://raw.githubusercontent.com/{}/{}/HEAD/pnet-app.json",
        src.owner, src.repo
    );
    if let Ok(body) = fetcher.get_text(&manifest_url) {
        if let Ok(m) = serde_json::from_str::<Manifest>(&body) {
            return Ok((app_from_manifest(src, &m), "pnet-app.json"));
        }
    }
    let api = format!("https://api.github.com/repos/{}/{}", src.owner, src.repo);
    match fetcher.get_text(&api) {
        Ok(body) => {
            let g: GhRepo = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            Ok((app_from_github(src, &g), "github-api"))
        }
        Err(e) => Err(e),
    }
}

fn app_from_manifest(src: &SourceEntry, m: &Manifest) -> CatalogApp {
    let id = m
        .id
        .as_deref()
        .filter(|s| card::valid_id(s))
        .map(|s| s.to_string())
        .unwrap_or_else(|| card::id_from_repo(&src.repo));
    let name = m
        .name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| src.repo.clone());
    let summary = m
        .summary
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("GitHub: {}", src.url));
    let fabric = m
        .fabric_alias
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.clone());
    let web_slug = m
        .web_slug
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    CatalogApp::new(
        &id,
        &name,
        &summary,
        m.placement.as_deref().unwrap_or(""),
        m.os.as_deref().unwrap_or("Linux (v1)"),
        src,
        &src.repo,
        &fabric,
        web_slug.as_deref(),
        m.notes.as_deref().unwrap_or(""),
        "available",
    )
}

fn app_from_github(src: &SourceEntry, g: &GhRepo) -> CatalogApp {
    let id = card::id_from_repo(&src.repo);
    let name = g
        .name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| src.repo.clone());
    let summary = g
        .description
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("GitHub: {}", src.url));
    CatalogApp::new(
        &id,
        &name,
        &summary,
        "",
        "Linux (v1)",
        src,
        &src.repo,
        &id,
        None,
        &format!("Listed from {} via GitHub.", src.source_file),
        "available",
    )
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceEntry;
    use std::collections::HashMap;

    struct MapFetcher(HashMap<String, String>);
    impl Fetcher for MapFetcher {
        fn get_text(&self, url: &str) -> Result<String, String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| format!("missing {url}"))
        }
    }

    fn src() -> SourceEntry {
        SourceEntry {
            url: "https://github.com/acme/pnet-timesheets".into(),
            owner: "acme".into(),
            repo: "pnet-timesheets".into(),
            source_file: "acme.list".into(),
        }
    }

    #[test]
    fn prefers_manifest() {
        let mut map = HashMap::new();
        map.insert(
            "https://raw.githubusercontent.com/acme/pnet-timesheets/HEAD/pnet-app.json".into(),
            r#"{"id":"timesheets","name":"Timesheets","summary":"Hours","fabric_alias":"timesheets","web_slug":"timesheets"}"#.into(),
        );
        let (app, kind) = fetch_listing(&src(), &MapFetcher(map)).unwrap();
        assert_eq!(kind, "pnet-app.json");
        assert_eq!(app.id, "timesheets");
        assert_eq!(app.summary, "Hours");
        assert_eq!(app.web_slug.as_deref(), Some("timesheets"));
        assert_eq!(app.source_file, "acme.list");
    }

    #[test]
    fn falls_back_to_github_api() {
        let mut map = HashMap::new();
        map.insert(
            "https://api.github.com/repos/acme/pnet-timesheets".into(),
            r#"{"name":"pnet-timesheets","description":"Track hours"}"#.into(),
        );
        let (app, kind) = fetch_listing(&src(), &MapFetcher(map)).unwrap();
        assert_eq!(kind, "github-api");
        assert_eq!(app.id, "timesheets");
        assert_eq!(app.summary, "Track hours");
    }

    #[test]
    fn cache_roundtrip() {
        let mut n = [0u8; 8];
        let _ = getrandom::getrandom(&mut n);
        let dir = std::env::temp_dir().join(format!("pnet-cache-{:x}", u64::from_le_bytes(n)));
        fs::create_dir_all(&dir).unwrap();
        let app = CatalogApp::new(
            "timesheets",
            "Timesheets",
            "Hours",
            "",
            "Linux (v1)",
            &src(),
            "pnet-timesheets",
            "timesheets",
            None,
            "",
            "available",
        );
        let entry = CacheEntry {
            url: src().url,
            fetched_at: 100,
            source: "pnet-app.json".into(),
            app,
        };
        save_cache(&dir, &entry).unwrap();
        let loaded = load_cache(&dir, "acme", "pnet-timesheets").unwrap();
        assert_eq!(loaded.app.id, "timesheets");
        assert!(cache_fresh(&loaded, 100 + 60));
        assert!(!cache_fresh(&loaded, 100 + CACHE_TTL_SECS + 1));
    }
}
