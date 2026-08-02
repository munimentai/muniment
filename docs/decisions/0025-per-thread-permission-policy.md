# 0025 — Per-thread permission policy over the run journal

- Status: accepted
- Date: 2026-08-02
- Context: desktop spec §8 and ADRs 0002, 0016, and 0022

## Context

Desktop spec §8 offers `Allow once`, `Allow for this thread`, and `Deny` on
the permission ask card. The shipped card supports one-shot answers alone.
No contract defines where a thread answer lives or how a later gate uses it.

ADR 0002 rejects authoritative mutable session rows. ADR 0016 provides an
append-only `thread_events` ledger and permits rebuildable projections. The
standing migration exclusion bars every new table and column. A policy must
therefore use the existing ledger as data and remain safe after replay.

The run coordinate loop owns gate resolution. `chat_answer_permission` is the
native answer seam. ADR 0022 also treats an ACP editor answer as input to a
Muniment gate, never as authority by itself.

## Decision

### Decision bars

The policy design must meet these bars:

- **Auditable authority.** Every durable answer and revocation remains in an
  append-only authoritative stream.
- **Conservative scope.** A policy cannot cover a broader resource or action
  than the user saw.
- **Honest replay.** Every gate remains visible with its resolution and actor.
- **Current checks.** A saved answer cannot bypass current owner policy,
  workspace scope, capability, entitlement, or kill-switch checks.
- **Deterministic concurrency.** Rebuild and concurrent appends produce one
  effective policy from ledger order.
- **No migration.** The design adds no table and no column under the standing
  migration exclusion.

### Policy event family

Per-thread policy uses two versioned event types in the existing
`thread_events` ledger:

- `thread.permission.policy.decided` creates one durable allow or deny policy.
- `thread.permission.policy.revoked` revokes one earlier decision.

A decided payload contains `policy_id`, `decision`, `request_kind`, `resource`,
`actor`, `answer_source`, and optional `gate_id`. `policy_id` is a stable
opaque UUIDv7. `decision` is `allow` or `deny`. `answer_source` is `native` or
`acp`. `actor` identifies the authenticated user. `gate_id` binds a decision
created from a pending gate.

`resource` is a tagged value. A path value contains the canonical absolute
path and the requested operation. A command value contains the exact argument
vector and canonical working directory. Other request kinds must define an
equally exact, versioned resource value before they may create durable policy.
Display text, shell reserialization, path prefixes, globs, and inferred tool
groups never form match authority.

A revoked payload contains `policy_id`, `actor`, and `reason`. The envelope
supplies event identity, time, provenance, and contiguous `thread_seq`.
Validation rejects unknown decisions, request kinds, resource tags, missing
fields, mismatched gate data, and a revocation of an unknown policy. It also
rejects a second decision that reuses a `policy_id` with different content.

The effective-policy projection reduces `thread_events` in `thread_seq` order.
A valid decision becomes active until a later valid revocation names it.
When several active policies match, the latest matching decision wins. The
projection is disposable and rebuildable from the ledger.

### Match rule and authority checks

A policy matches only inside its stamped `thread_id`. The current request must
have the same `request_kind` and byte-equivalent canonical `resource` value.
For paths, both the operation and canonical absolute path must match. For
commands, both the argument vector and canonical working directory must match.
Changing an argument, operation, path, working directory, or request kind
requires another answer.

The runtime validates and canonicalizes the pending request before lookup. It
then checks the current subject, workspace scope, capability, entitlement,
owner policy, and kill switch. An owner always-ask rule suppresses an allow
policy and shows the gate. A current authority failure cannot become an allow,
even when an older policy matches. Missing, malformed, contradictory,
unsupported, foreign-thread, or stale projection state fails closed and shows
the gate when the request remains valid.

The run coordinate loop performs lookup and resolution. The frontend, ACP
adapter, and Pi process cannot claim a match or auto-answer a gate. A deny
policy may auto-deny a matching valid request. It grants no authority.

### Lifetime and revocation

A policy starts when its decided event commits. It covers later matching gates
in the same thread, including gates in later runs. It does not cover an
already resolved gate or another thread. Run completion, application restart,
adapter reconnect, and projection rebuild do not end it.

The user may revoke an active policy through a thread permission-settings
surface. A new contradictory answer does not rewrite or implicitly revoke an
older policy. It appends a new decision, and latest matching policy wins.
Explicit revocation exposes the preceding active matching policy, if one
exists. The surface must show that result before confirmation.

Thread deletion ends every policy with the thread tombstone. Policies do not
survive thread export/import as authority unless a future import contract
explicitly preserves trusted identity and provenance. Sign-out does not erase
the events, but profile and workspace checks still gate every use.

### Gate journal and receipt

Every incoming gate first appends its ordinary `permission.requested` event.
Only after that commit may the coordinate loop consult the effective-policy
projection. A match appends `permission.resolved` before any allowed effect
starts. A crash between those events restores a pending gate and evaluates
current policy again. A crash after resolution never repeats the answer or
effect.

For an automatic answer, `permission.resolved` records `gate_id`, `decision`,
`actor: "thread_policy"`, `policy_id`, the deciding thread event ID, and the
matched `request_kind` and canonical `resource`. It also records
`resolution: "automatic"`. The receipt projects those fields with the
original request, thread ID, run ID, effect ID when present, and final effect
outcome. This evidence shows what matched, which policy supplied the answer,
and whether an allowed effect completed, failed, or has an uncertain outcome.

A user answer records `actor: "user"` and `resolution: "interactive"` under
the existing receipt rules. If that answer creates policy, the policy event
ID and `policy_id` also appear in the resolution and receipt. The coordinate
loop commits the policy decision and gate resolution as one ordered batch.
If either append fails, it sends no allowed answer and starts no effect.

### ACP option mapping

ADR 0022 remains the authority boundary for
`session/request_permission`. ACP `allow_once` maps to the existing one-shot
allow answer. ACP `reject_once` maps to the existing one-shot deny answer.

ACP `allow_always` maps to `Allow for this thread`. After full runtime
validation, it appends a `thread.permission.policy.decided` allow event for
the exact match tuple. ACP `reject_always` appends the same event with a deny
decision. The word `always` never extends beyond the bound Muniment thread.

The adapter submits the selected option ID and matching gate identity. It does
not append policy or resolve the gate. If the request kind lacks an exact
resource contract, either `_always` answer fails closed as unsupported. An
unknown, cancelled, duplicate, expired, contradictory, or stale ACP answer
creates no policy and grants no authority.

### Implementation slices

The first implementation slice is **thread permission policy foundation**.
It adds the two event codecs, validation, append APIs, rebuildable projection,
and coordinate-loop auto-resolution. It adds contract tests for exact matches,
near misses, owner always-ask, revocation, replay, stale answers, concurrent
appends, and partial batch failure. It changes no ask-card controls.

The following **ask-card thread answer** slice adds `Allow for this thread` to
the native card and sends that typed answer through `chat_answer_permission`.
It also adds the thread permission-settings surface for review and revocation.
A later ACP slice maps the two `_always` options after its gate bridge exists.

## Alternatives considered

**Add a mutable policy table.** Lookup would be direct, but the table would
become new authority. It violates ADR 0002 and the standing migration
exclusion.

**Add policy columns to a thread row.** This overwrites history, cannot model
several exact resources, and requires a forbidden column migration.

**Store policy in a run event.** A thread spans several runs. Selecting one
run as authority makes lifetime and ordering depend on unrelated run history.

**Keep policy in process memory.** Restart, reconnect, and concurrent clients
would lose or disagree about authority. Receipts could not prove the answer.

**Match a path prefix, executable, tool group, or normalized command text.**
Each choice grants more than the user saw. Exact typed resources provide the
conservative floor for a first release.

**Let ACP `allow_always` mean an account-wide grant.** ACP supplies an answer,
not Muniment authority. Account-wide scope would exceed the bound gate and
the desktop's per-thread promise.

## Consequences

- Durable permission answers and revocations remain auditable thread events.
- Exact matching limits reuse and may show more gates than a broader policy.
- Every automatic answer remains visible in run replay and receipts.
- Current authority and owner policy can override an older allow decision.
- Rebuildable projections make lookup fast without creating new authority.
- This decision adds no schema object, dependency, or runtime code.
