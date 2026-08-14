# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud
prerequisites for desktop chat went live on 2026-07-11. Client work that uses
them must exercise the real contracts. It must add no mocked production path.

> **Compacted 2026-08-04, again 2026-08-07, again 2026-08-12, again
> 2026-08-13, and again 2026-08-14.** This document reached 265 KB and no longer
> fit in one read.
> Every landed slice used to carry its own paragraph. Those paragraphs are now
> per-lane summaries with their ticket ranges. Every open item, parked item,
> held item, gated item, and do-not-re-file measurement is preserved below. Git
> history holds the full slice-by-slice record. The first 2026-08-12 pass took
> the ADR 0012 extraction section. The second took the ADR 0024 code-diff chain.
> The third took the memory index and retrieval section. The fourth took the
> signed-in shell section. The fifth took the desktop QA automation repair
> waves. The sixth took the Phase 3 voice section. The seventh took the
> companion execution surfaces section. The eighth took the onboarding and
> Muniment Home section. The ninth took the durable local run journal section.
> The tenth took the cross-surface contracts section. The eleventh took the
> stable release and distribution section. The twelfth took the desktop QA
> automation section. The thirteenth and the fifteenth both took the ADR 0012
> runtime-service extraction section. The fourteenth took the Phase 2 client
> core section. The sixteenth through the twenty-fifth all took that
> extraction section again, and so did the twenty-sixth, the twenty-seventh, and
> the twenty-eighth. It grows every wave, so it stays the next compaction target.

## M0 — Scaffold (done 2026-07-09)

DONE — Tauri v2 desktop shell, specs, mockups, design reference, Rust and
frontend test harnesses, and Linux, Windows, and macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements

DONE — Svelte 5 and Vite shell, design tokens, signed-out and signed-in states,
native installation-bound authorization, keychain persistence, refresh, session,
and revocation flows, live entitlement projection, access-device states,
server-unreachable recovery, and a bounded retry for a rate-limited first-run
device registration (MUNIDESK-927). The production contract is
`/v1/auth/native/*`. Generic OIDC remains test groundwork and is not the
production handshake.

SPEC LANDED 2026-08-05 — the org capability catalog joins the Your access view
(MUNIDESK-926). `docs/spec/02-desktop-app.md` adds an `Available in your org`
list under the entitlement peek. Each entry names the capability, its subunit
type, the approved one-line description, and either a `granted` chip or a
`Request access` button.

GATED — no implementation slice for that catalog is fileable yet.
`EntitlementSnapshotView` (`src-tauri/core/src/auth/native_session.rs:70`)
carries the viewer's own groups alone, so the desktop has no source for the
published org library. The listing endpoint and the `CreateAccessRequest`
contract both live in muniment-cloud. The lane selects the first slice after
that contract publishes.

### 9. Pi sidecar and cloud chat

DONE — verified Pi runtime acquisition and supervision, signed-in streamed chat,
steering and follow-up, durable tool effects, provenance and receipts, inline
tool cards, extension UI, permission-gate replay and answers, and safe
interrupted-session resume.

DO NOT RE-FILE — explicit resume from a bound Pi session is built.
`src-tauri/core/src/chat_resume.rs` holds `ResumeContext` and
`resumable_context`, the `chat_resume` command (`src-tauri/src/chat.rs:625`)
refreshes auth, reopens the journal-bound session, and continues the same
`run_id`, `RESUME_PROMPT` (`:449`) sends one continuation request instead of the
protected original prompt, and `install_resume_run` (`:565`) opens the resumed
run's memory session. The transcript offers `Resume` beside `Try again`
(`src/App.svelte:917`) and shows `Resume` only for a resumable interrupted run.

DONE — the permission gate reaches the user end to end (MUNIDESK-577, 582, 587,
594, 605, 766). The core seam validates an answer against its request, the
coordinate loop drains a typed answer queue and appends `permission.resolved`,
and the thread renders a mono ask card for the `confirm`, `select`, `input`, and
`editor` kinds with the refusal control apart from the request's own choices.
`test/probe/permission.html`, `input.html`, `editor.html`, and `select.html` are
the fixtures.

DONE 2026-08-02 — ADR 0025 decides per-thread permission answers (MUNIDESK-800).
They are two versioned event types in the `thread_events` ledger,
`thread.permission.policy.decided` and `thread.permission.policy.revoked`, with
exact-resource matching and coordinate-loop-only resolution. The ledger half
landed (MUNIDESK-808), and the effective-policy projection followed
(MUNIDESK-810). `RunJournal::active_thread_permission_policies`
(`src-tauri/core/src/journal/mod.rs:777`) replays a thread's policy events per
read, and `lookup_thread_permission_policy` (`mod.rs:871`) answers one
`(request_kind, resource)` tuple.

PARKED — the two ADR 0025 consumer slices wait on the Pi wire contract.
`parse_extension_ui_request` (`src-tauri/core/src/sidecar/pi_chat.rs:270`) yields
display strings alone: title, message, options, placeholder, and prefill. ADR
0025 gives display text no match authority, so no live gate maps to a canonical
`(request_kind, resource)` tuple. Coordinate-loop auto-resolution and the
ask-card `Allow for this thread` control both wait for a structured resource on
the extension UI request.

PARKED — the gate-timeout countdown waits on the same contract. All four
`PermissionRequest` kinds carry `timeout` and the value reaches the frontend, so
the plumbing exists. No desktop code enforces the value. Nothing cancels a gate,
fails a run, or appends an event when it elapses, so the copy at zero would
describe Pi rather than the desktop. A journal-restored gate also carries no
recorded time to anchor a countdown against.

### 10. Local model sidecar

SUPERSEDED 2026-07-29 by the cloud ingress ruling. The desktop ships no resident
model. DONE 2026-07-30 — the removal finished (MUNIDESK-680).
`src-tauri/core/src/llama.rs` and its four submodules are gone, together with
`src/lib/model-acquisition-state.js`, seven core test binaries, and the routing
classifier golden fixture. ADR 0018 and ADR 0021 carry superseded status.

### 11. Attachments and local CAS

DONE groundwork — pure-Rust content-addressed local store with atomic
deduplication, constant-memory input and output, verification, stale-temp
cleanup, journal reference accounting, retention, export, and compaction.

DONE — the local half of item 11 is built end to end. The composer selects
files, `open_selected_files` (`src-tauri/src/chat.rs:794`) validates each open
handle, `ingest_attachment` (`src-tauri/core/src/attachment.rs:246`) streams the
bytes into CAS and appends one `chat.attachment.ingested` event, and
`prepare_pi_images` (`attachment.rs:33`) sends the supported images with the
first prompt. `chat_attachments` (`src-tauri/core/src/chat_view.rs:42`) projects
each attachment without its path or its hash, and the transcript renders a chip
row per file.

GATED — the control-plane file store client is not fileable. harness-spec §7
puts every shared attachment behind the cloud file store, and ADR 0019 makes
muniment-cloud the source of that contract. No published artifact describes the
upload, so this lane waits exactly as the code-diff producer waits.

DONE 2026-08-09 — the attachment copy is honest per file (MUNIDESK-1022, 1023).
Each saved chip states its own file kind, the delivery rule renders once for the
whole row, and an image-limit failure names the file and the limit it crossed.

### Durable local run journal

DECIDED — ADR 0002 makes a per-run append-only SQLite event journal
authoritative. External effects are never silently re-executed, and large bodies
live in CAS.

DONE — schema, envelope, atomic append, reducer and replay, Pi translation,
deletion, collection, retention, deterministic export, crash-safe compaction,
cursor-paginated run summaries, and authorized Linux companion `thread.list` and
`thread.open` over redacted projections. The two thread-summary query defects are
fixed (MUNIDESK-584, 721). On a replica holding 4,000 threads and 400,000 events
the old cursor boundary check cost 39.1ms and the direct read costs 0.5ms.
Retention also picks its candidates before it loads them (MUNIDESK-885).
`apply_retention` (`src-tauri/core/src/journal/retention.rs:55`) skips a run that
carries no terminal event type or that is newer than the cutoff, and it added no
column, table, index, or migration. No production caller reaches retention yet,
and the ADR 0012 runtime service will own it.

RULE 2026-07-12 — pre-launch schema work on the desktop's local journal is in
scope for this lane. The owner's post-go-live restriction applies to the
muniment-cloud deploy path, not to this repository.

**HELD — the owner ruling this lane waits on.** The standing owner exclusion on
self-initiated database migrations and the 2026-07-12 RULE above disagree about
this repository's local journal. Migration steps 2, 3, and 4 all landed under
that RULE. Nineteen waves have now passed with no answer, and the lane files
nothing each time. The memory lane read the same exclusion as covering durable
stores alone, and it filed its disposable cache without a ruling. This lane keeps
waiting, because its work alters the durable journal schema rather than a
rebuildable file. Everything in the next three paragraphs waits behind it.

HELD — the ADR 0002 launch-path amendment (MUNIDESK-720) names two
implementation slices, and both need a schema step first. On open the desktop is
to run `PRAGMA quick_check`, then validate the 256 highest-`append_ordinal`
envelopes in `events` and in `thread_events` plus each stream's boundary
predecessor. The full pass runs after the first frame and gates every external
effect until it succeeds. `reconcile_interrupted_runs` is to read candidate run
IDs from a transactionally maintained recovery-state projection. The first slice
adds `append_ordinal` to `events`, the same column to `thread_events`, and the
recovery-state projection with its backfill. That is three storage changes, so it
splits by table and then by projection. The `events` column is the cheapest first
slice. `validate_thread_schema` (`src-tauri/core/src/journal/mod.rs:2086`)
asserts the exact stored SQL of `thread_events` and `run_threads`, and two
corruption tests tamper with those objects. `VACUUM INTO` copies column values,
so compaction needs no work either way.

HELD — three redundant index drops. `thread_events_thread_order`,
`run_threads_thread_run`, and `thread_projection_run_order` each duplicate the
implicit index their own UNIQUE or PRIMARY KEY constraint already creates on the
same columns in the same order. This is the MUNIDESK-589 shape, and that drop
already landed for `events_run_order` as a schema-v4 step. The three belong in
one slice, because it edits the schema validator and two corruption tests
together.

MEASURED 2026-07-31 (planner, read every SQLite call site in
`src-tauri/core/src/journal/`) — app launch reads the whole journal twice.
`validate_database` (`mod.rs:2013`) opens with `PRAGMA integrity_check`, then
runs an unbounded ordered `SELECT` over `events`, re-serializes each row through
`canonical_envelope`, scans `thread_events` the same way, and runs five more
unbounded aggregates. `RunJournal::open` calls it on every open, and
`ChatState::new` opens the journal once per launch. `reconcile_interrupted_runs`
then calls `run_event_types`, a second unbounded ordered read. A journal holding
50,000 events pays 50,000 parses plus 50,000 re-serializations before the first
frame.

DO NOT RE-FILE — `append_batch` prepares its statements inside the per-event
loop, and the saving is not worth a slice. A scratch rusqlite 0.32 benchmark
measured 32.7µs per append against an in-memory database and 11.3µs with
`prepare_cached`. The same append against a real file measured 26.6ms, because
the journal opens WAL with `synchronous=FULL`. The commit fsync outweighs the
parse by three orders of magnitude.

OPEN — `chat_thread_open_page` projects each run through `project_history_entry`,
which loads and reduces every envelope of that run. `thread_projection_entries`
already holds the assistant text, `receipt_projection` holds the receipt, and
`permission_pending_projection` holds the gate, so only `tool_activity`,
`attachments`, `phase`, and `resumable` still force the full load. Deciding which
fields those projections own is a contract call rather than a query rewrite, and
carrying it out would add columns, so it waits behind the same owner ruling.

### Capability vocabulary and provenance

DONE — user surfaces say "capabilities". Receipts render only server-supplied
route, model, cost, time, and capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)

SUPERSEDED 2026-07-29 — cloud ingress classification replaces the completed local
classifier contract.

DONE 2026-08-07 — the desktop runs one mode, **the thread surface**, and SPEC
law 13 states it (MUNIDESK-969). No desktop source computes a routing tier, a
routing label, or a classification. Every model call rides server-supplied grant
values alone. `chat_coordinate.rs:203` passes the scoped virtual key, the
gateway URL, and the optional pinned model into the sidecar environment.
`the_grant_request_carries_no_client_classification` and
`the_receipt_request_carries_only_the_run_id`
(`src-tauri/core/src/chat_grant.rs:257`) read both cloud-bound requests off the
wire and reject any tier, label, or classification field. `test/smoke.sh` holds
the mode name and the no-classification rule. MUNICLOUD-968 ratified the
mandated classification path on 2026-08-07.

OPEN — the cloud `RoutingSurface` enum still reads `desktop_thread_chat`
(`api/src/routing-policy-resolver.ts:5` in muniment-cloud). That value names a
mode the desktop does not have. The desktop asks MUNICLOUD for `desktop_thread`
and renames no cloud contract on its own.
[docs/desktop-single-mode.md](docs/desktop-single-mode.md) carries the ask, its
evidence, and its ratification status. No MUNICLOUD ticket has ruled on the name
yet, so this lane files no rename slice.

DONE — the voice direction is Parakeet verbatim capture with Kokoro read-aloud
and global hotkeys, and the whole dictation lane is built. It covers pinned
Parakeet and Silero acquisition and publication, sherpa-onnx packaging and
bindings, bounded capture, segmentation, and chunk-invariant composition, a safe
VAD boundary, redacted command and event wiring, composer controls, press-and-hold
dictation with Escape to cancel, a fixed system-wide hold-to-talk shortcut,
hands-free promotion on rapid double activation, user rebinding with
collision-safe rollback, a reproducible target-hardware evaluator, and a bounded
100-utterance endurance mode. ADR 0004 pins muniment's own Parakeet conversion
(MUNIDESK-678), so the desktop depends on no third-party conversion for its ASR
graphs. ADR 0005's on-demand install is built end to end (MUNIDESK-752, 764, 771,
774, 777, 780). `parakeet_install_facts` reports the pinned identity, the
revision, the source repository, 672,384,307 download bytes, 940,819,763 required
free bytes, and both artifact licenses, and the install card reports byte progress
and cancels an active install. Handy's architecture notes are vendored as voice
reference.

GATED — no further Kokoro slice is selected. Native runtime and G2P packaging
remain subject to ADR 0015's redistribution approval gate. Synthesis, playback,
response controls, and physical target-hardware runs stay separately deferred.
The text-processing, segmentation, packing, and repair boundaries are built.

DO NOT RE-FILE — `natural_candidates` (`src-tauri/core/src/kokoro/packing.rs:375`)
suppresses the rank-3 word boundary after any closer, where ADR 0015 suppresses
only a closer already covered by rank 1 or 2. The ADR wording and the intended
behavior need an owner or author call before the one-line change.

## Companion execution surfaces (§13)

DONE E0 and E0.5 — governed protocol, pairing, authorization, idempotency,
bounded transport, the ADR 0011 repository and lane decision, and the ADR 0012
per-user runtime-service direction.

DONE E1 — Rust CLI lane, Linux discovery and pairing, thread list and open, run
start with ordered progress and flow control, typed permission handling, tool
details, and receipts. The CLI answers `--help`, `-h`, and `--version`
(MUNIDESK-658).

DONE E2 — canonical fixtures and drift checks, the TypeScript package and its CI,
cross-platform discovery, framing, pairing, authorization, thread documents,
typed run streaming, workspace and current-file context, receipts, and
fail-closed permission interaction.

OWNER RULING 2026-07-29 — muniment takes the ACP agent direction. The muniment
CLI surface is DEFERRED indefinitely. The first-party editor extension is
RETIRED, and its tree is removed (MUNIDESK-858, 861).

### ACP interop

DONE — ADR 0022 decides the direction and the adapter is built and shipped
(MUNIDESK-673, 683, 686 through 857). The Muniment ACP adapter is the only
component that speaks ACP, and it is a thin `muniment.attach/1` client of the ADR
0012 runtime service. That service keeps sole ownership of sessions, threads,
runs, effects, and receipts, so the editor-spawned process never becomes a second
executor. An editor answer to `session/request_permission` is an input to a
Muniment gate rather than a grant. The ADR names the supported v1 method subset,
answers no to the filesystem and terminal client capabilities, negotiates the
integer `1`, rejects the v2 draft, and pins the Rust crate
`agent-client-protocol` at exactly `2.0.0`. `src-tauri/acp/` holds the
`muniment-acp` binary crate. It pairs with a persisted identity under
`$XDG_CONFIG_HOME/muniment/`, claims the client kind `acp-adapter`, creates a
durable thread through attach `thread.create`, writes a versioned session record,
streams released assistant text as `agent_message_chunk`, translates live tool
calls, bridges a permission gate to `session/request_permission`, cancels through
`run.cancel`, ends a prompt on `run.needs_attention` and on a non-resumable
`stream.closed`, and restores a recorded session through `session/load` with
`loadSession: true`. The Linux package installs it at
`/usr/lib/muniment/muniment-acp`, `docs/acp-editors.md` is the setup page, and
the installed nightly drives it through `initialize`.

DONE — the assistant-text redaction lane behind that streaming is complete. ADR
0009 carries the `assistant-text-v1` rule set: four `secret.*` rules, two
`path.*` rules, and the `content.withheld` metadata rule, each with a finite
maximum span of at most 131,068 bytes. `src-tauri/core/src/assistant_text.rs`
holds the scanner, `assistant_text/ledger.rs` holds the envelope attribution
ledger, and `assistant_text/projector.rs` is the stateful retained-suffix
projector. `thread.open` and the attach run stream both read that one projector.

DONE 2026-08-04 — ADR 0022 carries the capability-revocation amendment and the
adapter half is built (MUNIDESK-875, 882). It supersedes the reservation in the
prompt-ending amendment. A revoked prompt ends with JSON-RPC code `-32000` and
the exact message `Muniment capability revoked`. The adapter removes
`acp-client-credential`, keeps `acp-client-id`, and reaches fresh visible approval
on its next attach. `ClientError::CapabilityRevoked`
(`src-tauri/acp/src/main.rs:40`) is its own typed variant, the run-stream loop
drops the client credential and ends the prompt on
`RunStreamMessage::CapabilityRevoked` (`:749`), a revoked `run.cancel` maps to the
same failure (`:775`), and the prompt answers code `-32000` with the exact
message (`:807`).

PARKED — ADR 0022 names plan updates in its method subset, and no code produces
one, because the journal carries no plan or thought content. `agent_thought_chunk`
has the same missing upstream. Both wait on the Pi wire contract.

OWNER WORK — in-editor release validation for the Zed and JetBrains claims needs
real editor installs.

MEASURED 2026-08-09 (ninth wave, planner, read the CLI run-stream loop and its
guidance table) — the CLI treats `capability.revoked` and every `stream.closed`
as `UnexpectedMessage` (`src-tauri/cli/src/main.rs:423`). `client_guidance`
(`:566`) answers that variant from its catch-all arm, so a revoked companion
prints `the desktop pairing response was invalid; update Muniment and try again`.
The user revoked the CLI in the desktop `Connected programs` panel, and the line
sends them to an update instead. The ACP adapter already carries the honest
shape, because `ClientError::CapabilityRevoked` is its own typed variant with the
exact message `Muniment capability revoked`. OWNER QUESTION — the 2026-07-29
ruling defers the CLI surface indefinitely, and the 2026-08-09 grooming promoted
one CLI ticket. The planner files no slice until the owner says how wide that
promotion runs.

### Companion revocation and management

DONE 2026-08-04 — ADR 0009 carries the companion revocation amendment, and the
emitter and the desktop management surface are both built (MUNIDESK-864 through
867, 874, 876, 877, 881). Revocation is a local decision by the attach owner. It
removes the selected companion's persisted client credential, blocks admission
while it persists the removal, invalidates every live connection capability
authenticated with that credential, and emits exactly one `capability.revoked`
event per affected connection with `reason: "companion_revoked"` and the
connection's `connection_event_id` as `subscription_id`. A revoked companion
reconnects only through a fresh visible approval. `LiveConnectionRegistry`
(`src-tauri/core/src/attach/linux.rs:398`) tracks connections by credential and
carries the block, resume, and revoke states.
`AttachListenerState::revoke_companion` (`src-tauri/src/attach_service.rs:153`)
persists before it emits and restores authority when the write fails.
`list_companions` (`:171`) reports each companion's identity, claimed kind,
claimed version, and approval time. `attach_companions` and
`attach_revoke_companion` are registered Tauri commands, and the profile popover
carries a `Connected programs` section under Devices. The MUNIDESK-876 merge
overwrote the MUNIDESK-877 revoke control from a stale base, and MUNIDESK-881
restored the control and its tests.

### ADR 0012 runtime-service extraction

DONE 2026-08-04 — ADR 0012 carries the extraction-sequence amendment
(MUNIDESK-863). Extraction starts on Linux with the `muniment-runtime` crate.
Phase one moves the Pi child and dispatcher, journal and CAS, device session,
entitlement snapshot, credentials, workspace authorization, permission gates, and
the ADR 0009 listener into that crate. The desktop keeps windows, navigation,
composer state, selected local paths, audio capture, shortcuts, and other
presentation state. Both processes share ADR 0009's one per-profile instance
lock, and the amendment pins the quiesce, handoff-nonce, and readiness-probe rules
that keep a temporary interval with no owner but never an interval with two.
Through phase one the desktop presents the service-owned approval challenge, and
a new connection fails closed when no desktop can present it.

DONE — the runtime crate, its packaging, the instance lock, and thirty-four core
moves landed (MUNIDESK-868 through 954, 982). The crate has its own checks,
handles its CLI and signals, and adds no dependency. The Linux package ships it
beside `muniment-acp`, and each desktop call site keeps a thin wrapper.

DONE — the migration control chain is built end to end (MUNIDESK-913 through
1067). ADR 0012 carries the control-authority amendment and the migration control
session admission amendment. Only the waiting runtime service may send
`migration.control`, an approved client credential grants no authority, and on
Linux the desktop resolves the `SO_PEERCRED` peer PID to the installed
`muniment-runtime` payload. `evaluate_quiesce`
(`src-tauri/core/src/attach/quiesce.rs`) weighs five inputs in field order and
names the first blocker, and every input carries a production call site through
`RuntimeActivityRegistry` (`attach/runtime_activity.rs`). `PreparedHandoffSlot`
and `mint_handoff_nonce` (`attach/handoff.rs`) bound the nonce at 128 printable
ASCII bytes and the deadline at 60,000 milliseconds and refuse a second
preparation. `ErrorCode::MigrationNotReady` answers a temporary quiesce blocker,
where `unsupported_operation` reads as a stale desktop.
`control_desktop_migration` (`src-tauri/src/attach_service.rs:73`),
`release_prepared_handoff` (`:126`), and `cancel_handoff_and_restart` (`:147`)
are the desktop half, and `MigrationControlClient::control_migration`
(`src-tauri/attach/src/client.rs:1351`) is the client half.
`run_migration_takeover` (`src-tauri/runtime/src/migration.rs:64`) is the runtime
composition, and it serves companions through `run_bound_attach_listener`.
Nothing wires the takeover into `main`.

DONE — the attach listener reports its own state truthfully, and one API base-URL
rule serves the whole workspace (MUNIDESK-1001, 1007, 1015, 1103).
`attach_listener_start_diagnostic` (`src-tauri/core/src/attach/linux.rs:58`) names
the filesystem, instance-lock, and bind failures, `AttachListenerLifecycle`
(`attach/listener_lifecycle.rs`) carries the pending, listening, failed, and
stopped records, and `Connected programs` says the listener did not start instead
of rendering an empty list. `api_base_url` (`src-tauri/core/src/auth/mod.rs:34`)
reads `MUNIMENT_API_BASE_URL`, then `MUNIMENT_ISSUER`, then the shipped default.

DONE — every attach operation has a dormant runtime twin (MUNIDESK-1150 through
1191). `src-tauri/runtime/src/service/` holds them in `run.rs`, `session.rs`,
`threads.rs`, and `workspace.rs` behind `service::*` re-exports. They cover the
device session, the entitlement snapshot, sign-out, native installations, the
cross-project Home, companion workspaces, companion listing and revocation,
thread reads and mutations, retention, run streaming and subscription, prompt
acceptance, steering, cancellation, permission answers, and resume. Every run
entry takes the shared storage, the shared run control slot, the shared Pi
runtime slot, the shared session-thread tracker, and the shared activity
registry, so every mark lands where `evaluate_quiesce` can read it.
`configure_run` (`service/run.rs:76`) rejects a requested workspace the signed
`grant.workspace` does not authorize, through the one core predicate
`grant_authorizes_workspace` that the desktop `configure_run`
(`src-tauri/src/chat.rs:250`) also reads. A preparation failure after the
prepared append records itself through `record_persistence_failure`.
`RuntimeChatEventSink` (`src-tauri/runtime/src/sink.rs`) implements
`ChatEventSink` and `PiLaunchBoundaries` and answers the `muniment-runtime`
provenance, and `RuntimeAttachBoundaries`
(`src-tauri/runtime/src/attach_boundaries.rs:37`) answers the six
`RunAttachBoundaries` reads and the fourteen `RunStartBoundaries` methods.
`src-tauri/runtime/tests/` proves each entry against a live run, and
`tests/common/mod.rs` owns the shared loopback HTTP stub, both credential
fixtures, the shared chat grant, the warm Pi stub staging, and the
temporary-profile guard. Nothing in `main` calls any entry.

DONE 2026-08-13 and 2026-08-14 — the attach approval seam is built on all three
sides (MUNIDESK-1188 through 1238). The core half holds the seam under
`src-tauri/core/src/attach/`. `approval.rs` carries the signed workspace and the
one-presenter claim, `workspace_context.rs` rejects a relative path and records
both canonical companion directories, `peer_authority.rs` checks the peer
executable, `presenter_admission.rs` negotiates version 1 and issues a
credential-free grant, `approval_present.rs` sends one request and reads its
correlated answer, `presenter_session.rs` claims the coordinator and holds it for
the life of the connection, `presented_approval.rs` turns one presented request
into a coordinator round trip and bounds both untrusted claims,
`connection_route.rs` names a new connection's route from the peer executable and
the claimed kind without consuming a byte, and `deadline_io.rs` is the one
deadline-bounded read and write helper set. `Operation::ApprovalPresent`
(`src-tauri/attach/src/envelope.rs:277`) and the two
`protocol-fixtures/muniment.attach/1/` approval files hold the wire.

DONE 2026-08-13 and 2026-08-14 — the runtime half holds the service and the
desktop half holds the client. `src-tauri/runtime/src/directories.rs` resolves the
profile directory, the config directory, and the installed desktop payload.
`compose_attach_service` (`runtime/src/attach_service.rs:15`) builds a
`DesktopAttachService` from the runtime boundaries and reads the confirmed Home
before the default one. `RuntimeAttachState` (`runtime/src/attach_state.rs:19`)
opens the profile storage and the companion registry once and owns the one
`ApprovalCoordinator`. `run_attach_listener` (`runtime/src/attach_listener.rs:48`)
owns the profile endpoint, takes the instance lock, and answers the handoff
readiness probe. `run_bound_attach_listener` (`:85`) names each accepted
connection's route, admits an approval presenter, holds the presenter session for
the life of the connection, and falls back to the companion route when it
resolves no installed desktop payload. `sign_in`
(`runtime/src/service/session.rs:55`) runs the native browser flow through an
injected `&dyn BrowserOpener`. `ApprovalPresenterClient`
(`src-tauri/attach/src/client.rs:1379`) answers each validated request,
`serve_approval_presenter_at` (`:1775`) reconnects through an interruptible
connect until its `ApprovalPresenterStopHandle` fires, and
`AttachCompanionState::start_approval_presenter`
(`src-tauri/src/attach_service.rs:353`) starts that supervisor once after a
confirmed handoff probe or a held instance lock and stops it when the desktop
listener starts. `test/installed-executable-paths.test.js` ties the resolved
desktop executable to the packaged product name. The probe-only handoff listener
is gone.

DONE 2026-08-13 and 2026-08-14 — ADR 0012 carries four attach amendments, and
`THREAT_MODEL.md` records each (MUNIDESK-1199, 1223, 1230, 1237). They decide
approval presentation over the existing `muniment.attach/1` connection, the
routing rule that reads the first hello, the desktop presenter lifecycle with its
two start states, and desktop client session admission. Through phase one the
runtime presents its ADR 0009 approval challenge to the desktop as an
`approval.present` request that carries the requesting surface, the workspace
scopes, the single-use challenge, and the decision deadline. On Linux the runtime
routes a connection whose `SO_PEERCRED` peer PID resolves to the installed desktop
payload by its claimed kind. A `desktop` hello takes the presenter session. A
`desktop-client` hello takes a connection-bound desktop client session that
carries owner authority over the signed `grant.workspace` value and carries
neither migration control nor approval presentation. A `desktop-handoff-probe`
hello keeps the readiness answer. Every other connection takes the companion
route. The runtime admits one presenter at a time, and a disconnected presenter, a
deny, a missed two-minute deadline, or a late, repeated, unknown, or mismatched
choice all fail closed.

DONE 2026-08-14 — four sixty-fifth-wave slices landed (MUNIDESK-1235 through
1238), and one drained (MUNIDESK-1239). All five sixty-sixth-wave slices landed
(MUNIDESK-1240 through 1244), including the refiled proof. These slices built
the desktop presenter supervisor and its two start states, the runtime accept-loop
fallback to the
companion route, the ADR 0012 desktop client session admission amendment, the
proof that a mismatched presenter answer fails closed, the proof that a companion
pairs through a live presenter connection, the desktop listener refusal that
answers a presenter or a desktop client connection without pairing it, the
`desktop-client` route, the core desktop client admission, and the `Connected
programs` copy for a runtime-owned endpoint.

VERIFIED 2026-08-14 (sixty-seventh wave, planner, ran
`cargo test -p muniment-core -p muniment-runtime -p muniment-attach`) — 1,295
tests pass over 118 test binaries, and the build prints no warning.

MEASURED 2026-08-14 (sixty-seventh wave, planner, read
`run_bound_attach_listener` beside `admit_desktop_client`) — the runtime names the
desktop client route and then drops the connection.
`AttachConnectionRoute::DesktopClient => return`
(`src-tauri/runtime/src/attach_listener.rs:165`) closes the socket before any
admission runs, so `admit_desktop_client`
(`src-tauri/core/src/attach/desktop_client_admission.rs:35`) has no production
caller. That core function writes the welcome and a credential-free grant, and no
function serves a request over the admitted stream. `run_migration_control_session`
(`src-tauri/core/src/attach/linux.rs:1773`) is the nearest shape, and it answers
one operation alone. The sixty-seventh wave files the slice.

MEASURED 2026-08-14 (sixty-seventh wave, planner, read every
`MigrationControlAuthorized` construction) — one frame type now names three
different grants. `src-tauri/attach/src/negotiation.rs:270` declares it, and
migration control (`src-tauri/core/src/attach/linux.rs:1798`), the approval
presenter (`attach/presenter_admission.rs:102`), and the desktop client
(`attach/desktop_client_admission.rs:104`) each send it under different field
rules. The migration and presenter grants carry an empty profile and no scopes,
and the desktop client grant carries a real profile and one scope entry. A rename
touches the same files as two sixty-seventh-wave slices, so the lane holds it for
a later wave.

MEASURED 2026-08-14 (sixty-sixth wave, planner, read the accept loop with no
expected desktop payload) — a runtime that resolves no installed desktop payload
denies the desktop presenter rather than prompting for it. The connection takes
the companion route, `request_approval` (`runtime/src/attach_listener.rs:190`)
finds no registered presenter, and `ApprovalCoordinator::request` answers false at
once. That is the fail-closed shape a development runtime should have, so the lane
files no slice for it.

MEASURED 2026-08-12 (thirty-ninth wave, planner, read the runtime manifest beside
`test/runtime-dependency-boundary.sh`) — the runtime crate cannot build a
`serde_json` payload. `src-tauri/runtime/Cargo.toml` depends on `muniment-core`
and `muniment-attach` alone, and the boundary check fails on a third direct
package. A runtime entry that must append an event therefore needs a core seam
that owns the payload, the way `record_preparation_failure`
(`src-tauri/core/src/run_preparation.rs:267`) owns the attachment failure. The
check reads the whole dependency tree, so a dev-dependency is barred too, and a
runtime test composes its inputs from core and attach exports alone.

MEASURED 2026-08-11 (twenty-sixth wave, planner, read `git log` between each
roadmap merge) — a drained slice measures batch position rather than ticket
content. The twenty-second through twenty-fifth waves each selected four slices,
and exactly two merged in each interval. The existing-thread run start then merged
on its second filing with no change of shape. This retires the two-drain
escalation rule that held the run control slot and the attach listener lifecycle.
The lane re-files a drained slice in strict priority order instead, and it puts
the slice it most wants built in the first position. The selected-file open rule
keeps its own hold, because eight identical filings are a different measurement.

HELD 2026-08-06 (eighth drain) — the selected-file open rule drained again, and
the planner has stopped re-filing it. `src-tauri/core/src/selected_file.rs` does
not exist. The eighth filing carried the core half alone, with the target module
path, the function names, and the test cases, so ticket size is not the cause.
Eight identical filings that never reach a pull request are an orchestration
question, and a ninth filing would only repeat the measurement. The work itself
still stands. `open_selected_files` (`src-tauri/src/chat.rs:1069`) repeats
`chat_file_metadata` (`:993`). Each copy opens the path, reads the metadata off
the open handle, rejects anything that is not a file, and takes the display name
from the last path segment. Reading the open handle rather than the path closes
a replacement window, and no test guards that defense. The two copies also
disagree, because `chat_file_metadata` rejects a path with no usable final
segment and `open_selected_files` does not. A third copy has since landed at
`open_selected_files` (`src-tauri/runtime/src/attach_boundaries.rs:281`), which
repeats the same four steps for the runtime. The lane still waits for an owner
look at why this one ticket never dispatches.

RETIRED 2026-08-12 (forty-seventh wave, planner, read `ChatProjector::projection`
beside `RunReducer::finish`, then ran an empty projector) — the runtime
projection-failure gap cannot fire. `projection`
(`src-tauri/core/src/journal/reducer.rs:613`) fails only when `finish` (`:857`)
finds no run state, which is the empty-journal case. `accept_prompt` reaches its
projection branch only after `prepare_new_run_*` appends `run.started`, so the
projector always holds state there. A test seam would also prove nothing, because
`record_persistence_failure` appends through `projector.apply` and an empty
projector rejects that event too. The lane re-opens this only against a new
failure route.

MERGE HAZARD — the sixty-seventh wave puts slice 1 in
`src-tauri/core/src/attach/linux.rs` and
`src-tauri/runtime/src/attach_listener.rs`, slice 2 in
`src-tauri/attach/src/client.rs`, slice 3 in `src-tauri/attach/src/fixtures.rs`
and `protocol-fixtures/muniment.attach/1/`, and slice 4 in
`docs/decisions/0012-user-level-runtime-service.md` and `THREAT_MODEL.md`. No two
slices share a file. Each ticket still tells the implementer to rebase on `main`
before it opens the pull request. The 2026-08-04 silent revert came from a stale
base.

SELECTED 2026-08-14 (sixty-seventh wave) — four slices in priority order.

1. The runtime serves requests over an admitted desktop client session.
2. The attach client half that dials the runtime as a desktop client.
3. The golden fixtures for the desktop client hello and its grant.
4. The ADR 0012 amendment that names the desktop client session lifecycle.

SEQUENCED 2026-08-14 (sixty-seventh wave) — the desktop client supervisor follows
slices 2 and 4. The desktop command surfaces move onto that supervisor next, and
Linux user-unit registration follows them. Each later slice sits behind a dormant
service entry point, and the desktop stays the owner throughout. The final cutover
slice activates the listener, approval coordinator, journal, CAS, Pi, device
session, credentials, authorization, and permission gates together. Remote Control
follows the cutover. User-unit registration must not precede the cutover, because
a service that takes the instance lock first would stop the desktop listener.

DONE — all three slices of the ADR 0009 attach workspace namespace amendment are
built (MUNIDESK-883, 887, 893, 896, 905). The signed `grant.workspace` value is
the only workspace authority for `thread.list`, `thread.open`, `thread.create`,
and `run.start`. `AttachCompanionState` (`src-tauri/src/attach_service.rs:123`)
holds the current signed workspace and returns no approval without one.
`onboard_workspace` (`:563`) records each canonical companion directory under
that workspace and the client identity, and `authorized_workspace` (`:603`) reads
it back. The `RunStart` branch of `dispatch_request`
(`src-tauri/core/src/attach/linux.rs:2149`) passes the resolved directory to
`start_run` as a separate execution root, and `configure_run`
(`src-tauri/src/chat.rs:250`) still rejects a requested workspace that differs
from `grant.workspace`. With no current cloud grant the approval and all four
operations fail closed. `THREAT_MODEL.md` records the rule.

DONE 2026-08-05 — the companion approval prompt names the authority it grants
(MUNIDESK-898). The dialog reports the signed workspace beside the claimed kind
and version, and it names the two scopes `thread.read` and `run.write`. The
listener resolves the approval before it prompts, so a connection with no cloud
grant fails closed without asking the user.

### Build-composition guards

DONE 2026-07-30 — the ACP SDK pulls `schemars`, which enables
`serde_json/preserve_order` across one cargo invocation. `begin_native_authorization`
now collects the unsigned sign-in body into a `BTreeMap`, so sorted key order is a
written rule rather than a cargo-feature accident (MUNIDESK-694). CI builds
`muniment-core` beside `muniment-acp` in one invocation.

DONE 2026-07-31 — `src-tauri/core/src/attach/mod.rs:12` names each `cursor` export
instead of re-exporting the module, so `muniment_core::attach::RunStreamWindow`
resolves to one type whatever packages share the invocation (MUNIDESK-727). CI
runs `cargo clippy` over the same package pair with `-D warnings`.

DONE 2026-08-06 — a test module never inherits a production timeout constant
(MUNIDESK-972). A test deadline is an argument.
`src-tauri/core/tests/timeout_constant_guard.rs` fails when a test module
references one.

## Cross-surface contracts (owner ideas 2026-07-29)

DONE 2026-07-29 — ADR 0019 makes muniment-cloud the source of truth for every
published cloud contract (MUNIDESK-626). Canonical schemas live under
`contracts/e0/<contract>/<major>/`, TypeSpec is the source format, generation runs
only in the cloud publication lane, and every surface pins one exact artifact
version.

DONE 2026-07-29 through 2026-08-12 — ADR 0020 and ADR 0024 decide the code diff
contract, and the whole local chain is built (MUNIDESK-630, 640, 765, 772, 775,
779, 1029, 1036, 1041, 1042, 1043, 1046 through 1049, 1051, 1052, 1057, 1062,
1066, 1070, 1071, 1073, 1074, 1075, 1078, 1082, 1083, 1087, 1089, 1090, 1092,
1093, 1097, 1137). `code-diff/1` became a desktop-owned local
contract on 2026-08-09, because it crosses no cloud boundary and the ADR 0019
cloud artifact never published. ADR 0024 makes the desktop core the sole trusted
producer, and the desktop shell, the CLI, and the ACP adapter are its three
consumers. The desktop renders through the `diff2html` parser and generator with
a design-token override sheet and adds no React, and the file header states each
file status in the shell register. ADR 0019 keeps every E0 cloud contract, and
the cloud lane publishes no `code-diff` artifact.
`src-tauri/code-diff/` holds the `muniment-code-diff` package with the model, its
validation, its canonical bytes, and a fixture exporter, and
`protocol-fixtures/code-diff/1/` holds the golden examples behind the
`code-diff-fixtures-current` CI job. `src-tauri/cli/src/code_diff_render.rs`
renders a validated diff to terminal text and is wired into no command.
`compute_code_diff` (`src-tauri/core/src/code_diff.rs:12`) computes the diff with
intra-line word segments, exact-content rename detection, three context lines,
and the 200-file, 20,000-line, and 2 MiB canonical bounds.
`src-tauri/core/src/write_plan.rs`, `code_diff_staging.rs`, `code_diff_journal.rs`,
`code_diff_observe.rs`, and `code_diff_apply.rs` carry the bounded write plan, the
staging step, the CAS and journal event pair with its replay and its gate binding,
the workspace observation with its pre-write stale check, and the write executor
that applies through verified parent handles. The gate, its answer verification,
the coordinate wiring, and the filter un-gate put a `code_diff` permission card
into production, and the transcript renders each applied diff through an
`Applied file changes` card. `src/lib/CodeDiff.svelte` renders both cards,
`src/lib/code-diff.js` holds the one generator configuration and its
`rawTemplates` header, and `test/probe/code-diff.html` and
`test/probe/applied-diff.html` drive the built bundle.

DONE 2026-08-12 — a code card sizes its layout from its own width
(MUNIDESK-1175). `src/styles/code-diff.css:108` reads
`@container (width < 480px)`, so the `Proposed file changes` card stacks its two
sides when the card is narrow rather than when the window is narrow. The
fifty-third wave measured each side at 122 CSS pixels with the artifact rail open
at a 960 pixel window, where the old `max-width: 720px` media query never fired.

DONE 2026-08-12 — both unavailable code-diff states now say what happened
(MUNIDESK-1178). A stored diff body goes missing when its CAS object is gone or
its recorded hash no longer matches, and the journal event survives either way.
The applied card reads `The changes were applied, but their record is no longer
stored.` (`src/App.svelte:922`), so it no longer reads as a failed write. The gate
card reads `Muniment will not apply a change it cannot show. Deny is the only
choice.` (`:935`) beside its single `Deny` control. The fifty-fifth wave shot
`test/probe/code-diff-unavailable.html` at 1100x720 and confirmed both strings
render.

PARKED — the producer's Pi input waits on the Pi wire contract, with the ADR
0025 consumers. ADR 0024 requires structured proposed operations from Pi
before any write starts, and no Pi message carries a file operation today.
The bounded streaming decoder in `pi_chat.rs` follows the contract. It is the
last open item in this chain.

CLOSED 2026-07-30 — ADR 0021 landed as a superseded record rather than an
on-device classifier store design, because the cloud ingress ruling arrived
first. The mobile store is void under the same ruling, and harness-spec §§12.1
and 12.2 and ADR 0007 stand unamended.

## Browser-control runtime capability (§6.8)

OWNER GO 2026-07-16 — the v1 browser-only actuator line is a governed runtime
capability rather than a companion surface.

DONE E0–E14 — extension-only real-profile architecture through injected
memory-only pairing and the MV3 authorized relay.

BLOCKED E15 — the desktop-to-extension handoff waits for the owner to upload a
real package to the private Chrome Web Store draft and to supply its public key.
Do not generate a replacement key or ID. The reserved extension ID is
`cdedcfbgomnhfpifpgdlpfkkanaofkjd`. After the matching key lands, compose and
validate E15 as one slice. Runtime tools, policy, receipts, and the kill switch
follow. Store publication and upload stay owner-gated, and v1 excludes operating
system, filesystem, and other-app control.

## Remote Control (§14) — a design study, not a build lane

CORRECTED 2026-07-29 — this repository holds no remote-control code, so no polish
ticket can attach to it. harness-spec §14.1 gives the single outbound relay leg to
the ADR 0012 runtime service, which is a decided direction rather than a built
surface. The shipped MV3 relay is the browser-control loopback relay (§6.8),
which is a different surface.

DONE 2026-07-29 — `docs/design-reference/remote-control-ux.md` records the
reference material from t3code, Claude Code remote control, VS Code Live Share,
and Google Cast, states plainly that the surface is unbuilt, and marks every
proposed state as pending owner mockup confirmation (MUNIDESK-674). `DESIGN.md`
links it.

## Onboarding, memory, and the Muniment Home (owner rulings 2026-07-19)

RATIFIED — a visible human-editable Markdown Home under `memory/`, `agents/`,
`projects/<name>/`, and `sessions/`. Files are the source of truth, indexes are
rebuildable caches, and no hidden primary store or Muniment sync exists.
Transcript names are semantic and retention is visible.

RATIFIED — the desktop has an unskippable first-run directory picker, defaulting
to `Documents/Muniment`. The CLI and VS Code use the opened directory by default
and lazily create a user Home for cross-project need. Honor the nearest
`AGENTS.md`. Instructions and memory stay distinct.

RATIFIED — the agent system prompt has five binding rules, a hand-written,
versioned, evaluation-gated base v0, and a bounded audited labeled-data tail.

SUPERSEDED 2026-07-29 — the required resident model no longer serves onboarding
triage or routing. Import retains consent, preview, provenance, and verbatim
originals.

DONE — the onboarding lane is built end to end (MUNIDESK-657, 660, 661, 682, 684,
692, 944). It covers the first-run Home picker, the fail-open default that returns
`<home>/Documents/Muniment` when that parent exists and `<home>/Muniment`
otherwise, the scaffold that runs before the sign-in screen, the bounded ZIP
preview and manifest review, bounded extraction of explicitly selected entries
with verbatim text and stable provenance, the consent checklist, conflict-safe
persistence with rollback and symlink defenses, one typed completion command with
structured invalid-input, destination-conflict, and save-failure results, and the
approved-originals save step. `src-tauri/core/src/home.rs` holds the write-plan
compiler and the default Home rule, and `src-tauri/core/tests/home_scaffold.rs`
guards the scaffold. The surface is a fixed header, one scrolling content region,
and a fixed footer, so its actions stay on screen at the 960x640 window minimum.
`test/probe/onboarding.html` and `test/probe/approved-files.html` drive the built
bundle.

DONE 2026-08-13 — a companion `home.ensure` scaffolds the Home the user
confirmed (MUNIDESK-1195). `resolve_attach_home`
(`src-tauri/src/attach_service.rs:502`) reads `configured_home` first and falls
back to `choose_default_home` only when the user recorded no choice, so
`DesktopAttachService.home` is the directory the model reads. The fifty-seventh
wave measured the old path scaffolding `memory/`, `agents/`, `projects/`, and
`sessions/` under `<documents>/Muniment` whatever the user picked. The runtime
composition owes the same resolution when it builds its own service.

### Memory index and retrieval (§17)

RATIFIED 2026-08-06 — memory retrieval has two phases. Phase one builds a lexical
index with zero new dependencies. Phase two adds vector search and a pinned
embedding artifact.

DONE 2026-08-06 — ADR 0026 and harness-spec §17 carry the binding rules
(MUNIDESK-959). Files stay the source of truth and the index stays a rebuildable
cache. Retrieved memory reaches the model only through one memory-search tool.
The tool set stays fixed for a conversation. Every retrieval is capped, and the
character budget comes from the selected model capability record. Every write
filters secrets, and every recall carries a receipt.

DONE 2026-08-07 through 2026-08-09 — phase one is built end to end
(MUNIDESK-960, 966, 967, 980, 981, 984, 990, 991, 994, 998, 1006, 1008, 1012,
1016, 1019, 1025, 1026). `src-tauri/core/src/memory_index.rs` holds the
rebuildable SQLite FTS5 cache, the bounded Home walk over the four scaffold
directories, the `RetrievalLimits` rule that lowers but never raises a cap, the
static `memory-search` tool declaration, and the `RecallRecord` receipt.
`src-tauri/core/src/memory_secret.rs` rejects a record that carries one of the
four `secret.*` rules, and `compile_onboarding_home_write_plan`
(`src-tauri/core/src/home.rs:509`) rejects an approved import file that carries a
credential and names that file. `src-tauri/src/memory.rs` composes one session
per run and writes the Pi agent extension atomically, a resumed run opens its own
session, and `coordinate_memory_search`
(`src-tauri/src/chat_coordinate.rs:672`) appends one `memory.recalled` event per
recall. A search caps at 1,024 Unicode scalar values and 64 terms, answers every
failure with a named `error.kind`, reads the cache without scanning the Home, and
reads its `source_file_state` digest from the `memory_cache_state` table beside a
cache generation. A damaged cache rebuilds, a timed-out first build rolls back,
and one session's build no longer blocks another session's search. The chat
projection carries each recall, and the shell renders it as one `Memory` row in
the expanded receipt record.

ANSWERED IN PRACTICE 2026-08-07 — the memory lane filed the phase-one store slice
without the owner ruling it had promised to wait for. MUNIDESK-960 landed the
cache, so the 2026-08-06 owner question no longer blocks this lane. The lane reads
the standing exclusion on self-initiated database work as covering durable stores
alone. The memory index is a new disposable file that any run deletes and rebuilds
from the Markdown sources. The journal migration alters the schema of the durable
store that holds the only copy of run history. That reading releases the cache and
leaves the journal migration held.

DO NOT RE-FILE — the per-run index build is not a defect. On a Home of 2,000
Markdown files a release-build reindex costs 30ms once the cache holds the
current hashes, and the first build costs about 600ms.
`ApplicationMemoryRuntime::open_session` runs one build per run start, so a warm
run start pays 30ms.

DESIGN CALL 2026-08-07 — a recall belongs in the expanded receipt record under
the provenance line, beside the route, model, cost, and time rows, rather than in
a new transcript element. The owner mockups name no memory surface, and
design-spec §2.2 already promises the expanded receipt names the connections a
reply touched. A running search also already renders as an ordinary
`Memory search` tool card, because Pi reports the registered tool. DESIGN.md
states that a recall renders there and nowhere else, and that an error rejecting
one item from a set names that item. `RunEventProjection`
(`src-tauri/core/src/journal/mod.rs:230`) stayed a strict field allowlist, so a
companion still receives no recall payload.

DO NOT RE-POLISH — the memory receipt row has taken three waves. MUNIDESK-994
projected the recall, MUNIDESK-998 rendered it, and MUNIDESK-1008 fixed the
value it carried. A later wave needs a new measurement before it touches that
row again.

DEFERRED — phase two embeddings follow phase one and a pinned artifact decision.

### The signed-in shell

DONE — the shell is built to design-spec §2. It holds the sidebar, thread, and
artifact rail layout, the adjustable rail, the collapsing sidebar, the measured
streaming underline on the active line alone, the milled thinking ring, the
provenance line, the receipt record grid, the per-message Copy row, inline tool
cards, the composer with its ten-line cap, dictation controls, and the profile
popover with its Appearance, Your access, Devices, Connected programs, and Voice
shortcut sections over a fixed Sign out footer.

DONE — the design laws carry enforcing lints. `src/styles/` holds
`signal-allowlist.test.js` for §1.2's color law, `shape-scale.test.js` for the
radius and shadow scales, `type-scale.test.js` for the `var(--text-*)` rule,
`class-usage.test.js` for a class no rule defines, `text-wrap.test.js` for six
text-bearing selectors, `contrast.test.js` for the token matrix,
`record-font.test.js` for the record register, and `target-size.test.js` for the
24 pixel floor. `npm run lint:copy` lints component copy.

DONE — ADR 0016 gives threads a durable identity over the run journal
(MUNIDESK-544 through 744). Thread identity is an append-only `thread.*` event
ledger plus an immutable `run_threads` stamping edge, and `PRAGMA user_version`
is an ordered migration boundary. The shell lists threads with relative times,
marks the current one, opens one by click or by the platform chord plus a digit,
reaches older pages through an explicit `Older threads` control outside the list,
renames inline from the titlebar, deletes through a quiet row control with an
inline confirm, and sets the operating-system window title from the open thread.

DONE — ADR 0023 decides assistant Markdown rendering, and it is built
(MUNIDESK-699 through 734). `src/lib/assistant-markdown.js` parses with the
pinned `marked` 18.0.7 and sanitizes with the pinned `dompurify` 3.4.12. Every
construct outside the subset renders as literal source text.
`src/lib/AssistantMarkdown.svelte` renders it, a fenced block and a table each
scroll inside their own block and take a tab stop only while they overflow, and
`src/lib/external-link.js` opens an anchor through the opener plugin for `https:`,
`http:`, and `mailto:` alone.

DONE — the shell answers the accessibility floor, and its run records are honest
(MUNIDESK-704, 862, 890, 986, 996, 1008, 1020, 1030, 1038). The workspace carries
one `h1`, `Threads` and `Artifacts` as its two `h2` groups, a real thread list, a
named transcript region, one coarse live-region announcement per run phase, and a
visually hidden `Message` label tied to its hint through `aria-describedby`.
`Send` and `Sign in` take `aria-disabled` rather than `disabled`, so activation
never drops focus, and the signed-out lockup names the product once as that
screen's `h1`. Tool activity renders as one named list rather than one live
region per tool. A failed, interrupted, or stopped reply carries its own mono
record beside the control that recovers it, a failed tool row takes `--oxide` and
says `failed` once, and the transcript error banner offers the recovery that
repeats the failed action. The receipt summary carries a rotating expand marker
and its memory row. A multi-line permission gate names its commit chord, and it
renders `⌘⏎` on macOS and `Ctrl ⏎` elsewhere. The sidebar delete, the delete
confirm, the run-record controls, and the receipt summary all meet the 24 pixel
target-size floor. The entitlement toast is the design system's first toast
primitive, and the pairing approval dialog is its first dialog primitive.

DONE — the frontend is factored into injectable modules with direct tests:
`chat-controller.js`, `chat-transcript-controller.js`, `dictation-controller.js`,
`voice-gesture.js`, `voice-shortcut.js`, `sidebar-state.js`, `chat-state.js`,
`thread-title.js`, `window-title.js`, `scroll-follow.js`, `scroll-region.js`,
`composer-size.js`, `streaming-underline.js`, and `entitlement-toast.js`.
`src/App.svelte` is about 1,400 lines.

DESIGN CALL 2026-07-31 (the settled-run sidebar refresh) — a settled run re-reads
the newest page alone, and the retained older pages keep the titles and times they
were read with. A thread in the refreshed newest page is dropped from the retained
pages, so no thread renders twice. The sidebar accepts one staleness: a thread
that the refresh pushes out of the newest page leaves the list until the next
`Older threads` activation or the next launch. DONE (MUNIDESK-739).

DO NOT RE-POLISH — the receipt summary has taken three consecutive waves.
MUNIDESK-996 added the expand marker, MUNIDESK-1008 fixed the memory row, and
MUNIDESK-1038 raised the target size. A later wave needs a new measurement
before it touches that element again.

NOT FILED — the empty workspace reads `New thread` three times, in the titlebar,
the sidebar action, and the sidebar current-thread record. The owner mockup sets
both strings (`docs/mockups/desktop/Muniment Desktop App.dc.html:256`, `:561`,
`:733`). Renaming either one is an owner call.

NOT FILED — the empty-state line reads `Ask anything. Your org's routing decides
which model answers.`, the composer placeholder reads `Ask anything`, and the
composer hint explains routing again. `docs/spec/01-design-system.md:139` asks an
empty state for one sentence. The sentence is owner ground truth in the mockup
and in two specs, so rewording it is an owner call. The planner asks for one.

WITHDRAWN 2026-07-31 — the claim that the open thread's row exposes no `current`
state does not survive its own check. This Chromium build reports no `current`
property for `<a aria-current="page">` either, so `Accessibility.getFullAXTree`
does not expose the state at all. A later wave needs a different instrument before
it re-files.

DO NOT RE-FILE — the shell is not the cost of opening a long thread. Against an
instant stub the built bundle reached a full render in 176ms for 100 runs, 366ms
for 500, and 1,154ms for 2,000 at 1100x720. DOM cost is about 0.5ms per turn and
linear, so the launch cost lives in the journal read. No windowing slice is
selected.

DO NOT RE-FILE — wide windows need no slice. The 760px thread column measured
centered at 1920x1080 and at 1440x900. The choice gate also fits the 960x640
minimum: six options and the refusal control render on one row with room to
spare.

DO NOT RE-FILE — the artifact rail at the minimum window is a designed trade-off
rather than a defect. The planner opened the rail on `test/probe/history.html`
at 960x640 and measured a 260px sidebar, a 380px rail, and a 320px thread
region whose text column is 272px after its padding.
`availableArtifactRailWidth` (`src/App.svelte:291`) already subtracts a
`minimumThreadWidth` of 320 before it clamps, and design-spec §2 pins the rail
between 380 and 560. The layout meets its own floor, and the empty rail is the
Phase 4 placeholder. A card inside that column still owes the reader a layout
that fits 246 pixels, which is what the fifty-third-wave code-diff measurement
records.

DONE 2026-08-12 — the profile popover sizes itself to the space above the profile
button (MUNIDESK-1179). The fifty-fourth wave measured a 448 pixel panel at
960x640 that hid 156 pixels of content, including the whole `Voice shortcut`
section. The fifty-fifth wave re-opened the popover in headless Chromium at
960x640 and measured a 558 pixel panel whose content region shows 449 of its 495
pixels. All six sections and the fixed `Sign out` footer render, and the remaining
46 pixels scroll. DO NOT RE-POLISH the panel height. A later wave needs a new
measurement before it touches that dimension again.

DONE 2026-08-13 — the two undersized popover controls meet the floor
(MUNIDESK-1184). `.companion-revoke` and `.close-access` both carry `min-width`
and `min-height`, and `src/styles/target-size.test.js` now reads
`src/lib/AccessPanel.svelte` beside `src/App.svelte`. DO NOT RE-POLISH either
control. A later wave needs a new measurement before it touches them.

KNOWN LIMIT — that lint still checks a hand-written list of six selectors, so a
compact control in a third component stays unguarded.
`docs/journal-ideas/a-hand-listed-lint-shrinks-the-rule-it-guards.md` records the
lesson. No static rule separates a compact control from a full-size one, and the
instrument that can is the probe capture rather than the vitest job, so the lane
files no widening slice.

DO NOT RE-FILE — asset weight is not worth a slice. The frontend emits one
252,280-byte script, one 62,240-byte stylesheet, and 154,444 bytes of webfont.
`marked` and DOMPurify account for roughly 55,000 bytes. Tauri serves those bytes
from local disk, so a dynamic import would trade a few milliseconds of parse time
for an async boundary in a synchronous component.

PARKED — `--space-*` tokens are absent, and a spacing sweep would touch nearly
every declaration in the app, so it stays parked rather than half-done. The
titlebar has no `⌘K` hint and should not get one while the palette is deferred.
The full §1.8 ring behavior engine is gated on a thinking surface of 34px or more.
`⌘K`, `⌘F`, and `⌘,` wait on the palette, search, and full settings surfaces.

DEFERRED — artifact data, rendering, persistence, sharing, and cloud contracts
belong to Phase 4 item 19. Select the first real-contract slice only when its
authoritative event and data boundary is specified. CLI and VS Code defaults wait
on workspace memory semantics.

OPEN OWNER ITEMS — the headless and server CLI stance needs a ruling. Final model
promotion waits on the MUNIQA routing evaluation. Gemma 4 E2B stays a future
owner-gated contingency pending llama.cpp PLE support. The MUNICLOUD model
artifact proxy and redirect is a ripple.

## Desktop QA automation

RATIFIED — layered frontend browser coverage plus installed-nightly real-app
validation on serialized pve01 desktop-ci VMs. Windows and Linux use WebdriverIO.
macOS runs an install and launch smoke plus screendumps and a manual owner pass.

DONE — ADR 0013, the canonical runner contract, the Linux `.deb` real-sign-in
smoke, the Windows MSI real-sign-in smoke, the macOS install and launch smoke, and
stable Linux and Windows JUnit artifacts. Each installed lane submits one unique
prompt through authenticated production chat, verifies a non-empty assistant turn
and a server receipt route, and attaches a generated PNG that the model must read
back. Rendered production conversations stay out of uploaded screenshots. A failed
platform job opens or updates one SHA-scoped triage issue.

DECIDED 2026-07-26 — no headless browser joins the CI gate. The smoke job runs on
a self-hosted runner with no browser, and the whole class of markup-versus-style
defects is statically detectable inside the existing vitest job. Planning-time
product inspection keeps rendering the built bundle against a stubbed signed-in
`window.__TAURI__` in headless Chromium. That is a planner tool, deliberately not
a CI layer, and it is the layer that finds geometry and interaction defects.

DONE — `test/probe/` is the committed planner harness (MUNIDESK-552, 558, 632,
715, 769, 1013). Its fixture pages cover the empty workspace, restored history,
Markdown, signed out, onboarding, approved files, the three code diff states, and
the four permission-gate kinds. `npm run probe` builds and prints every URL.
DONE 2026-08-13 — `test/probe/access.html` opens the profile popover
(MUNIDESK-1185), so a wave that inspects `Appearance`, `Your access`, `Devices`,
`Connected programs`, or `Voice shortcut` writes no throwaway script. The
fifty-sixth wave shot it at 1100x720 and read all six sections plus the fixed
`Sign out` footer. `auth_devices` now returns one active device and one revoked
device, and `attach_companions` returns one approved program.
`fixtureRendered` compares rendered text with whitespace removed, and
`markProbeReady` awaits `document.fonts.ready` before it sets `data-probe-ready`,
so a capture never shoots the fallback type. `test/probe-harness.test.js` guards
the single ready-marker write. `test/probe/stub.js` fails loudly on a command it
does not know, so a missing command breaks a fixture instead of hiding a panel.

DONE — the suite runs on Windows. Three slices gated every block that spawns a
POSIX shell, and `test/posix-shell-gate.test.js` fails when a new `bash` call site
appears outside a guarded block (MUNIDESK-679). Its walk skips any path segment
named `target` or `dist`, so it never descends into build output (MUNIDESK-731).

DONE — the installed Linux, Windows, and macOS lanes are green, and five repair
waves carried them there (MUNIDESK-757, 758, 759, 789, 827, 828, 872, 873, 900,
901, 934 through 946, 950, 951, 970, 973, 974, 976, 977, 978, 1033, 1034, 1056).
All three lanes drive the embedded WDIO WebDriver behind the `e2e-webdriver`
Cargo feature, which a release build never carries (`src-tauri/Cargo.toml:8`,
`src-tauri/e2e/capability.json`). The Linux lane runs under Xvfb with the
document portal mounted, and it builds `muniment-acp` and `muniment-runtime`
before the Tauri bundle. The macOS probe counts windows with CoreGraphics, so it
needs no privacy permission. Pull request CI runs the 14 Windows-only tests,
`test/windows-pr-gate.test.js` guards that gate, and MUNIDESK-950 pinned the
working tree to LF. Git history holds the per-lane repair detail.

OPERATING CONSTRAINT — the planning clone cannot compile the `src-tauri` desktop
crate, because the container has no ALSA headers for `alsa-sys`. The desktop-ci VM
builds remain the gate for that crate. The planner runs the workspace library
crates, the frontend suite, and the probe capture instead.

OPEN OWNER ITEM — on 2026-08-02 two green branches merged into a build break,
because each pull request built against its own stale base and the merge
combination never rebuilt. On 2026-08-04 the same hazard cost a shipped feature:
MUNIDESK-876 merged from a base older than MUNIDESK-877 and silently reverted the
companion revoke control and its tests. MUNIDESK-881 restored it. Both pull
requests stayed green, because the later merge removed the tests that guarded the
earlier one. Requiring an up-to-date branch before merge, or a merge queue, is a
repository-settings change that sits with the owner. The planner files no ticket
for it.

VERIFIED 2026-08-14 (sixty-seventh wave, planner) — one
`cargo test -p muniment-core -p muniment-runtime -p muniment-attach` run passes
1,295 tests over 118 test binaries. One `npm ci` then `npm test` run passes 933
frontend tests over 63 files with 31 skipped, plus 3 browser tests. This entry
replaces the earlier ledger.

MEASURED 2026-08-13 (fifty-eighth wave, planner, counted each path with
`git log --name-only --since=2026-08-01 -- <path>`) — `src-tauri/src/chat.rs` is
the busiest file in the tree at 38 touches and 2,437 lines, ahead of
`src-tauri/runtime/src/service.rs` at 35 and `src-tauri/src/attach_service.rs` at
33. That is the churn shape that justified
the runtime `service.rs` split (MUNIDESK-1181). The lane files no split for
`chat.rs` anyway. It sits in the `muniment-desktop` crate, which neither the
planning clone nor an implementer container can compile, so a pure-move refactor
would reach a desktop-ci VM with no local proof. The split waits for either a
compilable path or an owner call.

MEASURED 2026-08-12 (fifty-second wave, planner, read the vitest JSON report) —
all 31 skipped frontend tests are Windows-only cases. Every one sits behind
`it.skipIf(process.platform !== 'win32')` in `test/desktop-e2e-harness.test.js`,
and the pull request Windows gate runs them. The skip count is the designed
shape rather than dead coverage, so the lane files no slice for it.

PLANNER PROCEDURE — the planning clone ships no `node_modules`, so run `npm ci`
before `npm test`. Without it the run dies with `vitest: not found`, which reads
as a broken harness. Run `npm test` rather than `npx vitest run`, because the
bare command loads the browser tests into the jsdom environment and reports three
false failures. For a capture, run `npm run build`, serve the repository root
with `python3 -m http.server --directory .`, and pass
`--wait-for-selector "[data-probe-ready]"` to playwright. The fixture loads the
built bundle asynchronously, so a capture without that flag shoots a blank page.
A stray server started from another directory answers 404, and the wait then
hangs until the timeout.

PLANNER PROCEDURE — pick an unused port for the capture server. Port 8899 was
already bound by another workspace in this container, and the stale server
answered 404 for every probe path until the planner moved to 8944. The
fifty-third wave used 8951, the fifty-fourth used 8962, the fifty-fifth used
8975, the fifty-sixth used 8988, the fifty-eighth used 8993, and the
sixty-seventh used 9014. The fifty-seventh, the fifty-ninth, the sixtieth, the
sixty-first, the sixty-fifth, and the sixty-sixth waves each ran no capture,
because each read code alone. A server started from `src-tauri` answers 404 for
every `test/probe/` path, so start it from the repository root.

PLANNER PROCEDURE — for a measurement that needs a click, write a short
playwright script and run it from the repository root. Playwright is a project
dependency rather than a global one, so a script under `/tmp` cannot import it.
Delete the script before the wave ends, so the clone stays clean.

MEASURED 2026-08-14 (sixty-seventh wave, planner) — the capture ran after
`npm run build`, and headless Chromium shot the `confirm`, `select`, `input`, and
`editor` permission fixtures at 960x640. Each card names its request in the mono
record register, puts the refusal control apart from the request's own choices,
and fits the minimum window. The `editor` card alone carries the `Ctrl ↵ submits`
line, which is the design call for a multi-line gate. No design slice came out of
the pass, so the wave spent its whole budget on the ADR 0012 extraction lane.

MEASURED 2026-08-13 (fifty-eighth wave, planner) — the capture ran after
`npm run build`, and headless Chromium shot the restored-history fixture at
1100x720 and the Markdown fixture at 960x640. The sidebar, thread column, tool
cards, provenance line, interrupted-reply record, and composer all render as
design-spec §2 describes, and the Markdown fixture's `Receipt unavailable` line
is the designed state that `src/App.svelte:1013` renders and `App.test.js` guards.
No design slice came out of the pass, so the wave spent its whole budget on the
ADR 0012 extraction lane. The fifty-sixth wave read the same result from the
access, onboarding, and signed-out fixtures.

## Stable release and distribution

DONE — the distribution lane is built (MUNIDESK-113, 115, 716, 726, 801, 1031).
A rolling nightly builds one pinned SHA across Linux, signed Windows, and
unsigned macOS, and owner-triggered SemVer promotion copies exact green
artifacts. Promotion hashes the MSI, generates the `Muniment.Muniment` WinGet
manifest, opens a draft fork pull request, and reads the macOS sentence from the
nightly body rather than a fixed string. macOS Developer ID signing,
notarization, and stapling sit behind the same environment-injection seam
Windows signing uses. With no Apple credentials the build stays a clean unsigned
no-op, and a partial credential set fails fast and names only the missing
variables. The nightly carries seven assets, including an MDM-consumable macOS
package that installs `muniment.app` into `/Applications`
(`.github/lib/macos-signing.mjs:65`). `Casks/muniment-nightly.rb` is the Homebrew
cask, and the nightly workflow bumps its version and hashes.
`THIRD_PARTY_NOTICES.md` names every bundled frontend package at its resolved
version and both webfont families, and `THIRD_PARTY_RUST_NOTICES.md` names every
non-workspace crate with its version and license expression. All three platform
bundle configs ship both records beside the two webfont license texts.
`test/homebrew-cask.test.js` and `test/third-party-notices.test.js` guard both
shapes, and `docs/macos-signing.md`, `docs/macos-packages.md`, and
`docs/homebrew.md` are the pages.

OWNER-GATED — fork and token setup, WinGet publication, Homebrew tap publication, Apple
enrollment Y5DUNHQA74, public download and install docs, distribution accounts,
marketplace and store publishing, production launch, and publicity. Switching
macOS signing on is secrets-only. No monetization or promotional surface is
implied.

## Phase 4+ — Org surface (§9 items 16–19)

Remote MCP consumption, a local stdio allowlist, the capability install flow, the
artifact side panel, and projects with redaction
(`output withheld · connection not granted`).

## Runtime security posture

OWNER PICKED 2026-07-28 — two candidates from the 2026-07-21 operator competitive
review of Odysseus open this lane. Both start as documents.

DONE — ADR 0018, the untrusted-content boundary, is accepted and implemented
(MUNIDESK-614, 617, 631). `ChatMessage::untrusted_json` was the single wrapper,
its fields and constructors are private to their module, and
`src-tauri/core/tests/resident_message_boundary.rs` fails when a raw construction
appears outside the reviewed sites. The resident model left the product, so the Pi
lane is now the model-context boundary: the desktop sends the user prompt through
`PromptCommand` and Pi assembles its own context.

DONE — `THREAT_MODEL.md` is the runtime half of the trust boundary
(MUNIDESK-615, 688, 691, 819, 840, 847, 857, 879, 905, 931). It states the
boundary once, tables roles by capability over harness-spec §3, matrixes the
per-surface capabilities, and answers for each loopback what identity it trusts
and why a user cannot mint it. The ACP adapter row records the built surface. The
attach approval prompt names the program that asked, with its claimed kind and
version bounded, stripped of control characters, and marked as claimed rather
than verified. The attach socket row records companion revocation and the
workspace namespace. The ADR 0012 runtime service row records the per-profile
instance lock, the shipped `muniment-runtime` binary with its dormant service
entries, the migration control peer check, the one prepared handoff, the desktop
answer with its release path, and the same-user limitation (MUNIDESK-1113).

OPEN — the MUNIQA prompt-injection suite still follows ADR 0018's landed slices.

OPEN — the standing policy line for the agent system prompt stays a proposal
inside ADR 0018. harness-spec §16.1 rule 4 gates prompt text on review and
evaluation, and §16.2 records base v0 as owner-accepted verbatim. The
muniment-cloud control-plane sibling document belongs to that repository, and any
public security page derived from either document stays owner-gated.

## Standing gates

- Code pull requests gate on the structure smoke, the frontend and Rust tests,
  and the applicable path-scoped checks or desktop builds.
- Markdown-only pull requests gate on the structure smoke alone. Pushes to `main`
  run the smoke only.
- Build desktop targets only, unless an accepted ADR changes the companion checks.
- Every pull request and every push to `main` runs the gitleaks secret scan
  (`.github/workflows/secret-scan.yml`). The scan reads the pull request's own
  commits, `.gitleaks.toml` no longer re-declares the default rules, and the
  workflow prints the findings it meets (MUNIDESK-607, 760).
- `SPEC.md` carries the production-ready release-gate section and the folder
  hierarchy standard, and each criterion names the check that enforces it
  (MUNIDESK-761, 763).
- No user-facing text carries an em dash, in any form (MUNIDESK-1003). SPEC law
  5 states the owner ruling, and `npm run lint:copy` fails on the character and
  on every escaped spelling of it across `src`, `src-tauri`, `browser-control`,
  and `docs/mockups`.
- Distribution accounts, marketplace and store publishing, production launch, and
  publicity remain owner-gated.
