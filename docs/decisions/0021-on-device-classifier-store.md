# 0021 — The shared on-device classifier store

- Status: superseded by the 2026-07-29 owner ruling
- Date: 2026-07-29
- Context: owner decision 2026-07-29, harness-spec §§12.1, 12.2, and 13,
  ADRs 0007, 0011, and 0012

## Context

The 2026-07-29 owner ruling supersedes this decision. The cloud now classifies
prompts at ingress. Desktop clients do not download or run a classifier.

The desktop, CLI, and editor extension classify each prompt on the device.
They send the prompt and classification to the Muniment cloud. The cloud then
routes the request to the customer's model providers. Local classification is
a pre-flight step. It is not local prompt execution.

The current candidate is a 278-million-parameter multilingual encoder with a
small classifier head. The int8 ONNX artifact measures 278 MB. The unquantized
artifact measures 1,110 MB. The int8 artifact answers a query in about 7 to 10
ms on four CPU threads. Accuracy survives int8 quantization. Each model needs
its own tested quantization mode. Candidates near 107 to 118 million parameters
remain under measurement for the mobile budget.

One download must serve the desktop, CLI, and editor extension. ADR 0012 makes
one per-user core service the owner of runtime state. ADR 0011 and
`test/cli-dependency-boundary.sh` forbid the CLI from linking `muniment-core`
or an inference runtime. The shared store must fit both boundaries.

The repository already has the required download and publication machinery.
`src-tauri/src/main.rs:32` resolves the current model root from
`app.path().app_data_dir()` and `models/<name>/`.
`src-tauri/core/src/llama.rs:36-118` defines `ResidentModelDescriptor`,
its `byte_size` and `sha256` pins, and `verify_model_artifact`.
`src-tauri/core/src/llama/lifecycle.rs` defines staging, resumable `.part`
files, `publish_lock_held`, `replace_revision`, and the `current`, `previous`,
and `rejected` pointers. `src-tauri/core/src/model_install_native.rs:60-98`
defines `NativeInstallLock` over `install.lock`.
`src-tauri/core/src/model_acquisition_transport.rs:358-393` separates
`checked_initial_url` from `checked_redirect_url`.

## Decision

### Store location and owner

The core service owns one classifier store for each logged-in OS user. It uses
`app_data_dir()/models/classifier/`, beside the current app-data model root.
The resolved roots are:

| Platform | Shared classifier store |
| --- | --- |
| macOS | `~/Library/Application Support/ai.muniment.desktop/models/classifier/` |
| Windows | `%APPDATA%\ai.muniment.desktop\models\classifier\` |
| Linux | `$XDG_DATA_HOME/ai.muniment.desktop/models/classifier/`, or `~/.local/share/ai.muniment.desktop/models/classifier/` when unset |

No surface owns a private copy. This choice gives all three desktop surfaces
one per-user path and one custody boundary.

### Discovery and classification

Each surface first attaches to the core service through the ADR 0009 endpoint.
If the service is stopped, the surface asks the per-user service manager to
start it under ADR 0012. The surface then retries attach within a bounded
readiness deadline. The service reads the `current` pointer before it considers
a download.

The core service exposes classification through the local attach contract.
The CLI sends the prompt to that method and receives the classification. It
does not link an inference runtime or `muniment-core`. A second surface finds
the same service and pointer, so it cannot trigger a second download.

This choice keeps model discovery and inference behind one attach boundary. It
also preserves the CLI dependency boundary when the desktop is not running.

### Artifact pin and verification

The service stores a descriptor for the exact artifact identity, revision,
byte size, SHA-256 digest, and quantization mode. It never infers a
quantization mode from the file type or model family.
`verify_model_artifact` checks the byte size and SHA-256 digest before
publication and before use. It returns `DigestMismatch` for the wrong digest
and refuses that artifact.

After refusal, the service keeps the last verified `current` revision when one
exists. Without one, it reports that classification is unavailable. The
surface still sends the prompt with an explicit unclassified state. It shows
that routing will use the cloud fallback. It never relabels the prompt from
unverified bytes.

This choice binds every classification to known bytes and a tested
quantization mode. A corrupt first install degrades routing instead of
blocking the surface.

### Concurrent cold install

The first request on a cold machine starts one core-service install. The
service holds `NativeInstallLock` on `install.lock` for the install decision
and publication. A concurrent request joins the same install state. Both
surfaces wait for its bounded result.

The installer downloads into the staging directory and resumes through the
`.part` file. It verifies the completed artifact before
`publish_lock_held` calls `replace_revision` and replaces `current`.
Readers resolve only a published revision. They never inspect staging or a
`.part` file.

This choice produces one download when requests race. Both surfaces receive
the same result, and no partial file becomes visible.

### Upgrade and eviction

An upgrade stages and verifies the new revision before it changes `current`.
Publication moves the former `current` value to `previous`. The `current` and
`previous` revisions may coexist during a rollout.

The service gives each classifier process an open file handle or equivalent
revision lease before inference starts. Eviction removes only an unpinned
revision with no lease. It may remove the old revision after all users release
it. A failed activation writes `rejected` and restores the verified
`previous` revision through the existing pointer path.

This choice permits rollback and safe eviction. Removing an old revision
cannot break a running classification.

### Offline and air-gapped installs

A surface works when no classifier model is present. The core service returns
an explicit unclassified state and does not execute a local classifier. The
surface sends that state with the prompt. The cloud applies its fallback
routing policy.

An offline install may import an administrator-supplied artifact and
descriptor into staging. It uses the same identity, byte-size, SHA-256,
quantization, lock, verification, and atomic publication rules. Network
access is not a verification exception.

This choice keeps every surface usable offline from the classifier store.
Cloud prompt execution still needs the customer's configured provider path.

### Fetch and redirect policy

The descriptor's initial URL must use HTTPS on the pinned artifact host.
`checked_initial_url` enforces that exact origin and the Muniment organization
path when the artifact uses Muniment's Hugging Face repository.

Every redirect must also use HTTPS. `checked_redirect_url` accepts only the
provider's explicit redirect host families. For Hugging Face, this includes
its regional Xet CDN hosts. The client checks every redirect hop and rejects
any other host. It never applies the initial host rule unchanged to redirects.

This choice limits the initial source while allowing the CDN redirects needed
for first-run downloads. It preserves the MUNIDESK-571 fix without accepting
arbitrary redirect hosts.

### Mobile amendment and sequencing

Harness-spec §12.1 says phones run no local models. Harness-spec §12.2 item 2
says mobile has no resident local classifier and gets its label from the
server. ADR 0007 repeats the local-model prohibition as a reason for the
separate mobile repository.

A classifier pre-flight does not execute the prompt or run an agent. It emits
routing metadata before the cloud sends the prompt to the customer's model
provider. The owner should amend all three rules to distinguish local
pre-flight classification from local prompt execution.

Slice 2 will decide the mobile store and mobile behavior before its model
arrives. This ADR makes no other mobile decision.

## Consequences

- The desktop, CLI, and editor extension share one verified classifier copy.
- The core service owns download, inference, publication, rollback, and
  eviction.
- The CLI gets a classification through attach and adds no inference runtime.
- Concurrent cold requests cause one download and expose no partial artifact.
- Two verified revisions may coexist while active users hold revision leases.
- Missing or refused artifacts produce an explicit unclassified cloud
  fallback.
- Fetches keep separate initial-host and redirect-host rules.
- Slice 2 must settle the mobile store and its pre-model behavior.
- This ADR adds no dependency, store implementation, model artifact, or
  surface change.
