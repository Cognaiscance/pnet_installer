# pnet_installer

Bootstraps pNet onto this machine from a local `pnet` binary. It asks for the
node parameters (or takes them as flags), writes `~/.pnet/node.env` and
`start.sh`, and starts `pnet`.

It is not a pNet app. It does not register with the fabric, and it does not
install other programs. Start each app yourself on the device where it should
run, then approve it in Config on that node.

```bash
# pnet must sit next to this program, or pass --from DIR.
# A terminal dialog asks for a connection code (DG) or SG identity,
# plus the key passphrase, unless those flags are already on the command line.
./pnet_installer bootstrap
```

After a server-grade install, sign in at `http://127.0.0.1:8777/`. A
device-grade node does not serve a website.

Windows bootstrap (install and run `pnet`) is planned in
[descriptions/windows-bootstrap.md](descriptions/windows-bootstrap.md) and is
not implemented yet.

A version manager (install and switch pNet versions, nvm-style) is proposed in
[descriptions/version-manager.md](descriptions/version-manager.md) and is not
implemented yet.
