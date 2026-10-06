# Windows bootstrap

**Status:** plan. Not implemented. Current code is Linux bootstrap only
(`start.sh`, `HOME`, Unix file modes).

**Goal:** on Windows, `pnet_installer bootstrap` installs the local `pnet`
binary and starts it in the user session. First-run parameters are the same
dialog or command-line flags as on Linux (`node.env`). A machine that already
has `pnet` follows the forward-only rule in
[forward-upgrade.md](forward-upgrade.md): a newer binary replaces the
installed one, an older binary does not, and `node.env` is left in place.
A device-grade node does not bind a website. A server-grade portal is
`http://127.0.0.1:8777/` after those parameters are applied — there is no
`/setup` page.

The installer is not a pNet app. Bootstrap does not copy or start
`pnet_installer`, and it does not install any other program.

**Companion change:** pNet's data directory is chosen in the pNet repo
(`pNet/src/main.rs`, `data_dir()`). That lookup must learn `USERPROFILE` in
the same change, or the node writes `node.toml` into the current directory
when `HOME` is unset. This repo cannot fix that alone.

---

## What already works

pNet is ordinary Rust: UDP 7777, HTTP 8777, `ctrlc` with the `termination`
feature (console close on Windows). Crypto and serde have no Unix-only crates.
`0600` / `0700` in `pNet/src/lib/persistence.rs` and `writer.rs` are already
`#[cfg(unix)]`.

---

## What fails today

| Blocker | Where |
|---------|--------|
| Unconditional `use std::os::unix::fs::PermissionsExt` — `cargo build` for `x86_64-pc-windows-msvc` does not compile | `src/bootstrap.rs` (`chmod` to `0755` / `0600`) |
| Prefix comes only from `HOME`. cmd and PowerShell set `USERPROFILE`. Unset `HOME` falls back to `.` | `src/bootstrap.rs` `home_dir()`, pNet `data_dir()` |
| `infer_from` / `plan` look for a file named `pnet`. `Path::is_file` does not add `.exe` | `src/bootstrap.rs` `BINS`, `infer_from`, `plan` |
| Launch writes and runs `start.sh` (`#!/bin/sh`, background `&`, `kill -0` on a pid file). `Command::new` will not run that script on Windows | `src/bootstrap.rs` `start_script`, `execute` |

---

## Decisions

| Topic | Decision |
|-------|----------|
| Layout | `%USERPROFILE%\.pnet` (`bin\`, `logs\`, `run\`). If `HOME` is set (Git Bash), it wins, so one tree is shared. Override remains `--prefix`. One `pnet.exe`. No `versions\` tree. |
| Already installed | The decision table in [forward-upgrade.md](forward-upgrade.md). Missing binary: copy it and write `node.env` when that file is missing. Upgrade: stop, replace, start. Downgrade: refuse, and leave the running node up. `--force` replaces an equal or unversioned binary and still refuses a downgrade. |
| Binary | On Windows the file name is `pnet.exe`. Copy that name into `bin\`. Do not copy `pnet_installer.exe` into the prefix. |
| Launch | Linux keeps `start.sh` unchanged (it starts `pnet` only). Windows starts `pnet.exe` from Rust: detached, new process group, stdout/stderr appended to `logs\pnet.log`, pid file `run\pnet.pid`. Skip a start when that pid is still alive (`OpenProcess` query; the Unix check stays `kill -0` inside `start.sh`). |
| Stop | `pnet_installer stop` reads `run\pnet.pid` and terminates that process. Linux `start.sh` has no stop command; Windows has no `kill` for a pid file, so stop ships with the launcher. |
| Session | The process lives in the user session and exits at logoff. That matches `start.sh`. Logon autostart is a later task (scheduled task or service) and is not this plan. |
| Permissions | Unix `chmod` stays behind `#[cfg(unix)]`. Windows inherits the profile ACL. Tightening `%USERPROFILE%\.pnet` to the current user is a follow-up, not a gate. |
| Dist | Unpacked folder containing `pnet.exe`, plus `pnet_installer.exe bootstrap --from` that folder. No MSI. Target `x86_64-pc-windows-msvc`, built on Windows. Cross-compile from Linux is not a done criterion. |
| Signing / firewall | Operator notes. Unsigned exes trip SmartScreen. Inbound UDP 7777 is required when this PC is a fabric peer. Loopback HTTP 8777 is not. Binding the portal on `0.0.0.0` needs an inbound rule for that port. |

---

## Work

### 1. Compile on Windows

- Gate `PermissionsExt` and the chmod helpers in `src/bootstrap.rs` with `#[cfg(unix)]`.
- No new crate for this step.

### 2. One home lookup

Shared helper in `src/bootstrap.rs`:

1. `HOME` if set and non-empty
2. else `USERPROFILE` if set and non-empty
3. else `.`

Default prefix is that directory plus `.pnet`.

**pNet repo (required companion):** `data_dir()` in `pNet/src/main.rs` uses the same three-step lookup, then `.pnet/data`. No other pNet change for the first run.

### 3. Binary name

`bin_file("pnet")` is `pnet.exe` on Windows and `pnet` elsewhere. `infer_from`, `plan`, and the copy loop use that name. `--from` is the directory that contains `pnet` (unpacked dist, or `target/debug` / `target/release` after `cargo build` of the pNet crate).

### 4. Windows launcher

In `execute`, when `start` is set:

- **Unix:** write and run `start.sh`, as now.
- **Windows:** do not write `start.sh`. Spawn `bin\pnet.exe`.
  - `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS` (`std::os::windows::process::CommandExt`).
  - Stdout and stderr appended to the log file.
  - Pid recorded under `run\pnet.pid`.
  - If the recorded pid is alive, leave it running.
- Liveness and stop use `windows-sys` (`Win32_System_Threading`: `OpenProcess`, `TerminateProcess`), dependency limited to `cfg(windows)`:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
  "Win32_Foundation",
  "Win32_System_Threading",
] }
```

- New subcommand `stop`. Unknown commands stay an error. `help` lists `stop`.
- `stop` is a no-op success when the pid file is missing or the process is already gone.

`--no-start` still copies `pnet` when the forward-upgrade rule says to copy, and writes `bootstrap.json`. On Windows it does not write `start.sh`. `node.env` is written only when that file is missing and setup parameters were collected. The next-step line points at the SG portal `http://<http-bind>:8777/` or, for device grade, says this node does not serve a website. An upgrade with `--no-start` still stops the running process before replacing `pnet.exe`, and does not start it again.

### 5. Tests

Existing bootstrap tests plant `#!/bin/sh` stand-ins and call `chmod`. Keep them `#[cfg(unix)]`.

On every OS:

- `bin_file` unit test for the expected suffix.
- Home helper: `HOME` wins over `USERPROFILE`; `USERPROFILE` is used when `HOME` is unset. Set and restore the env vars inside the test.
- `plan` errors when the file `bin_file` looks up is missing. The fixture file name follows `bin_file`, so the same test runs on Linux with the unsuffixed name and on Windows with `.exe`.

`#[cfg(windows)]`: `execute` with `--no-start` copies `pnet.exe`, writes `bootstrap.json`, and does not write `start.sh`.

A live spawn of the real node is not part of the unit tests (the fixtures are not a pNet binary).

### 6. Docs in this repo

- `description.md` and `README.md` already point here. After this lands, replace the "not implemented" line with the `pnet_installer.exe bootstrap` invocation.

---

## Done when

1. `cargo test` still passes on Linux.
2. On Windows, `cargo build --target x86_64-pc-windows-msvc` succeeds for this crate and for `pnet`.
3. `pnet_installer.exe bootstrap --from <dir-with-pnet.exe> --no-start` fills `%USERPROFILE%\.pnet\bin` with `pnet.exe` only.
4. Without `--no-start`, `pnet.exe` stays up after the bootstrap process exits and `logs\pnet.log` grows. A server-grade node answers `http://127.0.0.1:8777/`. A device-grade node does not listen on that port.
5. A second bootstrap does not start a second `pnet.exe`. A newer `pnet.exe` replaces the installed one after the old process has exited. An older `pnet.exe` leaves the installed one running.
6. `pnet_installer.exe stop` ends that process.
7. pNet's `node.toml` is under `%USERPROFILE%\.pnet\data`, not the current directory.

---

## Out of scope

- Installing or starting any program other than `pnet`.
- A catalog, install desire, or package exec.
- Several pNet versions on disk, or downloading a release (that fetch is phase E in [forward-upgrade.md](forward-upgrade.md)). Windows still installs from `--from` or from a `pnet.exe` beside the installer.
- Per-user scheduled task at logon, Windows service, account, and recovery policy.
- MSI / WiX, code signing, SmartScreen reputation.
- macOS.
- DACL equivalent of `0600` on `node.toml` (follow-up if the PC is shared).
- Firewall rule installation. Document the UDP 7777 requirement; do not shell out to `netsh` from bootstrap.
- Linux CI that cross-compiles the MSVC target.
