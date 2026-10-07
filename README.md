# pnet_installer

Bootstraps pNet onto this machine from a local `pnet` binary, or from the
latest published release when you do not have one. It asks for the
node parameters (or takes them as flags), writes `~/.pnet/node.env` and
`start.sh`, and starts `pnet`.

It is not a pNet app. It does not register with the fabric, and it does not
install other programs. Start each app yourself on the device where it should
run, then approve it in Config on that node.

```bash
# With no local pnet, this downloads the latest published release.
# Pass --from DIR, or place pnet next to this program, to use a local binary.
# A terminal dialog asks for a connection code (DG) or SG identity,
# plus the key passphrase, unless those flags are already on the command line.
./pnet_installer bootstrap
```

After a server-grade install, sign in at `http://127.0.0.1:8777/`. A
device-grade node does not serve a website.

A second run compares versions. A newer `pnet` replaces the installed one
after the running node has stopped. An older one is refused. `node.env` is
written only when it is missing, and `~/.pnet/data` is left alone. `--force`
replaces an equal or unreadable binary and still refuses a downgrade. The
rules are in
[descriptions/forward-upgrade.md](descriptions/forward-upgrade.md).
With no local binary, bootstrap downloads the latest published `pnet` for
this machine and checks the archive sha256 before running it. The published
release is `v0.1.0`. Pass `--from`, or place `pnet` next to this program, to
skip the network.

Windows bootstrap (install and run `pnet`) is planned in
[descriptions/windows-bootstrap.md](descriptions/windows-bootstrap.md) and is
not implemented yet. It follows the same forward-only rule.
