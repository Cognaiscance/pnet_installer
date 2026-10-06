# Version manager

**Status:** proposal. Not implemented. The program on `develop` is still a
one-shot bootstrap: it copies a `pnet` binary you already have on the machine,
writes `node.env` and `start.sh`, and starts that one copy. See
[description.md](../description.md).

**Goal:** `pnet_installer` works like a language version manager. The models
are nvm (Node) and the Ruby managers (rbenv with ruby-build, and RVM). You
put the manager on a machine once. It keeps several pNet versions on disk and
chooses which one runs. A machine that only wants to run a node does not
need the pNet source tree.

The installer stays a local program. It is not a pNet app. It does not
register with the fabric, and it does not install any program other than
`pnet`. This does not bring back the retired app catalog (signed app
tarballs, install-desire sync, a long-running installer app). That decision
stands in `pNet/descriptions/app-store-installer.md`.

---

## What those tools actually do

A version manager does two jobs, and the tools split them differently.

**Select.** nvm's `use`, rbenv's shims, and a `.nvmrc` or `.ruby-version`
file decide which installed version a command runs. The other versions stay
on disk. Switching does not delete them and does not reinstall the language.

**Obtain.** This is the part that decides what the pNet repository has to
publish.

- nvm's normal path downloads a prebuilt binary. The Node project publishes
  one build per OS and CPU. `nvm install 22` fetches the tarball for this
  machine into `~/.nvm/versions/node/` and points the shell at it. Compiling
  Node from source is an option, not the default. The target machine does
  not need a compiler.
- Ruby's classic path compiles. rbenv does not download Ruby. ruby-build
  checks out a version and compiles it, so the machine needs a compiler and
  libraries. RVM uses a published binary when one exists for that platform
  and compiles otherwise.

pNet is a Rust program, so rustup is a useful third picture: it downloads a
prebuilt toolchain for a target triple (`x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, …) and never compiles the compiler on your
machine. The behavior requested here follows nvm and the Ruby managers. The
target-triple naming is the part worth copying from rustup.

---

## How this installer would work

Versions of `pnet` live side by side under the install prefix. Node identity
and node data stay in one place and are shared by every version. Selecting a
version changes which binary `start.sh` launches. It does not create a second
node.

```
~/.pnet/
  versions/<version>/bin/pnet     # one tree per installed version
  version                          # selected version name
  bin/pnet                         # that version, or a shim that execs it
  node.env                         # first-run parameters, shared
  start.sh
  data/                            # pNet state: node.toml, keys, write log
  logs/  run/
```

`data/` is already where pNet writes. `data_dir()` in `pNet/src/main.rs` is
`$HOME/.pnet/data`, independent of the directory that contains the executable.
The installer can move the binary without moving the node.

Commands, in the style of those tools:

| Command | What it does |
|---------|----------------|
| `install <version>` | Put that version under `versions/`. Default is a published binary for this OS and CPU. |
| `install <version> --from DIR` | Use a `pnet` you already built. Same idea as today's `bootstrap --from`. |
| `install <version> --build` | Compile that git tag on this machine. The Ruby path. Needs Rust. |
| `use <version>` | Write `version` and point `bin/pnet` at it. Leave `node.env` and `data/` alone. |
| `list` | Versions installed under the prefix. |
| `list-remote` | Versions the release index publishes for this machine. |
| `uninstall <version>` | Remove that version's tree. Refuse when it is the selected one and the node is running. |
| `current` | Print the selected version. |

First-run setup stays once per machine, not once per version. The dialog (or
the same flags as today) still collects grade, device name, connection code
or user alias, rank, hosts, portal password, and key passphrase, and still
writes them to `node.env` mode `0600`. A later `install` or `use` does not
ask again and does not rewrite that file.

One process. `start.sh` still refuses to launch a second `pnet` while
`run/pnet.pid` is alive. `use` while a node is running either restarts onto
the newly selected binary or refuses until the node is stopped. Two versions
must not both bind UDP 7777.

A pin file inside a project directory is the wrong analogue. A pNet node
belongs to the machine, not to a source checkout. The pin is
`~/.pnet/version`.

Today's `bootstrap` remains the first-run command: ensure a version is
installed, write `node.env` if it is missing, write `start.sh`, start the
node. After that, version changes go through `install` and `use`.

`--prefix` still overrides `~/.pnet`. Linux only, until
[windows-bootstrap.md](windows-bootstrap.md) is built. A server-grade node
still serves `http://127.0.0.1:8777/`. A device-grade node still has no
website.

---

## What the pNet repository has to provide

### Published binaries, if install should feel like nvm

Yes. For `pnet_installer install 0.2.0` to work on a machine that does not
have Rust, each version needs a `pnet` binary built for that machine, posted
where the installer can download it.

"Popular hardware" here means one archive per version per **target**, not one
generic "x86" file and not a binary committed into the git tree. A target is
operating system + CPU + C library. The binary produced by `cargo build
--release` on a developer machine is an ELF that dynamically links glibc and
`libgcc_s`. A build made on a new glibc does not start on an older
distribution, even when the CPU matches. An aarch64 binary does not start on
x86_64. The tree already contains both of those builds: host `x86_64` and a
cross build for `aarch64-unknown-linux-gnu`.

Post these if that platform is a node you expect to install with one command:

| Target | Why |
|--------|-----|
| `x86_64-unknown-linux-gnu` | PCs and the server-grade node on n64. |
| `aarch64-unknown-linux-gnu` | ARM boards and ARM servers. Already cross-built. |
| `x86_64-unknown-linux-musl` (and aarch64 musl if ARM should be portable the same way) | One Linux binary that does not depend on the host glibc. Optional. This is the fix for "built on a new distro, fails on an older one." |
| `x86_64-apple-darwin`, `aarch64-apple-darwin` | Only if Macs run nodes. Not an installer target today. |
| `x86_64-pc-windows-msvc` | Only after Windows bootstrap exists. The installer does not compile for Windows yet. |

32-bit x86 is out unless a real machine needs it.

Ship each one as a release asset next to a checksum, for example
`pnet-0.2.0-x86_64-unknown-linux-gnu.tar.gz` and a `.sha256`. GitHub Releases
on the pNet repo are enough. Do not commit the binaries into the source tree.

Also publish a small index the installer can fetch without scraping HTML. One
JSON document listing, for each asset: version, target triple, URL, sha256.
The installer checks the hash before it chmod's the file and execs it.

There is none of this today. `pNet/Cargo.toml` is `0.1.0`, the repo has no
release tags, and it has no `.github` workflow. The live test notes build
x86_64 on n64 and cross-build aarch64 on sanosuke by hand. That is enough for
two known machines. It is not enough for `install 0.2.0` on a third machine.

### Tags and source, if install should feel like ruby-build

A compile-on-the-machine path does not need posted binaries. It needs a git
tag the installer can check out (`v0.2.0`) and a source tree that builds with
`cargo build --release --bin pnet`. Every target machine then needs a Rust
toolchain and a few minutes. Dependencies in `pNet/Cargo.toml` are pure Rust
(no OpenSSL), so the extra system packages are the Rust toolchain itself.

That path matches classic Ruby. It does not match "copy a binary onto the
machine and run it."

### The mix

Publish binaries for the Linux targets above that you actually install, and
keep `--from` and `--build` for everything else: a version you just compiled,
or a CPU you have not shipped. That is how current ruby-build and rustup
split the same problem.

### Rules the pNet binary has to keep

These are requirements on pNet, not on the installer UI.

1. **Versions are tags.** `v0.2.0` in git, the same number in `Cargo.toml`,
   and the same number in the asset name. `develop` is not a version the
   installer installs.

2. **The binary is relocatable.** It already is. State lives under
   `~/.pnet/data`, not beside the executable. The installer will place each
   version at `~/.pnet/versions/<version>/bin/pnet`.

3. **Every version shares one data directory.** `node.env`, `node.toml`,
   sealed keys, and `write_log.toml` belong to the machine. `use` must not
   point a new version at an empty directory and invent a second identity.
   Load already migrates an older `node.toml` (write log split out, plaintext
   keys sealed on next save). There is no schema number an older binary can
   use to refuse a file written by a newer one. pNet needs that, and a
   downgrade that cannot read the files must exit without rewriting them.

4. **Peers can be on different versions.** Wire versioning
   (`pNet/descriptions/wire-versioning.md`) has no negotiation yet. A breaking
   wire change means both ends move together. A version manager makes mixed
   versions on a fabric the normal case, so pNet has to keep a compatibility
   rule for each release: which older versions this binary still speaks to.
   The installer cannot invent that.

5. **Release builds are repeatable.** A tag push builds the target matrix,
   writes checksums, and uploads the index. Hand builds on n64 and sanosuke
   can still produce the artifacts, as long as the names and hashes match
   what the installer expects.

6. **The manager and the node are different releases.** nvm is installed
   once; Node versions come later. `pnet_installer` is still a binary you
   copy onto the machine once (or a small script that downloads that one
   binary). After that, `install` fetches `pnet`. The installer's version
   number and pNet's version number are not the same number, and a pNet
   release does not have to ship a new installer.

### What you do not need

- The pNet source on a machine that is only running a node, once binaries
  exist for its target.
- A binary for every CPU. Two Linux targets cover the machines this project
  runs today.
- The installer joining the fabric, or pNet downloading anything itself.
- App release tarballs. Apps stay "start the program on the device, then
  approve it in Config."

---

## Work this implies, later

Not this change. This file is the model.

1. pNet: tag a version, add a format version the node can reject on
   downgrade, and document the wire compatibility of that tag.
2. pNet: CI (or a written release script) that uploads one archive + sha256
   per target, and an index JSON, to GitHub Releases.
3. Installer: `install`, `use`, `list`, `list-remote`, `uninstall`, `current`,
   with the directory layout above. `bootstrap` becomes "install if needed,
   then first-run setup, then start."
4. Installer: verify sha256 before executing a download. Keep `--from` and
   `--build` for local binaries and source tags.
