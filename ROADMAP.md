# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud
prerequisites for desktop chat went live on 2026-07-11. Client work that uses
them must exercise the real contracts. It must add no mocked production path.

> **Compacted 2026-08-04.** This document reached 265 KB and no longer fit in one
> read. Every landed slice used to carry its own paragraph. Those paragraphs are
> now per-lane summaries with their ticket ranges. Every open item, parked item,
> held item, gated item, and do-not-re-file measurement is preserved below. Git
> history holds the full slice-by-slice record.

## M0 — Scaffold (done 2026-07-09)

DONE — Tauri v2 desktop shell, specs, mockups, design reference, Rust and
frontend test harnesses, and Linux, Windows, and macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements

DONE — Svelte 5 and Vite shell, design tokens, signed-out and signed-in states,
native installation-bound authorization, keychain persistence, refresh, session,
and revocation flows, live entitlement projection, access-device states, and
server-unreachable recovery. The production contract is `/v1/auth/native/*`.
Generic OIDC remains test groundwork and is not the production handshake.

DONE 2026-08-05 — first-run registration retries a rate-limited device
registration (MUNIDESK-927). `NativeRegistrationError::RateLimited`
(`src-tauri/core/src/auth/native_registration.rs`) carries the server delay, the
transport reads `Retry-After` as seconds or as an RFC 2822 date, and one sign-in
attempt waits at most 300 seconds across its retries.

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

DONE — the permission gate reaches the user end to end (MUNIDESK-577, 582, 587,
594, 605, 766). The core seam validates an answer against its request. The
coordinate loop drains a typed answer queue and appends `permission.resolved`.
The thread renders a mono ask card for the `confirm`, `select`, `input`, and
`editor` kinds. Plain Enter commits a single-line field, and the platform chord
plus Enter commits a multi-line field. The refusal control sits apart from the
request's own choices. `test/probe/permission.html`, `input.html`, `editor.html`,
and `select.html` are the fixtures.

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
cleanup, journal reference accounting, retention, export, and compaction. The
cloud file flow follows items 8d and 9.

### Durable local run journal

DECIDED — ADR 0002 makes a per-run append-only SQLite event journal
authoritative. External effects are never silently re-executed, and large bodies
live in CAS.

DONE — schema, envelope, atomic append, reducer and replay, Pi translation,
deletion, collection, retention, deterministic export, crash-safe compaction,
cursor-paginated run summaries, and authorized Linux companion `thread.list` and
`thread.open` over redacted projections.

RULE 2026-07-12 — pre-launch schema work on the desktop's local journal is in
scope for this lane. The owner's post-go-live restriction applies to the
muniment-cloud deploy path, not to this repository.

**HELD — the owner ruling this lane waits on.** The standing owner exclusion on
self-initiated database migrations and the 2026-07-12 RULE above disagree about
this repository's local journal. Migration steps 2, 3, and 4 all landed under
that RULE. Seventeen waves have now passed with no answer, and the lane files
nothing each time. Everything in the next three paragraphs waits behind it.

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

DONE — the thread-summary query defects are fixed. The `run_times` CTE reads each
run's highest-`run_seq` event through a correlated subquery (MUNIDESK-584), and
the cursor boundary check reads the named thread's own maximum event time rather
than aggregating every thread (MUNIDESK-721). On a replica holding 4,000 threads
and 400,000 events the old boundary check cost 39.1ms and the direct read costs
0.5ms.

DO NOT RE-FILE — `append_batch` prepares its statements inside the per-event
loop, and the saving is not worth a slice. A scratch rusqlite 0.32 benchmark
measured 32.7µs per append against an in-memory database and 11.3µs with
`prepare_cached`. The same append against a real file measured 26.6ms, because
the journal opens WAL with `synchronous=FULL`. The commit fsync outweighs the
parse by three orders of magnitude.

DONE 2026-08-04 — retention picks its candidates before it loads them
(MUNIDESK-885). `apply_retention` (`src-tauri/core/src/journal/retention.rs:55`)
reads `run_event_types_with_newest_recorded_at`, groups the rows by run, and skips
a run that carries no terminal event type or that is newer than the cutoff. Only a
surviving candidate pays the full `journal.events` load, and the existing `reduce`
check still guards each deletion. The read added no column, table, index, or
migration. No production caller reaches retention yet, and the ADR 0012 runtime
service will own it.

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

DONE — the voice direction is Parakeet verbatim capture with Kokoro read-aloud
and global hotkeys. Pinned Parakeet and Silero acquisition and publication,
sherpa-onnx packaging and bindings, fixed-capacity microphone capture, bounded
utterance segmentation, a safe VAD boundary, chunk-invariant dictation
composition, desktop command and event wiring with redacted statuses, composer
dictation controls, a reproducible target-hardware evaluator, a bounded
100-utterance endurance mode, press-and-hold dictation with Escape to cancel, a
fixed system-wide hold-to-talk shortcut, hands-free promotion on rapid double
activation, and user rebinding with collision-safe rollback.

DONE 2026-07-29 — ADR 0004 pins muniment's own Parakeet conversion
(MUNIDESK-678), so the desktop depends on no third-party conversion for its ASR
graphs.

DONE 2026-08-01 — ADR 0005's on-demand speech-model install is implemented end to
end (MUNIDESK-752, 764, 771, 774, 777, 780). `parakeet_install_facts` reports the
pinned identity, the revision, the source repository, 672,384,307 download bytes,
940,819,763 required free bytes, and both artifact licenses. Pressing `Voice`
without a verified model opens the install card with those facts and an explicit
Install action. The card reports byte progress and cancels an active install.
Handy's architecture notes are vendored as voice reference.

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

DONE — ADR 0022 decides the direction across two slices (MUNIDESK-673, 683). The
Muniment ACP adapter is the only component that speaks ACP, and it is a thin
`muniment.attach/1` client of the ADR 0012 runtime service. That service keeps
sole ownership of sessions, threads, runs, effects, and receipts, so the
editor-spawned process never becomes a second executor. An editor answer to
`session/request_permission` is an input to a Muniment gate rather than a grant.
The ADR names the supported v1 method subset, answers no to the filesystem and
terminal client capabilities, negotiates the integer `1`, rejects the v2 draft,
and pins the Rust crate `agent-client-protocol` at exactly `2.0.0`.

DONE — the adapter is built and shipped (MUNIDESK-686, 690, 695, 698, 703, 707,
712 through 724, 736 through 746, 754, 768, 773, 776 through 799, 803 through
813, 815 through 819, 821 through 825, 829 through 831, 833 through 840, 843
through 852, 854 through 857). `src-tauri/acp/` holds the `muniment-acp` binary
crate. It pairs with a persisted identity under `$XDG_CONFIG_HOME/muniment/`,
claims the client kind `acp-adapter`, creates a durable thread through attach
`thread.create`, writes a versioned session record, streams released assistant
text as `agent_message_chunk`, translates live tool calls, bridges a permission
gate to `session/request_permission`, cancels through `run.cancel`, ends a prompt
on `run.needs_attention` and on a non-resumable `stream.closed`, and restores a
recorded session through `session/load` with `loadSession: true`. The Linux
package installs it at `/usr/lib/muniment/muniment-acp`, `docs/acp-editors.md` is
the setup page, and the installed nightly drives it through `initialize`.

DONE — the assistant-text redaction lane behind that streaming is complete. ADR
0009 carries the `assistant-text-v1` rule set: four `secret.*` rules, two
`path.*` rules, and the `content.withheld` metadata rule, each with a finite
maximum span of at most 131,068 bytes. `src-tauri/core/src/assistant_text.rs`
holds the scanner, `assistant_text/ledger.rs` holds the envelope attribution
ledger, and `assistant_text/projector.rs` is the stateful retained-suffix
projector. `thread.open` and the attach run stream both read that one projector.

DONE 2026-08-04 — ADR 0022 carries the capability-revocation amendment
(MUNIDESK-875). It supersedes the reservation in the prompt-ending amendment. A
revoked prompt ends with JSON-RPC code `-32000` and the exact message `Muniment
capability revoked`. The adapter removes `acp-client-credential`, keeps
`acp-client-id`, and reaches fresh visible approval on its next attach.

DONE 2026-08-04 — the adapter half of that amendment is built (MUNIDESK-882).
`ClientError::CapabilityRevoked` (`src-tauri/acp/src/main.rs:40`) is its own typed
variant, the run-stream loop drops the client credential and ends the prompt on
`RunStreamMessage::CapabilityRevoked` (`:749`), a revoked `run.cancel` maps to the
same failure (`:775`), and the prompt answers code `-32000` with the exact
message (`:807`).

PARKED — ADR 0022 names plan updates in its method subset, and no code produces
one, because the journal carries no plan or thought content. `agent_thought_chunk`
has the same missing upstream. Both wait on the Pi wire contract.

OWNER WORK — in-editor release validation for the Zed and JetBrains claims needs
real editor installs.

OPEN — the CLI treats `capability.revoked` and every `stream.closed` as
`UnexpectedMessage` (`src-tauri/cli/src/main.rs:423`). The CLI is deferred
indefinitely, so no slice is filed.

### Companion revocation and management

DONE 2026-08-04 — ADR 0009 carries the companion revocation amendment
(MUNIDESK-864). Revocation is a local decision by the attach owner. It removes
the selected companion's persisted client credential, blocks admission while it
persists the removal, invalidates every live connection capability authenticated
with that credential, and emits exactly one `capability.revoked` event per
affected connection with `reason: "companion_revoked"` and the connection's
`connection_event_id` as `subscription_id`. A revoked companion reconnects only
through a fresh visible approval.

DONE 2026-08-04 — the emitter half is built (MUNIDESK-865, 866, 867).
`LiveConnectionRegistry` (`src-tauri/core/src/attach/linux.rs:398`) tracks
connections by credential and carries the block, resume, and revoke states.
`AttachListenerState::revoke_companion` (`src-tauri/src/attach_service.rs:153`)
persists before it emits and restores authority when the write fails.
`list_companions` (`:171`) reports each companion's identity, claimed kind,
claimed version, and approval time, and the credential store records all three.

DONE 2026-08-04 — the desktop management surface is built and whole
(MUNIDESK-874, 876, 877, 881). `attach_companions` and `attach_revoke_companion`
(`src-tauri/src/attach_service.rs:164`, `:178`) are registered Tauri commands, and
the profile popover carries a `Connected programs` section under Devices. The
MUNIDESK-876 merge overwrote the MUNIDESK-877 revoke control from a stale base,
and MUNIDESK-881 restored the control and its tests.

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

DONE 2026-08-04 — the instance lock and the first extraction slice landed
(MUNIDESK-868, 869). `AttachFilesystem::acquire_instance_lock` is the core
primitive, `start_attach_listener` acquires it before it binds, and
`src-tauri/runtime/` holds the `muniment-runtime` binary crate with its own
format, lint, test, and dependency-boundary CI steps
(`test/runtime-dependency-boundary.sh`). The scaffold opens no endpoint, journal,
CAS, or Pi.

DONE 2026-08-04 — the approval coordinator moved into muniment-core
(MUNIDESK-878). `src-tauri/core/src/attach/approval.rs` holds
`ApprovalCoordinator` with presenter registration, the pending-decision map, the
timeout denial, and six direct tests. The desktop registers the presenter and
keeps the Tauri dialog. A shared core module is the extraction shape, and the
`muniment-runtime` binary composes it at the cutover.

DONE 2026-08-04 — interrupted-run reconciliation moved into muniment-core
(MUNIDESK-884). `src-tauri/core/src/journal/reconciliation.rs` holds
`reconcile_interrupted_runs` and its `event_types_are_terminal` classifier with
six direct tests. The core function takes its caller's provenance, so the
recorded `run.needs_attention` envelope did not change. Three desktop call sites
import it.

DONE 2026-08-04 — two of the three selected core moves landed. The chat storage
layout moved first (MUNIDESK-888). `src-tauri/core/src/chat_profile.rs` holds
`ChatProfile`, which owns `runs.sqlite3`, `cas`, and `pi-sessions` under one
profile directory, and `ChatState::new` (`src-tauri/src/chat.rs:705`) composes it.
The companion credential store followed (MUNIDESK-889).
`src-tauri/core/src/attach/credential.rs` carries the `O_NOFOLLOW` open, the owner
and mode check, the legacy unversioned read, and the 0600 temporary-file write.

DONE 2026-08-05 — the Pi-to-journal event translation moved into muniment-core
(MUNIDESK-892). `src-tauri/core/src/journal/pi_translation.rs` holds the
permission payload, the tool entry, the open-effect close, and the model stream
delta. `src-tauri/src/chat_coordinate.rs` lost 270 lines and imports them now.

DONE 2026-08-05 — `muniment-runtime` releases the instance lock on a signal
(MUNIDESK-894). `TerminationSignalWait` is the core primitive, and `run`
(`src-tauri/runtime/src/main.rs:43`) waits on it instead of parking forever.
`SIGTERM` and `SIGINT` both end the process, so a Linux user unit stops it
without the kill timeout. User-unit registration still comes after the cutover.

DONE 2026-08-05 — the cloud chat grant snapshot moved into muniment-core
(MUNIDESK-897). `src-tauri/core/src/chat_grant.rs` holds `ChatGrant`,
`fetch_grant`, `validate_grant`, `fetch_receipt`, and the typed
`FetchGrantError`. The desktop crate dropped its own `ureq` dependency, and core
carries the call behind its `tls` feature.

DONE 2026-08-05 — the chat storage open path moved into muniment-core
(MUNIDESK-902). `ChatProfile::open_storage`
(`src-tauri/core/src/chat_profile.rs:38`) creates the profile directories, opens
`runs.sqlite3`, opens the CAS, and returns the pair behind the typed
`ChatProfileError`. `ChatState::new` (`src-tauri/src/chat.rs:705`) is three lines
now, and it opens neither store by hand.

DONE 2026-08-05 — the run-resume eligibility check moved into muniment-core
(MUNIDESK-904). `src-tauri/core/src/chat_resume.rs` holds `resumable_context`,
`resumable_locator`, and the typed `ChatResumeError` with nine direct tests. The
desktop keeps the user-facing strings in `chat_resume_error_message`
(`src-tauri/src/chat.rs:776`), and `history_resumable`
(`src-tauri/src/chat_threads.rs:195`) calls the core check for its history rows.

DONE 2026-08-05 — the session-thread selector moved into muniment-core
(MUNIDESK-907). `src-tauri/core/src/session_thread.rs` decides which thread a
run joins and carries nine direct tests.

DONE 2026-08-05 — the thread ownership check moved into muniment-core
(MUNIDESK-910). `src-tauri/core/src/thread_ownership.rs` holds
`subject_owns_first_run` with its typed `ThreadOwnershipError`, and six call
sites in `src-tauri/src/chat_threads.rs` import it.

DONE 2026-08-05 — the run-event append and projection step moved into
muniment-core (MUNIDESK-912). `src-tauri/core/src/journal/run_append.rs` holds
`append_run_event`, and `append_emit` (`src-tauri/src/chat_coordinate.rs:742`)
keeps only its `chat-event` emit.

NOTE 2026-08-05 — the companion workspace-context map landed on its fifth filing
(MUNIDESK-924). The selected-file open rule drained a fifth time.
`open_selected_files` (`src-tauri/src/chat.rs:991`) still repeats
`chat_file_metadata` (`:914`). The planner filed it a sixth time with the target
module path, the function names, and the test cases it must carry. A seventh
drain needs an owner look at why this lane keeps dropping it.

DONE 2026-08-05 — the owned thread paging moved into muniment-core
(MUNIDESK-915).
`src-tauri/core/src/owned_threads.rs` holds
`newest_owned_workspace_thread` and `chat_thread_summaries_page`. They page
journal summaries and keep the threads the core check accepts. Both carry the
same 100-page cap and four direct tests. The desktop keeps thin wrappers in
`src-tauri/src/chat_threads.rs`, so the callers do not change.

DONE 2026-08-05 — the entitlement snapshot tracker moved into muniment-core
(MUNIDESK-921). `src-tauri/core/src/auth/entitlement_snapshot.rs` holds
`EntitlementSnapshotTracker` and its version transition rule. `AuthState`
(`src-tauri/src/auth/mod.rs:45`) holds the tracker, and `observe_snapshot`
(`:53`) keeps the desktop `entitlement-changed` emit.

DONE 2026-08-05 — `muniment-runtime` answers `--version`, `--help`, and `-h`,
and rejects an unknown argument before it reaches the instance lock
(MUNIDESK-908). The crate took no new dependency.

DONE 2026-08-05 — the companion workspace-context map moved into muniment-core
(MUNIDESK-924). `src-tauri/core/src/attach/workspace_context.rs` holds
`WorkspaceContextMap` with `record`, `authorized_directory`, and `instructions`.
`onboard_workspace` (`src-tauri/src/attach_service.rs:579`) and
`authorized_workspace` (`:619`) read that type instead of a bare three-level
`HashMap`.

RE-FILED 2026-08-05 (sixth filing) — the selected-file open rule moves into
muniment-core. `open_selected_files` (`src-tauri/src/chat.rs:991`) and
`chat_file_metadata` (`:914`) carry one rule twice. Each opens the path, reads
the metadata off the open handle, rejects anything that is not a file, and takes
the display name from the last path segment. Reading the open handle rather than
the path closes a replacement window, and no test guards that defense today. The
two copies also disagree, because `chat_file_metadata` rejects a path with no
usable final segment and `open_selected_files` does not. The target is
`src-tauri/core/src/selected_file.rs` with its own test binary.

MERGE HAZARD — the open slices edit `src-tauri/src/chat.rs`,
`src-tauri/src/chat_threads.rs`, `src-tauri/src/auth/mod.rs`,
`src-tauri/Cargo.toml`, and `src-tauri/core/src/attach/linux.rs`. Each ticket
tells the implementer to rebase on `main` before it opens the pull request. The
2026-08-04 silent revert came from a stale base.

SEQUENCED — the later extraction slices are the remaining Pi execution move, the
shared device session, the desktop client conversion, and Linux user-unit
registration, each behind a dormant service entry point. The desktop stays the
owner throughout. The final cutover slice activates the listener, approval
coordinator, journal, CAS, Pi, device session, credentials, authorization, and
permission gates together. Remote Control follows the cutover. User-unit
registration must not precede the cutover, because a service that takes the
instance lock first would stop the desktop listener.

DONE 2026-08-05 — ADR 0012 carries the migration control authority amendment
(MUNIDESK-913). Only the waiting runtime service may send the request. An
approved client credential grants no authority, and a claimed kind grants none
either. On Linux the desktop resolves the `SO_PEERCRED` peer PID to its
executable path and requires the installed `muniment-runtime` payload. The
desktop prepares at most one handoff at a time. The rule does not defend against
same-user compromise, which `THREAT_MODEL.md` defers.

DONE 2026-08-05 — the first two code slices of that request landed. The wire
slice reserves `migration.control` as an `Operation` variant with its canonical
fixture, and the dispatcher keeps answering `unsupported_operation` until the
handler lands (`src-tauri/attach/src/envelope.rs:241`) (MUNIDESK-916). The
authority slice adds the core peer check that resolves a peer PID to the
installed runtime executable (MUNIDESK-917). It reuses the `LinuxProcReader`
boundary that `src-tauri/core/src/browser_control/linux_identity.rs:103` already
publishes.

DONE 2026-08-05 — the quiesce rule and the prepared-handoff slot landed
(MUNIDESK-919, 920). `src-tauri/core/src/attach/quiesce.rs` holds
`evaluate_quiesce`, which weighs an active run, a pending permission gate, an
authentication operation, a session refresh, and an in-flight external effect in
that order and names the first blocker. `src-tauri/core/src/attach/handoff.rs`
holds `PreparedHandoffSlot`, which bounds the nonce at 128 printable ASCII
bytes, bounds the deadline at 60,000 milliseconds, refuses a second preparation
while one is live, and matches a nonce only before the deadline. Neither module
has a call site yet.

RE-FILED 2026-08-05 — the dispatcher branch that answers `migration.control` is
the next migration control slice, and the first filing drained.
`dispatch_request` (`src-tauri/core/src/attach/linux.rs:2023`) still falls
through to the `ThreadList` check at `:2447` and answers
`unsupported_operation`. The slice adds the branch, bounds the request body
against the same limits the prepared slot carries, and puts the decision behind
one new `ThreadListService` method whose default answers
`unsupported_operation`. The desktop implementation composes the landed peer
check, quiesce rule, and prepared slot in a later slice.

DONE 2026-08-05 — the attach welcome reserves an optional handoff nonce
(MUNIDESK-923). `Welcome` (`src-tauri/attach/src/negotiation.rs:96`) carries an
optional `handoff_nonce`, `with_handoff_nonce` sets it, and the deserializer
accepts an absent field.
`protocol-fixtures/muniment.attach/1/negotiation-welcome-handoff.json` is the
canonical fixture. No listener sets the value yet.

RE-FILED 2026-08-05 — the Linux package must ship `muniment-runtime`, and the
first filing drained. The landed migration control authority check requires the
installed runtime payload, and no package installs one today.
`.github/build-linux.sh:38` builds `muniment-acp` alone, and
`src-tauri/tauri.linux.conf.json` maps that one binary into the bundle. The
slice copies the `muniment-acp` pattern, adds a bundle guard beside
`test/acp-bundle.test.js`, and adds an installed-lane check beside the adapter
check in `test/e2e/runner/linux.sh:190`.

RE-FILED 2026-08-05 — the thread rename and delete appends move into
muniment-core, and the first filing drained. `rename_thread` and `delete_thread`
(`src-tauri/src/chat_threads.rs:208`, `:241`) carry one rule twice: check
ownership, read the last thread sequence, and append the `thread.*` event. ADR
0012 phase one names the journal as runtime-service state. The target is
`src-tauri/core/src/journal/thread_mutation.rs`.

FILED 2026-08-05 — the platform keychain credential store moves into
muniment-core. `src-tauri/src/auth/keyring_store.rs` holds `PlatformKeychain`
and `KeyringNativeCredentialStore`, and both wrap
`CoherentNativeCredentialStore` from core. ADR 0012 phase one names credentials
and the device session as runtime-service state, and `muniment-runtime` cannot
reach the keychain today. The move follows the `tls` pattern, so core takes
`keyring` behind a new optional feature and the desktop crate turns it on. The
same file holds the unused `KeyringTokenStore`, which belongs to the superseded
generic OIDC path and goes with the move.

DONE 2026-08-04 — ADR 0009 carries the attach workspace namespace amendment
(MUNIDESK-883). The signed `grant.workspace` value is the only workspace
authority for `thread.list`, `thread.open`, `thread.create`, and `run.start`. A
companion-supplied directory is only a local execution root, and the attach owner
canonicalizes and records it as a mapping under that authority. With no current
cloud grant the approval and all four operations fail closed. The amendment names
three implementation slices: grant workspace authorization, local execution-root
mapping, and attach workspace enforcement.

DONE 2026-08-04 — slice one of that amendment landed (MUNIDESK-887).
`AttachCompanionState` (`src-tauri/src/attach_service.rs:121`) holds the current
signed workspace, `record_workspace` and `clear_workspace` track it, and
`approval` (`:166`) returns `None` when no grant workspace exists. The session
workspace reaches every `thread.*` and `run.start` call through
`muniment_core::attach::linux` (`:1385`, `:1440`).

DONE 2026-08-05 — slice two of that amendment landed (MUNIDESK-893).
`onboard_workspace` (`src-tauri/src/attach_service.rs:579`) records each canonical
companion directory under the session workspace and the client identity, and
`authorized_workspace` (`:622`) reads the same two-level map.

DONE 2026-08-05 — slice three of that amendment landed (MUNIDESK-896). The
`RunStart` branch of `dispatch_request`
(`src-tauri/core/src/attach/linux.rs:2149`) resolves a companion directory through
`authorized_workspace` and passes it to `start_run` as a separate execution root.
The signed workspace stays the run's authority, and `configure_run`
(`src-tauri/src/chat.rs:556`) still rejects a requested workspace that differs
from `grant.workspace`. All three slices of the amendment are built.

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

## Cross-surface contracts (owner ideas 2026-07-29)

DONE 2026-07-29 — ADR 0019 makes muniment-cloud the source of truth for every
published cloud contract (MUNIDESK-626). Canonical schemas live under
`contracts/e0/<contract>/<major>/`, TypeSpec is the source format, generation runs
only in the cloud publication lane, and every surface pins one exact artifact
version.

DONE 2026-07-29 — ADR 0020 decides the code diff renderer across two slices
(MUNIDESK-630, 640). The shared contract is `code-diff/1`. The desktop renders
through the `diff2html` parser and generator with a design-token override sheet
and adds no React. The CLI selects a hand-written standard-library terminal
renderer that adds no crate.

DONE 2026-08-01 — the desktop half is built. ADR 0020 carries the desktop build
order amendment (MUNIDESK-765), `src/lib/code-diff.js` is the presentation-only
adapter (MUNIDESK-772), and `src/lib/CodeDiff.svelte` renders it with its warning
and empty states (MUNIDESK-779). ADR 0024 makes the desktop core the sole trusted
`CodeDiff` producer, puts the immutable plan in CAS behind two journal events, and
gates its first implementation slice on the published ADR 0019 Rust artifact
(MUNIDESK-775).

GATED — no further code-diff slice is fileable. Slice 4 renders a produced diff
inside the permission gate, and no producer can exist before the published
contract crate. The lane waits on the cloud repository. Slice 5, the receipt
replay of an applied diff, waits behind slice 4.

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

DONE — first-run Home picker and scaffold, bounded ZIP preview and manifest
review, bounded extraction of explicitly selected entries with verbatim text and
stable provenance, the consent checklist, conflict-safe persistence with rollback
and symlink defenses, and one typed completion command with structured
invalid-input, destination-conflict, and save-failure results.

DONE 2026-07-30 — the import ends by saving the approved verbatim originals
(MUNIDESK-682, 692). `compile_onboarding_home_write_plan`
(`src-tauri/core/src/home.rs:471`) emits one
`memory/imports/<date>-<name>-<digest>.md` document per approved entry and
nothing else. The `approved-review` screen is headed `Save approved files` and
carries `Back to archive review` beside `Save Home and finish`.
`test/probe/approved-files.html` drives the built bundle to it.

DONE 2026-07-29 — first-run folder setup fails open (MUNIDESK-660, 661, 684).
`choose_default_home` returns `<home>/Documents/Muniment` when that parent exists
and `<home>/Muniment` otherwise. The onboarding `load-error` branch carries
`Choose folder…` beside `Try again`, and the Linux attach Home takes the same
fallback.

DONE 2026-07-29 — the onboarding surface is a fixed header, one scrolling content
region, and a fixed footer, so its actions stay on screen at the 960x640 window
minimum (MUNIDESK-657).

### The signed-in shell

DONE — the shell is built to design-spec §2. It holds the sidebar, thread, and
artifact rail layout, the adjustable rail, the collapsing sidebar, the measured
streaming underline on the active line alone, the milled thinking ring, the
provenance line, the receipt record grid, the per-message Copy row, inline tool
cards, the composer with its ten-line cap, dictation controls, and the profile
popover with its Appearance, Your access, Devices, Connected programs, and Voice
shortcut sections over a fixed Sign out footer.

DONE — the design laws carry enforcing lints. `src/styles/signal-allowlist.test.js`
guards §1.2's color law, `shape-scale.test.js` guards the radius and shadow
scales, `type-scale.test.js` permits a component font size only through a
`var(--text-*)` token, `class-usage.test.js` fails on a class no rule defines,
`text-wrap.test.js` lints six text-bearing selectors, `contrast.test.js` guards
the token matrix, `record-font.test.js` guards the record register, and
`npm run lint:copy` lints component copy.

DONE — ADR 0016 gives threads a durable identity over the run journal
(MUNIDESK-544 through 546, 551, 554, 557, 562, 565, 566, 573, 574, 578, 580 through
584, 588, 595, 596, 598, 599, 601 through 604, 608 through 610, 618, 619, 627,
628, 687, 712, 739, 740, 743, 744). Thread identity is an append-only `thread.*`
event ledger plus an immutable `run_threads` stamping edge. `PRAGMA user_version`
is an ordered migration boundary. The shell lists threads with relative times,
marks the current one, opens one by click or by the platform chord plus a digit,
reaches older pages through an explicit `Older threads` control outside the list,
renames inline from the titlebar, deletes through a quiet row control with an
inline confirm, and sets the operating-system window title from the open thread.

DONE — ADR 0023 decides assistant Markdown rendering, and it is built
(MUNIDESK-699, 702, 706, 711, 734). `src/lib/assistant-markdown.js` parses with
the pinned `marked` 18.0.7 and sanitizes with the pinned `dompurify` 3.4.12.
Every construct outside the subset renders as literal source text.
`src/lib/AssistantMarkdown.svelte` renders it, a fenced block and a table each
scroll inside their own block and take a tab stop only while they overflow, and
`src/lib/external-link.js` opens an anchor through the opener plugin for `https:`,
`http:`, and `mailto:` alone.

DONE — the shell answers the accessibility floor. One workspace `h1`, `Threads`
and `Artifacts` as its two `h2` groups, a real thread list, a named transcript
region, one coarse live-region announcement per run phase, a visually hidden
`Message` label on the composer tied to its hint through `aria-describedby`, a
persistent `Send` and a persistent `Sign in` that take `aria-disabled` rather than
`disabled` so activation never drops focus, and a signed-out lockup that names the
product once as that screen's `h1`.

DONE — the run records are honest. A failed reply, an interrupted reply, and a
stopped reply each carry their own mono record beside the control that recovers
it. A failed tool row takes `--oxide` and says `failed` once. The entitlement
toast is the design system's first toast primitive, and the pairing approval
dialog is its first dialog primitive (MUNIDESK-862).

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

DONE 2026-08-04 — the multi-line permission gate names its commit chord
(MUNIDESK-890). The card carries a hint under the field, and it renders `⌘⏎` on
macOS and `Ctrl ⏎` elsewhere. The single-line kind commits on a plain Enter and
needs no hint.

DONE 2026-08-05 — the companion approval prompt names the authority it grants
(MUNIDESK-898). The dialog reports the signed workspace beside the claimed kind
and version, and it names the two scopes `thread.read` and `run.write`. The
listener resolves the approval before it prompts, so a connection with no cloud
grant fails closed without asking the user.

NOT FILED — the empty workspace reads `New thread` three times, in the titlebar,
the sidebar action, and the sidebar current-thread record. The owner mockup sets
both strings (`docs/mockups/desktop/Muniment Desktop App.dc.html:256`, `:561`,
`:733`). Renaming either one is an owner call.

NOT FILED — the empty-state line reads `Ask anything. Your org's routing decides
which model answers.`, the composer placeholder reads `Ask anything`, and the
composer hint explains routing again. `docs/spec/01-design-system.md:139` asks an
empty state for one sentence. The sentence is owner ground truth in the mockup
and in two specs, so rewording it is an owner call. The planner asks for one.

NOT SELECTED — every tool row carries `role="status"`, so a run with several tools
inserts several polite live regions beside the one coarse per-phase announcement.
MUNIDESK-704 added those roles deliberately, so the card rests. A later wave should
weigh a real list against a live region and should measure the announcement count
first.

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
715, 769). Ten fixture pages cover the empty workspace, restored history,
Markdown, signed out, onboarding, approved files, and the four permission-gate
kinds. `npm run probe` builds and prints every URL. `fixtureRendered` compares
rendered text with whitespace removed, and `markProbeReady` awaits
`document.fonts.ready` before it sets `data-probe-ready`, so a capture never shoots
the fallback type. `test/probe-harness.test.js` guards the single ready-marker
write.

DONE — the suite runs on Windows. Three slices gated every block that spawns a
POSIX shell, and `test/posix-shell-gate.test.js` fails when a new `bash` call site
appears outside a guarded block (MUNIDESK-679). Its walk skips any path segment
named `target` or `dist`, so it never descends into build output (MUNIDESK-731).

DONE — the installed Linux and Windows lanes are green. The most recent repairs
are MUNIDESK-757, 758, 759, 789, 827, 828, 872, 873, 900, and 901. MUNIDESK-900
waits for the four Home `README.md` files instead of reading them the moment the
`Sign in` control appears. MUNIDESK-901 widens the folder-picker window match to
accept a title that names a file, which the installed portal dialog uses.

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

VERIFIED 2026-08-05 (this wave, from a clean clone) — every suite the planning
container can run passed. `cargo test` on the standalone `muniment-core`
manifest reported `ok` for every test binary with no failure. `cargo test` for
`muniment-attach`, `muniment-cli`, `muniment-acp`, and `muniment-runtime`
reported the same. `cargo check` over those four crates plus `muniment-core`
finished clean on the locked manifest. `npm test` passed 838 tests with 25
skipped across 57 files, and the browser suite passed 3. `npm run build`
produced a 252,667-byte script and a 62,244-byte stylesheet. The planner read
the dispatcher, the negotiation welcome, the Linux packaging, and every
remaining ADR 0012 extraction target in the desktop crate before it filed this
wave's slices. Earlier waves recorded the same shape of verification, and this
entry replaces that ledger.

MEASURED 2026-08-05 — the probe capture ran again after this wave's build.
`python3 -m http.server` served the repository root, and headless Chromium
captured `test/probe/history.html` at 1100x720. The restored thread renders its
sidebar, titlebar, transcript, two tool rows, provenance line, interrupted-reply
record with its `Resume` control, and composer. This is a roadmap-fulfilment
wave, so it filed no design or layout slice, and the capture recorded no new
defect.

NOTE 2026-08-05 — a capture must pass `--wait-for-selector "[data-probe-ready]"`
to playwright. The fixture loads the built bundle asynchronously, so a capture
without that flag shoots a blank page. Serve the repository root with
`--directory`, because a stray server started from another directory answers
404 and the wait then hangs until the timeout.

NOTE 2026-08-05 — `npx vitest run` with no arguments loads the browser tests into
the jsdom environment and reports three failures. `npm test` is the correct
command. It excludes `**/*.browser.test.js` and then runs `npm run test:browser`
against the browser config. A planner or an implementer that runs the bare
command reads a false failure.

## Stable release and distribution

DONE — a rolling nightly builds one pinned SHA across Linux, signed Windows, and
unsigned macOS. Owner-triggered SemVer promotion copies exact green artifacts and
labels unsigned macOS. Promotion hashes the MSI, generates the `Muniment.Muniment`
WinGet manifest, and opens a draft fork pull request.

DONE 2026-07-24 — macOS Developer ID signing, notarization, and stapling are
pre-staged behind the same environment-injection seam Windows signing uses. With
no Apple credentials the build stays a clean unsigned no-op. A partial credential
set fails fast and names only the missing variables. `docs/macos-signing.md`
records the six vault keys and the verification checklist.

DONE — the shipped package carries its notices (MUNIDESK-716, 726, 801).
`THIRD_PARTY_NOTICES.md` names every bundled frontend package at its resolved
version and both webfont families. `THIRD_PARTY_RUST_NOTICES.md` names every
non-workspace crate with its version and license expression. All three platform
bundle configs ship both records beside the two webfont license texts, and
`test/third-party-notices.test.js` fails when an entry is missing or wrong.

OWNER-GATED — fork and token setup, WinGet publication, Homebrew, Apple
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
(MUNIDESK-615, 688, 691, 819, 840, 847, 857). It states the boundary once, tables
roles by capability over harness-spec §3, matrixes the per-surface capabilities,
and answers for each loopback what identity it trusts and why a user cannot mint
it. The ACP adapter row records the built surface. The attach approval prompt
names the program that asked, with its claimed kind and version bounded, stripped
of control characters, and marked as claimed rather than verified.

DONE 2026-08-04 — `THREAT_MODEL.md` records companion revocation and the landed
per-profile instance lock (MUNIDESK-879). The attach socket row names revocation,
and the ADR 0012 runtime service row names the lock and the `muniment-runtime`
scaffold.

OPEN — the MUNIQA prompt-injection suite still follows ADR 0018's landed slices.

DONE 2026-08-05 — `THREAT_MODEL.md` records the workspace namespace
(MUNIDESK-905). The attach socket row states the landed rule. The signed
`grant.workspace` value is the only workspace authority for `thread.list`,
`thread.open`, `thread.create`, and `run.start`. A companion-supplied directory
is only a local execution root, and it grants no scope. With no current cloud
grant the approval and all four operations fail closed.

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
- Distribution accounts, marketplace and store publishing, production launch, and
  publicity remain owner-gated.
