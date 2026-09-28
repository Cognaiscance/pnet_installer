# Windows bootstrap

**Status:** plan. Not implemented. Current code is phase 3b: Linux bootstrap
only (`start.sh`, `HOME`, Unix file modes).

**Goal:** on Windows, `pnet_installer bootstrap` installs the local `pnet` and
`pnet_installer` binaries and starts them in the user session, the same job
`bootstrap` does on Linux today. First-run parameters are the same dialog or
command-line flags as on Linux (`node.env`). A device-grade node does not
bind a website. A server-grade portal is `http://127.0.0.1:8777/` after those
parameters are applied — there is no `/setup` page.

**Companion change:** pNet's data directory is chosen in the pNet repo
(`pNet/src/main.rs`, `data_dir()`). That lookup must learn `USERPROFILE` in
the same change, or the node writes `node.toml` into the current directory
when `HOME` is unset. This repo cannot fix that alone.

Phase 4 (signed catalog tarball + `systemd --user`) stays as locked in
`pNet/descriptions/app-store-installer.md`. This plan does not start catalog
apps, install a Windows service, or build an MSI.

---

## What already works

pNet is ordinary Rust: UDP 7777, HTTP 8777, `ctrlc` with the `termination`
feature (console close on Windows). Crypto and serde have no Unix-only crates.
`0600` / `0700` in `pNet/src/lib/persistence.rs` and `writer.rs` are already
`#[cfg(unix)]`. The long-running installer agent (catalog, desire, status) has
the same shape once it can find its state directory. It still does not exec
packages.

---

## What fails today

| Blocker | Where |
|---------|--------|
| Unconditional `use std::os::unix::fs::PermissionsExt` — `cargo build` for `x86_64-pc-windows-msvc` does not compile | `src/bootstrap.rs`, `src/sources.rs` (`chmod` to `0755` / `0700` / `0600`) |
| State and prefix come only from `HOME`. cmd and PowerShell set `USERPROFILE`. Unset `HOME` falls back to `.` | `src/bootstrap.rs` `home_dir()`, `src/main.rs` `home_dir()`, pNet `data_dir()` |
| `infer_from` / `plan` look for files named `pnet` and `pnet_installer`. `Path::is_file` does not add `.exe` | `src/bootstrap.rs` `BINS`, `infer_from`, `plan` |
| Launch writes and runs `start.sh` (`#!/bin/sh`, background `&`, `kill -0` on a pid file). `Command::new` will not run that script on Windows | `src/bootstrap.rs` `start_script`, `execute` |

---

## Decisions

| Topic | Decision |
|-------|----------|
| Layout | `%USERPROFILE%\.pnet` (`bin\`, `logs\`, `run\`, `installer\app_sources\`). If `HOME` is set (Git Bash), it wins, so one tree is shared. Override remains `--prefix`. |
| Binaries | On Windows the file names are `pnet.exe` and `pnet_installer.exe`. Copy those names into `bin\`. |
| Launch | Linux keeps `start.sh` unchanged. Windows starts the two exes from Rust: detached, new process group, stdout/stderr appended to `logs\pnet.log` and `logs\installer.log`, pid files in `run\`. Skip a start when that pid is still alive (`OpenProcess` query; the Unix check stays `kill -0` inside `start.sh`). |
| Stop | `pnet_installer stop` reads the two pid files and terminates those processes. Linux `start.sh` has no stop command; Windows has no `kill` for a pid file, so stop ships with the launcher. |
| Session | Processes live in the user session and exit at logoff. That matches `start.sh`. Logon autostart is a later task (scheduled task or service), analogous to a systemd user unit, and is not this plan. |
| Permissions | Unix `chmod` stays behind `#[cfg(unix)]`. Windows inherits the profile ACL. Tightening `%USERPROFILE%\.pnet` to the current user is a follow-up, not a gate. |
| Dist | Unpacked folder of the two exes plus `pnet_installer.exe bootstrap`. No MSI. Target `x86_64-pc-windows-msvc`, built on Windows. Cross-compile from Linux is not a done criterion. |
| Signing / firewall | Operator notes. Unsigned exes trip SmartScreen. Inbound UDP 7777 is required when this PC is a fabric peer. Loopback HTTP 8777 is not. Binding the portal on `0.0.0.0` needs an inbound rule for that port. |

---

## Work

### 1. Compile on Windows

- Gate `PermissionsExt` and `chmod_755` in `src/bootstrap.rs` with `#[cfg(unix)]`.
- Gate `chmod` in `src/sources.rs` the same way. `ensure_app_sources` still creates the directory and writes `pnet.list` on every OS.
- No new crate for this step.

### 2. One home lookup

Shared helper, used by `src/bootstrap.rs` and `src/main.rs`:

1. `HOME` if set and non-empty
2. else `USERPROFILE` if set and non-empty
3. else `.`

Default prefix is that directory plus `.pnet`. `PNET_INSTALLER_STATE` still overrides the agent state dir.

**pNet repo (required companion):** `data_dir()` in `pNet/src/main.rs` uses the same three-step lookup, then `.pnet/data`. No other pNet change for the first run.

### 3. Binary names

`bin_file("pnet")` is `pnet.exe` on Windows and `pnet` elsewhere. `infer_from`, `plan`, and the copy loop use that name. `--from` is still the directory that contains both files (unpacked dist, or `target/debug` / `target/release` after `cargo build`).

### 4. Windows launcher

In `execute`, when `start` is set:

- **Unix:** write and run `start.sh`, as now.
- **Windows:** do not write `start.sh`. Spawn `bin\pnet.exe` then `bin\pnet_installer.exe` with arguments `run`.
  - `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS` (`std::os::windows::process::CommandExt`).
  - Stdout and stderr appended to the log files.
  - Pid recorded under `run\pnet.pid` and `run\installer.pid`.
  - If the recorded pid is alive, leave it running.
- Liveness and stop use `windows-sys` (`Win32_System_Threading`: `OpenProcess`, `TerminateProcess`), dependency limited to `cfg(windows)`:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
  "Win32_Foundation",
  "Win32_System_Threading",
] }
```

- New subcommand `stop` (and `--stop` is unnecessary). Unknown commands stay an error. `help` lists `stop`.
- `stop` is a no-op success when the pid file is missing or the process is already gone.

`--no-start` still copies binaries and writes `app_sources` and `bootstrap.json`. On Windows it does not write `start.sh`. When setup parameters were collected, it also writes `node.env`. The next-step line points at the SG portal `http://<http-bind>:8777/` or, for device grade, says this node does not serve a website.

### 5. Tests

Existing bootstrap tests plant `#!/bin/sh` stand-ins and call `chmod`. Keep them `#[cfg(unix)]`.

On every OS:

- `bin_file` unit test for the expected suffix.
- Home helper: `HOME` wins over `USERPROFILE`; `USERPROFILE` is used when `HOME` is unset. Set and restore the env vars inside the test.
- `plan` errors when the Windows names are what the code looks up and those files are missing. The fixture file name follows `bin_file`, so the same test runs on Linux with unsuffixed names and on Windows with `.exe`.

`#[cfg(windows)]`: `execute` with `--no-start` copies `pnet.exe` and `pnet_installer.exe`, writes `bootstrap.json` and `app_sources/pnet.list`, and does not write `start.sh`.

A live spawn of the real node is not part of the unit tests (the fixtures are not a pNet binary).

### 6. Docs in this repo

- `description.md`: point here. Phase 3b non-goals stay (no systemd, no catalog exec).
- `README.md`: one line that Windows bootstrap is specified in this plan and not implemented until the work lands. After it lands, replace that line with the `pnet_installer.exe bootstrap` invocation.

---

## Done when

1. `cargo test` still passes on Linux.
2. On Windows, `cargo build --target x86_64-pc-windows-msvc` succeeds for this crate and for `pnet`.
3. `pnet_installer.exe bootstrap --from <dir-with-both-exes> --no-start` fills `%USERPROFILE%\.pnet\bin` with the two exes.
4. Without `--no-start`, both processes stay up after the bootstrap process exits and logs grow under `logs\`. A server-grade node answers `http://127.0.0.1:8777/`. A device-grade node does not listen on that port.
5. A second bootstrap does not start a second pair while the pids are alive.
6. `pnet_installer.exe stop` ends both processes.
7. pNet's `node.toml` is under `%USERPROFILE%\.pnet\data`, not the current directory.

---

## Out of scope

- Phase 4 package fetch, verify, unpack, and start. That design is a systemd user unit. A Windows catalog launcher is a separate plan.
- Per-user scheduled task at logon, Windows service, account, and recovery policy.
- MSI / WiX, code signing, SmartScreen reputation.
- macOS.
- DACL equivalent of `0600` on `node.toml` (follow-up if the PC is shared).
- Firewall rule installation. Document the UDP 7777 requirement; do not shell out to `netsh` from bootstrap.
- Linux CI that cross-compiles the MSVC target.
