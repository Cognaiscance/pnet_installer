//! Store catalog built from `app_sources/` GitHub URL lists.

use std::path::Path;

use crate::card;
use crate::fetch::{self, CacheEntry, Fetcher};
use crate::sources::{self, SourceEntry};

pub use crate::card::{id_from_repo, install_cmd, valid_id, CatalogApp};

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub apps: Vec<CatalogApp>,
}

impl Catalog {
    pub fn baked() -> Self {
        Catalog {
            apps: official_baked()
                .into_iter()
                .map(|(_, a)| a)
                .collect(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&CatalogApp> {
        self.apps.iter().find(|a| a.id == id)
    }

    pub fn all(&self) -> &[CatalogApp] {
        &self.apps
    }

    /// Ensure `app_sources/`, parse files, attach cards (cache / fetch / baked).
    pub fn load(state_dir: &Path, fetcher: &dyn Fetcher, network: bool) -> Result<Self, String> {
        let dir = sources::ensure_app_sources(state_dir)?;
        let (entries, warnings) = sources::read_entries(&dir);
        for w in &warnings {
            if w.line == 0 {
                eprintln!("[installer] app_sources {}: {}", w.file, w.message);
            } else {
                eprintln!(
                    "[installer] app_sources {}:{}: {}",
                    w.file, w.line, w.message
                );
            }
        }
        let baked = official_baked();
        let now = fetch::unix_now();
        let mut apps = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        for src in entries {
            let mut app = resolve_one(&src, &baked, state_dir, fetcher, network, now);
            if !seen_ids.insert(app.id.clone()) {
                eprintln!(
                    "[installer] catalog skip duplicate id {} from {}",
                    app.id, src.url
                );
                continue;
            }
            if app.source_file.is_empty() {
                app.source_file = src.source_file.clone();
            }
            apps.push(app);
        }
        Ok(Catalog { apps })
    }
}

fn resolve_one(
    src: &SourceEntry,
    baked: &[(String, CatalogApp)],
    state_dir: &Path,
    fetcher: &dyn Fetcher,
    network: bool,
    now: u64,
) -> CatalogApp {
    let cached = fetch::load_cache(state_dir, &src.owner, &src.repo);
    if network && cached.as_ref().map(|c| fetch::cache_fresh(c, now)) != Some(true) {
        match fetch::fetch_listing(src, fetcher) {
            Ok((mut app, kind)) => {
                overlay_baked_id(&mut app, src, baked);
                app.source_file = src.source_file.clone();
                app.github_url = src.url.clone();
                app.install_cmd = card::install_cmd(&src.url, &app.crate_name, app.web_slug.as_deref());
                let _ = fetch::save_cache(
                    state_dir,
                    &CacheEntry {
                        url: src.url.clone(),
                        fetched_at: now,
                        source: kind.to_string(),
                        app: app.clone(),
                    },
                );
                return app;
            }
            Err(e) => {
                eprintln!("[installer] catalog {}/{}: {e}", src.owner, src.repo);
            }
        }
    }
    if let Some(c) = cached {
        let mut app = c.app;
        app.source_file = src.source_file.clone();
        app.github_url = src.url.clone();
        return app;
    }
    if let Some((_, b)) = baked.iter().find(|(u, _)| url_key(u) == url_key(&src.url)) {
        let mut app = b.clone();
        app.source_file = src.source_file.clone();
        return app;
    }
    minimal_card(src)
}

fn overlay_baked_id(app: &mut CatalogApp, src: &SourceEntry, baked: &[(String, CatalogApp)]) {
    if let Some((_, b)) = baked.iter().find(|(u, _)| url_key(u) == url_key(&src.url)) {
        // Keep stable catalog ids for official apps even if the repo name differs.
        if app.id != b.id && card::valid_id(&b.id) {
            app.id = b.id.clone();
        }
        if app.fabric_alias.is_empty() {
            app.fabric_alias = b.fabric_alias.clone();
        }
        if app.web_slug.is_none() {
            app.web_slug = b.web_slug.clone();
        }
        if app.placement.is_empty() {
            app.placement = b.placement.clone();
        }
    }
}

fn url_key(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn minimal_card(src: &SourceEntry) -> CatalogApp {
    let id = id_from_repo(&src.repo);
    CatalogApp::new(
        &id,
        &src.repo,
        &format!("From {}. Listing not cached yet.", src.url),
        "See the app README.",
        "Linux (v1)",
        src,
        &src.repo,
        &id,
        None,
        &format!("Added from {}.", src.source_file),
        "preview",
    )
}

fn official_source(url: &str) -> SourceEntry {
    let parsed = sources::parse_github_url(url).expect("official URL");
    SourceEntry {
        url: parsed.url,
        owner: parsed.owner,
        repo: parsed.repo,
        source_file: sources::MANAGED_FILE.to_string(),
    }
}

fn official_baked() -> Vec<(String, CatalogApp)> {
    let filesync = official_source(sources::OFFICIAL_URLS[0]);
    let hello = official_source(sources::OFFICIAL_URLS[1]);
    let chat = official_source(sources::OFFICIAL_URLS[2]);
    let installer = official_source(sources::OFFICIAL_URLS[3]);
    vec![
        (
            filesync.url.clone(),
            CatalogApp::new(
                "filesync",
                "Filesync",
                "Folder replica plus portal web viewport.",
                "Desktops you want in the set; also the rank-1 SG for always-on web.",
                "Linux (v1)",
                &filesync,
                "pnet_filesync",
                "filesync",
                Some("filesync"),
                "Approve in Config → Pending Apps unless PNET_AUTO_APPROVE_APPS=1.",
                "available",
            ),
        ),
        (
            hello.url.clone(),
            CatalogApp::new(
                "hello",
                "Hello",
                "Sample hybrid page at /apps/hello/.",
                "Usually the SG (portal demo).",
                "Linux (v1)",
                &hello,
                "pnet_web_hello",
                "web-hello",
                Some("hello"),
                "Smoke-test for portal mounts.",
                "available",
            ),
        ),
        (
            chat.url.clone(),
            CatalogApp::new(
                "chat",
                "Chat",
                "Room-oriented chat (pipe + framing; rooms later).",
                "Host on rank-1 SG when rooms land; agents on member devices.",
                "Linux (preview)",
                &chat,
                "pnet_chat",
                "pnet-chat",
                None,
                "Preview / skeleton. Not a full product yet.",
                "preview",
            ),
        ),
        (
            installer.url.clone(),
            CatalogApp::new(
                "installer",
                "Installer",
                "This agent: catalog, desire, and status across your devices.",
                "Every device that runs pNet (especially the rank-1 SG for the UI).",
                "Linux (v1)",
                &installer,
                "pnet_installer",
                "installer",
                Some("installer"),
                "Extra apps: drop a GitHub URL list in app_sources/. Catalog stays notify-only until signed install (phase 4).",
                "available",
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::Fetcher;
    use std::collections::HashMap;
    use std::fs;

    struct MapFetcher(HashMap<String, String>);
    impl Fetcher for MapFetcher {
        fn get_text(&self, url: &str) -> Result<String, String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| format!("missing {url}"))
        }
    }

    fn tmp() -> std::path::PathBuf {
        let mut n = [0u8; 8];
        let _ = getrandom::getrandom(&mut n);
        let p = std::env::temp_dir().join(format!("pnet-cat-{:x}", u64::from_le_bytes(n)));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn baked_ids_unique_and_valid() {
        let c = Catalog::baked();
        let mut seen = std::collections::HashSet::new();
        for a in c.all() {
            assert!(valid_id(&a.id), "bad id {}", a.id);
            assert!(seen.insert(a.id.clone()), "duplicate {}", a.id);
            assert!(a.github_url.starts_with("https://github.com/"));
            assert!(a.install_cmd.contains("git clone"));
        }
        assert!(c.get("filesync").is_some());
        assert!(c.get("hello").is_some());
        assert!(c.get("nope").is_none());
    }

    #[test]
    fn load_offline_includes_official_and_extra_minimal() {
        let state = tmp();
        sources::ensure_app_sources(&state).unwrap();
        fs::write(
            sources::sources_dir(&state).join("acme.list"),
            "https://github.com/acme/pnet-timesheets\n",
        )
        .unwrap();
        let cat = Catalog::load(&state, &MapFetcher(HashMap::new()), false).unwrap();
        assert!(cat.get("filesync").is_some());
        let extra = cat.get("timesheets").expect("extra app");
        assert_eq!(extra.source_file, "acme.list");
        assert!(extra.github_url.contains("pnet-timesheets"));
        assert_eq!(extra.status, "preview");
    }

    #[test]
    fn load_network_uses_manifest_and_cache() {
        let state = tmp();
        sources::ensure_app_sources(&state).unwrap();
        fs::write(
            sources::sources_dir(&state).join("acme.list"),
            "https://github.com/acme/pnet-timesheets\n",
        )
        .unwrap();
        let mut map = HashMap::new();
        map.insert(
            "https://raw.githubusercontent.com/acme/pnet-timesheets/HEAD/pnet-app.json".into(),
            r#"{"id":"timesheets","name":"Timesheets","summary":"Hours from GH"}"#.into(),
        );
        let cat = Catalog::load(&state, &MapFetcher(map), true).unwrap();
        assert_eq!(cat.get("timesheets").unwrap().summary, "Hours from GH");
        // Fetcher empty: still served from cache.
        let cat2 = Catalog::load(&state, &MapFetcher(HashMap::new()), false).unwrap();
        assert_eq!(cat2.get("timesheets").unwrap().summary, "Hours from GH");
    }

    #[test]
    fn id_from_repo_strips_prefix() {
        assert_eq!(id_from_repo("pnet_filesync"), "filesync");
        assert_eq!(id_from_repo("pnet-timesheets"), "timesheets");
        assert_eq!(id_from_repo("pnet_web_hello"), "web-hello");
    }
}
