//! Empty-machine bootstrap — install `pnet` from a local binary directory,
//! then optionally start it.
//!
//! Does not fetch packages and does not install any other program. The user
//! points at a folder that already contains `pnet` (unpacked dist, or
//! `target/debug` after `cargo build`).

use std::fs;
use std::io::IsTerminal;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyKind {
    Copy,
    SkipExists,
    Overwrite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub copies: Vec<(PathBuf, PathBuf, CopyKind)>,
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
                        let g = it.next().ok_or("--grade needs sg or dg")?.to_ascii_lowercase();
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
                        opts.setup.user_alias = it.next().ok_or("--user-alias needs a name")?.clone();
                    }
                    "--connection-code" | "--invitation-code" => {
                        opts.setup.connection_code =
                            it.next().ok_or("--connection-code needs a code")?.clone();
                    }
                    "--sg-rank" => {
                        let rank = it.next().ok_or("--sg-rank needs a number")?;
                        let n: u32 = rank.parse().map_err(|_| "--sg-rank must be a number >= 1")?;
                        if n < 1 {
                            return Err("--sg-rank must be a number >= 1".into());
                        }
                        opts.setup.sg_rank = n.to_string();
                    }
                    "--hosts" => {
                        opts.setup.hosts = it.next().ok_or("--hosts needs a host list")?.clone();
                    }
                    "--admin-password" => {
                        opts.setup.admin_password =
                            it.next().ok_or("--admin-password needs a password")?.clone();
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
        other => Err(format!(
            "unknown command {other:?} (try: bootstrap | help)"
        )),
    }
}

pub fn help_text() -> &'static str {
    "pnet_installer — bootstrap pNet onto this machine\n\
     \n\
     Commands:\n\
       bootstrap            Copy a local pnet binary, write node.env and start.sh, then start\n\
       help                 This text\n\
     \n\
     With no command, this help is printed. pnet_installer does not register\n\
     as a pNet app and does not install other programs.\n\
     \n\
     bootstrap flags:\n\
       --from DIR           Directory containing the pnet binary\n\
                            (default: directory of this executable, if pnet is there)\n\
       --prefix DIR         Install prefix (default: ~/.pnet)\n\
       --force              Overwrite an existing pnet binary\n\
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
        return Err(format!("--from is not a directory: {}", opts.from.display()));
    }
    let bin_dir = opts.prefix.join("bin");
    let mut copies = Vec::new();
    for name in BINS {
        let src = opts.from.join(name);
        if !src.is_file() {
            return Err(format!("missing {} in {}", name, opts.from.display()));
        }
        let dest = bin_dir.join(name);
        let kind = if dest.is_file() {
            if opts.force {
                CopyKind::Overwrite
            } else {
                CopyKind::SkipExists
            }
        } else {
            CopyKind::Copy
        };
        copies.push((src, dest, kind));
    }
    Ok(Plan {
        copies,
        start_sh: opts.prefix.join("start.sh"),
        record: opts.prefix.join("bootstrap.json"),
    })
}

pub fn execute(opts: &Opts, plan: &Plan) -> Result<String, String> {
    let mut log = String::new();
    if opts.dry_run {
        log.push_str("dry-run (no writes)\n");
        for (src, dest, kind) in &plan.copies {
            log.push_str(&format!(
                "  {:?} {} -> {}\n",
                kind,
                src.display(),
                dest.display()
            ));
        }
        log.push_str(&format!("  write {}\n", plan.start_sh.display()));
        log.push_str(&format!("  write {}\n", plan.record.display()));
        if opts.setup.is_complete() {
            log.push_str(&format!("  write {}\n", opts.prefix.join("node.env").display()));
        }
        return Ok(log);
    }

    fs::create_dir_all(opts.prefix.join("bin")).map_err(|e| e.to_string())?;
    fs::create_dir_all(opts.prefix.join("logs")).map_err(|e| e.to_string())?;
    fs::create_dir_all(opts.prefix.join("run")).map_err(|e| e.to_string())?;

    for (src, dest, kind) in &plan.copies {
        match kind {
            CopyKind::SkipExists => {
                log.push_str(&format!("keep {}\n", dest.display()));
            }
            CopyKind::Copy | CopyKind::Overwrite => {
                fs::copy(src, dest).map_err(|e| format!("copy {}: {e}", src.display()))?;
                chmod_755(dest)?;
                log.push_str(&format!("{:?} {}\n", kind, dest.display()));
            }
        }
    }

    if opts.setup.is_complete() {
        let env_path = opts.prefix.join("node.env");
        fs::write(&env_path, opts.setup.to_env()).map_err(|e| e.to_string())?;
        chmod_600(&env_path)?;
        log.push_str(&format!("wrote {}\n", env_path.display()));
    }

    let start = start_script(&opts.prefix, &opts.http_bind, &ready_line(opts));
    fs::write(&plan.start_sh, start).map_err(|e| e.to_string())?;
    chmod_755(&plan.start_sh)?;
    log.push_str(&format!("wrote {}\n", plan.start_sh.display()));

    let rec = format!(
        "{{\n  \"installed_at\": {},\n  \"prefix\": {},\n  \"from\": {},\n  \"http_bind\": {}\n}}\n",
        unix_now(),
        json_str(&opts.prefix.to_string_lossy()),
        json_str(&opts.from.to_string_lossy()),
        json_str(&opts.http_bind),
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
        // second run without --force keeps existing
        let p2 = plan(&o).unwrap();
        assert!(p2.copies.iter().all(|c| c.2 == CopyKind::SkipExists));
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
        let mode = fs::metadata(prefix.join("node.env")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
