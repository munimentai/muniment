# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud
prerequisites for desktop chat went live on 2026-07-11. Client work that uses
them must exercise the real contracts. It must add no mocked production path.

> **Compacted 2026-08-04, again 2026-08-07.** This document reached 265 KB and no
> longer fit in one read. Every landed slice used to carry its own paragraph.
> Those paragraphs are now per-lane summaries with their ticket ranges. Every
> open item, parked item, held item, gated item, and do-not-re-file measurement
> is preserved below. Git history holds the full slice-by-slice record.

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
that RULE. Eighteen waves have now passed with no answer, and the lane files
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
are registered Tauri commands, and the profile popover carries a
`Connected programs` section under Devices. The MUNIDESK-876 merge overwrote the
MUNIDESK-877 revoke control from a stale base, and MUNIDESK-881 restored the
control and its tests.

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

DONE — nineteen core moves landed one slice at a time (MUNIDESK-878, 884, 888,
889, 892, 897, 902, 904, 907, 910, 912, 915, 921, 924, 930, 953, 956, 962, 965).
muniment-core now owns the approval coordinator, interrupted-run reconciliation,
the chat profile layout and its storage open path, the companion credential store
and its registry, Pi-to-journal event translation, the cloud chat grant snapshot,
run-resume eligibility, the session-thread selector, the thread ownership check,
the run-event append, owned-thread paging, the entitlement snapshot tracker, the
companion workspace-context map, the thread rename and delete appends, the
platform keychain credential store, the native device session composition, and
the protected prompt store. Each desktop call site keeps a thin wrapper, so no
caller changed shape.

DONE — `muniment-runtime` is a well-behaved Linux program. It answers
`--version`, `--help`, and `-h` and rejects an unknown argument before it reaches
the instance lock (MUNIDESK-908). It releases the lock on `SIGTERM` and `SIGINT`
(MUNIDESK-894). It backs off instead of polling every 25 milliseconds and prints
one line naming the wait (MUNIDESK-954). The Linux package builds and ships the
binary beside `muniment-acp`, and the installed nightly reads its `--version`
(MUNIDESK-929). The crate took no new dependency for any of it.

DONE — the migration control request is wired from the wire to the desktop seam
(MUNIDESK-913, 916, 917, 919, 920, 923, 928, 931, 952). ADR 0012 carries the
control-authority amendment. Only the waiting runtime service may send the
request, an approved client credential grants no authority, and on Linux the
desktop resolves the `SO_PEERCRED` peer PID to the installed `muniment-runtime`
payload. `migration.control` is an `Operation` variant with its canonical
fixture. `evaluate_quiesce` (`src-tauri/core/src/attach/quiesce.rs`) names the
first blocker in field order. `PreparedHandoffSlot`
(`src-tauri/core/src/attach/handoff.rs`) bounds the nonce at 128 printable ASCII
bytes and the deadline at 60,000 milliseconds and refuses a second preparation.
The dispatcher branch (`src-tauri/core/src/attach/linux.rs:2091`) bounds both
values, hands `control_migration` the connection's own `CompanionProvenance`, and
echoes the nonce. The default seam still answers `unsupported_operation`, so no
desktop implementation runs yet. `THREAT_MODEL.md` records the rule and the
same-user limitation.

DONE — three of the five quiesce inputs are wired. `evaluate_quiesce` weighs
five inputs in field order. They are an active run, a pending permission gate,
an authentication operation, a session refresh, and an in-flight external
effect. `RuntimeActivityRegistry`
(`src-tauri/core/src/attach/runtime_activity.rs`) carries one mark method per
activity and a guard that clears its mark on drop (MUNIDESK-957). `main`
(`src-tauri/src/main.rs:22`) creates one registry and manages it, and
`ActiveRun` (`src-tauri/src/chat.rs:141`) holds a guard for exactly the life of
a run (MUNIDESK-961). `AuthState` (`src-tauri/src/auth/mod.rs:95`) takes the
registry and marks both the authentication operation and the session refresh
(MUNIDESK-979). The coordinate loop marks the last two inputs (MUNIDESK-985).
`open_effects` and `pending_permission` (`src-tauri/src/chat_coordinate.rs:85`,
`:96`) each carry a guard for exactly as long as the state they track. All five
quiesce inputs now have a production call site.

DONE — the handoff nonce is separate work from the quiesce marks.
`mint_handoff_nonce` (`src-tauri/core/src/attach/handoff.rs:22`) mints a
single-use nonce from `getrandom` and took no new dependency (MUNIDESK-964). The
attach `welcome` reserves the optional `handoff_nonce` field with its canonical
fixture (MUNIDESK-923), and no listener sets the value yet.

DONE 2026-08-08 — the wire catalog carries the error the desktop answer needs
(MUNIDESK-989). `ErrorCode::MigrationNotReady`
(`src-tauri/attach/src/envelope.rs:238`) is retryable and reads `The desktop
cannot hand off ownership yet.`, and `fixtures.rs` carries its canonical record.
ADR 0012 reads `unsupported_operation` as a stale desktop that never hands off,
so a temporary quiesce blocker takes the new code instead.

DONE 2026-08-08 — muniment-core decides whether a probe `welcome` confirms a
handoff (MUNIDESK-988). `confirm_handoff_probe`
(`src-tauri/core/src/attach/handoff_probe.rs:32`) checks the readiness deadline
first, then the returned nonce, and returns `ConfirmedHandoff` or one of three
bounded reasons. The module has no call site yet, which is the shape `quiesce.rs`
and `handoff.rs` landed in.

DONE 2026-08-08 — the desktop answers the migration control request
(MUNIDESK-993). `control_desktop_migration`
(`src-tauri/src/attach_service.rs:65`) resolves the packaged `muniment-runtime`
resource, verifies the peer through `verify_migration_control_peer`, reads the
shared `RuntimeActivityRegistry` snapshot through `evaluate_quiesce`, and
prepares one `PreparedHandoffSlot`. A peer failure answers `unauthorized`, and a
quiesce blocker answers `migration_not_ready`.

DONE 2026-08-08 — the transport carries a stop handle (MUNIDESK-999).
`AttachStopHandle` (`src-tauri/core/src/attach/linux.rs:274`) is cloneable and
another thread may hold it. `AttachTransport::accept` (`:412`) polls the listener
beside the stop eventfd and returns `Closed` once `stop` fires, so a blocked
accept no longer runs forever.

DONE 2026-08-08 — muniment-core holds the readiness probe client
(MUNIDESK-1004). `read_handoff_probe_welcome`
(`src-tauri/core/src/attach/handoff_probe.rs:62`) connects to the endpoint,
sends one `hello`, reads the `welcome`, and closes before the authorization
step. `src-tauri/core/tests/attach_handoff_probe.rs` drives it against a fake
listener.

DONE 2026-08-08 — the desktop publishes its listener stop handle
(MUNIDESK-1010). `AttachCompanionState` (`src-tauri/src/attach_service.rs:169`)
holds the handle behind a mutex and a condvar. `run_attach_listener` publishes
the handle before it accepts, and `stop_attach_listener` stops the transport and
waits for the accept loop to drop the listener and the instance lock.

SELECTED 2026-08-08 (eighth wave) — one release-step slice remains. It composes
the probe client with `confirm_handoff_probe`
(`src-tauri/core/src/attach/handoff_probe.rs:180`) under one bounded readiness
deadline. The runtime service binds its endpoint only after it takes the
instance lock, so the first probe meets a refused connection and the composition
must retry until the deadline. Wiring the stop handle, the prepared nonce, and
that composition into one release step is the slice after it.

MEASURED 2026-08-08 (eighth wave, planner, read the listener status and stop
paths after MUNIDESK-1010) — the stop path leaves the reported status wrong, and
a stop that arrives early is dropped. `record_listener_stopped`
(`src-tauri/src/attach_service.rs:237`) clears the handle and never touches
`listener_start`, so `listener_status` (`:262`) still answers `started: true`
after the accept loop ends. `stop_listener` (`:245`) returns at once when no
handle is published yet, so a stop requested during the bind window neither
persists nor blocks, and the listener then accepts connections anyway. ADR 0012
forbids an interval with two owners, so the release step needs both repairs.
SELECTED 2026-08-08 (eighth wave).

DONE 2026-08-08 — a failed attach listener start now says why (MUNIDESK-1001).
`attach_listener_start_diagnostic` (`src-tauri/core/src/attach/linux.rs:58`)
names the filesystem, instance-lock, and bind failures, and
`start_attach_listener` prints one line for each.

DONE 2026-08-08 — `Connected programs` says the listener did not start
(MUNIDESK-1007). `AttachCompanionState` records the start outcome,
`attach_listener_status` reports it, and the panel waits for that outcome before
it reads the companion list. The planner had measured a broken listener
rendering as an empty list.

DONE 2026-08-07 — the permission gate coordination rules moved into
muniment-core on the fourth filing (MUNIDESK-982).
`src-tauri/core/src/permission_gate.rs` holds
`coordinate_extension_ui_request`, `coordinate_permission_answer`,
`ChatPermissionAnswer`, and `PendingPermissionAnswer`. Every call site changed
only its import path. Naming the target module, the four items, the call sites,
and the tests to move is what carried the fourth filing to a merge.

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
segment and `open_selected_files` does not. The lane waits for an owner look at
why this one ticket never dispatches.

MERGE HAZARD — the open slices edit `src-tauri/src/attach_service.rs`,
`src-tauri/core/src/attach/handoff_probe.rs`,
`src-tauri/core/src/memory_runtime.rs`, `src-tauri/src/chat.rs`,
`src-tauri/core/src/memory_scan.rs`, and `src/App.svelte`. One open slice edits
each of those files this wave. Each ticket tells the implementer to rebase on
`main` before it opens the pull request. The 2026-08-04 silent revert came from a
stale base.

DONE 2026-08-08 — the memory runtime is the twentieth core move
(MUNIDESK-995). `src-tauri/core/src/memory_runtime.rs` composes one
`MemoryRuntimeSession` per run, writes the Pi agent extension, and dispatches
each tool call. `src-tauri/src/memory.rs` is now the one-line wrapper.

DONE 2026-08-08 — the chat view types are the twenty-first move
(MUNIDESK-1000). `src-tauri/core/src/chat_view.rs` holds `SelectedFile`,
`ChatAttachment`, `ChatToolActivity`, `ChatPendingPermission`,
`chat_attachments`, and `chat_pending_permission`.

DONE 2026-08-08 — the run-start coordinator is the twenty-second move
(MUNIDESK-1005). `src-tauri/core/src/run_start.rs` holds `RunStartRequest`,
`RunStartLaunch`, the `RunStartBoundaries` trait, `RunStartError`, `ActiveRun`,
`start_desktop_run`, and `prepare_desktop_run`. `TauriRunStartBoundaries` and
`FakeRunStartBoundaries` stayed in the desktop as the two implementations.

DONE 2026-08-08 — the thread history projection is the twenty-third move
(MUNIDESK-1011). `src-tauri/core/src/thread_history.rs` holds `HistoryEntry`,
`ChatThreadOpenPage`, `load_prompt`, `project_history_entry`,
`history_resumable`, and `chat_thread_open_page`. `src-tauri/src/chat_threads.rs`
is now the command layer and its error sentences.

SELECTED 2026-08-08 (eighth wave) — the active-run message queue is the
twenty-fourth move. `queue_message` (`src-tauri/src/chat.rs:1095`),
`cancel_active_run` (`:1181`), and `queue_permission_answer` (`:1219`) all take
`&Mutex<Option<ActiveRun>>` and touch no Tauri type. `ActiveRun`
(`src-tauri/core/src/run_start.rs:34`), `PiRunAdapter`, `PiRpcTransport`,
`cancel_command`, and `ChatPermissionAnswer` already live in the core crate, so
the move needs no new dependency. `ChatDelivery`, `ChatQueueRequest`, and the
`QUEUE_TIMEOUT` constant travel with them, and each command keeps a thin
wrapper.

SEQUENCED — the later extraction slices are the remaining Pi execution move, the
desktop client conversion, and Linux user-unit registration, each behind a
dormant service entry point. The desktop stays the owner throughout. The final
cutover slice activates the listener, approval coordinator, journal, CAS, Pi,
device session, credentials, authorization, and permission gates together. Remote
Control follows the cutover. User-unit registration must not precede the cutover,
because a service that takes the instance lock first would stop the desktop
listener.

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
(`src-tauri/src/chat.rs:556`) still rejects a requested workspace that differs
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
(`src-tauri/core/src/home.rs:482`) emits one
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

DONE 2026-08-06 — first-run onboarding scaffolds the chosen Home before it
reaches the sign-in screen (MUNIDESK-944). The completion path skipped the
scaffold, so a new user finished onboarding with an empty folder.
`src-tauri/core/src/home.rs` carries the fix and
`src-tauri/core/tests/home_scaffold.rs` guards it.

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

DONE 2026-08-07 — phase one is built end to end (MUNIDESK-960, 966, 967).
`collect_markdown` (`src-tauri/core/src/memory_index.rs:696`) reads the four
scaffold directories under bounded limits, and the earlier
`src-tauri/core/src/memory_scan.rs` never gained a caller.
`src-tauri/core/src/memory_secret.rs` rejects a record that
carries one of the four `secret.*` rules. `src-tauri/core/src/memory_index.rs`
holds the rebuildable SQLite FTS5 cache, the `RetrievalLimits` rule that lowers
but never raises a cap, the static `memory-search` tool declaration, and the
`RecallRecord` receipt. `src-tauri/src/memory.rs` composes one session per run,
writes the Pi agent extension that registers the tool, and dispatches each call.
`coordinate_memory_search` (`src-tauri/src/chat_coordinate.rs:672`) appends a
`memory.recalled` event for every recall.

ANSWERED IN PRACTICE 2026-08-07 — the memory lane filed the phase-one store slice
without the owner ruling it had promised to wait for. MUNIDESK-960 landed the
cache, so the 2026-08-06 owner question no longer blocks this lane. The lane reads
the standing exclusion on self-initiated database work as covering durable stores
alone. The memory index is a new disposable file that any run deletes and rebuilds
from the Markdown sources. The journal migration alters the schema of the durable
store that holds the only copy of run history. That reading releases the cache and
leaves the journal migration held.

DONE 2026-08-07 — §17 rule 7 reaches production (MUNIDESK-980). The onboarding
import is the product's only memory write path, and
`compile_onboarding_home_write_plan` now rejects an approved file that carries a
secret.
DONE 2026-08-08 — the secret rejection names the approved file that carries the
credential (MUNIDESK-1012). `OnboardingHomeWritePlanError::SecretRejected` now
carries the offending entry's `source_name`, the command error carries it, and
the screen names the file. The loop returns the first offending entry, and
`reject_unsafe_metadata` already bounds the name. DESIGN.md carries the rule
that an error rejecting one item from a set names that item.

DONE 2026-08-07 — a resumed run opens its memory session (MUNIDESK-981).
`install_resume_run` (`src-tauri/src/chat.rs:885`) opens the session and clears
the active run when the open fails, so a resumed run never declares a tool it
cannot serve.

DONE 2026-08-08 — the Pi agent extension file is written atomically
(MUNIDESK-991). `write_agent_extension` (`src-tauri/src/memory.rs:57`) writes a
uniquely named temporary file and replaces the fixed path through `rename` on
POSIX and `MoveFileExW` on Windows. Two runs that start at once no longer hand
Pi a truncated `--extension` file. The planner had measured the race by reading
every call site.

DONE 2026-08-07 — a timed-out first index build no longer breaks memory search
for good (MUNIDESK-984). `MemoryIndex::open`
(`src-tauri/core/src/memory_index.rs:331`) sets `PRAGMA journal_mode = MEMORY`,
so a reindex that stops at its deadline rolls back rather than leaving half its
rows behind. `refreshed_cache` (`:303`) deletes and rebuilds a cache that SQLite
reports as damaged. The planner measured the original defect on a Home of 2,000
Markdown files, where the first build took 1.14 seconds and every later search
then failed with SQLite constraint error 1555 on `memory_files`.

DONE 2026-08-08 — the index build left the retrieval deadline (MUNIDESK-990).
`MemoryIndex::search` (`src-tauri/core/src/memory_index.rs:259`) calls
`read_cache`, which opens the cache and reads it without scanning the Home.
`MemoryRuntimeSession::build_with_timeout` (`:172`) owns the build under its own
deadline, and `ApplicationMemoryRuntime::open_session` runs it once per run.

MEASURED 2026-08-08, corrected on the fifth wave (planner, scratch `cargo test`
over a Home of 2,000 Markdown files) — the recall receipt reads every indexed
row. `source_state` (`src-tauri/core/src/memory_index.rs:696`) reads every
`memory_files` row and SHA-256 hashes the whole set on every search, only to
stamp `RecallRecord::source_file_state`. The fourth wave measured 9.5ms of a
16.2ms search and called the receipt dearer than the search. That run used a
debug build. A release build measures 5.4ms per search, with 0.9ms for the cache
open plus the digest, so the earlier reading overstated the cost. The scan is
still linear in the indexed file count, and the index caps at 10,000 files. All
sessions share `memory-index.sqlite3`, so a digest cached inside one
`MemoryIndex` can go stale when another session reindexes. Store the digest with
a cache generation in SQLite. Write both in the same reindex transaction as
`memory_files` and FTS5, and read them in the same snapshot as the search
results. Cache removal and rebuild must replace both values. Add a concurrency
test where one session reindexes before an earlier session searches. The earlier
session must return the updated rows and their matching `source_file_state`.
This removes the scan without a durable-store migration, because the database
remains a disposable cache. DONE 2026-08-08 (MUNIDESK-1006). The
`memory_cache_state` table stores the digest beside a cache generation, and one
reindex transaction writes both.

DO NOT RE-FILE — the per-run index build is not a defect. On the same 2,000-file
Home a release-build reindex costs 30ms once the cache holds the current hashes,
and the first build costs about 600ms. `ApplicationMemoryRuntime::open_session`
runs one build per run start, so a warm run start pays 30ms.

MEASURED 2026-08-08 (eighth wave, planner, read every lock site in
`memory_runtime.rs`) — one mutex serializes every session in the process.
`ApplicationMemoryRuntime` (`src-tauri/core/src/memory_runtime.rs:10`) holds one
`Mutex<BTreeMap<String, MemoryRuntimeSession>>`. `build_session_with_timeout`
(`:50`) holds that mutex for the whole index build, which runs up to the
5-second `DEFAULT_BUILD_TIMEOUT`. `dispatch_tool_call` (`:127`) holds the same
mutex for the whole search, because `MemoryRuntimeSession::call` takes `&mut
self`. A search is budgeted at 250ms, and the wait for the mutex sits outside
that budget. `prepare_desktop_run` opens the session on the `chat_submit` path,
so a second run start can also block a message submit. Give each session its own
`Arc<Mutex<MemoryRuntimeSession>>` and hold the map lock for the lookup alone.
SELECTED 2026-08-08 (eighth wave).

MEASURED 2026-08-08 (eighth wave, planner, read both Home walkers) — the tested
Home scanner is dead and the live one is a second copy.
`src-tauri/core/src/memory_scan.rs` exports `scan_home_documents`, and
`src-tauri/core/tests/memory_scan.rs` is its only caller. The index carries its
own private `collect_markdown` (`src-tauri/core/src/memory_index.rs:696`) with
its own `MAX_FILES`, `MAX_FILE_BYTES`, and `MAX_DIRECTORY_DEPTH` constants that
repeat the scanner's three. The live walker is the deadline-aware one, so the
dead module goes. SELECTED 2026-08-08 (eighth wave).

MEASURED 2026-08-07 — nothing renders a recall. The reducer drops
`memory.recalled`, so `ChatProjection`
(`src-tauri/core/src/journal/reducer.rs:434`) carries no recall and the frontend
receives none. DESIGN CALL 2026-08-07 — a recall belongs in the expanded receipt
record under the provenance line, beside the route, model, cost, and time rows,
rather than in a new transcript element. The owner mockups name no memory
surface, and design-spec §2.2 already promises the expanded receipt names the
connections a reply touched. A running search also already renders as an
ordinary `Memory search` tool card, because Pi reports the registered tool.
DONE 2026-08-08 — the chat projection carries the recall (MUNIDESK-994).
`ChatProjection::recalls` (`src-tauri/core/src/journal/reducer.rs:443`) holds one
`ProjectedRecall` per `memory.recalled` event with its query and its files. Both
webview payload sites carry the list, `chat_event`
(`src-tauri/src/chat_coordinate.rs:878`) and `project_history_entry`
(`src-tauri/src/chat_threads.rs:126`). `RunEventProjection` (`journal/mod.rs:230`)
stayed a strict field allowlist, so a companion still receives no recall payload.

DONE 2026-08-08 — the shell renders that recall (MUNIDESK-998).
`applyChatEvent` and `historyMessages` (`src/lib/chat-state.js:120`, `:136`)
carry the field, and `receiptRows` (`:62`) appends one `Memory` row per recall
to the expanded record. DESIGN.md states that a recall renders there and nowhere
else.

MEASURED 2026-08-08 (sixth wave, planner, built the bundle and captured
`test/probe/history.html` at 1100x760 with the receipt expanded) — that row
carries the wrong half of the recall. `receiptRows`
(`src/lib/chat-state.js:68`) joins `recall.files` with a comma and drops
`recall.query`, which `ProjectedRecall`
(`src-tauri/core/src/journal/reducer.rs:448`) supplies beside the files. Three
Home-relative paths in one value widened the record card from 329px to the full
760px thread column, so one receipt in a thread renders at a different width
from its neighbours. Two recalls in one run produce two `Memory` terms that name
nothing that tells them apart. A recall that matched no file renders the prose
`no files` where every other value is data. DONE 2026-08-08
(MUNIDESK-1008).

DEFERRED — phase two embeddings follow phase one and a pinned artifact decision.

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

MEASURED 2026-08-08 (eighth wave, planner, built the bundle and read every
button rect on `test/probe/history.html` at 1100x760) — three shell controls
render under the WCAG 2.2 target-size floor. The sidebar `Delete` control
(`.thread-delete`, `src/App.svelte:1278`) measures 56x21 CSS pixels, and it sits
absolutely positioned inside the 239x36 thread row, which is itself a control.
The two controls overlap, so the SC 2.5.8 spacing exception cannot rescue
either. The `.thread-delete-confirm` buttons take the same `padding: 3px 6px`.
The run-record `Resume` and `Try again` controls (`src/App.svelte:917`) measure
62x21. WCAG 2.2 SC 2.5.8 sets the floor at 24 by 24 CSS pixels, and the 36-pixel
row leaves room for it. DESIGN.md takes the law and a test guards it. SELECTED
2026-08-08 (eighth wave).

DONE 2026-08-08 — the receipt summary shows that it expands (MUNIDESK-996). The
expandable line carries a rotating marker (`src/App.svelte:990`, `.receipt-marker`
at `:1357`), and the static `Receipt unavailable` caption carries none. A
`prefers-reduced-motion` reader gets the same two states with no rotation. The
planner captured `test/probe/history.html` at 1100x720 on 2026-08-08 and read the
marker beside the provenance line.

NOT FILED — the empty workspace reads `New thread` three times, in the titlebar,
the sidebar action, and the sidebar current-thread record. The owner mockup sets
both strings (`docs/mockups/desktop/Muniment Desktop App.dc.html:256`, `:561`,
`:733`). Renaming either one is an owner call.

NOT FILED — the empty-state line reads `Ask anything. Your org's routing decides
which model answers.`, the composer placeholder reads `Ask anything`, and the
composer hint explains routing again. `docs/spec/01-design-system.md:139` asks an
empty state for one sentence. The sentence is owner ground truth in the mockup
and in two specs, so rewording it is an owner call. The planner asks for one.

DONE 2026-08-07 — tool activity is one named list rather than one live region
per tool (MUNIDESK-986). The planner had measured seven polite live regions on a
run with four sequential tools and three parallel tools, each speaking twice.
The coarse per-phase announcement is the transcript's only live region again.
MUNIDESK-704 added those `role="status"` attributes deliberately, and that
measurement was the evidence the earlier wave asked for.

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
DONE 2026-08-08 — the probe stub answers the listener-status command, and an
unknown command fails loudly (MUNIDESK-1013). `test/probe/stub.js` no longer
answers `null` to a command it does not know, so the next missing command breaks
a fixture instead of hiding a panel.

DONE — the suite runs on Windows. Three slices gated every block that spawns a
POSIX shell, and `test/posix-shell-gate.test.js` fails when a new `bash` call site
appears outside a guarded block (MUNIDESK-679). Its walk skips any path segment
named `target` or `dist`, so it never descends into build output (MUNIDESK-731).

DONE — the installed Linux and Windows lanes are green. The most recent repairs
are MUNIDESK-757, 758, 759, 789, 827, 828, 872, 873, 900, and 901. MUNIDESK-900
waits for the four Home `README.md` files instead of reading them the moment the
`Sign in` control appears. MUNIDESK-901 widens the folder-picker window match to
accept a title that names a file, which the installed portal dialog uses.

DONE 2026-08-06 — a repair wave carried the three lanes further (MUNIDESK-934
through 946, 950, and 951). All three desktop lanes now drive the embedded WDIO
WebDriver behind the `e2e-webdriver` Cargo feature, which a release build never
carries (`src-tauri/Cargo.toml:8`, `src-tauri/e2e/capability.json`). The Linux
lane opens the native folder picker under Xvfb and mounts the document portal.
The Windows lane speaks before it can fail, survives an `npm ci` deprecation
warning on stderr, resolves `npm` without `ComSpec`, and gives a PowerShell
spawn more than the 5000ms default. A targeted nightly dispatch no longer
reports success while skipping the job it was asked to run.

DONE 2026-08-07 — a second repair wave followed (MUNIDESK-970, 973, 974, 976,
977, 978). The Linux in-VM build installs `libasound2-dev` and builds
`muniment-acp` and `muniment-runtime` before the Tauri bundle, and the e2e Tauri
config resolves its capabilities file. The Windows lane launches the nested
browser suite through `npx` and declares `@vitest/browser-playwright`. The macOS
probe counts windows with CoreGraphics, so it needs no privacy permission.

DONE 2026-08-06 — the Windows-only tests run before they merge (MUNIDESK-946).
Pull request CI never ran the 14 of them, and 12 failed inside the Windows lane.
`test/windows-pr-gate.test.js` now guards the gate. MUNIDESK-950 pinned the
working tree to LF, because one CRLF checkout broke three contract suites at
once.

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

VERIFIED 2026-08-08 (eighth wave, from a clean clone) — `npm ci` then `npm test`
passed 886 frontend tests across 59 files, and the browser suite passed 3.
`cargo test -p muniment-core -p muniment-attach` passed with no failure. The
planner then read the handoff, probe, quiesce, and runtime-activity modules, the
desktop attach service with its listener status, stop, and migration-control
paths, the memory runtime and index, both Home walkers, and the active-run queue
and cancel functions. It rebuilt the bundle, captured six probe fixtures in
headless Chromium at 1100x760, and measured every button rect on the restored
history fixture. Earlier waves recorded the same shape of verification, and this
entry replaces that ledger.

NOTE 2026-08-06 — the planning clone ships no `node_modules`. Run `npm ci`
before `npm test`. Without it the run dies with `vitest: not found`, which reads
as a broken harness.

MEASURED 2026-08-07 — the probe capture ran again after `npm run build`.
`python3 -m http.server --directory .` served the repository root, and headless
Chromium captured `test/probe/history.html` at 1100x720. The restored thread
renders its sidebar, titlebar, transcript, two tool rows, provenance line,
interrupted-reply record with its `Resume` control, and composer. The capture
recorded no new visual defect. The live-region count above came from the markup
behind that capture rather than from the image.

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
(MUNIDESK-615, 688, 691, 819, 840, 847, 857, 879, 905, 931). It states the
boundary once, tables roles by capability over harness-spec §3, matrixes the
per-surface capabilities, and answers for each loopback what identity it trusts
and why a user cannot mint it. The ACP adapter row records the built surface. The
attach approval prompt names the program that asked, with its claimed kind and
version bounded, stripped of control characters, and marked as claimed rather
than verified. The attach socket row records companion revocation and the
workspace namespace. The ADR 0012 runtime service row records the per-profile
instance lock, the `muniment-runtime` scaffold, the migration control peer check,
the one prepared handoff, the unimplemented desktop answer, and the same-user
limitation.

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
