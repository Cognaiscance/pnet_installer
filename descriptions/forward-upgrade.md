# Forward upgrade

**Status:** local `--from` upgrade is what `bootstrap` does. See
[description.md](../description.md). Downloading a published release (work
step 5) is not implemented. `pnet --version` and `format_version` are changes
in the pNet repo. A binary that does not print `pnet X.Y.Z` is treated as
having no version.

**Goal:** running the installer is the install and the upgrade.

- No `pnet` under the prefix: copy the binary, write `node.env` when it is
  missing, write `start.sh`, start the node.
- `pnet` is already there: compare versions. The installed copy is older:
  stop it, replace the binary, start it again. `node.env` and `~/.pnet/data`
  stay. The installed copy is the same or newer: leave the binary in place.
  An older binary never replaces a newer one.

One binary on disk. One node. Versions only move forward.

The installer stays a local program. It is not a pNet app. It does not
register with the fabric, and it does not install any program other than
`pnet`. The retired app catalog stays retired
(`pNet/descriptions/app-store-installer.md`).

---

## Layout

Unchanged from today. State stays independent of which file `bin/pnet` is.

```
~/.pnet/
  bin/pnet          # the one installed binary
  node.env          # first-run parameters, written once
  start.sh
  bootstrap.json
  data/             # pNet state: node.toml, keys, write log
  logs/  run/
```

`data_dir()` in `pNet/src/main.rs` is `$HOME/.pnet/data`, not a path next to
the executable. Replacing `bin/pnet` does not create a second node.

There is no `versions/` directory and no selected-version file.

---

## What a run decides

The candidate is the `pnet` in `--from`, or the `pnet` sitting next to
`pnet_installer` when `--from` is omitted. Phase E (below) is the only path
that downloads a candidate. Until that phase, a missing candidate is still
an error.

| Installed | Candidate | Result |
|-----------|-----------|--------|
| Absent | Any file named `pnet` | Copy, write `node.env` if it is missing, start. |
| Older | Newer | Stop the running node, copy, start. |
| Same version | Same version | Keep the binary. Leave a running node running. |
| Newer | Older | Refuse. Exit non-zero. Do not stop the node. Do not copy. |
| No version output | Prints a version | Treat the installed binary as older and upgrade. |
| Prints a version | No version output | Keep, unless `--force`. |
| No version output | No version output | Keep, unless `--force`. |

"Newer" is the Cargo version, `major.minor.patch`, compared as three
numbers. `pnet --version` prints one line, `pnet 0.1.0`, and exits 0. Anything
else is "no version output." Pre-release suffixes are out of scope until a
release needs them.

`--force` replaces an equal or unversioned installed binary with the
candidate. It does not allow a downgrade. It does not rewrite `node.env`.

`--dry-run` prints install, keep, upgrade, or refuse, and writes nothing.
It still runs `<binary> --version` so that word is real, and it does not
start the node.

`--no-start` on an upgrade still stops the running process before the copy,
and does not start the new one. Leaving the old process up would keep the
old binary running after the file was replaced (on Linux the process keeps
the inode it already opened).

`--no-setup` still skips the first-run dialog. It does not change the version
rule.

---

## Files the upgrade touches

| Path | Upgrade |
|------|---------|
| `bin/pnet` | Replaced only in the install and upgrade rows above. Mode `0755`. |
| `node.env` | Written only when the file is missing and setup parameters were collected. Mode `0600`. A later run does not prompt and does not apply new setup flags over an existing file. |
| `start.sh` | Rewritten each run so the launcher matches this installer. A keep does not restart a live pid. |
| `bootstrap.json` | Rewritten with the candidate version string and the decision. |
| `data/` | Never read or written by the installer. |
| `run/pnet.pid` | On upgrade, the installer signals that pid, waits until it is gone, then copies. If the process does not exit, the installer stops and leaves `bin/pnet` unchanged. |

First-run setup is unchanged: device grade, new server, or joining server,
including the key passphrase. The dialog runs only when `node.env` is absent,
this is not a dry run, and `--no-setup` was not passed. The same flags as
today skip the dialog (`--grade`, `--device-alias`, `--connection-code`,
`--user-alias`, `--sg-rank`, `--hosts`, `--admin-password`,
`--key-passphrase`).

`start.sh` still refuses to launch a second `pnet` while `run/pnet.pid` is
alive. Two processes must not both bind UDP 7777.

A server-grade node still serves `http://127.0.0.1:8777/`. A device-grade
node still has no website.

Linux only, until [windows-bootstrap.md](windows-bootstrap.md). That plan
uses this same table.

---

## What the pNet repository has to provide

These are requirements on pNet. The installer cannot invent them.

### 1. The binary reports its version

`pNet/src/main.rs` `main` currently takes no arguments. It loads
`~/.pnet/data` and starts the node. `--version` has to run before that:
before `install_startup_passphrase`, before `persistence::load`, and before
any write.

- `pnet --version` and `pnet -V` print `pnet ` plus the `Cargo.toml` version
  and exit 0.
- The version is `env!("CARGO_PKG_VERSION")`.
- No other output on that path.

The installer runs `<binary> --version` on the candidate and on
`prefix/bin/pnet`. It does not parse log lines from a normal start.

### 2. A newer binary can read old data, and an older binary must not rewrite it

`pNet/src/lib/persistence.rs` `load` accepts today's `node.toml`. There is
no format number. If the file exists and does not parse, `load` logs the
error and returns `Node::new()`. `main` may then write that fresh node back
(`PNET_HOSTS`, the writer thread). An upgrade that the old binary does not
understand would erase the node.

Required behavior:

- `node.toml` carries `format_version`, a positive integer, written by
  `persistence::save`. Today's files have no such field. Load treats a
  missing field as version 1, the shape this tree already writes (split
  `write_log.toml`, sealed keys).
- This binary's maximum is that same number until a later change bumps it.
- Load migrates older numbers forward in memory. The next save writes the
  current number. Existing migrations stay: embedded `write_log` splits out,
  a plaintext `private_key` is sealed on the next save.
- `format_version` greater than this binary understands: print the number
  and exit 1 before any write of `node.toml`, `write_log.toml`, or
  `apps.toml`.
- A `node.toml` that exists but does not parse: exit 1 the same way. Do not
  substitute `Node::new()` once the file is present.

Document the field and the exit rule in `pNet/descriptions/data persistence.md`.

### 3. Each published tag says who it can still talk to

`pNet/descriptions/wire-versioning.md` has no runtime negotiation. Peers on
different versions are normal once machines upgrade one at a time. A breaking
wire change still means both ends move together.

No new handshake in this plan. Each release tag adds a short note to that
file: the tag, the `format_version` it writes, and which older tags it still
speaks to on UDP 7777. The installer does not read that note.

### 4. Published binaries, when the installer should fetch "latest"

Not required for the local `--from` upgrade. Required before phase E.

There is none of this today. `pNet/Cargo.toml` is `0.1.0`, the repo has no
release tags, and it has no `.github` workflow.

One archive per version per target, plus a checksum. A target is operating
system + CPU + C library. A release binary dynamically links glibc and
`libgcc_s`. A build made on a new glibc does not start on an older
distribution. An aarch64 binary does not start on x86_64.

| Target | Why |
|--------|-----|
| `x86_64-unknown-linux-gnu` | PCs and the server-grade node on n64. |
| `aarch64-unknown-linux-gnu` | ARM boards and ARM servers. |
| `x86_64-unknown-linux-musl` (and aarch64 musl if ARM should be portable the same way) | One Linux binary that does not depend on the host glibc. Optional. |
| `x86_64-pc-windows-msvc` | Only after Windows bootstrap exists. |

32-bit x86 is out unless a real machine needs it. macOS is out.

Ship `pnet-<version>-<target>.tar.gz` and a `.sha256` as GitHub Release
assets on the pNet repo. Do not commit the binaries into the source tree.
Also publish one JSON index listing, for each asset: version, target triple,
URL, sha256. The tag is `v` plus the `Cargo.toml` version (`v0.2.0`).
`develop` is not a version the installer installs.

The installer checks the hash before it chmods the file and execs it.

---

## Work

Each step is its own change, in this order. A later step can be written
against the contract of an earlier one, and a real node should not be
upgraded with this installer until steps 1 and 2 are in the pNet that node
runs.

### 1. pNet: `pnet --version`

Repo: `pNet`. Branch from `develop`.

- In `src/main.rs`, handle `--version` and `-V` before any data-dir work.
- Unit or process test that the process prints the package version and does
  not create `~/.pnet/data`.
- No installer change in this step.

### 2. pNet: format version, fail closed

Repo: `pNet`. After step 1, or in parallel on its own branch from `develop`.

- Add `format_version` on the on-disk node document in
  `src/lib/data_models.rs` and read it in `src/lib/persistence.rs`.
- Missing field loads as 1. A greater number, or a `node.toml` that exists
  and does not parse, exits 1 with no write.
- Tests: legacy file without the field still loads; a file with a future
  number does not save; a corrupt existing file does not become `Node::new()`.
- Update `descriptions/data persistence.md`.
- Add the per-tag note heading to `descriptions/wire-versioning.md` and
  record `0.1.0` / format 1 as the current shape. No wire-negotiation code.

### 3. Installer: local forward upgrade

Repo: `pnet_installer`. Depends on the `--version` contract from step 1.
Implemented: `bootstrap` follows the decision table, stops a live pid before
a replace, and writes `node.env` only when that file is missing.

In `src/bootstrap.rs`:

- Run `--version` on the candidate and on `prefix/bin/pnet`.
- Apply the decision table. New copy kinds: keep, upgrade, refuse.
- On upgrade, stop `run/pnet.pid` and wait. Then copy. If stop fails, do not
  copy.
- Write `node.env` only when it is absent.
- `--force` as specified above. Help text replaces "Overwrite an existing
  pnet binary" with the forward-only meaning.
- Tests use small shell scripts as stand-in binaries that print a version
  line. Cover install, keep, upgrade, refuse, `--force` on an equal version,
  `--force` refused on a downgrade, `node.env` left untouched, and no copy
  when the pid will not die.
- When this lands, update `description.md` and `README.md` so they describe
  this behavior as what the program does, and drop the "not implemented"
  line. Leave phase E marked as not implemented.

### 4. pNet: release assets

Repo: `pNet`. When a version should be installable on a machine that does
not have a local build.

- Tag `vX.Y.Z` matching `Cargo.toml`.
- A workflow (or a written script checked into the repo, if a workflow is
  more than this release needs) builds the Linux targets above, uploads each
  archive, its `.sha256`, and the index JSON to that GitHub Release.
- The index is the only document the installer fetches in step 5.

Hand builds on known machines can still produce those archives. The names
and hashes are what the installer checks.

### 5. Installer: fetch the latest binary

Repo: `pnet_installer`. Depends on step 4.

- No `--from`, and no `pnet` beside the installer: download the index,
  choose the newest version for this target, verify sha256, then use that
  file as the candidate in the same decision table.
- `--from`, or a sibling `pnet`, stays the candidate. Those runs do not
  contact the network.
- Refuse to exec a file whose hash does not match the index.
- Record the downloaded version in `bootstrap.json`.

The installer's own version and pNet's version are different numbers. A
pNet release does not have to ship a new installer. Re-running a current
installer is how a machine picks up a newer `pnet`.

A `pnet update` command, or a button on the portal, can exec this installer
later. That is a convenience once many machines should upgrade without a
person running the installer on each one. It is not part of steps 1–5.

---

## Done when

1. `pnet --version` prints the Cargo version and does not touch `~/.pnet/data`.
2. A `node.toml` with a future `format_version`, or a `node.toml` that does
   not parse, exits `pnet` before any data file is written.
3. A legacy `node.toml` without `format_version` still loads, and the next
   save writes the current number.
4. `pnet_installer bootstrap --from DIR` installs when `bin/pnet` is missing,
   replaces it when `DIR/pnet` is newer, keeps it when the versions match,
   and refuses when `DIR/pnet` is older.
5. An upgrade stops the pid from `run/pnet.pid` before the copy. `node.env`
   and `data/` are byte-for-byte unchanged.
6. After step 5, a machine with no local `pnet` binary and no `--from` gets
   the newest published build for its target, with the hash checked first.

## Out of scope

- Several versions side by side, `use`, `list`, or uninstall of an old version.
- Downgrade, including a previous binary kept for rollback.
- The running pNet process downloading a replacement.
- App release tarballs. Apps stay "start the program on the device, then
  approve it in Config."
- Wire capability negotiation. The per-tag note is the record until a break
  needs the plan already in `wire-versioning.md`.
- systemd units, Windows (its own plan), macOS.
