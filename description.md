# pNet installer

`pnet_installer bootstrap` installs **pNet** from a **local** binary directory
(unpacked dist, or `target/debug` after `cargo build`). It does not download
packages, and it does not register as a pNet app.

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
`pnet_installer`, `--from` can be omitted.

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
| `--force` | Overwrite an existing `bin/pnet` |
| `--dry-run` | Print the plan and write nothing |

## Non-goals

- Running as a pNet app (no fabric alias, no portal mount)
- A catalog, install desire, or package exec
- Installing or starting any program other than `pnet`
- systemd units (`start.sh` is enough)
- Windows (see [descriptions/windows-bootstrap.md](descriptions/windows-bootstrap.md))

A version-manager model (several pNet versions side by side, installed like
nvm) is proposed in
[descriptions/version-manager.md](descriptions/version-manager.md) and is not
implemented.
