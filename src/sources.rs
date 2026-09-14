//! `app_sources/` — lists of GitHub repo URLs that feed the store.
//!
//! `pnet.list` is managed (rewritten when missing or revision-stale). Any other
//! regular file is user/org config and is never overwritten.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const SOURCES_DIR: &str = "app_sources";
pub const MANAGED_FILE: &str = "pnet.list";
pub const MANAGED_REVISION: u32 = 1;

pub const OFFICIAL_URLS: &[&str] = &[
    "https://github.com/Cognaiscance/pnet_filesync",
    "https://github.com/Cognaiscance/pnet_web_hello",
    "https://github.com/Cognaiscance/pnet_chat",
    "https://github.com/Cognaiscance/pnet_installer",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    /// Canonical `https://github.com/Owner/Repo`.
    pub url: String,
    pub owner: String,
    pub repo: String,
    /// Filename inside `app_sources/` (e.g. `pnet.list`, `acme.list`).
    pub source_file: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub file: String,
    pub line: usize,
    pub message: String,
}

pub fn sources_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(SOURCES_DIR)
}

/// Create the directory and write/refresh managed `pnet.list`. Extra files are
/// left untouched.
pub fn ensure_app_sources(state_dir: &Path) -> Result<PathBuf, String> {
    let dir = sources_dir(state_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    chmod(&dir, 0o700);
    let managed = dir.join(MANAGED_FILE);
    if needs_rewrite(&managed) {
        fs::write(&managed, managed_contents()).map_err(|e| format!("write {}: {e}", managed.display()))?;
        chmod(&managed, 0o600);
    }
    Ok(dir)
}

fn needs_rewrite(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return true;
    };
    parse_managed_revision(&text) != Some(MANAGED_REVISION)
}

fn parse_managed_revision(text: &str) -> Option<u32> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# managed-revision:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

pub fn managed_contents() -> String {
    let mut s = format!(
        "# {MANAGED_FILE} — managed by pnet_installer. Do not edit.\n\
         # Add another file in this directory for extra apps (e.g. acme.list).\n\
         # managed-revision: {MANAGED_REVISION}\n\n"
    );
    for url in OFFICIAL_URLS {
        s.push_str(url);
        s.push('\n');
    }
    s
}

/// Read every eligible file. First occurrence of a normalized URL wins.
pub fn read_entries(sources_dir: &Path) -> (Vec<SourceEntry>, Vec<Warning>) {
    let mut files: Vec<PathBuf> = match fs::read_dir(sources_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => return (Vec::new(), Vec::new()),
    };
    // Official list first so extra files only add, they don't steal source labels.
    files.sort_by(|a, b| {
        let am = a.file_name().and_then(|n| n.to_str()) == Some(MANAGED_FILE);
        let bm = b.file_name().and_then(|n| n.to_str()) == Some(MANAGED_FILE);
        match (am, bm) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.cmp(b),
        }
    });
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in files {
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if skip_name(&name) {
            continue;
        }
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(Warning {
                    file: name,
                    line: 0,
                    message: format!("read error: {e}"),
                });
                continue;
            }
        };
        for (i, line) in text.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            match parse_github_url(t) {
                Ok(entry_base) => {
                    let key = format!(
                        "{}/{}",
                        entry_base.owner.to_ascii_lowercase(),
                        entry_base.repo.to_ascii_lowercase()
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    out.push(SourceEntry {
                        url: entry_base.url,
                        owner: entry_base.owner,
                        repo: entry_base.repo,
                        source_file: name.clone(),
                    });
                }
                Err(message) => warnings.push(Warning {
                    file: name.clone(),
                    line: i + 1,
                    message: message.to_string(),
                }),
            }
        }
    }
    (out, warnings)
}

fn skip_name(name: &str) -> bool {
    if name.starts_with('.') || name.ends_with('~') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".bak")
        || lower.ends_with(".tmp")
        || lower.ends_with(".swp")
        || lower.ends_with(".orig")
}

struct ParsedUrl {
    url: String,
    owner: String,
    repo: String,
}

/// Accept `https://github.com/Owner/Repo` with optional `.git` / trailing slash.
pub fn parse_github_url(raw: &str) -> Result<SourceEntry, &'static str> {
    let inner = parse_github_url_inner(raw)?;
    Ok(SourceEntry {
        url: inner.url,
        owner: inner.owner,
        repo: inner.repo,
        source_file: String::new(),
    })
}

fn parse_github_url_inner(raw: &str) -> Result<ParsedUrl, &'static str> {
    let t = raw.trim();
    let lower = t.to_ascii_lowercase();
    let prefix_len = if lower.starts_with("https://github.com/") {
        "https://github.com/".len()
    } else if lower.starts_with("http://github.com/") {
        "http://github.com/".len()
    } else if lower.starts_with("https://www.github.com/") {
        "https://www.github.com/".len()
    } else {
        return Err("not a github.com repo URL");
    };
    let mut rest = &t[prefix_len..];
    rest = rest.trim_end_matches('/');
    if let Some(stripped) = rest.strip_suffix(".git") {
        rest = stripped;
    }
    if rest.is_empty() || rest.contains('/') && rest.chars().filter(|c| *c == '/').count() != 1
    {
        return Err("expected github.com/owner/repo");
    }
    let (owner, repo) = rest.split_once('/').ok_or("expected github.com/owner/repo")?;
    if owner.is_empty() || repo.is_empty() {
        return Err("expected github.com/owner/repo");
    }
    if !valid_github_part(owner) || !valid_github_part(repo) {
        return Err("invalid owner or repo name");
    }
    if repo == "." || repo == ".." || owner == "." || owner == ".." {
        return Err("invalid owner or repo name");
    }
    Ok(ParsedUrl {
        url: format!("https://github.com/{owner}/{repo}"),
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

fn valid_github_part(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn chmod(path: &Path, mode: u32) {
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        let mut n = [0u8; 8];
        let _ = getrandom::getrandom(&mut n);
        let p = std::env::temp_dir().join(format!("pnet-src-{:x}", u64::from_le_bytes(n)));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn parse_accepts_common_shapes() {
        let a = parse_github_url("https://github.com/Cognaiscance/pnet_filesync").unwrap();
        assert_eq!(a.url, "https://github.com/Cognaiscance/pnet_filesync");
        assert_eq!(a.owner, "Cognaiscance");
        assert_eq!(a.repo, "pnet_filesync");
        let b = parse_github_url("https://github.com/Acme/App.git/").unwrap();
        assert_eq!(b.url, "https://github.com/Acme/App");
        assert!(parse_github_url("git@github.com:x/y.git").is_err());
        assert!(parse_github_url("https://gitlab.com/x/y").is_err());
        assert!(parse_github_url("https://github.com/x/y/tree/main").is_err());
    }

    #[test]
    fn ensure_writes_managed_and_leaves_extra() {
        let state = tmp();
        let dir = ensure_app_sources(&state).unwrap();
        let extra = dir.join("acme.list");
        fs::write(&extra, "https://github.com/acme/pnet-timesheets\n").unwrap();
        // Stale managed revision is rewritten; extra file stays.
        fs::write(dir.join(MANAGED_FILE), "# managed-revision: 0\nhttps://github.com/old/old\n")
            .unwrap();
        ensure_app_sources(&state).unwrap();
        let managed = fs::read_to_string(dir.join(MANAGED_FILE)).unwrap();
        assert!(managed.contains(&format!("managed-revision: {MANAGED_REVISION}")));
        assert!(managed.contains("pnet_filesync"));
        assert!(!managed.contains("github.com/old/old"));
        assert_eq!(
            fs::read_to_string(&extra).unwrap(),
            "https://github.com/acme/pnet-timesheets\n"
        );
    }

    #[test]
    fn read_skips_junk_dedups_and_warns() {
        let state = tmp();
        let dir = ensure_app_sources(&state).unwrap();
        fs::write(dir.join(".hidden.list"), "https://github.com/nope/hidden\n").unwrap();
        fs::write(dir.join("foo.bak"), "https://github.com/nope/bak\n").unwrap();
        fs::write(
            dir.join("acme.list"),
            "# org apps\n\
             https://github.com/Cognaiscance/pnet_filesync\n\
             https://github.com/acme/pnet-timesheets\n\
             not-a-url\n\
             https://github.com/acme/pnet-timesheets\n",
        )
        .unwrap();
        let (entries, warnings) = read_entries(&dir);
        let urls: Vec<_> = entries.iter().map(|e| e.url.as_str()).collect();
        assert!(urls.contains(&"https://github.com/Cognaiscance/pnet_filesync"));
        assert!(urls.contains(&"https://github.com/acme/pnet-timesheets"));
        assert!(!urls.iter().any(|u| u.contains("hidden") || u.contains("bak")));
        // Official file is sorted before acme.list; duplicate filesync dropped from acme.
        let filesync = entries
            .iter()
            .find(|e| e.repo == "pnet_filesync")
            .unwrap();
        assert_eq!(filesync.source_file, MANAGED_FILE);
        assert!(warnings.iter().any(|w| w.message.contains("not a github")));
        let timesheets = entries
            .iter()
            .find(|e| e.repo == "pnet-timesheets")
            .unwrap();
        assert_eq!(timesheets.source_file, "acme.list");
    }

    #[test]
    fn current_revision_not_rewritten() {
        let state = tmp();
        let dir = ensure_app_sources(&state).unwrap();
        let path = dir.join(MANAGED_FILE);
        let first = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{first}# keep-me\n")).unwrap();
        ensure_app_sources(&state).unwrap();
        let second = fs::read_to_string(&path).unwrap();
        assert!(second.contains("# keep-me"));
    }
}
