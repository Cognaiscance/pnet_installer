//! Empty-machine bootstrap — install `pnet` from a local binary directory,
//! or replace it when that binary is newer, then optionally start it.
//!
//! Does not fetch packages and does not install any other program. The user
//! points at a folder that already contains `pnet` (unpacked dist, or
//! `target/debug` after `cargo build`).

use std::cmp::Ordering;
use std::fs;
use std::io::{IsTerminal, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::setup::{self, NodeSetup};

const BINS: &[&str] = &["pnet"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opts {
    pub prefix: PathBuf,
    pub from: PathBuf,
    pub force: bool,
    pub start: bool,
    pub dry_run: bool,
    pub http_bind: String,
    /// When set, copy binaries and do not ask for node parameters.
    pub no_setup: bool,
    pub setup: NodeSetup,
}

/// Result of asking a binary for `pnet X.Y.Z`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionProbe {
    /// No file at the path.
    Absent,
    /// The file ran, and its output was not one `pnet X.Y.Z` line.
    Unknown,
    Semver(u64, u64, u64),
}

/// What `bootstrap` will do with `prefix/bin/pnet`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Install,
    Keep,
    Upgrade,
    Refuse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub decision: Decision,
    pub src: PathBuf,
    pub dest: PathBuf,
    pub installed: VersionProbe,
    pub candidate: VersionProbe,
    pub start_sh: PathBuf,
    pub record: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Bootstrap(Opts),
    Help,
}

pub fn default_prefix() -> PathBuf {
    home_dir().join(".pnet")
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// If `pnet` sits next to this binary, that directory is a valid `--from`.
pub fn infer_from(current_exe: &Path) -> Option<PathBuf> {
    let dir = current_exe.parent()?;
    if dir.join("pnet").is_file() {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

pub fn parse_args(args: &[String]) -> Result<Cmd, String> {
    let mut it = args.iter().skip(1);
    let Some(first) = it.next() else {
        return Ok(Cmd::Help);
    };
    match first.as_str() {
        "run" | "--run" => {
            return Err(
                "pnet_installer only bootstraps pNet. It does not run as an app. Try: bootstrap"
                    .into(),
            );
        }
        "help" | "--help" | "-h" => Ok(Cmd::Help),
        "bootstrap" => {
            let mut opts = Opts {
                prefix: default_prefix(),
                from: PathBuf::new(),
                force: false,
                start: true,
                dry_run: false,
                http_bind: "127.0.0.1".into(),
                no_setup: false,
                setup: NodeSetup::default(),
            };
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--prefix" => {
                        opts.prefix = PathBuf::from(it.next().ok_or("--prefix needs a path")?);
                    }
                    "--from" => {
                        opts.from = PathBuf::from(it.next().ok_or("--from needs a path")?);
                    }
                    "--force" => opts.force = true,
                    "--no-start" => opts.start = false,
                    "--dry-run" => opts.dry_run = true,
                    "--no-setup" => opts.no_setup = true,
                    "--http-bind" => {
                        opts.http_bind = it.next().ok_or("--http-bind needs an address")?.clone();
                    }
                    "--grade" => {
                        let g = it
                            .next()
                            .ok_or("--grade needs sg or dg")?
                            .to_ascii_lowercase();
                        if g != "sg" && g != "dg" {
                            return Err("--grade must be sg or dg".into());
                        }
                        opts.setup.grade = g;
                    }
                    "--device-alias" => {
                        opts.setup.device_alias =
                            it.next().ok_or("--device-alias needs a name")?.clone();
                    }
                    "--user-alias" => {
                        opts.setup.user_alias =
                            it.next().ok_or("--user-alias needs a name")?.clone();
                    }
                    "--connection-code" | "--invitation-code" => {
                        opts.setup.connection_code =
                            it.next().ok_or("--connection-code needs a code")?.clone();
                    }
                    "--sg-rank" => {
                        let rank = it.next().ok_or("--sg-rank needs a number")?;
                        let n: u32 = rank
                            .parse()
                            .map_err(|_| "--sg-rank must be a number >= 1")?;
                        if n < 1 {
                            return Err("--sg-rank must be a number >= 1".into());
                        }
                        opts.setup.sg_rank = n.to_string();
                    }
                    "--hosts" => {
                        opts.setup.hosts = it.next().ok_or("--hosts needs a host list")?.clone();
                    }
                    "--admin-password" => {
                        opts.setup.admin_password = it
                            .next()
                            .ok_or("--admin-password needs a password")?
                            .clone();
                    }
                    "--key-passphrase" => {
                        let passphrase = it
                            .next()
                            .ok_or("--key-passphrase needs a passphrase")?
                            .clone();
                        if passphrase.len() < setup::MIN_KEY_PASSPHRASE {
                            return Err(format!(
                                "--key-passphrase must be at least {} characters",
                                setup::MIN_KEY_PASSPHRASE
                            ));
                        }
                        opts.setup.key_passphrase = passphrase;
                    }
                    "--help" | "-h" => return Ok(Cmd::Help),
                    other => return Err(format!("unknown bootstrap flag: {other}")),
                }
            }
            Ok(Cmd::Bootstrap(opts))
        }
        other => Err(format!("unknown command {other:?} (try: bootstrap | help)")),
    }
}

pub fn help_text() -> &'static str {
    "pnet_installer — bootstrap pNet onto this machine\n\
     \n\
     Commands:\n\
       bootstrap            Install pnet, or replace it when the binary you brought is newer\n\
       help                 This text\n\
     \n\
     With no command, this help is printed. pnet_installer does not register\n\
     as a pNet app and does not install other programs.\n\
     \n\
     bootstrap flags:\n\
       --from DIR           Directory containing the pnet binary\n\
                            (default: directory of this executable, if pnet is there)\n\
       --prefix DIR         Install prefix (default: ~/.pnet)\n\
       --force              Replace bin/pnet when versions match or cannot be read\n\
                            A newer installed pnet is still left in place\n\
       --no-start           Copy and write start.sh only\n\
       --dry-run            Print the plan, write nothing\n\
       --http-bind ADDR     PNET_HTTP_BIND for a server-grade portal (default 127.0.0.1)\n\
       --no-setup           Install pnet without configuring the node\n\
     \n\
     Node parameters (if omitted on a terminal, a dialog asks for them):\n\
       --grade sg|dg\n\
       --device-alias NAME\n\
       --connection-code CODE   Device grade, or a server joining an existing user\n\
       --user-alias NAME        New server-grade user (no connection code)\n\
       --sg-rank N              Server grade (default 1)\n\
       --hosts LIST             Server grade reachable addresses (PNET_HOSTS)\n\
       --admin-password PASS    Server-grade portal password (at least 8 characters)\n\
       --key-passphrase PASS    Seals private keys (at least 8 characters; both grades)\n\
     \n\
     A device-grade node does not serve the website. The portal (default port\n\
     8777) is started only for server grade, after these parameters are applied.\n\
     pNet creates keys only when PNET_KEY_PASSPHRASE is set, so the key\n\
     passphrase is written into node.env for every grade.\n\
     \n\
     Start each app on the device where it should run, then approve it in\n\
     Config on that node.\n"
}

pub fn resolve_from(opts: &mut Opts, current_exe: &Path) -> Result<(), String> {
    if opts.from.as_os_str().is_empty() {
        opts.from = infer_from(current_exe).ok_or_else(|| {
            "no --from DIR and pnet is not next to this binary\n\
             Unpack a dist folder that contains pnet and pass --from, or point --from at target/debug after cargo build."
                .to_string()
        })?;
    }
    Ok(())
}

/// Ask for any node parameters the command line did not include.
///
/// Dry-run and `--no-setup` never prompt. A non-interactive start without a
/// complete parameter set fails, so a device-grade install cannot fall through
/// to a setup website.
pub fn prepare_setup(opts: &mut Opts) -> Result<(), String> {
    if opts.dry_run || opts.no_setup || opts.setup.is_complete() {
        return Ok(());
    }
    // A later run does not ask again and does not apply new flags over this file.
    if opts.prefix.join("node.env").is_file() {
        return Ok(());
    }
    if std::io::stdin().is_terminal() {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let stderr = std::io::stderr();
        let mut out = stderr.lock();
        return setup::prompt(&mut opts.setup, &mut input, &mut out, true);
    }
    if opts.start {
        return Err(setup::missing_params_message());
    }
    Ok(())
}

pub fn plan(opts: &Opts) -> Result<Plan, String> {
    if !opts.from.is_dir() {
        return Err(format!(
            "--from is not a directory: {}",
            opts.from.display()
        ));
    }
    let src = opts.from.join(BINS[0]);
    if !src.is_file() {
        return Err(format!("missing {} in {}", BINS[0], opts.from.display()));
    }
    let dest = opts.prefix.join("bin").join(BINS[0]);
    let candidate = probe_version(&src)?;
    let installed = if dest.is_file() {
        probe_version(&dest)?
    } else {
        VersionProbe::Absent
    };
    Ok(Plan {
        decision: decide(installed.clone(), candidate.clone(), opts.force),
        src,
        dest,
        installed,
        candidate,
        start_sh: opts.prefix.join("start.sh"),
        record: opts.prefix.join("bootstrap.json"),
    })
}

/// Forward-only choice. `force` replaces an equal or unreadable installed
/// binary. It does not replace a newer one.
pub fn decide(installed: VersionProbe, candidate: VersionProbe, force: bool) -> Decision {
    match installed {
        VersionProbe::Absent => Decision::Install,
        VersionProbe::Semver(imaj, imin, ipat) => match candidate {
            VersionProbe::Semver(cmaj, cmin, cpat) => {
                match (imaj, imin, ipat).cmp(&(cmaj, cmin, cpat)) {
                    Ordering::Less => Decision::Upgrade,
                    Ordering::Equal if force => Decision::Upgrade,
                    Ordering::Equal => Decision::Keep,
                    Ordering::Greater => Decision::Refuse,
                }
            }
            VersionProbe::Unknown | VersionProbe::Absent if force => Decision::Upgrade,
            VersionProbe::Unknown | VersionProbe::Absent => Decision::Keep,
        },
        VersionProbe::Unknown => match candidate {
            VersionProbe::Semver(..) => Decision::Upgrade,
            VersionProbe::Unknown | VersionProbe::Absent if force => Decision::Upgrade,
            VersionProbe::Unknown | VersionProbe::Absent => Decision::Keep,
        },
    }
}

pub fn parse_version_output(stdout: &str) -> VersionProbe {
    let line = stdout.trim();
    let Some(rest) = line.strip_prefix("pnet ") else {
        return VersionProbe::Unknown;
    };
    let mut parts = rest.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return VersionProbe::Unknown;
    };
    if !rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return VersionProbe::Unknown;
    }
    match (major.parse(), minor.parse(), patch.parse()) {
        (Ok(major), Ok(minor), Ok(patch)) => VersionProbe::Semver(major, minor, patch),
        _ => VersionProbe::Unknown,
    }
}

fn version_label(v: &VersionProbe) -> String {
    match v {
        VersionProbe::Absent => "absent".into(),
        VersionProbe::Unknown => "unknown".into(),
        VersionProbe::Semver(major, minor, patch) => format!("{major}.{minor}.{patch}"),
    }
}

fn decision_word(d: &Decision) -> &'static str {
    match d {
        Decision::Install => "install",
        Decision::Keep => "keep",
        Decision::Upgrade => "upgrade",
        Decision::Refuse => "refuse",
    }
}

pub fn execute(opts: &Opts, plan: &Plan) -> Result<String, String> {
    let mut log = String::new();
    let word = decision_word(&plan.decision);
    let installed = version_label(&plan.installed);
    let candidate = version_label(&plan.candidate);
    if opts.dry_run {
        log.push_str("dry-run (no writes)\n");
        log.push_str(&format!(
            "  {word} installed {installed} candidate {candidate}\n"
        ));
        log.push_str(&format!(
            "  {} -> {}\n",
            plan.src.display(),
            plan.dest.display()
        ));
        log.push_str(&format!("  write {}\n", plan.start_sh.display()));
        log.push_str(&format!("  write {}\n", plan.record.display()));
        let env_path = opts.prefix.join("node.env");
        if opts.setup.is_complete() && !env_path.exists() {
            log.push_str(&format!("  write {}\n", env_path.display()));
        }
        return Ok(log);
    }
    if plan.decision == Decision::Refuse {
        return Err(format!(
            "installed pnet {installed} is newer than {} {candidate}; not replacing",
            plan.src.display()
        ));
    }

    // The running process keeps the binary it already opened. Stop it before
    // the copy, including when --no-start was passed. A keep leaves it up.
    if matches!(plan.decision, Decision::Install | Decision::Upgrade) {
        stop_running(&opts.prefix)?;
    }

    fs::create_dir_all(opts.prefix.join("bin")).map_err(|e| e.to_string())?;
    fs::create_dir_all(opts.prefix.join("logs")).map_err(|e| e.to_string())?;
    fs::create_dir_all(opts.prefix.join("run")).map_err(|e| e.to_string())?;

    match plan.decision {
        Decision::Keep => {
            log.push_str(&format!("keep {}\n", plan.dest.display()));
        }
        Decision::Install | Decision::Upgrade => {
            copy_binary(&plan.src, &plan.dest)?;
            chmod_755(&plan.dest)?;
            log.push_str(&format!("{word} {}\n", plan.dest.display()));
        }
        Decision::Refuse => unreachable!("refuse returned above"),
    }

    let env_path = opts.prefix.join("node.env");
    if opts.setup.is_complete() && !env_path.exists() {
        fs::write(&env_path, opts.setup.to_env()).map_err(|e| e.to_string())?;
        chmod_600(&env_path)?;
        log.push_str(&format!("wrote {}\n", env_path.display()));
    } else if env_path.is_file() {
        log.push_str(&format!("keep {}\n", env_path.display()));
    }

    let start = start_script(&opts.prefix, &opts.http_bind, &ready_line(opts));
    fs::write(&plan.start_sh, start).map_err(|e| e.to_string())?;
    chmod_755(&plan.start_sh)?;
    log.push_str(&format!("wrote {}\n", plan.start_sh.display()));

    let rec = format!(
        "{{\n  \"installed_at\": {},\n  \"prefix\": {},\n  \"from\": {},\n  \"http_bind\": {},\n  \"decision\": {},\n  \"installed_version\": {},\n  \"candidate_version\": {}\n}}\n",
        unix_now(),
        json_str(&opts.prefix.to_string_lossy()),
        json_str(&opts.from.to_string_lossy()),
        json_str(&opts.http_bind),
        json_str(word),
        json_str(&installed),
        json_str(&candidate),
    );
    fs::write(&plan.record, rec).map_err(|e| e.to_string())?;

    if opts.start {
        let status = Command::new(&plan.start_sh)
            .current_dir(&opts.prefix)
            .status()
            .map_err(|e| format!("start.sh: {e}"))?;
        if !status.success() {
            return Err(format!("start.sh exited {status}"));
        }
        log.push_str("started via start.sh\n");
    } else {
        log.push_str("not starting (--no-start); run start.sh when ready\n");
    }

    log.push_str(&next_steps(opts));
    Ok(log)
}

fn ready_line(opts: &Opts) -> String {
    if !opts.setup.is_complete() {
        return "pNet started without node.env. Re-run bootstrap with setup parameters.".into();
    }
    if opts.setup.grade_normalized() == Some("dg") {
        "pNet started (device grade). This node does not serve a website.".into()
    } else if opts.setup.grade_normalized() == Some("sg") {
        format!("pNet started. Portal: http://{}:8777/", opts.http_bind)
    } else {
        "pNet started without node.env. Re-run bootstrap with setup parameters.".into()
    }
}

fn next_steps(opts: &Opts) -> String {
    if opts.setup.is_complete() {
        if opts.setup.grade_normalized() == Some("dg") {
            return "Device-grade node does not serve a website. Manage the network from a server-grade portal.\n\
                 Start each app on this device yourself, then approve it in Config.\n"
                .into();
        }
        if opts.setup.grade_normalized() == Some("sg") {
            return format!(
                "Portal: http://{}:8777/ (sign in with the admin password).\n\
                 Start each app on the device where it should run, then approve it in Config.\n",
                opts.http_bind,
            );
        }
    }
    "No node parameters were saved. Pass --grade and the other flags, or run bootstrap in a terminal.\n\
     A device-grade node will not open a setup website.\n"
        .into()
}

fn start_script(prefix: &Path, http_bind: &str, ready: &str) -> String {
    let p = prefix.display();
    let ready = ready.replace('\'', "'\\''");
    format!(
        "#!/bin/sh\n\
         set -e\n\
         PREFIX=\"{p}\"\n\
         if [ -f \"$PREFIX/node.env\" ]; then\n\
           set -a\n\
           . \"$PREFIX/node.env\"\n\
           set +a\n\
         fi\n\
         export PNET_HTTP_BIND={http_bind}\n\
         mkdir -p \"$PREFIX/logs\" \"$PREFIX/run\"\n\
         if [ ! -f \"$PREFIX/run/pnet.pid\" ] || ! kill -0 \"$(cat \"$PREFIX/run/pnet.pid\")\" 2>/dev/null; then\n\
           \"$PREFIX/bin/pnet\" >>\"$PREFIX/logs/pnet.log\" 2>&1 &\n\
           echo $! >\"$PREFIX/run/pnet.pid\"\n\
         fi\n\
         echo '{ready}'\n"
    )
}

fn chmod_755(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

fn chmod_600(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// How long to wait for `--version` before treating the binary as unversioned.
/// An older `pnet` ignores the flag and starts the node, so the probe is
/// killed instead of left running. Its home and ports are not the real node's.
const PROBE_WAIT: Duration = Duration::from_secs(2);

/// How long to wait after SIGTERM before refusing to replace the binary.
const STOP_WAIT: Duration = Duration::from_secs(5);

fn probe_version(path: &Path) -> Result<VersionProbe, String> {
    if !path.is_file() {
        return Ok(VersionProbe::Absent);
    }
    let home = std::env::temp_dir().join(format!(
        "pnet-probe-{}-{}-{}",
        std::process::id(),
        unix_now(),
        probe_seq()
    ));
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let result = probe_version_in(path, &home);
    let _ = fs::remove_dir_all(&home);
    result
}

fn probe_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// ETXTBSY. A binary that just exited, or a file that was just written, can
/// refuse exec or replace for a moment.
fn is_text_busy(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(26)
}

fn spawn_probe(path: &Path, home: &Path) -> Result<std::process::Child, String> {
    let mut last = String::new();
    for _ in 0..25 {
        match Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_clear()
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .env("PNET_UDP_PORT", "0")
            .env("PNET_HTTP_PORT", "0")
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(e) if is_text_busy(&e) => {
                last = e.to_string();
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                return Err(format!("could not run {} --version: {e}", path.display()));
            }
        }
    }
    Err(format!(
        "could not run {} --version: {last}",
        path.display()
    ))
}

fn copy_binary(src: &Path, dest: &Path) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..25 {
        match fs::copy(src, dest) {
            Ok(_) => return Ok(()),
            Err(e) if is_text_busy(&e) => {
                last = e.to_string();
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("copy {}: {e}", src.display())),
        }
    }
    Err(format!("copy {}: {last}", src.display()))
}

fn probe_version_in(path: &Path, home: &Path) -> Result<VersionProbe, String> {
    let mut child = spawn_probe(path, home)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("no stdout from {}", path.display()))?;
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < PROBE_WAIT => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "could not wait on {} --version: {e}",
                    path.display()
                ));
            }
        }
    };
    let buf = reader.join().unwrap_or_default();
    let Some(status) = status else {
        return Ok(VersionProbe::Unknown);
    };
    if !status.success() {
        return Ok(VersionProbe::Unknown);
    }
    let text = String::from_utf8_lossy(&buf);
    Ok(parse_version_output(&text))
}

fn stop_running(prefix: &Path) -> Result<(), String> {
    let pid_path = prefix.join("run/pnet.pid");
    let text = match fs::read_to_string(&pid_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("read {}: {e}", pid_path.display())),
    };
    let Ok(pid) = text.trim().parse::<i32>() else {
        return Ok(());
    };
    if pid <= 0 || !pid_alive(pid) {
        return Ok(());
    }
    let status = Command::new("kill")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("kill {pid}: {e}"))?;
    if !status.success() && pid_alive(pid) {
        return Err(format!(
            "could not signal pnet pid {pid}; left the installed binary unchanged"
        ));
    }
    let started = Instant::now();
    while pid_alive(pid) {
        if started.elapsed() >= STOP_WAIT {
            return Err(format!(
                "pnet pid {pid} did not exit; left the installed binary unchanged"
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn pid_alive(pid: i32) -> bool {
    // A zombie has exited. `kill -0` still succeeds for the parent, which
    // would make a replace wait forever in tests and in any process that has
    // not reaped the child. The installer is not that parent when start.sh
    // launched the node.
    let status = match fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(status) => status,
        Err(_) => return false,
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("State:") {
            return !rest.trim_start().starts_with('Z');
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp() -> PathBuf {
        let mut n = [0u8; 8];
        let _ = getrandom::getrandom(&mut n);
        let p = std::env::temp_dir().join(format!("pnet-boot-{:x}", u64::from_le_bytes(n)));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn dummy_from() -> PathBuf {
        let d = tmp();
        fs::write(d.join("pnet"), b"#!/bin/sh\necho pnet\n").unwrap();
        chmod_755(&d.join("pnet")).unwrap();
        d
    }

    fn opts(prefix: PathBuf, from: PathBuf) -> Opts {
        Opts {
            prefix,
            from,
            force: false,
            start: false,
            dry_run: false,
            http_bind: "127.0.0.1".into(),
            no_setup: false,
            setup: NodeSetup::default(),
        }
    }

    #[test]
    fn parse_help_default_and_bootstrap_flags() {
        assert_eq!(parse_args(&["pnet_installer".into()]).unwrap(), Cmd::Help);
        assert!(parse_args(&["pnet_installer".into(), "run".into()])
            .unwrap_err()
            .contains("does not run as an app"));
        match parse_args(&[
            "pnet_installer".into(),
            "bootstrap".into(),
            "--from".into(),
            "/dist".into(),
            "--prefix".into(),
            "/opt/pnet".into(),
            "--no-start".into(),
            "--dry-run".into(),
            "--force".into(),
        ])
        .unwrap()
        {
            Cmd::Bootstrap(o) => {
                assert_eq!(o.from, PathBuf::from("/dist"));
                assert_eq!(o.prefix, PathBuf::from("/opt/pnet"));
                assert!(!o.start);
                assert!(o.dry_run);
                assert!(o.force);
            }
            _ => panic!("expected bootstrap"),
        }

        match parse_args(&[
            "pnet_installer".into(),
            "bootstrap".into(),
            "--grade".into(),
            "dg".into(),
            "--device-alias".into(),
            "laptop".into(),
            "--connection-code".into(),
            "INV".into(),
            "--key-passphrase".into(),
            "secret12".into(),
            "--no-setup".into(),
        ])
        .unwrap()
        {
            Cmd::Bootstrap(o) => {
                assert_eq!(o.setup.grade, "dg");
                assert_eq!(o.setup.device_alias, "laptop");
                assert_eq!(o.setup.connection_code, "INV");
                assert_eq!(o.setup.key_passphrase, "secret12");
                assert!(o.setup.is_complete());
                assert!(o.no_setup);
            }
            _ => panic!("expected bootstrap"),
        }
        assert!(parse_args(&[
            "pnet_installer".into(),
            "bootstrap".into(),
            "--key-passphrase".into(),
            "short".into(),
        ])
        .unwrap_err()
        .contains("at least 8"));
        assert!(parse_args(&[
            "pnet_installer".into(),
            "bootstrap".into(),
            "--grade".into(),
            "phone".into(),
        ])
        .is_err());
    }

    #[test]
    fn infer_from_requires_pnet() {
        let d = dummy_from();
        let exe = d.join("pnet_installer");
        assert_eq!(infer_from(&exe), Some(d.clone()));
        fs::remove_file(d.join("pnet")).unwrap();
        assert!(infer_from(&exe).is_none());
    }

    #[test]
    fn plan_errors_on_missing_bin() {
        let d = tmp();
        let mut o = opts(tmp(), d);
        o.dry_run = true;
        assert!(plan(&o).unwrap_err().contains("missing pnet"));
    }

    #[test]
    fn execute_copies_and_writes_start_without_launching() {
        let from = dummy_from();
        let prefix = tmp();
        let o = opts(prefix.clone(), from);
        let p = plan(&o).unwrap();
        let log = execute(&o, &p).unwrap();
        assert!(log.contains("not starting"));
        assert!(log.contains("will not open a setup website"));
        assert!(prefix.join("bin/pnet").is_file());
        assert!(!prefix.join("bin/pnet_installer").exists());
        assert!(!prefix.join("node.env").exists());
        let sh = fs::read_to_string(prefix.join("start.sh")).unwrap();
        assert!(sh.contains("PNET_HTTP_BIND=127.0.0.1"));
        assert!(sh.contains("node.env"));
        assert!(sh.contains("bin/pnet"));
        assert!(!sh.contains("pnet_installer"));
        assert!(!sh.contains("/setup"));
        assert!(prefix.join("bootstrap.json").is_file());
        assert!(!prefix.join("installer").exists());
        // second run of two unversioned binaries keeps the installed file
        let p2 = plan(&o).unwrap();
        assert_eq!(p2.decision, Decision::Keep);
        let kept = fs::read(prefix.join("bin/pnet")).unwrap();
        execute(&o, &p2).unwrap();
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), kept);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let from = dummy_from();
        let prefix = tmp();
        let mut o = opts(prefix.clone(), from);
        o.start = true;
        o.dry_run = true;
        let p = plan(&o).unwrap();
        execute(&o, &p).unwrap();
        assert!(!prefix.join("bin/pnet").exists());
    }

    #[test]
    fn execute_writes_dg_node_env_without_a_portal_url() {
        let from = dummy_from();
        let prefix = tmp();
        let mut o = opts(prefix.clone(), from);
        o.setup.grade = "dg".into();
        o.setup.device_alias = "laptop".into();
        o.setup.connection_code = "INVITE".into();
        o.setup.key_passphrase = "secret12".into();
        let p = plan(&o).unwrap();
        let log = execute(&o, &p).unwrap();
        assert!(log.contains("does not serve a website"));
        let env = fs::read_to_string(prefix.join("node.env")).unwrap();
        assert!(env.contains("PNET_GRADE='dg'"));
        assert!(env.contains("PNET_INVITATION_CODE='INVITE'"));
        assert!(env.contains("PNET_DEVICE_ALIAS='laptop'"));
        assert!(env.contains("PNET_KEY_PASSPHRASE='secret12'"));
        let sh = fs::read_to_string(prefix.join("start.sh")).unwrap();
        assert!(sh.contains("does not serve a website"));
        assert!(!sh.contains("/setup"));
        let mode = fs::metadata(prefix.join("node.env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    fn version_script(version: &str) -> String {
        format!("#!/bin/sh\necho 'pnet {version}'\n")
    }

    fn write_bin(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join("pnet");
        fs::write(&path, body).unwrap();
        chmod_755(&path).unwrap();
    }

    /// Kills `pid` on drop so a failed assertion does not leave a sleeper behind.
    struct Kill(i32);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = Command::new("kill")
                .args(["-9", &self.0.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    fn spawn_sleeper() -> (std::process::Child, Kill) {
        let child = Command::new("sleep").arg("60").spawn().unwrap();
        let kill = Kill(child.id() as i32);
        (child, kill)
    }

    #[test]
    fn parse_version_output_accepts_only_one_semver_line() {
        assert_eq!(
            parse_version_output("pnet 1.2.3\n"),
            VersionProbe::Semver(1, 2, 3)
        );
        assert_eq!(parse_version_output("pnet\n"), VersionProbe::Unknown);
        assert_eq!(parse_version_output("pnet 1.2\n"), VersionProbe::Unknown);
        assert_eq!(
            parse_version_output("pnet 1.2.3-rc.1\n"),
            VersionProbe::Unknown
        );
        assert_eq!(
            parse_version_output("pnet 1.2.3\nextra\n"),
            VersionProbe::Unknown
        );
    }

    #[test]
    fn decide_matches_the_forward_only_table() {
        let v1 = VersionProbe::Semver(1, 0, 0);
        let v2 = VersionProbe::Semver(2, 0, 0);
        assert_eq!(
            decide(VersionProbe::Absent, v1.clone(), false),
            Decision::Install
        );
        assert_eq!(decide(v1.clone(), v2.clone(), false), Decision::Upgrade);
        assert_eq!(decide(v1.clone(), v1.clone(), false), Decision::Keep);
        assert_eq!(decide(v1.clone(), v1.clone(), true), Decision::Upgrade);
        assert_eq!(decide(v2.clone(), v1.clone(), false), Decision::Refuse);
        assert_eq!(decide(v2.clone(), v1.clone(), true), Decision::Refuse);
        assert_eq!(
            decide(VersionProbe::Unknown, v1.clone(), false),
            Decision::Upgrade
        );
        assert_eq!(
            decide(v1.clone(), VersionProbe::Unknown, false),
            Decision::Keep
        );
        assert_eq!(decide(v1, VersionProbe::Unknown, true), Decision::Upgrade);
        assert_eq!(
            decide(VersionProbe::Unknown, VersionProbe::Unknown, false),
            Decision::Keep
        );
        assert_eq!(
            decide(VersionProbe::Unknown, VersionProbe::Unknown, true),
            Decision::Upgrade
        );
    }

    fn installed(prefix: &Path, body: &str) {
        write_bin(&prefix.join("bin"), body);
    }

    #[test]
    fn upgrade_replaces_an_older_binary_and_stops_its_pid() {
        let from = tmp();
        write_bin(&from, &version_script("2.0.0"));
        let prefix = tmp();
        installed(&prefix, &version_script("1.0.0"));
        let (child, _kill) = spawn_sleeper();
        fs::create_dir_all(prefix.join("run")).unwrap();
        fs::write(prefix.join("run/pnet.pid"), format!("{}\n", child.id())).unwrap();

        let o = opts(prefix.clone(), from);
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Upgrade);
        let log = execute(&o, &p).unwrap();
        assert!(log.contains("upgrade "));
        assert!(fs::read_to_string(prefix.join("bin/pnet"))
            .unwrap()
            .contains("2.0.0"));
        assert!(!pid_alive(child.id() as i32));
        let record = fs::read_to_string(prefix.join("bootstrap.json")).unwrap();
        assert!(record.contains("\"decision\": \"upgrade\""));
        assert!(record.contains("\"installed_version\": \"1.0.0\""));
        assert!(record.contains("\"candidate_version\": \"2.0.0\""));
    }

    #[test]
    fn refuse_leaves_a_newer_binary_and_its_process() {
        let from = tmp();
        write_bin(&from, &version_script("1.0.0"));
        let prefix = tmp();
        installed(&prefix, &version_script("2.0.0"));
        let before = fs::read(prefix.join("bin/pnet")).unwrap();
        let (child, _kill) = spawn_sleeper();
        fs::create_dir_all(prefix.join("run")).unwrap();
        fs::write(prefix.join("run/pnet.pid"), format!("{}\n", child.id())).unwrap();

        let mut o = opts(prefix.clone(), from.clone());
        o.force = true;
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Refuse);
        let err = execute(&o, &p).unwrap_err();
        assert!(err.contains("not replacing"), "{err}");
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), before);
        assert!(pid_alive(child.id() as i32));
        assert!(!prefix.join("start.sh").exists());

        o.dry_run = true;
        o.force = true;
        let p = plan(&o).unwrap();
        let log = execute(&o, &p).unwrap();
        assert!(log.contains("refuse"));
        assert!(log.contains("dry-run"));
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), before);
    }

    #[test]
    fn equal_versions_stay_unless_force_and_node_env_is_written_once() {
        let from = tmp();
        write_bin(&from, "#!/bin/sh\n# candidate\necho 'pnet 1.0.0'\n");
        let prefix = tmp();
        installed(&prefix, "#!/bin/sh\n# installed\necho 'pnet 1.0.0'\n");
        let before = fs::read(prefix.join("bin/pnet")).unwrap();

        let mut o = opts(prefix.clone(), from);
        o.setup.grade = "dg".into();
        o.setup.device_alias = "laptop".into();
        o.setup.connection_code = "INVITE".into();
        o.setup.key_passphrase = "secret12".into();
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Keep);
        execute(&o, &p).unwrap();
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), before);
        let env = fs::read_to_string(prefix.join("node.env")).unwrap();
        assert!(env.contains("secret12"));

        o.setup.key_passphrase = "different-pass".into();
        o.setup.device_alias = "other".into();
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Keep);
        execute(&o, &p).unwrap();
        assert_eq!(fs::read_to_string(prefix.join("node.env")).unwrap(), env);
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), before);

        o.force = true;
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Upgrade);
        execute(&o, &p).unwrap();
        let replaced = fs::read_to_string(prefix.join("bin/pnet")).unwrap();
        assert!(replaced.contains("# candidate"));
        assert_eq!(fs::read_to_string(prefix.join("node.env")).unwrap(), env);
        prepare_setup(&mut o).unwrap();
    }

    #[test]
    fn a_versioned_candidate_upgrades_an_unversioned_install() {
        let from = tmp();
        write_bin(&from, &version_script("0.2.0"));
        let prefix = tmp();
        installed(&prefix, "#!/bin/sh\necho pnet\n");
        let o = opts(prefix.clone(), from);
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Upgrade);
        execute(&o, &p).unwrap();
        assert!(fs::read_to_string(prefix.join("bin/pnet"))
            .unwrap()
            .contains("0.2.0"));
    }

    #[test]
    fn an_unversioned_candidate_does_not_replace_a_versioned_install() {
        let from = tmp();
        write_bin(&from, "#!/bin/sh\necho pnet\n");
        let prefix = tmp();
        installed(&prefix, &version_script("0.2.0"));
        let before = fs::read(prefix.join("bin/pnet")).unwrap();
        let o = opts(prefix.clone(), from);
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Keep);
        execute(&o, &p).unwrap();
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), before);
    }

    #[test]
    fn upgrade_does_not_copy_when_the_pid_will_not_exit() {
        let from = tmp();
        write_bin(&from, &version_script("2.0.0"));
        let prefix = tmp();
        installed(&prefix, &version_script("1.0.0"));
        let before = fs::read(prefix.join("bin/pnet")).unwrap();
        let child = Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; while true; do sleep 0.2; done")
            .spawn()
            .unwrap();
        let _kill = Kill(child.id() as i32);
        fs::create_dir_all(prefix.join("run")).unwrap();
        fs::write(prefix.join("run/pnet.pid"), format!("{}\n", child.id())).unwrap();

        let o = opts(prefix.clone(), from);
        let p = plan(&o).unwrap();
        assert_eq!(p.decision, Decision::Upgrade);
        let err = execute(&o, &p).unwrap_err();
        assert!(err.contains("did not exit"), "{err}");
        assert_eq!(fs::read(prefix.join("bin/pnet")).unwrap(), before);
        assert!(pid_alive(child.id() as i32));
    }

    #[test]
    fn existing_node_env_skips_the_setup_prompt() {
        let prefix = tmp();
        fs::write(prefix.join("node.env"), "PNET_GRADE='dg'\n").unwrap();
        let mut o = opts(prefix, dummy_from());
        o.start = true;
        prepare_setup(&mut o).unwrap();
    }
}
