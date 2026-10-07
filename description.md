# pNet installer

`pnet_installer bootstrap` installs **pNet** from a local binary directory
(unpacked dist, or `target/debug` after `cargo build`), or from the latest
published pNet release when you do not have one. It does not download other
programs, and it does not register as a pNet app.

The person who wants an app on a device starts that app there. The app
registers with the local node. Approval stays on that device (Config → Pending
Apps).

## Bootstrap (empty machine)

```bash
cargo build --manifest-path ../pNet/Cargo.toml --bin pnet
cargo build
./target/debug/pnet_installer bootstrap --from ../pNet/target/debug --prefix ~/.pnet
```

`--from` is the directory that contains `pnet`. When that binary sits next to
`pnet_installer`, `--from` can be omitted. When neither is present, bootstrap
downloads the latest published release for this machine.

On a terminal, bootstrap asks what this machine is before it starts `pnet`:

- **Device grade:** device name, a connection code (an invitation from an SG), and a key passphrase. The node does not serve a website.
- **Server grade, new user:** your alias, device name, rank, reachable addresses, the portal password, and a key passphrase.
- **Server grade, join:** device name, connection code, rank, reachable addresses, the portal password, and a key passphrase.

The key passphrase is not the portal password. pNet creates and seals private keys only when `PNET_KEY_PASSPHRASE` is already set (at least 8 characters). Bootstrap writes that variable into `node.env` for every grade. Without it, a new server stays on the setup page and logs `new-user setup failed: passphrase`.

The answers are written to `~/.pnet/node.env` (mode 0600) and exported by `start.sh`. `start.sh` starts `pnet` only. Pass the same values as flags to skip the dialog (`--grade`, `--device-alias`, `--connection-code`, `--user-alias`, `--sg-rank`, `--hosts`, `--admin-password`, `--key-passphrase`). `--no-setup` copies the binary without configuring a node.

After an SG install, sign in at `http://127.0.0.1:8777/`. A DG has no such URL; use an SG's portal.

Default prefix `~/.pnet` (`bin/pnet`, `start.sh`, `node.env`, `logs/`). You ran `bootstrap`, so the copy and the start are the consent.

| Flag / variable | Meaning |
|-----------------|---------|
| `--prefix` | Install prefix (default `~/.pnet`) |
| `--from` | Directory containing the `pnet` binary |
| `--http-bind` | `PNET_HTTP_BIND` for a server-grade portal (default `127.0.0.1`) |
| `--no-start` | Write files and do not run `start.sh` |
| `--force` | Replace `bin/pnet` when the versions are equal, or when a version cannot be read. A newer installed binary is left in place |
| `--dry-run` | Print install, keep, upgrade, or refuse, and write nothing |

## Upgrade

One `pnet` binary lives under the prefix. `bootstrap` runs `<binary> --version` on the installed copy and on the one you brought.

- Nothing installed yet: copy the binary, write `node.env` when that file is missing, write `start.sh`, and start the node.
- The binary you brought is newer: stop the running node, replace `bin/pnet`, and start it again.
- The versions match: leave `bin/pnet` and a running node in place. `--force` replaces the file anyway.
- The installed binary is newer: refuse, leave the node running, and change nothing.
- A binary that does not print `pnet X.Y.Z` has no version. A versioned binary replaces an unversioned one. An unversioned binary does not replace a versioned one unless `--force` is set.

`node.env` is written only when it is missing. A later run does not prompt and does not apply new setup flags over that file. The installer does not read or write `~/.pnet/data`.

With no `--from` and no `pnet` beside this program, bootstrap downloads the index of the latest pNet release, unpacks the archive for this machine, and checks its sha256 before it runs that file. `--from`, or a `pnet` next to this program, is used as-is and does not contact the network. The downloaded version is written to `bootstrap.json` as `release_version`. A published `vX.Y.Z` tag is what creates that release. Until one is pushed, this path has nothing to download, and `--from` is how you install a binary you already built. The rules are in [descriptions/forward-upgrade.md](descriptions/forward-upgrade.md).

## Non-goals

- Running as a pNet app (no fabric alias, no portal mount)
- A catalog, install desire, or package exec
- Installing or starting any program other than `pnet`
- Several pNet versions side by side, or a command that switches between them
- The running `pnet` process downloading and replacing itself
- systemd units (`start.sh` is enough)
- Windows (see [descriptions/windows-bootstrap.md](descriptions/windows-bootstrap.md))
