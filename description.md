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
./target/debug/pnet_installer bootstrap --prefix ~/.pnet --no-start
# or omit --no-start to launch pnet + agent
# Create/join: http://127.0.0.1:8777/setup
```

`--from DIR` if the binaries are not next to `pnet_installer`. Default prefix
`~/.pnet` (`bin/`, `start.sh`, `logs/`). User-consented: you ran `bootstrap`.

## Run (agent only)

```bash
PNET_AUTO_APPROVE_APPS=1 cargo run --manifest-path ../pNet/Cargo.toml
cargo run
```

Sign in → Home → **Installer** (`/apps/installer/`, once this agent is mounted).

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

See `descriptions/app-store-installer.md`.
