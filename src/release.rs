//! Download the latest published `pnet` archive and check it before use.
//!
//! The index layout is `pNet/descriptions/release-assets.md`. sha256 is of
//! the gzip archive, not of the binary inside it. A hash mismatch writes
//! nothing and does not exec.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// `releases/latest` is the newest published tag. `develop` is not a version.
pub const DEFAULT_INDEX_URL: &str =
    "https://github.com/Cognaiscance/pNet/releases/latest/download/index.json";

/// Larger than a release `pnet` built with rustls, small enough to reject a
/// runaway download. Applies to the index body and the archive body.
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// A candidate unpacked from a release archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedRelease {
    /// Directory that contains the unpacked `pnet`. Removed by the caller.
    pub dir: PathBuf,
    /// `version` from the index, `major.minor.patch`.
    pub version: String,
    /// Target triple the archive was selected for.
    pub target: String,
}

pub trait ReleaseFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// HTTPS GET used when no local `pnet` was given.
pub struct UreqFetch;

impl ReleaseFetch for UreqFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .try_proxy_from_env(true)
            .build();
        let response = agent
            .get(url)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        let mut buf = Vec::new();
        response
            .into_reader()
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("GET {url}: {e}"))?;
        if buf.len() > MAX_BYTES {
            return Err(format!(
                "GET {url}: response is larger than {MAX_BYTES} bytes"
            ));
        }
        Ok(buf)
    }
}

/// Rust target triple of this installer, which is the archive it can run.
pub fn host_target() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        (os, arch) => Err(format!(
            "no published pnet build for {os}-{arch}; pass --from DIR"
        )),
    }
}

#[derive(Debug, serde::Deserialize)]
struct ReleaseIndex {
    version: String,
    tag: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct ReleaseAsset {
    target: String,
    name: String,
    url: String,
    sha256: String,
}

/// Fetch the index, check the archive for `target`, and unpack `pnet`.
///
/// The hash is compared before the directory is created. A mismatch leaves
/// `parent` unchanged and does not chmod or exec.
pub fn stage_release(
    fetch: &dyn ReleaseFetch,
    index_url: &str,
    target: &str,
    parent: &Path,
) -> Result<StagedRelease, String> {
    if !is_https_url(index_url) {
        return Err("release index URL must be https".into());
    }
    let index_bytes = fetch.get(index_url).map_err(|e| {
        format!(
            "{e}\nNo published pNet release was read. Pass --from DIR to use a local pnet binary."
        )
    })?;
    if index_bytes.len() > MAX_BYTES {
        return Err(format!("release index is larger than {MAX_BYTES} bytes"));
    }
    let index: ReleaseIndex = serde_json::from_slice(&index_bytes).map_err(|e| {
        format!(
            "release index is not JSON: {e}\nNo published pNet release was read. Pass --from DIR to use a local pnet binary."
        )
    })?;
    let asset = select_asset(&index, target)?;
    let archive = fetch.get(&asset.url)?;
    if archive.len() > MAX_BYTES {
        return Err(format!("{} is larger than {MAX_BYTES} bytes", asset.name));
    }
    let got = sha256_hex(&archive);
    if !hashes_equal(&got, &asset.sha256) {
        return Err(format!(
            "sha256 of {} is {got}, index says {}",
            asset.name, asset.sha256
        ));
    }

    let dir = unique_dir(parent)?;
    if let Err(e) = extract_pnet(&archive, &dir) {
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(StagedRelease {
        dir,
        version: index.version,
        target: target.to_string(),
    })
}

fn select_asset(index: &ReleaseIndex, target: &str) -> Result<ReleaseAsset, String> {
    if !is_semver(&index.version) {
        return Err(format!(
            "release index version is not major.minor.patch: {}",
            index.version
        ));
    }
    let expected_tag = format!("v{}", index.version);
    if index.tag != expected_tag {
        return Err(format!(
            "release index tag is {}, expected {expected_tag}",
            index.tag
        ));
    }
    let mut matches = index.assets.iter().filter(|asset| asset.target == target);
    let Some(asset) = matches.next() else {
        return Err(format!("release index has no archive for {target}"));
    };
    if matches.next().is_some() {
        return Err(format!(
            "release index lists more than one archive for {target}"
        ));
    }
    let expected_name = format!("pnet-{}-{target}.tar.gz", index.version);
    if asset.name != expected_name {
        return Err(format!(
            "release asset name is {}, expected {expected_name}",
            asset.name
        ));
    }
    if !is_https_url(&asset.url) {
        return Err(format!("release asset URL is not https: {}", asset.url));
    }
    if !is_sha256_hex(&asset.sha256) {
        return Err(format!(
            "release asset sha256 is not 64 lowercase hex digits: {}",
            asset.sha256
        ));
    }
    Ok(asset.clone())
}

fn is_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    !rest.is_empty() && !rest.starts_with('/') && !url.chars().any(char::is_whitespace)
}

fn is_semver(version: &str) -> bool {
    let mut parts = version.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    [major, minor, patch]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit()))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|ch| ch.is_ascii_digit() || (b'a'..=b'f').contains(&ch))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hashes_equal(got: &str, expected: &str) -> bool {
    if got.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in got.bytes().zip(expected.bytes()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn unique_dir(parent: &Path) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = parent.join(format!("pnet-release-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Unpack only a regular file whose tar path is exactly `pnet`. The output
/// path is `dir/pnet`, never the path stored in the archive.
fn extract_pnet(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|e| format!("release archive: {e}"))?;
    let mut wrote = false;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("release archive: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("release archive: {e}"))?
            .into_owned();
        if !is_pnet_path(&path) {
            continue;
        }
        if wrote {
            return Err("release archive contains more than one pnet file".into());
        }
        if !entry.header().entry_type().is_file() {
            return Err("pnet in the release archive is not a regular file".into());
        }
        let size = entry
            .header()
            .size()
            .map_err(|e| format!("release archive: {e}"))?;
        if size > MAX_BYTES as u64 {
            return Err(format!(
                "pnet in the release archive is larger than {MAX_BYTES} bytes"
            ));
        }
        let partial = dir.join("pnet.partial");
        let mut file = fs::File::create(&partial).map_err(|e| e.to_string())?;
        let n = io::copy(&mut (&mut entry).take(MAX_BYTES as u64 + 1), &mut file)
            .map_err(|e| format!("release archive: {e}"))?;
        drop(file);
        if n > MAX_BYTES as u64 {
            let _ = fs::remove_file(&partial);
            return Err(format!(
                "pnet in the release archive is larger than {MAX_BYTES} bytes"
            ));
        }
        let dest = dir.join("pnet");
        fs::rename(&partial, &dest).map_err(|e| e.to_string())?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        wrote = true;
    }
    if !wrote {
        return Err("release archive does not contain a file named pnet".into());
    }
    Ok(())
}

fn is_pnet_path(path: &Path) -> bool {
    let parts: Vec<_> = path
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect();
    matches!(parts.as_slice(), [Component::Normal(name)] if *name == "pnet")
}

#[cfg(test)]
pub(crate) fn archive_with_pnet(body: &[u8]) -> Vec<u8> {
    archive_with_member("pnet", body, tar::EntryType::Regular)
}

#[cfg(test)]
fn archive_with_member(name: &str, body: &[u8], kind: tar::EntryType) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path(name).unwrap();
    header.set_entry_type(kind);
    if kind.is_file() {
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, body).unwrap();
    } else {
        header.set_size(0);
        header.set_link_name("elsewhere").unwrap();
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
    }
    let raw = builder.into_inner().unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gzip, &raw).unwrap();
    gzip.finish().unwrap()
}

#[cfg(test)]
fn index_json(version: &str, target: &str, url: &str, sha: &str) -> Vec<u8> {
    let name = format!("pnet-{version}-{target}.tar.gz");
    format!(
        r#"{{"version":"{version}","tag":"v{version}","assets":[{{"target":"{target}","name":"{name}","url":"{url}","sha256":"{sha}"}}]}}"#
    )
    .into_bytes()
}

#[cfg(test)]
struct MapFetch {
    index_url: String,
    index: Vec<u8>,
    asset_url: String,
    archive: Vec<u8>,
}

#[cfg(test)]
impl ReleaseFetch for MapFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        if url == self.index_url {
            Ok(self.index.clone())
        } else if url == self.asset_url {
            Ok(self.archive.clone())
        } else {
            Err(format!("unexpected url {url}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const TARGET: &str = "x86_64-unknown-linux-gnu";
    const INDEX: &str = "https://releases.example/index.json";
    const ASSET: &str = "https://releases.example/pnet.tar.gz";

    fn tmp() -> PathBuf {
        let mut n = [0u8; 8];
        getrandom::getrandom(&mut n).unwrap();
        let path = std::env::temp_dir().join(format!("pnet-rel-{:x}", u64::from_le_bytes(n)));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fetch_of(archive: Vec<u8>) -> MapFetch {
        MapFetch {
            index_url: INDEX.into(),
            index: index_json("1.2.3", TARGET, ASSET, &sha256_hex(&archive)),
            asset_url: ASSET.into(),
            archive,
        }
    }

    #[test]
    fn host_target_matches_a_published_linux_triple() {
        match host_target() {
            Ok(target) => assert!(
                target == "x86_64-unknown-linux-gnu" || target == "aarch64-unknown-linux-gnu"
            ),
            Err(message) => assert!(message.contains("--from")),
        }
    }

    #[test]
    fn stage_release_unpacks_a_matching_archive() {
        let body = b"#!/bin/sh\necho 'pnet 1.2.3'\n";
        let archive = archive_with_pnet(body);
        let parent = tmp();
        let staged = stage_release(&fetch_of(archive), INDEX, TARGET, &parent).unwrap();
        let installed = fs::read(staged.dir.join("pnet")).unwrap();
        assert_eq!(installed, body);
        let mode = fs::metadata(staged.dir.join("pnet"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(!staged.dir.join("pnet.partial").exists());
        assert_eq!(staged.version, "1.2.3");
        assert_eq!(staged.target, TARGET);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn stage_release_rejects_a_bad_hash_without_writing() {
        let archive = archive_with_pnet(b"#!/bin/sh\necho 'pnet 1.2.3'\n");
        let mut fetch = fetch_of(archive);
        fetch.index = index_json("1.2.3", TARGET, ASSET, &"ab".repeat(32));
        let parent = tmp();
        let err = stage_release(&fetch, INDEX, TARGET, &parent).unwrap_err();
        assert!(err.contains("sha256"), "{err}");
        assert!(fs::read_dir(&parent).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn stage_release_rejects_a_symlink_and_a_nested_name() {
        let body = b"not pnet";
        let parent = tmp();
        let symlink = archive_with_member("pnet", body, tar::EntryType::Symlink);
        let err = stage_release(&fetch_of(symlink), INDEX, TARGET, &parent).unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
        assert!(fs::read_dir(&parent).unwrap().next().is_none());

        let nested = archive_with_member("dir/pnet", body, tar::EntryType::Regular);
        let err = stage_release(&fetch_of(nested), INDEX, TARGET, &parent).unwrap_err();
        assert!(err.contains("does not contain"), "{err}");
        assert!(!parent.join("pnet").exists());
        assert!(fs::read_dir(&parent).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn only_an_exact_pnet_path_is_unpacked() {
        assert!(is_pnet_path(Path::new("pnet")));
        assert!(is_pnet_path(Path::new("./pnet")));
        assert!(!is_pnet_path(Path::new("../pnet")));
        assert!(!is_pnet_path(Path::new("dir/pnet")));
        assert!(!is_pnet_path(Path::new("/pnet")));
    }

    #[test]
    fn select_asset_rejects_a_cleartext_url_and_a_duplicate_target() {
        let sha = "ab".repeat(32);
        let index = ReleaseIndex {
            version: "1.2.3".into(),
            tag: "v1.2.3".into(),
            assets: vec![ReleaseAsset {
                target: TARGET.into(),
                name: format!("pnet-1.2.3-{TARGET}.tar.gz"),
                url: "http://releases.example/pnet.tar.gz".into(),
                sha256: sha.clone(),
            }],
        };
        assert!(select_asset(&index, TARGET).unwrap_err().contains("https"));

        let mut dup = index;
        dup.assets[0].url = ASSET.into();
        dup.assets.push(dup.assets[0].clone());
        assert!(select_asset(&dup, TARGET)
            .unwrap_err()
            .contains("more than one"));

        dup.assets.pop();
        dup.tag = "v9.9.9".into();
        assert!(select_asset(&dup, TARGET).unwrap_err().contains("tag"));

        dup.tag = "v1.2.3".into();
        dup.assets[0].sha256 = sha.to_ascii_uppercase();
        assert!(select_asset(&dup, TARGET)
            .unwrap_err()
            .contains("lowercase"));
    }
}
