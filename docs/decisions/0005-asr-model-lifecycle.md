# 0005 — Install and atomically publish ASR model revisions

- Status: accepted
- Date: 2026-07-10
- Context: ROADMAP Phase 3 item 15; ADR 0004

## Context

ADR 0004 pins an approximately 670 MB, four-file Parakeet model set and
requires all four files to verify before recognition can see them. It does not
decide how those bytes reach an installation or how interrupted work recovers.
Hugging Face recommends full commit revisions for immutable downloads and
warns that its cached paths must not be modified. Muniment needs those
properties without importing the Python cache implementation.

## Decision

### Acquisition

The initial model set is an **on-demand first-use install** into the desktop
application-data directory. Opening dictation without a verified set presents
the download size, source, attribution, disk requirement, and an explicit
Install action. App startup never downloads it. Once installed, recognition is
entirely offline; acquisition is a separate native operation, not recognizer
runtime.

Bundling is rejected because it adds roughly 670 MB to every installer and app
update, including for users who never dictate. It would make first dictation
offline, but at disproportionate distribution cost. An automatic background
download is rejected because it spends bandwidth and disk without intent. A
separate model-pack installer is deferred because it adds another signed
artifact and support matrix, though a future air-gapped pack may enter the same
verification protocol. The choice keeps the app install small and otherwise
offline-first, while clearly requiring a connection for first-time dictation.

### Layout and atomic publication

Tauri resolves the platform application-data directory. The native app owns:

```text
asr/
  install.lock
  current                         # revision and manifest identity
  previous                        # last known-good pointer, if any
  revisions/
    [full-revision]/
      encoder.int8.onnx
      decoder.int8.onnx
      joiner.int8.onnx
      tokens.txt
  staging/
    [random-install-id]/
      [filename].part
```

Published revision directories and files are immutable. Recognition never
reads `staging`. Paths come only from compiled descriptors; remote input cannot
supply a filename or path.

The installer takes `install.lock` exclusively and creates a random stage on
the same filesystem. It downloads only ADR 0004's four filenames from the four
full-revision URLs. A complete part is renamed to its final name within the
stage. The installer then calls `verify_parakeet_model_set(stage)`. Only success
allows it to sync the files and stage, atomically rename the stage to
`revisions/[full-revision]`, and sync `revisions` where supported. An existing
revision is reused only if the same complete-set verification succeeds;
otherwise it is quarantined for cleanup, never merged with a stage.

The installer writes and syncs a temporary pointer, preserves the verified old
`current` as `previous`, then replaces `current` using the platform atomic
replace operation (POSIX rename and replace-existing file semantics on
Windows). It syncs `asr` where supported. Readers parse one pointer snapshot
and resolve exactly that immutable directory; they never scan stages or infer
current from directory names. A crash therefore exposes the old complete set
or the new complete set, never a mixture. Partial, wrong-size,
digest-mismatched, or unsyncable stages leave `current` untouched.

This requires a platform atomic-replace abstraction rather than assuming
`std::fs::rename` overwrites identically everywhere. Pointer files, not
symlinks, support Windows. A future revision adds a compiled manifest and
verifier; a pointer is valid only when both identities are known to the app.

### Bounded native download

The Rust-native downloader streams each response into a `.part` file and an
incremental SHA-256 state through a fixed buffer no larger than 1 MiB. Files
are sequential, so no artifact or set is held in memory. It rejects redirects
away from HTTPS or the expected Hugging Face host chain, non-success statuses,
invalid lengths/ranges, excess bytes, and results that fail compiled size or
digest. Errors and logs never contain redirect URLs, query strings, proxy
credentials, response bodies, or filesystem paths.

The initial implementation uses a 10-second connect timeout, a 30-second
no-progress/read timeout, and a 30-minute deadline per file. Cancellation
closes the response promptly and retains a valid partial. Connection loss and
timeouts get at most three attempts with capped jittered backoff (1, 2, then 4
seconds). Authentication, certificate, permission, storage, and verification
failures are not blindly retried.

Resume requests a byte `Range` at the existing part length. A 206 must contain
the expected `Content-Range`; a 200 truncates and restarts the part. A 416,
inconsistent range, oversized part, or changed validator also restarts that
file. Full size and digest remain authoritative. Cancelled/transient stages
survive for seven days. On the next locked install, older and malformed,
rejected, or unknown stages are deleted without following links or crossing
the owned `asr` root.

Before download, free space on the application-data volume must cover the
stage bytes still missing plus a 256 MiB margin; any installed current set is
retained rather than counted as reclaimable space. This permits the stage and
rollback revision to coexist. Space is rechecked before publication. If it
cannot be determined, installation stops with an actionable storage error.

The client honors OS HTTPS proxy and certificate configuration. It has no
proxy-credential UI and no insecure HTTP fallback. Offline, DNS, proxy, and
timeout failures keep a known-good set usable and produce a retryable redacted
state; startup never polls the network. UI states are `not installed`,
`checking storage`, `downloading` (file ordinal, aggregate verified bytes,
total, bounded rate/ETA), `verifying`, `installing`, `ready`,
`paused/cancelled`, and `failed` with a stable category and Retry/Remove. They
contain no bytes, bodies, credentials, raw URLs, or local paths.

### Recovery, update, rollback, and removal

Every process verifies the pointed complete set before recognizer startup. If
`current` is absent, a lock holder may recover an interrupted pointer publish
from a fully published known-manifest revision; it never promotes a stage. If
`current` is unknown or fails verification, a verified `previous` is restored
atomically. If neither verifies, ASR is `not installed`/`repair required`:
dictation is disabled with Install/Retry offered, while the rest of Muniment
continues.

Only the lock holder may stage, publish, change pointers, clean, or remove.
Other launches keep using their already-open immutable revision and observe
the result after release; they never start a second download. Lock waiting is
cancellable and reports that another Muniment process is managing the files.
A crash releases the OS lock; no stale PID file grants ownership.

An update requires a compiled-manifest change and uses the same protocol;
automatic update checks are out of scope. The old current remains `previous`
until the new revision verifies and completes one recognizer startup. If
recognizer construction/model loading fails, the app records only a redacted
category, atomically restores verified `previous`, and retries startup once.
The rejected revision is not automatically selected again. If rollback fails,
ASR is disabled. After success, revisions other than current and previous may
be removed under the lock.

“Remove downloaded model” stops recognition, takes the lock, atomically removes
the pointers, and deletes only known entries under `asr`, without following
links. Failure is retryable. Uninstallers must remove app-installed native
libraries and notices. Because some platforms retain application data, release
UI and uninstall documentation must expose model removal and state whether
that uninstaller removes `asr`; it must never delete a shared Hugging Face
cache.

### Trust and ownership boundaries

Pure `muniment-core` owns the compiled manifest, constant-memory verification,
lifecycle policy/state, locking abstraction, redacted errors, and explicit
model-directory input. It has no Tauri/webview dependency. The Tauri native
adapter supplies the application-data path, OS HTTP/proxy and free-space
implementations, atomic replacement, and commands returning typed state only.
Future UI may install, cancel, retry, and remove; it cannot choose URLs,
revisions, filenames, or paths.

The same trust boundary applies to ADR 0004's pinned `silero_vad.onnx`
(revision `af4fcfc9b8305246b1fe2ebcaf248975673166f1`, 1,807,522 bytes, SHA-256
`a35ebf52fd3ce5f1469b2a36158dba761bc47b973ea3382b3186ca15b1f5af28`).
Pure core accepts an explicitly supplied installed file path and verifies that
compiled identity before constructing the native detector. Missing,
non-regular, unreadable, wrong-size, or digest-mismatched files yield typed,
path-redacted errors. This slice does not add VAD acquisition or publication;
packaging must place the verified file and its upstream MIT notice alongside
the native runtime before capture integration is enabled.

Responses stream native-to-disk. Model bytes, partials, digests, paths, and
handles never cross Tauri IPC or enter the webview, telemetry, crash reports,
Pi, llama-server, prompts, or sidecar I/O. PCM retains ADR 0004's stricter
no-network boundary. Acquisition is the only ASR-related network component and
is inactive during recognition.

### Licenses and notices

The installed app must contain user-readable Third-Party Notices reachable
from Settings/About. It credits NVIDIA and `parakeet-tdt-0.6b-v3`, links the
source model and converted pinned revision, identifies the sherpa-onnx
conversion, includes or links CC BY 4.0, and preserves supplied notices. The
eventual release bundle must carry sherpa-onnx's Apache-2.0 license and reviewed
licenses/notices for the sherpa-onnx and ONNX Runtime artifacts actually
packaged.

The first-use screen shows attribution and links the installed notice before
download; it remains available offline and after model removal. Build/release
checks must assert notices are present beside the eventual native bundle. This
is a delivery contract, not a claim that native packaging is implemented.

## Consequences and follow-up slices

First dictation needs the transfer, staging space, and a wait. In return,
non-dictation users avoid that cost, publication is deterministic, interrupted
work recovers, concurrent mutation is serialized, and a bad update cannot
displace the last working set.

1. Add pure-core lifecycle planning/state over injected filesystem, HTTP,
   atomic-replace, and lock boundaries. Test tiny four-file manifests with
   temporary directories and an in-process HTTP server: cancellation, range
   resume, digest failure, interruption before pointer replacement, and
   concurrent installers. CI never downloads the real model.
2. Add Tauri app-data, free-space, platform lock/replace, native HTTP/proxy, and
   typed command adapters, still using local fixtures.
3. Add install/progress/cancel/repair/remove UI and installed attribution.
4. Package pinned sherpa-onnx/ONNX Runtime binaries and reviewed notices, then
   construct the recognizer only from a verified current path.
5. Exercise real acquisition outside CI and complete ADR 0004's hardware,
   quality, packaging, and uninstall matrix.

## Sources

- Hugging Face download guidance: <https://huggingface.co/docs/huggingface_hub/en/guides/download>
- Pinned model and license duties: [ADR 0004](0004-desktop-asr-runtime.md)
