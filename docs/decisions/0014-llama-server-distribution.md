# 0014 — Acquire a pinned llama-server executable

- Status: accepted
- Date: 2026-07-19
- Context: Phase 3 voice; ADRs 0003 and 0006

## Context

The resident Gemma boundary requires an explicit `llama-server` executable
path. ADR 0003 pins the model, ADR 0006 pins its acquisition lifecycle, and the
core already constrains the server to loopback and verifies the model before
spawn, but none identifies the runtime executable. Depending on a system
llama.cpp installation would make behavior and API compatibility
machine-dependent.

Upstream llama.cpp publishes frequent, versioned GitHub releases with several
CPU- and accelerator-specific archives. The desktop nightly targets Linux x64,
Windows x64, and universal macOS (arm64 and x64), so a reproducible selection
must cover all four native architectures without requiring a particular GPU,
driver, or instruction-set tier.

## Decision

### Identity and platform baseline

Muniment pins the **`ggml-org/llama.cpp` release `b10068`** under the MIT
license. The immutable release URLs are beneath
`https://github.com/ggml-org/llama.cpp/releases/download/b10068/`. The selected
descriptors, verified from the downloaded upstream archives, are:

| Nightly target | Upstream archive | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Linux x64 | `llama-b10068-bin-ubuntu-x64.tar.gz` | 16,066,558 | `6bf3d20de562e4df230f1a7c54fb7a06a80c7ff40f5311c953e8255744be4eb2` |
| Windows x64 | `llama-b10068-bin-win-cpu-x64.zip` | 18,007,324 | `01d5f30876acfb4a0be59396710f450213495c7181d8fbcce2fad045835ceb89` |
| macOS arm64 | `llama-b10068-bin-macos-arm64.tar.gz` | 10,603,591 | `13aa2d40c76ad1dcb8ebeec5f0d2814bf3b2f84a66935c7d4dc6f7cca8e38d68` |
| macOS x64 | `llama-b10068-bin-macos-x64.tar.gz` | 10,876,051 | `73a63a0fdcfd8d0625fe20aa8f2af62e3d6437c6380b46129ca1a9abacbde0d5` |

Linux and Windows deliberately use upstream's unaccelerated x64 archives.
Muniment does not select CUDA, HIP, Vulkan, SYCL, OpenVINO, or another
driver-specific build: CPU execution is slower but is the only fail-safe
baseline across supported hardware and avoids making first launch depend on a
GPU runtime. The architecture-native macOS archives are the upstream baseline;
they may use Metal where supported and retain CPU fallback. The universal app
selects the archive matching the running process architecture. There is no
Rosetta fallback, cross-architecture archive selection, or unrecognized-target
guess; unsupported OS/architecture combinations fail closed.

The archive name, exact byte count, digest, expected top-level
`llama-b10068/` directory, and `llama-server` (`llama-server.exe` on Windows)
path are compiled into a signed Muniment release. Muniment never follows a
latest release, mutable manifest, or user/webview-supplied URL or path. A pin,
archive, variant, layout, size, or digest change requires a reviewed descriptor
change in a signed app release and qualification on every affected target.

### Acquisition and publication

The runtime is **acquired on first use**, alongside but independently from the
resident model, rather than bundled in every installer. Before an explicit
install, the UI identifies llama.cpp, version, source, MIT license, transfer
size, and disk impact. Gemma-dependent polish and classification remain
unavailable until both artifacts verify; unrelated desktop features remain
usable. Bundling is rejected because it adds about 11–18 MB per platform to
each single-architecture installer, or about 21.5 MB to universal macOS, and to
every app update even where local Gemma features are unused.

Acquisition follows ADR 0006's bounded HTTPS download, owned same-filesystem
staging, install lock, free-space check, atomic publication, and
`current`/`previous` pointer pattern, as extended for executable archives by
ADR 0008. Extraction accepts only regular files and directories below the
expected top-level directory and rejects absolute paths, parent traversal,
links, and special files. The compressed byte count and SHA-256 are verified
before extraction; publication also requires the expected server and its
runtime libraries to be regular files in the declared layout. No executable
may spawn from staging.

The app owns the runtime root beneath platform application data and makes each
published revision non-writable before it can become current:

```text
llama-server/
  install.lock
  current
  previous
  revisions/
    b10068/<verified archive contents>
  staging/<random-install-id>/...
```

Pointer resolution accepts only descriptors compiled into the signed client.
Immediately before every spawn it resolves one pointer snapshot, re-verifies
the archive identity recorded at publication and the expected executable and
runtime layout, and launches that owned executable through the existing
`SupervisedLlamaServer`. The existing `LlamaServerConfig` remains authoritative
for separate arguments and loopback-only `127.0.0.1` or `::1` binding; a
verified runtime must not weaken the boundary documented in
[`docs/sidecar.md`](../sidecar.md).

Updates and recovery use ADRs 0006 and 0008: retain exactly one verified
previous revision, select a new revision only after its normal health-checked
activation, atomically restore `previous` after activation failure, and never
promote staging or scan for an arbitrary executable. Concurrent readers keep
their opened immutable revision while the lock holder alone publishes,
repoints, or cleans. If neither known revision verifies, Gemma-dependent
features fail closed and offer repair; the rest of the app continues.

The downloaded archive is not covered by Muniment's installer signature.
HTTPS authenticates transport and the compiled size and SHA-256 admit the
artifact; this decision does not claim an upstream signature. Release
qualification must check macOS Gatekeeper/notarization and Windows
publisher/reputation behavior, using the signed-mirror contingency described
by ADR 0008 if upstream payloads cannot ship acceptably.

llama.cpp's MIT license requires the copyright and permission notice to be
included with substantial portions of the software. Implementation must ship
the upstream license with the installed revision and add the llama.cpp
attribution and MIT text to the app's user-readable `THIRD_PARTY_NOTICES.md`.
This ADR does not make that implementation change.

### Considered alternatives

**Bundle the archives in installers — rejected.** This avoids a first-use
download but increases every platform installer and every update, couples
runtime replacement to app installation, and ships two macOS architectures in
the universal product whether local inference is used or not.

**Build llama.cpp from source in Muniment CI — rejected.** This could tune each
binary and apply Muniment signing, but makes compiler, SDK, CMake flags, and
transitive native dependencies part of the artifact identity and creates a
new reproducible-build and patch-maintenance obligation. The exact upstream
release binaries are the smaller auditable decision.

**Use accelerator-specific archives — rejected as the baseline.** CUDA, HIP,
Vulkan, SYCL, and OpenVINO can improve latency but add driver and device
compatibility matrices and can fail on otherwise supported machines. A later
opt-in tier requires its own pinned descriptors and measured fallback policy.

**Use in-process Rust bindings — rejected.** Bindings would remove loopback
HTTP overhead but move a large native ABI and its crashes into the desktop
process, duplicate the completed typed server/client boundary, and couple
llama.cpp upgrades to Rust FFI and application linking. The supervised process
preserves crash isolation and the existing restart/shutdown contract.

**Use a system-installed `llama-server` — rejected.** Its version, build flags,
binary integrity, and accelerator dependencies cannot be pinned or rolled back
by Muniment.

## Consequences and follow-up slices

The resident model now has a reproducible executable source on every desktop
nightly target, with a portable baseline and the same verified, recoverable
lifecycle as other native artifacts. First use needs a second download and
space for staging plus current/previous revisions; CPU-only Linux and Windows
may be slower than hardware-specific builds.

Implementation is deferred: this ADR adds no downloader, extraction code,
descriptor, supervisor behavior, command, bundle resource, notice file, or CI
job. A follow-up must implement acquisition, platform qualification, notice
delivery, and real-artifact activation tests before release.

## Sources

- llama.cpp `b10068` release and artifacts: <https://github.com/ggml-org/llama.cpp/releases/tag/b10068>
- llama.cpp MIT license at the pinned tag: <https://github.com/ggml-org/llama.cpp/blob/b10068/LICENSE>
- Loopback and verified-before-spawn boundary: [`docs/sidecar.md`](../sidecar.md)
- Resident model lifecycle: [ADR 0006](0006-resident-gemma-model-lifecycle.md)
- Executable acquisition precedent: [ADR 0008](0008-pi-runtime-distribution.md)
