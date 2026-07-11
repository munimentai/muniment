# 0008 — Acquire and supervise a pinned Pi executable

- Status: accepted
- Date: 2026-07-11
- Context: ROADMAP Phase 2 item 9; harness-spec §§2, 6.2

## Context

Muniment specifies Pi as its MIT-licensed agent engine, isolated as an RPC
sidecar. The desktop bundle has no Node.js runtime, and neither a package name
nor a runtime distribution contract previously made that architecture
executable. Requiring a user's system Node would make installations
machine-dependent and would turn a missing agent engine into an unexplained
prerequisite, distinct from the product's stated need for its hosted control
plane.

Pi is published on npm, while the same release also publishes Bun-compiled,
Node-compatible standalone executables. The latter include the runtime and are
the upstream project's supported non-Node installation path.

## Decision

### Identity and updates

Muniment pins **`@mariozechner/pi-coding-agent` 0.73.1**, upstream tag
`v0.73.1`, under the MIT license. The executable descriptors are:

| Target | Release archive | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| macOS arm64 | `pi-darwin-arm64.tar.gz` | 28,567,469 | `c64f501cad8fa0a581257dc9e878e1b2f351f295d0d85d573fe8d7967bfb1bee` |
| macOS x64 | `pi-darwin-x64.tar.gz` | 31,000,715 | `e59fded1f79fbc7b12e263bf43d1e358af598f6fb3c4d4f58e16d1c5ebe6b2b5` |
| Linux arm64 | `pi-linux-arm64.tar.gz` | 44,095,594 | `f47455b6a7ff6e43752a37c7c0a08b8054efd82cae1efc06d007c94f06a56318` |
| Linux x64 | `pi-linux-x64.tar.gz` | 45,540,364 | `00f0db9e93f6ba33deb1bb4d75b4eafede9fa5379b635a908cf967d5b37e366d` |
| Windows x64 | `pi-windows-x64.zip` | 48,225,045 | `8bdb8e612a4b820f939a524652709b167ac5f1d4d1bba25988a631bff0bbe80b` |

The archive URL is the immutable GitHub release URL under
`earendil-works/pi/releases/download/v0.73.1/`; archive name, byte count, and
digest are compiled into a signed Muniment release. Selection is an exact
OS/architecture match and unsupported targets fail closed.

Pi never follows npm `latest`, a semver range, or a mutable release manifest.
Changing the version, package identity, archive, or digest requires review of
the upstream changelog, license and RPC compatibility, a descriptor amendment
in a signed app release, and the real-artifact readiness test on every changed
target. Retain the previous verified revision until the replacement passes
activation health; rollback selects it after a failed activation.

### Distribution

The platform-native executable is **acquired on first use**, not bundled in
the desktop installer. The user is told that the agent engine is missing and
shown its version, source, license, download size and disk impact before an
explicit install. This is an honest recoverable feature state: the shell and
control-plane features remain usable, while chat is unavailable until Pi is
installed. Once installed, launching Pi itself is offline; model traffic still
uses the product's configured control plane.

Acquisition reuses the shared bounded HTTPS transport, verified staging,
install coordinator, lock/free-space protections, atomic publication,
`current`/`previous` pointers and activation rollback defined by ADRs 0005 and
0006. It adds a `pi` artifact descriptor and archive extraction inside the
verified stage; extraction rejects absolute paths, parent traversal, links,
extra executables and special files. Publication occurs only after archive
size/digest verification and verification that the expected `pi` (`pi.exe` on
Windows) is a regular executable. No webview-supplied URL or path participates.

Bundling the executable was rejected because it adds roughly 29–48 MB to every
installer and app update, including installations that cannot yet use chat.
Bundled Node plus a vendored npm dependency tree is larger, has more files to
audit, and adds a second runtime update surface. Installing the npm package
against system Node is rejected because upstream requires Node >=20.6.0 and
Muniment cannot assume, constrain, or patch that runtime.

The downloaded bytes are not covered by Muniment's installer signature.
HTTPS plus the compiled digest establishes the admitted identity; the app
records upstream provenance but does not claim upstream signature verification.
Release qualification must verify that the upstream macOS executable satisfies
Gatekeeper/notarization and that Windows publisher/reputation behavior is
acceptable when acquired after install. A client cannot add Muniment's signing
identity at first run. If upstream artifacts do not pass those checks, a signed
Muniment-controlled mirror must codesign/notarize the unchanged pinned payloads
ahead of time; switching URLs and hashes requires a descriptor amendment, while
the same verified staging flow remains in use. Linux relies on the compiled
digest and owned, non-writable executable path. Until those platform checks (or
the signed mirror) exist, distribution remains a development/CI facility and
is not release-ready.

### Spawn and readiness contract

`SidecarSupervisor` launches the verified current executable directly:

```text
program = <owned verified revision>/pi[.exe]
args    = --mode rpc --no-session
env     = no Pi-specific additions (the supervisor currently inherits the
          desktop process environment); later provider secrets must use a
          scoped channel and never appear in argv or diagnostics
```

The normal supervisor policy applies: at most five restarts per 60 seconds,
exponential backoff from 250 ms to 10 seconds, a 30-second startup deadline,
15-second health interval, and a two-second graceful-shutdown deadline before
termination. Exit, malformed output, a failed probe, or startup timeout is
diagnosed using bounded stderr and consumes the same restart budget.

Readiness sends `{"id":"muniment-ready-N","type":"get_state"}` and requires
a matching successful `response` for command `get_state` within the bounded
probe timeout. It is side-effect free and requires neither credentials nor a
model call. The production RPC dispatcher must be the sole stdout reader and
route matching probe responses without dropping agent events. Startup and
periodic health probes use that same dispatcher; a probe yields while a bounded
application call is in flight instead of competing for stdout.

### Verified RPC surface

Pi 0.73.1 RPC is newline-delimited JSON over stdio, **not JSON-RPC 2.0**: there
is no `jsonrpc` member or method/params envelope. Commands use a `type` field,
optional correlation `id`, and receive `{type:"response", command, success,
data?}` while agent events are interleaved on stdout. `get_state` is the
readiness equivalent of ping. The real pinned Linux x64 executable was run
with the launch arguments above and returned the required correlated state
response without a provider credential or network model call. The implemented
`PiRpcTransport` serializes application calls and periodic health probes through
one stdout consumer, correlates responses by ID, and broadcasts unrelated
events and responses in arrival order rather than consuming them.

The surface required by harness-spec §6.2 is present: `prompt` accepts
`streamingBehavior: "steer" | "followUp"`; explicit `steer` and `follow_up`
commands also exist. `set_steering_mode` and `set_follow_up_mode` select `all`
or `one-at-a-time`. Prompt success means accepted/queued, with subsequent
progress arriving as events rather than a second response. The chat slice must
preserve this distinction and must not send these envelopes through the
repository's JSON-RPC 2.0 transport.

## Consequences and follow-up slices

The application gains a reproducible runtime with modest on-demand transfer,
known rollback identity, and no system Node dependency. First chat use needs a
download and enough space for staging plus current/previous revisions. Release
engineering must complete platform signing/notarization and native extraction
before shipping.

The gated next slice is chat through this supervised Pi process against the
user's LiteLLM virtual endpoint, with one multiplexing RPC dispatcher, event
projection into the durable journal, permission gates, and steer/follow-up.
This ADR adds no chat UI, thread surface, provider call, or cloud mock.

CI does not vendor Pi. The real-spawn test is gated by
`MUNIMENT_PI_EXECUTABLE`, like model-dependent tests: ordinary three-platform
CI skips it; an artifact job downloads the exact target descriptor into a
temporary directory, verifies bytes and SHA-256, extracts it outside the repo,
sets the variable, and runs `cargo test --test pi_sidecar`.

## Sources

- Pi 0.73.1 release and platform artifacts: <https://github.com/earendil-works/pi/releases/tag/v0.73.1>
- Pi RPC protocol: <https://github.com/earendil-works/pi/blob/v0.73.1/packages/coding-agent/docs/rpc.md>
- Pinned npm package metadata: <https://www.npmjs.com/package/@mariozechner/pi-coding-agent/v/0.73.1>
- Shared lifecycle and verified publication: [ADR 0006](0006-resident-gemma-model-lifecycle.md)
- Shared acquisition rules: [ADR 0005](0005-asr-model-lifecycle.md)
