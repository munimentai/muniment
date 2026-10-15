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
ssh -T -o BatchMode=yes -o ConnectTimeout=10 -o ClearAllForwardings=yes -- build-box docker exec muniment-runtime muniment-server pair --json
```

It then forwards a loopback port to the runtime port and pairs through it:

```sh
ssh -N -T -o BatchMode=yes -o ConnectTimeout=10 -o ExitOnForwardFailure=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -L 127.0.0.1:<local port>:127.0.0.1:7443 -- build-box
```

The tunnel opens whenever the app attaches to the runtime and closes when the
app switches away or quits. The host and command take plain words only.

## States

| State | Next step |
| --- | --- |
| Online | None. |
| Offline | Check that the runtime is running and reachable. |
| Certificate changed | Pair it again to trust its new certificate. |
| Access revoked | Pair it again with a new link. |
| Version mismatch | Update the app or the runtime. |

## Updates

A runtime that runs an older version than the app shows Update when this
device holds the `runtime.admin` scope there. Update sends the runtime an
update request for the app's version. Update all sends it to every such
runtime. A runtime without that scope shows its version only.
