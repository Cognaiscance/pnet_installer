# pnet_installer

Installer agent (desire + status) and **bootstrap** (install pNet + agent from
local binaries). See [description.md](description.md).

```bash
# Empty machine (binaries in the same folder as this program).
# A terminal dialog asks for a connection code (DG) or SG identity,
# unless those flags are already on the command line.
./pnet_installer bootstrap

# Agent only, pNet already running:
PNET_AUTO_APPROVE_APPS=1 cargo run --manifest-path ../pNet/Cargo.toml
cargo run
```

The website exists only on a server-grade node: sign in → Home → **Installer**
(`/apps/installer/`). A device-grade node does not serve it.

Windows bootstrap (install and run pNet + this agent) is planned in
[descriptions/windows-bootstrap.md](descriptions/windows-bootstrap.md) and is
not implemented yet.
