# 0006 — Acquire and atomically publish resident model revisions

- Status: accepted
- Date: 2026-07-11
- Context: ROADMAP Phase 2 item 10; ADR 0003, superseded by ADR 0017

> **Subject updated 2026-07-27.** This ADR's subject is the resident artifact
> pinned by [ADR 0017](0017-resident-model-artifact-pin.md), which superseded
> ADR 0003. The acquisition, integrity, staging, atomic publication, update,
> rollback, recovery, and removal contract below is model-neutral and continues
> to govern it: read "Gemma" as "the resident model", and read the ~3.15 GB
> size, the `gemma-3-4b-it-q4_0.gguf` filename, the `gemma/` root, and the
> `google/…` revision as the historical ADR 0003 values. What ships today is
> `Qwen3.5-4B-Q4_K_M.gguf` (2,740,937,888 bytes) under the app-data root
> `models/qwen3.5-4b/` with the same `install.lock` / `current` / `previous` /
> `revisions/` / `staging/` layout; `Gemma*` code symbols and the
> `muniment-gemma-pointer-v1` pointer header are vocabulary debt renamed under
> a separate ticket.
>
> The exception is **Terms and notice delivery**. ADR 0017's artifact is
> Apache-2.0, so the Gemma Terms of Use acceptance gate, the section 3.2 use
> restrictions, and the section 3.1 notice wording below do not apply to it.
> Each published revision carries the Apache-2.0 attribution notice for the
> shipped artifact as its `NOTICE.txt`, and that section's mechanics — a
> bundled offline copy of the applicable licence and notice reachable from
> Settings/About before, during, and after installation, release checks for a
> descriptor-to-notice association, and an approval screen that identifies the
> publisher, artifact, source, and revision — apply to whatever licence the
> pinned artifact carries. An acceptance gate is required only for a pinned
> artifact whose licence imposes additional use restrictions.

## Context

ADR 0003 pins one approximately 3.15 GB Gemma GGUF and requires its byte size
and SHA-256 digest to verify before llama-server starts. It does not decide how
the artifact reaches an installation, how a later pinned revision replaces it,
or how the Gemma Terms and update notices reach the user. The model is too
large to add to every platform installer, and a remote mutable manifest would
weaken ADR 0003's reproducible identity.

## Decision

### Acquisition

The resident model is an **on-demand first-use install** from ADR 0003's
immutable Hugging Face revision URL into the desktop application-data
directory. Muniment startup never downloads it. A feature that needs Gemma
while no verified revision is installed presents the artifact size, source,
disk requirement, terms and notices, and an explicit Download and Install
action. After installation, inference and launch verification are offline.

Bundling is rejected because it would add 3,155,051,328 bytes to every
installer and application update. An automatic background download is
rejected because it would spend bandwidth and disk without user intent. A
Muniment-hosted mirror is rejected for the initial release because it adds
artifact custody, redistribution, and signing operations without improving
the pinned identity. A future offline model pack may use the same manifest and
publication protocol, but it must be separately signed and carry the same
terms and notices.

The native downloader accepts only the compiled URL formed from ADR 0003's
repository, full commit revision, and filename. It follows redirects only over
HTTPS through the expected Hugging Face content-host chain. URLs, filenames,
revisions, digests, and local paths never come from the webview or a remote
manifest. If upstream later gates, moves, or removes the artifact, download
fails with a retryable/unavailable state; Muniment does not request or store a
user Hugging Face token and an already installed verified revision continues
to work.

### Integrity, staging, and publication

The native app owns this layout beneath the platform application-data
directory:

```text
gemma/
  install.lock
  current                         # revision and descriptor identity
  previous                        # last known-good pointer, if any
  revisions/
    [full-revision]/
      gemma-3-4b-it-q4_0.gguf
      NOTICE.txt
  staging/
    [random-install-id]/
      gemma-3-4b-it-q4_0.gguf.part
```

The installer takes `install.lock`, creates a random stage on the same
filesystem, and streams the response through a fixed buffer into `.part`
while computing SHA-256. It enforces the compiled byte size before accepting
the file and verifies the compiled digest; no model-sized buffer is allocated.
Only a complete match may be renamed inside the stage. The release-bundled
Gemma notice is copied into that stage, and both files are synced before the
stage is atomically renamed to `revisions/[full-revision]`. Recognition never
reads `staging`.

The verified old `current` is preserved as `previous`; a synced temporary
pointer then atomically replaces `current` using the platform replace
operation. Pointer files rather than symlinks support Windows. Readers resolve
one pointer snapshot to a known compiled descriptor and verify the regular
file's size and SHA-256 as ADR 0003 requires before starting llama-server.
They never scan directories or infer a revision. A crash therefore exposes
the old complete revision or the new complete revision, never partial bytes.

HTTPS authenticates the transport, but it is not the artifact identity. The
compiled SHA-256 and size are authoritative. A model descriptor is admitted
only through a signed Muniment application release and code review; there is
no remotely updatable checksum list. Hugging Face commit or LFS metadata and
Git signatures are supporting provenance, not substitutes for verification.
Because the selected upstream artifact has no detached signature that
Muniment can independently require, this decision does not claim upstream
signature verification. A Muniment-hosted model pack would additionally need
a signature from a dedicated release key over its descriptor and payload
digest, verified before staging.

Downloads use the bounded timeout, cancellation, retry, range-resume,
redaction, proxy/certificate, stage-expiry, and free-space rules in ADR 0005,
adapted to this single file. Free space must cover the remaining staged bytes
plus a 256 MiB margin while retaining current and previous revisions.

### Updates, rollback, and recovery

Model updates never follow Hugging Face `main` and are not independent silent
updates. A new model, quantization, filename, revision, size, or digest
requires an ADR 0003 descriptor amendment delivered in a signed Muniment app
release. The app may then announce that known update, but downloads it only
after explicit user approval showing download size, disk impact, source,
material capability or terms changes, and a link to the installed notices.
Dismissal leaves the verified current revision selected and may be revisited
in Settings. Security-critical updates may be labeled recommended or required
for the affected feature, but still do not silently transfer model bytes.

An update uses the same staged verification and atomic publication protocol.
The old current stays as `previous` until the new revision verifies and
llama-server completes one health-checked startup. Startup failure records a
redacted category, atomically restores verified `previous`, and retries once.
The rejected revision is not selected again automatically. If `current` is
missing, unknown, or corrupt, a lock holder restores a verified known
`previous`; it never promotes a stage. If neither verifies, Gemma-dependent
features are disabled with Repair/Download offered while the rest of Muniment
continues.

Only the lock holder stages, publishes, changes pointers, cleans, or removes.
After successful activation it retains at most current and previous; older
known revisions and expired stages are deleted without following links or
crossing the owned `gemma` root. Concurrent processes keep using an already
opened immutable revision and observe pointer changes after releasing it.

“Remove downloaded model” first stops Gemma-dependent work and llama-server,
takes the lock, atomically removes both pointers, then deletes only known
entries below the owned root without following links. Failure is retryable.
It never deletes a shared Hugging Face cache. Release UI and uninstall
documentation must state whether the platform uninstaller removes application
data and must expose in-app removal when it does not.

### Terms and notice delivery

Before download, the user can open the complete Gemma Terms of Use and the
incorporated Prohibited Use Policy and must affirm acceptance. Muniment's
terms governing the downloaded model must include the Section 3.2 use
restrictions as enforceable provisions and state that Gemma remains subject to
them. A terms version/change requiring renewed acceptance blocks acquisition
or update, not use of unrelated app features. Product/legal review of the
actual release terms remains a release gate.

Every installed revision is accompanied by `NOTICE.txt` containing the exact
notice required by Gemma Terms section 3.1:

> Gemma is provided under and subject to the Gemma Terms of Use found at
> ai.google.dev/gemma/terms

The desktop bundle also includes an offline copy of the applicable Gemma Terms
and required notice, reachable from Settings/About → Third-Party Notices
before download, while installed, and after removal. The acquisition and
update screens identify Google, Gemma 3 4B, the QAT Q4_0 GGUF, its source and
revision, and link the bundled notice. They do not use Google marks to imply
endorsement. If Muniment ever modifies model files, those files and the notice
must prominently identify the modifications. Release checks fail if the
notice, applicable terms copy, prohibited-use reference, or descriptor-to-
notice association is absent.

An app update may refresh notice text independently of model bytes and calls
out material terms changes in release notes and Settings. A model update's
approval screen summarizes model and notice changes before download. Notices
are application resources as well as revision companions, so rollback or
removal cannot make the governing text unavailable.

## Consequences and follow-up slices

First use needs a large transfer, enough staging space, explicit terms
acceptance, and a wait. In return, installers stay small, artifact identity is
release-controlled, interrupted work cannot publish partial bytes, and one
known-good revision survives a bad update. Direct upstream acquisition avoids
operating a model CDN but remains dependent on upstream availability.

Implementation should follow the existing ADR 0005 core/adapter boundary:
pure core owns descriptors, verification, lifecycle state, redacted errors,
and tests over tiny fixtures; the native Tauri adapter owns app-data paths,
locking, atomic replacement, free-space checks, HTTPS/proxy behavior, and typed
progress. Model bytes, credentials, raw URLs, and paths never cross IPC or
enter logs, telemetry, prompts, or crash reports. CI must use tiny local
fixtures and never download the real GGUF.

This ADR is a distribution contract only. It does not implement the downloader,
UI, release terms, notices inventory, packaging, or uninstall behavior.

## Sources

- Pinned model identity and launch verification: [ADR 0017](0017-resident-model-artifact-pin.md), superseding [ADR 0003](0003-resident-gemma-model.md)
- Existing native lifecycle pattern: [ADR 0005](0005-asr-model-lifecycle.md)
- Gemma Terms of Use: <https://ai.google.dev/gemma/terms>
- Gemma Prohibited Use Policy: <https://ai.google.dev/gemma/prohibited_use_policy>
- Immutable upstream artifact: <https://huggingface.co/google/gemma-3-4b-it-qat-q4_0-gguf/tree/15f73f5eee9c28f53afefef5723e29680c2fc78a>
- Hugging Face immutable download guidance: <https://huggingface.co/docs/huggingface_hub/en/guides/download>
