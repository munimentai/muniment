# Runtimes

The desktop app always has its own runtime. It can also attach to Muniment on
another desktop, or to a `muniment-server` runtime on a server or in a
container. The app attaches to one runtime at a time. No account is needed.

Settings → Runtimes lists this computer first, then each paired runtime with
its kind, version and state. A runtime's provider keys and files stay on that
runtime. Threads stay on the runtime that holds them, and the thread list,
models, memory, search and context settings show the selected runtime.
The cloud account and phone pairing stay with this computer.

## Pair with a link

The runtime creates a one-time pairing link:

```text
muniment://pair?v=1&id=<runtime id>&name=<name>&fp=<certificate sha256>&host=<host:port>#<secret>
```

Paste the whole link under Add a runtime and select Pair. The secret after `#`
works once. The app trusts only the certificate the link names, and keeps a
device credential in an owner-only file under `~/.muniment/runtimes/`.

## Pair over SSH

Select Over SSH and enter:

- **SSH host**: any host `ssh` accepts, including an `ssh_config` alias or `user@host`.
- **Remote command**: `muniment-server` by default. For a container, run it through `docker exec`.
- **Runtime port**: `7443` by default.

The app runs the system `ssh` with your keys and no prompt:

```sh
ssh -T -o BatchMode=yes -o ConnectTimeout=10 -o ClearAllForwardings=yes -- build-box docker exec muniment-runtime muniment-server pair --scopes thread.read,run.write,runtime.admin --json
```

It then forwards a loopback port to the runtime port and pairs through it:

```sh
ssh -N -T -o BatchMode=yes -o ConnectTimeout=10 -o ExitOnForwardFailure=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -L 127.0.0.1:<local port>:127.0.0.1:7443 -- build-box
```

The tunnel opens whenever the app attaches to the runtime and closes when the
app switches away or quits. The host and command take plain words only.

The app checks each value the command prints against the link: the
fingerprint, runtime id, runtime name, endpoints, scopes and expiry. A user who
runs the pairing command over SSH controls that host, so the device asks for the
`runtime.admin` scope there.

## Pair with this computer

Under This computer, turn on Allow pairing. Pairing is off until you turn it on.
The listen port defaults to `7443`. The page lists the addresses a link names
and where each comes from: local network, private overlay network, public
address, this computer only, or set in these settings. Create pairing link
shows the link, a QR code and the time it stops working. Allow updates adds the
`runtime.admin` scope to the next link. Paired devices lists each device with
its scopes and when it was last seen, with Revoke.

## States

| State | Next step |
| --- | --- |
| Online | None. |
| Offline | Check that the runtime is running and reachable. |
| Certificate changed | Pair it again to trust its new certificate. |
| Access revoked | Pair it again with a new link. |
| Version mismatch | Update the app or the runtime. |

## Updates

A paired desktop updates to the app's version. A server or container updates to
the core release the app bundles. The app asks a runtime that runs an older
version to update each time it connects, at most once every ten minutes, when
this device holds the `runtime.update` or `runtime.admin` scope there. It does
not ask a runtime that installs by hand, or ask again for a version the runtime
failed to install.

Each row shows the update state the runtime reports:

| State | Shown |
| --- | --- |
| Queued | Update queued |
| Downloading | Downloading vX |
| Draining | Finishing current work, with what it waits for in a tooltip |
| Installing or restarting | Updating to vX |
| Failed | Update failed, with the error in a tooltip |
| Behind, installs by hand | vX available, update manually |

A request the runtime has not acted on shows Update requested. A runtime that
reports no update state shows its version only.

Update in a row's menu asks that runtime. Update all asks every runtime that is
behind and online, lists the runtimes it skips as offline, no permission, manual
install or already current, and updates this computer last.

When a paired client asks this computer's runtime to update, or a paired desktop
runs a newer app, the app checks its release feed and downloads the verified
update. It waits up to two minutes for running replies, then installs. Replies
it stops resume after the restart, marked Resumed after update. When you
install an update yourself, the app names the replies it stops and asks first.
The app clears the request once it runs the requested version.

## Remove

Remove deletes the device credential on this computer. With `runtime.admin`, the
app first revokes the device on the runtime. Otherwise the runtime still lists
the device until you revoke it there.
