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

The current upstream package identity is **`@earendil-works/pi-coding-agent`**.
Muniment's production executable pin remains **0.73.1**, upstream tag
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
The signed client compiles both the new descriptor and exactly one retained
predecessor descriptor during a pin update. Pointer resolution accepts only
those identities, verifies each revision with its own size, digest, archive
name and executable layout, and atomically repoints `current` to `previous`
when supervisor activation of the new pin fails. `current` resolves the pin
alone and `previous` resolves the retained predecessor, so a host that holds
only the predecessor acquires the pin instead of running the predecessor. It
never resolves an arbitrary version or filesystem path from pointer contents.

### Amendment 2026-09-05: desktop-owned version policy

The production pin records the exact Pi and extension versions a shipped
desktop carries. A separate candidate records the exact versions the nightly
exercises and may run ahead of production. Only Muniment Desktop's passing
nightly evidence on Linux, macOS, and Windows qualifies that candidate for
promotion through a signed desktop release. Nothing else promotes a version.
The desktop never adopts a release because it is newest.

The harness installs only packages listed on [pi.dev/packages](https://pi.dev/packages)
and executables from the official [earendil-works/pi repository](https://github.com/earendil-works/pi).
It rejects unlisted npm packages, forks, and mirrors outside Muniment's control.
The distribution checks below still govern any Muniment-controlled mirror.
A pin move retains the verified predecessor and the pointer rollback rule above.
[SPEC.md](../../SPEC.md#pi-version-policy) records the approved extensions,
built-in tools, and Active LTS Node requirement from the pinned Pi's `engines`
field. The production descriptor stays at 0.73.1.

### Candidate track

The candidate is **0.85.1** from the official `earendil-works/pi` tag `v0.85.1`.
Its release base is `https://github.com/earendil-works/pi/releases/download/v0.85.1`.
The candidate descriptors are:

| Target | Release archive | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| macOS arm64 | `pi-darwin-arm64.tar.gz` | 31,035,676 | `d5f70e3c0cf7398eac239fd0261ee074d98b7ba7f6b43fe3617f052ed5b79d06` |
| macOS x64 | `pi-darwin-x64.tar.gz` | 33,544,584 | `adb918b845625f184d8bea408d55eacaf21aa87238793c0f5b4f3b9737bce62b` |
| Linux arm64 | `pi-linux-arm64.tar.gz` | 42,628,180 | `042d20ae885ee4f3b102815f3280b962c377b2e9fb44de4037908cc530eae4d4` |
| Linux x64 | `pi-linux-x64.tar.gz` | 42,560,927 | `494e498f47d74d21f40b3386f6a5e921a3d49531a169cab55bbdaca0ea1fe25a` |
| Windows x64 | `pi-windows-x64.zip` | 45,009,021 | `002fa95b90d521245b9985d8f168caebc237ad56e7e30b319807dee1b2e17e1c` |

The candidate pins these five packages from [pi.dev/packages](https://pi.dev/packages):

| Package | Version |
| --- | --- |
| [pi-web-access](https://pi.dev/packages/pi-web-access) | `0.28.0` |
| [pi-subagents](https://pi.dev/packages/pi-subagents) | `0.65.1` |
| [pi-background-tasks](https://pi.dev/packages/pi-background-tasks) | `2.5.0` |
| [pi-mcp-adapter](https://pi.dev/packages/pi-mcp-adapter) | `2.32.1` |
| [pi-claude-bridge](https://pi.dev/packages/pi-claude-bridge) | `0.7.0` |

Every candidate chat launch merges these exact npm sources into the Pi agent directory's `settings.json`.
The same merge sets `defaultTools` to `read`, `bash`, `powershell`, `edit`, `write`, `grep`, `find`, and `ls`.
The names match the candidate's [built-in registry](https://github.com/earendil-works/pi/blob/v0.85.1/packages/coding-agent/src/core/tools/index.ts#L96-L105).
The desktop never passes `--tools`.
The merge preserves other keys, including `defaultProvider`, and shares Pi's directory lock with local provider writes.
The lock follows proper-lockfile's ten-second stale threshold and five-second heartbeat.
The directory resolver follows the candidate's [`normalizePath`](https://github.com/earendil-works/pi/blob/v0.85.1/packages/coding-agent/src/utils/paths.ts#L78-L107), including file URLs and Windows shell drive paths.
Production adds neither key and leaves existing settings untouched.

The web package supports an Anthropic `claude-haiku` model for summaries and requires a Bright Data `serp` zone for Bright Data search.
The desktop configures neither requirement.
A missing Bright Data zone produces a tool error message without stopping Pi.
The package also supports keyless search providers.

The build-time environment switch `MUNIMENT_PI_CANDIDATE=1` selects the candidate.
Every other value, including an absent switch, selects production 0.73.1.
A runtime environment variable cannot change the compiled track.
The nightly passes the switch through `desktop-ci --env-stdin` for the build and all three installed E2E lanes.
This also covers the macOS E2E rebuild.
Production builds and the PR gate leave the switch unset.

The candidate retains exactly one predecessor, the verified 0.73.1 production descriptor.
A failed candidate activation restores that installed predecessor through the same `current`/`previous` pointer rule.
A fresh installation has no predecessor to restore.
Both tracks use the same size, digest, extraction, and readiness checks.
The candidate Windows ZIP has a flat root.
Extraction places its files beneath the installed `pi/` directory and rejects unsafe paths.

The candidate package's [`engines.node`](https://registry.npmjs.org/@earendil-works/pi-coding-agent/0.85.1) sets the Node floor at **22.19.0** (`>=22.19.0`).
The selected Node line is **24, Active LTS**, which satisfies that floor.
The [Node release schedule](https://github.com/nodejs/Release/blob/main/schedule.json) governs the Active LTS requirement.
Node 22 defines the minimum, not the selected Active LTS line.
The standalone executable includes its runtime.
The desktop acquires the five packages through the verified executable's embedded Bun package manager before RPC starts.
It sets [`BUN_BE_BUN=1`](https://bun.com/docs/bundler/executables#act-as-the-bun-cli) only for acquisition and disables install scripts and peer dependencies.
The package cache lives in the Pi agent directory's `npm` directory.
An OS lock excludes concurrent desktop installs and releases after a crash.
A completion marker admits the cache only after acquisition succeeds and all four manifests match the pins.
The first launch needs network access, not system Node or npm.
The candidate allows 120 seconds for acquisition and 120 seconds for the first readiness response.
Later health probes keep their 10-second deadline.

The candidate's [RPC contract](https://github.com/earendil-works/pi/blob/v0.85.1/packages/coding-agent/docs/rpc.md) uses newline-delimited JSON over stdio.
The installed nightly tests qualify the dispatcher against the candidate before promotion.
A dialect failure blocks promotion and needs a dispatcher fix in a separate change.

### Distribution

The platform-native executable is **acquired on first use**, not bundled in
the desktop installer. The user is told that the agent engine is missing and
shown its version, source, license, download size and disk impact before an
explicit install. This is an honest recoverable feature state: the shell and
control-plane features remain usable, while chat is unavailable until Pi is
installed. After Pi resolves its packages, later launches can use the local package cache.
Model traffic uses the configured provider.

Acquisition reuses the shared bounded HTTPS transport, verified staging,
install coordinator, lock/free-space protections, atomic publication,
`current`/`previous` pointers and activation rollback defined by ADRs 0005 and
0006. It adds a `pi` artifact descriptor and archive extraction inside the
verified stage; extraction admits only regular files and directories beneath
the installed `pi/` directory and rejects absolute paths, parent
traversal, links and special files. Publication occurs only after archive
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
program = <owned verified revision>/pi/pi[.exe]
args    = --mode rpc --session-dir <app-owned Pi session root>
env     = no Pi-specific additions (the supervisor currently inherits the
          desktop process environment); later provider secrets must use a
          scoped channel and never appear in argv or diagnostics
```

The session-resume groundwork amends the arguments above to `--mode rpc
--session-dir <app-data>/pi-sessions` for a new persistent conversation, plus
`--session <validated-file>` only for an explicit reopen. The app creates and
owns the root. Rust accepts only a regular JSONL file whose canonical path is
beneath it, and path failures use redacted diagnostics.

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
`PiRpcTransport` has a persistent stdout reader, serializes application calls
and periodic health probes through it, correlates responses by ID, and
broadcasts unrelated events and responses in arrival order. The reader keeps
draining progress events after command acceptance even if no later RPC occurs.

The surface required by harness-spec §6.2 is present: `prompt` accepts
`streamingBehavior: "steer" | "followUp"`; explicit `steer` and `follow_up`
commands also exist. `set_steering_mode` and `set_follow_up_mode` select `all`
or `one-at-a-time`. Prompt success means accepted/queued, with subsequent
progress arriving as events rather than a second response. The chat slice must
preserve this distinction and must not send these envelopes through the
repository's JSON-RPC 2.0 transport.

The pure chat boundary exposes validated `steer` and `follow_up` envelopes and
submits them through that same dispatcher. Their matching successful response
means only that the message was queued: `steer` is delivered during the active
turn, while `follow_up` waits until it finishes. Agent events around either
acknowledgement remain on the existing ordered stream.

## Consequences and follow-up slices

The application gains a reproducible runtime with modest on-demand transfer,
known rollback identity, and no system Node dependency. First chat use needs a
download and enough space for staging plus current/previous revisions. Release
engineering must complete platform signing/notarization and native extraction
before shipping.

After prompt acceptance, the pinned `get_state` contract supplies
`data.sessionFile`. Muniment validates that identity and durably binds its
root-relative filename to the local run with a single typed
`runtime.pi_session.bound` journal event. The local journal remains the only
source for rendered history, receipts, and provenance; raw Pi session JSONL is
runtime recovery state and never crosses the webview boundary. Duplicate or
conflicting binding and malformed payloads fail closed during reduction.

This decision does not authorize automatic continuation. On startup an
interrupted run remains `run.needs_attention`, even when it has a binding; no
prompt, permission answer, or tool is re-executed. Explicit safe continuation
is deferred to the next slice.

The gated next slice is chat through this supervised Pi process against the
user's LiteLLM virtual endpoint, with one multiplexing RPC dispatcher, event
projection into the durable journal, permission gates, and steer/follow-up.
This ADR adds no chat UI, thread surface, provider call, or cloud mock.

CI does not vendor Pi. The real-spawn test is gated by `MUNIMENT_PI_ARCHIVE`,
which names the downloaded release archive. The test passes that archive
through the production verification, safe extraction and publication lifecycle
before resolving and supervising the installed executable. Like model-dependent
tests, ordinary three-platform CI skips it; an artifact job downloads the exact
target descriptor to a temporary directory, sets the variable, and runs
`cargo test --test pi_sidecar`.

## Sources

- Pi 0.73.1 release and platform artifacts: <https://github.com/earendil-works/pi/releases/tag/v0.73.1>
- Pi RPC protocol: <https://github.com/earendil-works/pi/blob/v0.73.1/packages/coding-agent/docs/rpc.md>
- Current npm package identity: <https://www.npmjs.com/package/@earendil-works/pi-coding-agent>
- Historical 0.73.1 package metadata: <https://github.com/earendil-works/pi/blob/v0.73.1/packages/coding-agent/package.json>
- Shared lifecycle and verified publication: [ADR 0006](0006-resident-gemma-model-lifecycle.md)
- Shared acquisition rules: [ADR 0005](0005-asr-model-lifecycle.md)
