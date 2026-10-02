# pNet Installer (phases 2–3b)

Installer **agent** as a pNet app: catalog, **desired** apps/devices, and
**status**. Catalog apps are still **notify only** (no signed package exec —
phase 4).

**Catalog sources:** `~/.pnet/installer/app_sources/` (or `$PNET_INSTALLER_STATE/app_sources/`).
Each file is a list of GitHub repo URLs. `pnet.list` is managed (official apps).
Drop another file (e.g. `acme.list`) to add org apps. Store cards come from
`pnet-app.json` in the repo, else the GitHub description, else a cache/fallback.

**Phase 3:** `pnet_installer bootstrap` installs **pNet + this agent** from a
**local** binary directory (unpacked dist, or `target/debug` after `cargo build`).
It does not download packages from the network.

pNet stays a dumb pipe. Desire and status are installer↔installer app payloads.

## What the agent does

1. Registers as fabric alias `installer` and portal slug `/apps/installer/`.
2. Shows the catalog from `app_sources/` GitHub URL lists (official `pnet.list`
   plus extra files) at `/apps/installer/`. Core has no `/store` fallback.
3. On the **rank-1 SG** (lowest `sg_rank` among own-user SGs): you enable an app
   and pick devices. That **desire** syncs to other installer agents.
4. Each agent looks at local `get_data`: if the target alias is registered and
   approved → **installed**; if desired but missing → **pending** with the
   copy-install command. It does **not** run `cargo` or unpack a tarball.

Solo node (no SG in the directory) may write desire locally.

## Bootstrap (empty machine)

```bash
# After cargo build -p pnet -p pnet_installer, both bins sit in target/debug:
cargo build --manifest-path ../pNet/Cargo.toml --bin pnet
cargo build
./target/debug/pnet_installer bootstrap --prefix ~/.pnet
```

On a terminal, bootstrap asks what this machine is before it starts `pnet`:

- **Device grade:** device name, a connection code (an invitation from an SG), and a key passphrase. The node does not serve a website.
- **Server grade, new user:** your alias, device name, rank, reachable addresses, the portal password, and a key passphrase.
- **Server grade, join:** device name, connection code, rank, reachable addresses, the portal password, and a key passphrase.

The key passphrase is not the portal password. pNet creates and seals private keys only when `PNET_KEY_PASSPHRASE` is already set (at least 8 characters). Bootstrap writes that variable into `node.env` for every grade. Without it, a new server stays on the setup page and logs `new-user setup failed: passphrase`.

The answers are written to `~/.pnet/node.env` (mode 0600) and exported by `start.sh`. Pass the same values as flags to skip the dialog (`--grade`, `--device-alias`, `--connection-code`, `--user-alias`, `--sg-rank`, `--hosts`, `--admin-password`, `--key-passphrase`). `--no-setup` copies binaries without configuring a node.

After an SG install, sign in at `http://127.0.0.1:8777/`. A DG has no such URL; use an SG's portal.

`--from DIR` if the binaries are not next to `pnet_installer`. Default prefix
`~/.pnet` (`bin/`, `start.sh`, `node.env`, `logs/`). User-consented: you ran `bootstrap`.

## Run (agent only)

```bash
PNET_AUTO_APPROVE_APPS=1 cargo run --manifest-path ../pNet/Cargo.toml
cargo run
```

On an SG, sign in → Home → **Installer** (`/apps/installer/`, once this agent is mounted). On a DG the agent still syncs desire and status, and it does not bind its own website.

| Variable | Default |
|----------|---------|
| `PNET_INSTALLER_WEB_PORT` | `9091` |
| `PNET_INSTALLER_SLUG` | `installer` |
| `PNET_INSTALLER_STATE` | `~/.pnet/installer` |
| `PNET_PORTAL` | `http://127.0.0.1:8777` |
| `PNET_ADDR` | `127.0.0.1:7777` |
| `PNET_SKIP_FABRIC=1` | UI only, no directory sync |
| `PNET_INSTALLER_NO_NETWORK=1` | Do not fetch GitHub; cache + baked cards only |

## Non-goals (this phase)

- Signed package fetch / exec of catalog apps (phase 4)
- Scraping GitHub Pages HTML (cards use `pnet-app.json` / repo API)
- systemd units (start.sh is enough)
- Contact-shared or public catalogs
- Auto-approve of target apps

See `descriptions/app-store-installer.md` in the pNet repo.

Windows install and launch of pNet + this agent is specified in
[descriptions/windows-bootstrap.md](descriptions/windows-bootstrap.md)
(not implemented).
