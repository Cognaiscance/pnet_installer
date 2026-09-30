//! First-run node parameters collected before `pnet` is started.
//!
//! A device-grade node needs a connection code. A server-grade node needs an
//! identity (new user, or a connection code to join) plus reachable addresses
//! and the portal password. Both grades need a key passphrase: pNet will not
//! create keys unless `PNET_KEY_PASSPHRASE` is already set. When those values
//! are already on the bootstrap command line, nothing is asked. Otherwise a
//! terminal dialog fills them in.

use std::io::{BufRead, Write};
use std::process::Command;

pub const MIN_ADMIN_PASSWORD: usize = 8;
/// Matches `pnet::keystore::MIN_PASSPHRASE_LEN`. This crate does not depend on pNet.
pub const MIN_KEY_PASSPHRASE: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NodeSetup {
    /// `"sg"` or `"dg"`. Empty until chosen.
    pub grade: String,
    pub device_alias: String,
    /// Server-grade, new user. Empty when joining with a connection code.
    pub user_alias: String,
    /// Invitation / connection code. Required for device grade and for an SG join.
    pub connection_code: String,
    /// Server-grade rank. Empty means 1.
    pub sg_rank: String,
    /// `PNET_HOSTS` value. Required for server grade.
    pub hosts: String,
    /// Portal password. Required for server grade. Unused on device grade.
    pub admin_password: String,
    /// Seals private keys (`PNET_KEY_PASSPHRASE`). Required for both grades.
    pub key_passphrase: String,
}

impl NodeSetup {
    pub fn grade_normalized(&self) -> Option<&'static str> {
        if self.grade.eq_ignore_ascii_case("sg") {
            Some("sg")
        } else if self.grade.eq_ignore_ascii_case("dg") {
            Some("dg")
        } else {
            None
        }
    }

    pub fn is_complete(&self) -> bool {
        if self.device_alias.trim().is_empty() {
            return false;
        }
        if !self.key_passphrase_ok() {
            return false;
        }
        match self.grade_normalized() {
            Some("dg") => !self.connection_code.trim().is_empty(),
            Some("sg") => self.sg_identity_ok() && self.sg_rank_ok() && self.sg_hosts_and_password_ok(),
            _ => false,
        }
    }

    fn key_passphrase_ok(&self) -> bool {
        self.key_passphrase.len() >= MIN_KEY_PASSPHRASE
    }

    fn sg_identity_ok(&self) -> bool {
        if !self.connection_code.trim().is_empty() {
            true
        } else {
            !self.user_alias.trim().is_empty()
        }
    }

    fn sg_rank_ok(&self) -> bool {
        let rank = self.sg_rank.trim();
        rank.is_empty() || rank.parse::<u32>().ok().is_some_and(|n| n >= 1)
    }

    fn sg_hosts_and_password_ok(&self) -> bool {
        !self.hosts.trim().is_empty() && self.admin_password.len() >= MIN_ADMIN_PASSWORD
    }

    /// `KEY=value` lines for `node.env`, sourced by `start.sh`.
    /// Caller must only write this when [`Self::is_complete`] is true.
    pub fn to_env(&self) -> String {
        let mut lines = Vec::new();
        match self.grade_normalized() {
            Some("dg") => {
                lines.push(env_line("PNET_GRADE", "dg"));
                lines.push(env_line("PNET_DEVICE_ALIAS", self.device_alias.trim()));
                lines.push(env_line("PNET_INVITATION_CODE", self.connection_code.trim()));
                lines.push(env_line("PNET_KEY_PASSPHRASE", &self.key_passphrase));
            }
            Some("sg") => {
                let rank = {
                    let t = self.sg_rank.trim();
                    if t.is_empty() { "1" } else { t }
                };
                lines.push(env_line("PNET_GRADE", "sg"));
                lines.push(env_line("PNET_DEVICE_ALIAS", self.device_alias.trim()));
                if self.connection_code.trim().is_empty() {
                    lines.push(env_line("PNET_USER_ALIAS", self.user_alias.trim()));
                } else {
                    lines.push(env_line("PNET_INVITATION_CODE", self.connection_code.trim()));
                }
                lines.push(env_line("PNET_SG_RANK", rank));
                lines.push(env_line("PNET_HOSTS", self.hosts.trim()));
                lines.push(env_line("PNET_ADMIN_PASSWORD", &self.admin_password));
                lines.push(env_line("PNET_KEY_PASSPHRASE", &self.key_passphrase));
            }
            _ => {}
        }
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        body
    }
}

fn env_line(key: &str, value: &str) -> String {
    format!("{key}={}", shell_single_quote(value))
}

/// Single-quote a string for `sh`, so a connection code or password can be sourced.
pub fn shell_single_quote(s: &str) -> String {
    let mut out = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

pub fn missing_params_message() -> String {
    "node setup parameters are required before pnet starts.\n\
     Pass them on the command line, or run bootstrap in a terminal for the dialog.\n\
     \n\
     Device grade:\n\
       --grade dg --device-alias NAME --connection-code CODE \\\n\
         --key-passphrase PASS\n\
     \n\
     New server grade:\n\
       --grade sg --user-alias NAME --device-alias NAME --hosts HOSTS \\\n\
         --admin-password PASS --key-passphrase PASS [--sg-rank N]\n\
     \n\
     Join an existing user as server grade:\n\
       --grade sg --device-alias NAME --connection-code CODE --hosts HOSTS \\\n\
         --admin-password PASS --key-passphrase PASS [--sg-rank N]\n\
     \n\
     The key passphrase seals private keys (at least 8 characters). It is not\n\
     the portal password. --no-setup installs the binaries without configuring\n\
     a node.\n"
        .to_string()
}

/// The installer agent's own listener is a website. Skip it on device grade.
pub fn agent_serves_website(grade: Option<&str>) -> bool {
    !grade
        .map(|g| g.trim().eq_ignore_ascii_case("dg"))
        .unwrap_or(false)
}

/// Fill any missing fields from the terminal. Flags already set are kept.
pub fn prompt<R: BufRead, W: Write>(
    setup: &mut NodeSetup,
    input: &mut R,
    out: &mut W,
    hide_secrets: bool,
) -> Result<(), String> {
    writeln!(
        out,
        "pNet setup\n\
         The website is served only by a server-grade (SG) node.\n\
         A device-grade (DG) node joins with a connection code and does not host a site."
    )
    .map_err(|e| e.to_string())?;

    if setup.grade_normalized().is_none() {
        let choice = ask(
            input,
            out,
            "What is this machine?\n  1) Server grade (SG)\n  2) Device grade (DG)\nChoice [1/2]: ",
        )?;
        setup.grade = match choice.to_ascii_lowercase().as_str() {
            "1" | "sg" | "s" | "server" => "sg".into(),
            "2" | "dg" | "d" | "device" => "dg".into(),
            other => return Err(format!("unrecognized grade {other:?}; use 1 or 2")),
        };
    }

    if setup.grade_normalized() == Some("dg") {
        fill_line(input, out, &mut setup.device_alias, "Device name: ")?;
        fill_line(input, out, &mut setup.connection_code, "Connection code: ")?;
        fill_key_passphrase(input, out, setup, hide_secrets)?;
        return Ok(());
    }

    let joining = if !setup.connection_code.trim().is_empty() {
        true
    } else if !setup.user_alias.trim().is_empty() {
        false
    } else {
        let choice = ask(
            input,
            out,
            "Is this the first server for a new user, or is it joining an existing user?\n\
             1) New user\n  2) Join with a connection code\nChoice [1/2]: ",
        )?;
        match choice.to_ascii_lowercase().as_str() {
            "1" | "new" | "n" => false,
            "2" | "join" | "j" => true,
            other => return Err(format!("unrecognized choice {other:?}; use 1 or 2")),
        }
    };

    fill_line(input, out, &mut setup.device_alias, "Device name: ")?;
    if joining {
        fill_line(input, out, &mut setup.connection_code, "Connection code: ")?;
    } else {
        fill_line(input, out, &mut setup.user_alias, "Your name or alias: ")?;
    }
    if setup.sg_rank.trim().is_empty() {
        let rank = ask(input, out, "SG rank [1]: ")?;
        setup.sg_rank = if rank.is_empty() { "1".into() } else { rank };
        if !setup.sg_rank_ok() {
            return Err(format!("SG rank must be a number >= 1 (got {:?})", setup.sg_rank));
        }
    }
    fill_line(
        input,
        out,
        &mut setup.hosts,
        "Reachable addresses (host or host:port, comma-separated): ",
    )?;
    if setup.admin_password.len() < MIN_ADMIN_PASSWORD {
        loop {
            writeln!(out, "Admin password for the portal (at least {MIN_ADMIN_PASSWORD} characters).")
                .map_err(|e| e.to_string())?;
            let password = ask_secret(input, out, "Admin password: ", hide_secrets)?;
            let confirm = ask_secret(input, out, "Confirm admin password: ", hide_secrets)?;
            if password.len() < MIN_ADMIN_PASSWORD {
                writeln!(out, "Password is too short.").map_err(|e| e.to_string())?;
                continue;
            }
            if password != confirm {
                writeln!(out, "Passwords do not match.").map_err(|e| e.to_string())?;
                continue;
            }
            setup.admin_password = password;
            break;
        }
    }
    fill_key_passphrase(input, out, setup, hide_secrets)?;
    Ok(())
}

fn fill_key_passphrase<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    setup: &mut NodeSetup,
    hide_secrets: bool,
) -> Result<(), String> {
    if setup.key_passphrase.len() >= MIN_KEY_PASSPHRASE {
        return Ok(());
    }
    loop {
        writeln!(
            out,
            "Key passphrase (at least {MIN_KEY_PASSPHRASE} characters). This seals private keys. It is not the portal password."
        )
        .map_err(|e| e.to_string())?;
        let passphrase = ask_secret(input, out, "Key passphrase: ", hide_secrets)?;
        let confirm = ask_secret(input, out, "Confirm key passphrase: ", hide_secrets)?;
        if passphrase.len() < MIN_KEY_PASSPHRASE {
            writeln!(out, "Key passphrase is too short.").map_err(|e| e.to_string())?;
            continue;
        }
        if passphrase != confirm {
            writeln!(out, "Key passphrases do not match.").map_err(|e| e.to_string())?;
            continue;
        }
        setup.key_passphrase = passphrase;
        break;
    }
    Ok(())
}

fn fill_line<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    field: &mut String,
    label: &str,
) -> Result<(), String> {
    if !field.trim().is_empty() {
        return Ok(());
    }
    let value = ask(input, out, label)?;
    if value.is_empty() {
        return Err(format!("required: {label}"));
    }
    *field = value;
    Ok(())
}

fn ask<R: BufRead, W: Write>(input: &mut R, out: &mut W, label: &str) -> Result<String, String> {
    write!(out, "{label}").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    let n = input.read_line(&mut line).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("setup cancelled".into());
    }
    Ok(line.trim().to_string())
}

fn ask_secret<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    label: &str,
    hide: bool,
) -> Result<String, String> {
    if hide {
        let _ = Command::new("stty").arg("-echo").status();
    }
    let value = ask(input, out, label)?;
    if hide {
        let _ = Command::new("stty").arg("echo").status();
        writeln!(out).map_err(|e| e.to_string())?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn dg_complete_requires_code_and_alias() {
        let mut s = NodeSetup {
            grade: "dg".into(),
            device_alias: "laptop".into(),
            connection_code: "abc".into(),
            key_passphrase: "secret12".into(),
            ..NodeSetup::default()
        };
        assert!(s.is_complete());
        s.connection_code.clear();
        assert!(!s.is_complete());
        s.connection_code = "abc".into();
        s.key_passphrase = "short".into();
        assert!(!s.is_complete());
    }

    #[test]
    fn sg_new_and_join_env_lines() {
        let new_user = NodeSetup {
            grade: "SG".into(),
            device_alias: "home".into(),
            user_alias: "alice".into(),
            sg_rank: String::new(),
            hosts: "pnet.example:7777".into(),
            admin_password: "password1".into(),
            key_passphrase: "secret12".into(),
            ..NodeSetup::default()
        };
        assert!(new_user.is_complete());
        let env = new_user.to_env();
        assert!(env.contains("PNET_GRADE='sg'\n"));
        assert!(env.contains("PNET_USER_ALIAS='alice'\n"));
        assert!(env.contains("PNET_SG_RANK='1'\n"));
        assert!(env.contains("PNET_KEY_PASSPHRASE='secret12'\n"));
        assert!(!env.contains("PNET_INVITATION_CODE"));

        let join = NodeSetup {
            grade: "sg".into(),
            device_alias: "home-2".into(),
            connection_code: "code+/=x".into(),
            sg_rank: "2".into(),
            hosts: "a,b".into(),
            admin_password: "it's long".into(),
            key_passphrase: "key's long".into(),
            ..NodeSetup::default()
        };
        assert!(join.is_complete());
        let env = join.to_env();
        assert!(env.contains("PNET_INVITATION_CODE='code+/=x'\n"));
        assert!(env.contains("PNET_ADMIN_PASSWORD='it'\\''s long'\n"));
        assert!(env.contains("PNET_KEY_PASSPHRASE='key'\\''s long'\n"));
        assert!(!env.contains("PNET_USER_ALIAS"));
    }

    #[test]
    fn dialog_collects_dg_connection_code() {
        let mut setup = NodeSetup::default();
        let mut input = Cursor::new("2\nlaptop\nINVITECODE\nshort\nshort\nsecret12\nsecret12\n");
        let mut out = Vec::new();
        prompt(&mut setup, &mut input, &mut out, false).unwrap();
        assert_eq!(setup.grade, "dg");
        assert_eq!(setup.device_alias, "laptop");
        assert_eq!(setup.connection_code, "INVITECODE");
        assert_eq!(setup.key_passphrase, "secret12");
        assert!(setup.is_complete());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Key passphrase is too short."));
    }

    #[test]
    fn dialog_collects_new_sg_and_keeps_flags() {
        let mut setup = NodeSetup {
            grade: "sg".into(),
            ..NodeSetup::default()
        };
        let mut input = Cursor::new(
            "1\nHome Server\nAlice\n\nsg.example\nshort\nshort\npassword1\npassword1\nsecret12\nsecret12\n",
        );
        let mut out = Vec::new();
        prompt(&mut setup, &mut input, &mut out, false).unwrap();
        assert_eq!(setup.user_alias, "Alice");
        assert_eq!(setup.device_alias, "Home Server");
        assert_eq!(setup.sg_rank, "1");
        assert_eq!(setup.hosts, "sg.example");
        assert_eq!(setup.admin_password, "password1");
        assert_eq!(setup.key_passphrase, "secret12");
        assert!(setup.is_complete());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("too short"));
    }

    #[test]
    fn website_is_not_served_for_device_grade() {
        assert!(!agent_serves_website(Some("dg")));
        assert!(!agent_serves_website(Some(" DG ")));
        assert!(agent_serves_website(Some("sg")));
        assert!(agent_serves_website(None));
    }
}
