//! Keep `prefix/logs` from growing without a bound.
//!
//! `start.sh` appends to `pnet.log` with the shell, so the node holds that
//! file open for its whole life. Renaming it would not stop the growth.
//! This pass copies a short tail, gzip-compresses it, and truncates the
//! same file. Archives older than [`MAX_AGE`] are removed.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flate2::write::GzEncoder;
use flate2::Compression;

/// A live `*.log` larger than this is truncated after its tail is archived.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Bytes kept from the end of an oversized log.
pub const TAIL_BYTES: u64 = 1024 * 1024;

/// Gzip archives older than this are deleted.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How many archives to keep for one log name. Older extras are deleted.
pub const MAX_ARCHIVES: usize = 4;

const MARKER: &str = "# pnet-log-tidy";

pub fn tidy(logs: &Path) -> Result<String, String> {
    if !logs.is_dir() {
        return Ok(format!("no log directory {}\n", logs.display()));
    }
    let now = SystemTime::now();
    let mut note = String::new();
    let mut names: Vec<PathBuf> = fs::read_dir(logs)
        .map_err(|e| format!("{}: {e}", logs.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    names.sort();

    for path in &names {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.ends_with(".log.gz") {
            if archive_expired(path, now) {
                fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
                note.push_str(&format!("removed expired {}\n", path.display()));
            }
            continue;
        }
        if !name.ends_with(".log") {
            continue;
        }
        let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len <= MAX_BYTES {
            continue;
        }
        archive_tail(path, name, now)?;
        note.push_str(&format!(
            "truncated {} ({len} bytes)\n",
            path.display()
        ));
    }

    // Drop archives past the per-log cap after new ones exist.
    let mut archives: Vec<PathBuf> = fs::read_dir(logs)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.ends_with(".log.gz"))
        })
        .collect();
    archives.sort();
    trim_archive_count(&archives, &mut note)?;
    Ok(note)
}

fn archive_expired(path: &Path, now: SystemTime) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|m| m.modified()) else {
        return false;
    };
    now.duration_since(modified).ok().is_some_and(|age| age > MAX_AGE)
}

fn archive_tail(path: &Path, name: &str, now: SystemTime) -> Result<(), String> {
    let mut src = File::options()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let len = src.metadata().map_err(|e| e.to_string())?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    src.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut tail = Vec::new();
    src.read_to_end(&mut tail).map_err(|e| e.to_string())?;

    let stamp = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = name.trim_end_matches(".log");
    let archive = path.with_file_name(format!("{stem}.{stamp}.log.gz"));
    let out = File::create(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut enc = GzEncoder::new(out, Compression::fast());
    enc.write_all(&tail).map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())?;

    src.set_len(0).map_err(|e| format!("truncate {}: {e}", path.display()))?;
    Ok(())
}

fn trim_archive_count(archives: &[PathBuf], note: &mut String) -> Result<(), String> {
    let mut groups: Vec<(String, Vec<&PathBuf>)> = Vec::new();
    for path in archives {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let stem = name.split('.').next().unwrap_or(name);
        if let Some((_, group)) = groups.iter_mut().find(|(s, _)| s == stem) {
            group.push(path);
        } else {
            groups.push((stem.to_string(), vec![path]));
        }
    }
    for (_, group) in groups {
        if group.len() <= MAX_ARCHIVES {
            continue;
        }
        let drop_n = group.len() - MAX_ARCHIVES;
        for path in group.into_iter().take(drop_n) {
            fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            note.push_str(&format!("removed extra {}\n", path.display()));
        }
    }
    Ok(())
}

/// User crontab lines. `@reboot` covers boot. `59 23` is 23:59 every night.
pub fn cron_lines(installer: &Path, prefix: &Path) -> String {
    let cmd = format!(
        "{} tidy-logs --prefix {}",
        installer.display(),
        prefix.display()
    );
    format!("{MARKER}\n@reboot {cmd}\n59 23 * * * {cmd}\n")
}

/// Insert the tidy lines into the current user crontab when they are absent.
pub fn install_crontab(installer: &Path, prefix: &Path) -> Result<String, String> {
    let current = crontab_list();
    if current.contains(MARKER) {
        return Ok("log tidy crontab already installed\n".into());
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&cron_lines(installer, prefix));
    crontab_install(&next)?;
    Ok("installed log tidy crontab (boot and 23:59)\n".into())
}

fn crontab_list() -> String {
    let out = std::process::Command::new("crontab")
        .arg("-l")
        .output();
    match out {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => String::new(),
    }
}

fn crontab_install(body: &str) -> Result<(), String> {
    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("crontab: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("crontab stdin")?
        .write_all(body.as_bytes())
        .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("crontab exited {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_logs() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pnet-log-tidy-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn small_log_is_left_alone() {
        let dir = temp_logs();
        fs::write(dir.join("pnet.log"), b"hello").unwrap();
        let note = tidy(&dir).unwrap();
        assert!(note.is_empty());
        assert_eq!(fs::read(dir.join("pnet.log")).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_log_keeps_a_tail_and_truncates() {
        let dir = temp_logs();
        let mut body = vec![b'a'; (MAX_BYTES as usize) + 50];
        let tail_mark = b"TAIL-MARK-END";
        let at = body.len() - tail_mark.len();
        body[at..].copy_from_slice(tail_mark);
        fs::write(dir.join("pnet.log"), &body).unwrap();
        let note = tidy(&dir).unwrap();
        assert!(note.contains("truncated"));
        assert_eq!(fs::metadata(dir.join("pnet.log")).unwrap().len(), 0);
        let gz = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n.starts_with("pnet.") && n.ends_with(".log.gz"))
            })
            .unwrap();
        let raw = fs::read(&gz).unwrap();
        let mut dec = flate2::read::GzDecoder::new(&raw[..]);
        let mut text = Vec::new();
        dec.read_to_end(&mut text).unwrap();
        assert!(text.ends_with(tail_mark));
        assert_eq!(text.len(), TAIL_BYTES as usize);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_archive_is_removed() {
        let dir = temp_logs();
        let archive = dir.join("pnet.1.log.gz");
        fs::write(&archive, b"not-a-real-gzip").unwrap();
        let old = SystemTime::now() - MAX_AGE - Duration::from_secs(60);
        let file = File::options().write(true).open(&archive).unwrap();
        file.set_modified(old).unwrap();
        let note = tidy(&dir).unwrap();
        assert!(note.contains("removed expired"));
        assert!(!archive.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cron_lines_name_boot_and_night() {
        let text = cron_lines(Path::new("/opt/pnet_installer"), Path::new("/home/w/.pnet"));
        assert!(text.contains("@reboot "));
        assert!(text.contains("59 23 * * * "));
        assert!(text.contains("tidy-logs --prefix /home/w/.pnet"));
    }
}
