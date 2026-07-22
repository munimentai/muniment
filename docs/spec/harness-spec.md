# Muniment — Product & Engineering Spec (Harness)

**Status:** Draft v2 for dev team handoff (v2: hosted-SaaS stance synced)
**Author:** Mikey Pruitt
**Companions:** 01–05 design docs · monetization-and-marketing.md
**Date:** July 2026

---

## 1. Product thesis

Every existing tool falls in one of two camps. Desktop-native multi-model clients (Cherry Studio, Jan, Msty, OpenCode Desktop) are single-user with zero org governance. Org-governed platforms (Open WebUI, LibreChat) are web apps with no desktop presence, no local agentic execution, and no local models.

We build the thing in the gap: a desktop-native AI work app with one powerful mode, backed by an org control plane that handles identity, group-based capability entitlements, automatic model routing, MCP connection management, scheduled workflows, and a shared library of capabilities and artifacts.

Muniment is a **hosted SaaS**: we operate the control plane and gateway as a multi-tenant cloud; customers run nothing. The desktop app is a thin client to our cloud. **Closed source, commercial.** Pricing: one plan — $15/seat/mo billed annually, $20/seat/mo billed monthly, 10-seat minimum, 30-day full-product trial. Customers bring their own provider keys or model endpoints; muniment never marks up inference. No partner/reseller program. Self-hosting is not offered publicly; a self-hosted enterprise deployment is held in reserve as an unadvertised sales card only, so the architecture must remain deployable by compose/K8s even though we never say so.

### Non-goals (v1)

- **No serverless/solo mode.** The desktop app requires a control plane connection. Accepted tradeoffs: no offline use, no bottom-up solo-dev adoption funnel. Benefit: policy is always enforced, no provider API keys on laptops, one source of truth.
- **No hosted MCP servers.** We manage MCP *connections* only. Hosting/governance of remote MCPs is delegated to services like MintMCP.
- **No multiple UI modes.** One mode. No chat/cowork/code split.
  (Clarified 2026-07-12 — §13 adds the CLI and editor extension as
  governed companion windows onto the same single mode; the non-goal
  stands: no mode switcher, no second mode.)
- **Mobile is a companion surface, never a peer execution surface.** (Amended 2026-07-10 — was "No mobile client".) A phased mobile companion app is in scope: see §12. Phones never run local models, local MCP servers, or sandboxes; the desktop remains the only local-execution surface.
- **No promise of laptop-grade sandbox isolation on Windows** (see 6.5).

---

## 2. Architecture overview

Two components, one shared harness.

```
┌──────────────────────────────┐        ┌───────────────────────────────────┐
│  DESKTOP CLIENT (Tauri v2)   │        │  CONTROL PLANE (muniment cloud)   │
│                              │        │                                   │
│  UI (webview, single mode)   │◄──WS──►│  API + admin web app              │
│  Pi sidecar (RPC/stdio)      │        │  better-auth (OIDC) + SCIM 2.0    │
│  llama.cpp sidecar:          │        │  Postgres (entitlements, registry,│
│    - Gemma quant (polish +   │        │            artifacts, audit)      │
│      routing classifier)     │  HTTPS │  LiteLLM gateway (models, keys,   │
│  sherpa-onnx: Parakeet ASR   │───────►│    budgets, routing)              │
│  Kokoro TTS                  │        │  Flue runtime (scheduled          │
│  Local MCP clients (stdio)   │        │    workflows, durable sessions)   │
│  Remote MCP clients (via CP  │        │  Capability registry             │
│    connection registry)      │        │  Object storage (files/artifacts) │
└──────────────────────────────┘        └───────────────────────────────────┘
                                                     │
                                          Providers: Anthropic, OpenAI,
                                          Google, OpenRouter, org-hosted
                                          open-weight endpoints (vLLM etc.)
```

**Why Pi + Flue and not a fork of OpenCode:** Pi (MIT) is designed for embedding, with RPC over stdin/stdout and an SDK. Flue (Apache 2.0, Astro team) is built on Pi, so skills, tools, sessions, and sandbox semantics are identical on the laptop and on the server. One skill format org-wide. OpenCode (MIT) is reference material for its permission config and client/server split, not a fork base: everything that differentiates this product (multi-user, orgs, groups, IdP, routing policy, shared libraries) is absent from OpenCode, and forking a repo at that velocity means permanent rebase pain in exactly the core files we would modify.

### License inventory

| Layer | Choice | License | Notes |
|---|---|---|---|
| Agent engine | Pi | MIT | Sidecar via RPC mode |
| Server agent runtime | Flue | Apache 2.0 | Durable execution, sandboxes, MCP |
| Desktop shell | Tauri v2 | MIT/Apache 2.0 | Capability model doubles as FS enforcement |
| Model gateway | LiteLLM | MIT | Virtual keys, budgets, routing |
| Auth | better-auth | MIT | OIDC; SCIM endpoint is custom |
| ASR | Parakeet-TDT 0.6B v3 INT8 (sherpa-onnx v1.13.2) | CC-BY-4.0 model; Apache-2.0 runtime | Offline, CPU-only desktop; exact artifacts in ADR 0004 |
| Polish/classifier | Gemma (small quant) | Gemma Terms of Use | Commercial use permitted; review terms before sale |
| TTS | Kokoro v1.0 INT8 (ONNX Runtime v1.20.1) | Apache 2.0 model; MIT runtime | Offline, CPU-only English read-aloud; exact foundation and validation gates in ADR 0015 |
| Router bootstrap | RouteLLM pretrained | Apache 2.0 | mf / sw_ranking routers |

**Do not use as code base layers:** Open WebUI (custom license, branding restrictions), Cherry Studio and Paseo (AGPL). All three are fine as reference reading.

---

## 3. Roles and identity

Three roles, org-scoped: `user`, `admin`, `owner`.

- **User:** consumes what their groups grant. Chat, agentic runs, entitled models, capabilities, and artifacts. Connections and workflows appear only through the capabilities that bind them.
- **Admin:** manages people, groups, artifacts, capabilities (review/publish), and workflow oversight within each containing capability.
- **Owner:** everything admin has, plus routing policy, provider credentials, budgets, org security posture (sandbox policy, local model policy, local stdio MCP allowlist).

### 3.1 Auth

- better-auth with OIDC for IdP login (Okta, Entra, Google Workspace) plus local email/password for orgs without an IdP.
- Sessions are short-lived tokens issued to the desktop client. No provider API keys ever reach the client.

### 3.2 SCIM 2.0 (v1, not deferred)

Custom SCIM 2.0 server endpoint on the control plane. Scope: `/Users` and `/Groups` CRUD + PATCH, bearer-token auth per IdP integration. IdP-sourced groups land in `groups` with `source='idp'` and members sync into `user_groups` with `source='scim'`. Manual group edits on IdP-sourced groups are rejected (mirrors Open WebUI's lockout-avoidance lesson: org owners are exempt from deprovisioning via SCIM alone; deactivation of an owner requires a second owner or break-glass procedure, log it either way).

SCIM is a small, well-specified REST surface. Build it in-house; do not take a WorkOS dependency in a product we may sell.

---

## 4. Entitlements

Single polymorphic grants table. Explicit deny exists and wins. Server is the only enforcer; the client receives a signed snapshot purely for UX (hiding controls). `org_id` on everything.

### 4.1 Schema

> **§11 anchor:** `resource_type='capability'` is reserved for the capability manifest as the end-user grant target. The pre-§11 `packages` / `package_versions` rows below describe the existing implementation baseline; the capability schema and migration are follow-up work, not part of this codification.

```sql
-- principals
users(id, org_id, email, idp_subject, status, entitlement_version)
groups(id, org_id, name, source enum('manual','idp'), idp_group_id)
user_groups(user_id, group_id, source enum('manual','scim'))
org_members(user_id, org_id, role enum('user','admin','owner'))

-- resources (registries, not payloads)
models(id, org_id, litellm_model_name, display_name, tier, enabled)
mcp_connections(id, org_id, name, url, auth_ref, transport, enabled)
packages(id, org_id, name, kind enum('skill','plugin','extension','prompt'),
         source, signed_by, review_status)
package_versions(id, package_id, semver, manifest_hash)
artifacts(id, org_id, owner_user_id, title, current_version_id)
artifact_versions(id, artifact_id, storage_ref, created_by)
workflows(id, org_id, owner_user_id, flue_ref, schedule_cron,
          run_as enum('owner_identity','service'))
files(id, org_id, owner_user_id, storage_ref, mime, sha256)

-- the core
grants(
  id, org_id,
  principal_type enum('user','group','org'),
  principal_id,                       -- null when principal_type='org'
  resource_type enum('model','mcp_connection','package','artifact',
                     'workflow','file','capability'),
  resource_id,                        -- null = wildcard for that type
  action enum('use','read','edit','manage','publish','install','run'),
  effect enum('allow','deny'),
  granted_by, created_at, expires_at
)

-- money
budget_policies(id, org_id, principal_type, principal_id,
                period enum('daily','monthly'), limit_usd,
                scope enum('all','tier'), tier)

-- receipts
audit_log(id, org_id, actor_user_id, action, resource_type,
          resource_id, decision, request_meta jsonb, created_at)  -- append-only
```

### 4.2 Capabilities

Grants with `resource_type='capability'`, key in `resource_id`. Initial set:

`local_models.use`, `voice.cloud_cleanup`, `sandbox.full_auto`, `mcp.local_stdio`, `artifacts.publish_org`, `router.override` (user may manually pick a model instead of the router's choice), `workflows.create`, `packages.submit`.

> **§11 anchor:** this pre-§11 list is a set of dotted permission flags, not capabilities in the product vocabulary. Follow-up schema work must remove that naming collision while preserving the capability grant target defined in §11.2.

### 4.3 Resolution algorithm

For (user, resource, action): collect matching grants across user / user's groups / org, including wildcards.

1. Any user-level row matching: it decides (deny beats allow within the level).
2. Else any group-level deny: **deny**.
3. Else any group-level allow: allow.
4. Else org default row: apply.
5. Else: **deny**.

Wildcards make defaults cheap: org-level `allow use model:*` plus group-level `deny use model:<frontier>` is two rows.

### 4.4 Snapshot and propagation

Materialize per-user snapshots at login and on any relevant change; bump `users.entitlement_version`; push refresh to connected desktops over the existing websocket. Snapshots are signed; clients treat them as display hints only.

### 4.5 Gateway coupling (critical)

On any change to a user's model set or budget, regenerate their LiteLLM virtual key with the allowed model list and budget baked in. A compromised client cannot call outside its entitlements because the gateway itself refuses.

### 4.6 Workflow coupling (critical)

Scheduled Flue runs resolve entitlements against the owning user **at run time**, not schedule time. Revoking a user's MCP access kills their nightly job's access on the next run.

---

## 5. Model routing

### 5.1 Flow

1. User submits a prompt in the client.
2. Resident local Gemma classifies: task type (code-plan, code-edit, general, extraction, vision, long-context, etc.) and difficulty tier. **The local model classifies, it never routes.**
3. Label rides as request metadata to the LiteLLM gateway.
4. Gateway maps label → model per owner-defined policy (e.g. `code-plan/high → glm-5.2 via OpenRouter`, `vision → gemma-4 org endpoint`, `general/low → haiku-class`). Owner policy is authoritative; the gateway may ignore or re-derive the label.
5. Users with `router.override` may pin a model; the pin is honored only within their entitled model list.

### 5.2 Bootstrap and training pipeline (v1, not deferred)

Cold-start reality: no traffic means no trained router on day one. v1 ships the full pipeline:

- Day one routing = heuristic label→model policy + RouteLLM pretrained routers (`mf`, `sw_ranking`) as a second opinion where useful.
- Log from message one: prompt features (not raw prompts where policy forbids), classifier label, gateway decision, chosen model, latency, cost, and outcome signals (regeneration, model-switch retry, thumbs, task completion where detectable).
- Eval harness: replay logged traffic against candidate routing policies offline.
- Once volume justifies it, train the router on our traffic and hot-swap weights. Training becomes a data job, not a code project.

### 5.3 Abuse and edge cases

- Label spoofing (client forces "hard" to reach the expensive model): budgets are the backstop; add server-side sampled re-classification (~1-5% of traffic) for audit drift detection.
- Local models (Ollama/llama.cpp on the user's machine): unreachable by the gateway. Client-side provider exception, gated by `local_models.use`, with usage telemetry still reported to the control plane. Owner can disable org-wide.

---

## 6. Desktop client

Tauri v2. Frontend framework: team's choice (React or Svelte), Tailwind fine. Tauri capability model is part of the security design, not just packaging.

### 6.1 Single mode UI

One conversation surface that scales from chat to full agentic runs. No mode switcher. The Pi engine's tool activity renders inline (tool calls, diffs, command output) with collapse/expand. Artifacts render in a side panel. Global hotkeys: new conversation, voice dictation (hold-to-talk and toggle), quick-switch conversations.

### 6.2 Pi sidecar

- Client spawns Pi in RPC mode (JSON over stdio) as a managed child process.
- Client supplies: system context, entitled skills/extensions (installed from the registry), MCP tool surface, model = the user's LiteLLM virtual endpoint.
- Steering and follow-up mid-run (Pi supports steer vs queued follow-up; expose both).
- Session events stream to the UI and, in summary form, to the control plane for continuity and audit.

### 6.3 Local model sidecar

- llama.cpp server managed by the app, loading one resident small Gemma quant (3-4 GB class). Serves two roles without reload: dictation polish and routing classification.
- Health-managed: crash restart, version pinning from the control plane, owner can pin the model build org-wide.

### 6.4 MCP

- **Remote connections:** pulled from the control plane connection registry (section 8), filtered by entitlements. OAuth handled centrally; the client receives short-lived tokens.
- **Local stdio servers** (filesystem, browser automation): allowed only if `mcp.local_stdio` is granted AND the server binary/command matches the owner allowlist. Owner kill switch disables all local stdio org-wide.

### 6.5 Filesystem access and sandboxing (honest version)

v1 default is **permission gates, not containers**:

- Workspace-scoped file access; the workspace root is chosen per conversation/project and enforced by both the Tauri capability scope and a Pi permission-gate extension.
- ask/allow/deny prompts on commands and out-of-scope paths, with per-group owner policy able to force stricter modes (e.g. contractors always-ask).

Full-auto mode (no prompts) is opt-in and requires `sandbox.full_auto` plus isolation:

- Linux: bubblewrap.
- macOS: Seatbelt (`sandbox-exec` profile).
- Windows: no credible native equivalent; full-auto requires WSL2 or routes the run to a server-side Flue sandbox instead. Do not promise laptop isolation on Windows.
- Flue's `just-bash` virtual sandbox is available for runs that don't need the real filesystem.

### 6.6 Attachments

- Images and PDFs: sent to multimodal models directly (respect per-model size/page caps from the model registry).
- docx/xlsx/csv/text: extracted client-side before send.
- Every attachment is content-addressed (sha256) and stored via the control plane file store so attachments referenced by shared artifacts and scheduled workflows resolve under the same entitlements.

### 6.7 Voice (all on-device)

Zero voice bytes leave the machine. This is a selling point; keep it true.

- **Capture (ASR):** the immutable Parakeet-TDT 0.6B v3 INT8 conversion via sherpa-onnx v1.13.2, pinned in ADR 0004. The desktop core owns 16 kHz mono PCM and in-process CPU inference; recognition is utterance-final/offline (VAD chunking would be simulated streaming), with no CUDA requirement and no ASR network boundary. Latency, memory, and quality remain gated on ADR 0004's target-hardware matrix. Eval alternative: Qwen3-ASR (verify open weights + license + CPU latency). whisper.cpp is fallback only (too slow for live dictation UX).
- **Polish:** two-stage Eloquent pattern. Stage 1 verbatim live transcript; stage 2 on pause, the resident Gemma strips fillers, applies mid-sentence self-corrections, and offers transforms (key points / formal / short / long). Custom vocabulary per user (org jargon, names) stored locally, optionally seeded from the control plane org dictionary.
- **Output (TTS):** Kokoro v1.0 INT8 through ONNX Runtime v1.20.1, pinned in [ADR 0015](../decisions/0015-kokoro-read-aloud-runtime.md). Offline, CPU-only English read-aloud for responses and artifacts; performance and quality remain gated on that ADR's target-hardware matrix. OS voices remain a separately labelled zero-effort fallback only. Qwen3-TTS is off the list under the on-device constraint (too heavy per laptop).
- Hold-to-talk and toggle modes on a global hotkey; hold-to-talk is the default (clean capture boundaries).

### 6.8 Browser control (governed runtime capability — owner decision 2026-07-13)

The runtime exposes an actuator that drives the user's real browser — active control (navigate, read, click, type, extract) in the browser and profile where the user already works and is signed in. This is not a chat sidebar; it is the AI acting in the browser under policy.

Mechanism: a muniment browser extension (MV3 service worker using
`chrome.debugger`) pairs with the desktop runtime over a local loopback relay;
the desktop app owns the relay and pairing. This extension mode is the **only**
v1 path into a user's real/default profile. External CDP attachment (including
`--remote-debugging-port` or `--remote-debugging-pipe`) to that profile is
prohibited, not a fallback: Chromium 136+ ignores those switches for the
default data directory (the [Chrome security
notice](https://developer.chrome.com/blog/remote-debugging-port) is the
upstream constraint). A custom automation profile or Chrome for Testing may
remain useful for tests, but neither is a substitute for controlling the real
profile where the user is signed in.

**Relay pairing (fail closed):** for each pairing attempt the desktop issues a
cryptographically random token of at least 256 bits and establishes the
expected canonical browser executable path, which remains desktop-side rather
than being accepted back as a client assertion. The desktop relay independently
identifies the OS process that owns the accepted loopback connection and
obtains that process's executable through the platform process/socket APIs;
this OS-derived identity, not an extension- or browser-supplied path claim, is
the actual path used in the comparison. The platform adapter passes the process
handle plus process start identity and resolved executable identity directly
to the relay, which rejects a PID-reuse or connection-owner change before
authorization.

Both the expected and actual paths use the same platform canonicalizer before
comparison. On Linux, resolve the proc-owned executable and every symlink with
`realpath` and compare the resulting absolute path byte-for-byte. On macOS,
resolve Finder aliases/application bundles to their executable, resolve
symlinks with `realpath`, and compare the filesystem's canonical absolute
paths using that volume's case semantics. On Windows, open the executable,
resolve junctions and symlinks, obtain its final path and volume/file identity,
strip the `\\?\` spelling distinction, normalize separators, and compare path
case-insensitively plus exact volume serial/file ID. Failure to map the socket
to one live owning process, open or canonicalize either executable, or obtain
the required file identity fails closed. An extension-provided value or echo
of the desktop-issued expected path is never proof of executable identity.

The extension/relay session is authorized only when the token matches and the
independently observed executable identity matches the desktop's expected
canonical executable; missing, stale, malformed, or contradictory values close
the attempt without attaching a tab. The token is single-use, expires after
two minutes, and is rotated on every attempt, successful reconnect, relay
restart, desktop lock or sign-out, and kill-switch transition. The relay binds
only numeric loopback
(`127.0.0.1` and/or `::1`), never wildcard, hostname, LAN, or externally
reachable interfaces. The token exists only in desktop and extension memory:
it is never placed in `chrome.storage` (local, sync, session, or managed), logs,
URLs, or persisted diagnostics. Errors expose a closed reason code and redact
the token and executable path.

**Connect-tab anchor:** the desktop creates the pairing request, then the
extension creates and owns one ordinary, visible browser tab for that request.
Its fixed Muniment page says “Muniment browser control is connected. Keep this
tab open; closing it disconnects browser control.” The tab is the sole lifetime
anchor for one extension/relay session; it is not a hidden background page and
the desktop does not adopt or close a pre-existing user tab. A transport drop
leaves the anchor visible with “Browser control disconnected. Reconnect from
the Muniment desktop app.” Reconnect is always desktop-initiated, creates a new
single-use token, revalidates the expected executable path, and reuses the
anchor only if the same extension instance still owns it; it never silently
reattaches or resumes tool work.

Closing the anchor tab immediately detaches every `chrome.debugger` target,
revokes the token/session, cancels pending browser actions, and closes the
relay connection. Extension reload/unload performs the same detach/revocation;
its lifecycle handler also closes its owned anchor when possible. Because an
ordinary tab can survive extension teardown, the anchor page independently
watches the relay heartbeat and, on loss, durably replaces the connected copy
with “Muniment browser control is disconnected. Reconnect from the Muniment
desktop app.” It must not show connected again without a new authorized
pairing. After reload there is no session until a new desktop-initiated pairing
first discovers recorded/tagged Muniment anchors, closes any stale anchors (or
confirms their durable disconnected state before safely replacing them), and
creates exactly one new owned anchor. Thus every live session has exactly one
anchor, and no stale or surviving tab claims a connection after its session
ends. Relay death deterministically detaches targets and marks the
anchor disconnected; relay restart does not restore them. The desktop kill
switch first blocks new actions, then cancels pending actions, commands detach,
revokes/rotates pairing material, closes the relay, and closes the
extension-owned anchor when reachable. Cleanup is idempotent, and failure to
confirm any required detach keeps browser control disabled and surfaces a
redacted needs-attention state rather than assuming cleanup succeeded.

**Taxonomy (clarifies §1/§12/§13):** *Execution surfaces* — the desktop app, CLI, and editor extension — are each a full place a human does governed AI work; they are peers, and each drives the SAME local runtime. *Runtime capabilities* — local models, MCP tools, sandbox, virtual-key model access, and now browser control — are actuators the runtime exposes to whichever surface is driving. Browser control is a runtime capability, not a surface: a user never "sits in" it. Mobile is a companion surface with no local runtime and therefore can never invoke browser control (§12's existing rule already covers this).

**Governance (existing gates, applied — nothing new invented):** every browser action is a governed runtime tool-call. Entitlement-gated (a browser-control grant). Permission-gated under the same ask/allow/deny model as 6.5: per-domain allowlist, read vs. act distinction, explicit approval on authenticated/sensitive domains. Every action lands in the run journal and its receipts like any other tool call (§11.3). Owner kill switch org-wide, same as local stdio MCP (6.4). Fleet distribution of the extension is admin managed-install via enterprise browser policy, not the public web store.

Implementation base: a thin, closed-source-compatible fork of the Apache-2.0
Playwright extension/CDP-relay core, kept upstream-mergeable (attribution and
NOTICE are mandatory; changed files marked; no Microsoft marks). v1 remains in
this repository. Developer-mode/unpacked extension loading is sufficient for
v1; managed enterprise installation remains the fleet-distribution direction,
not a prerequisite for implementation.

**Non-goal (the boundary):** the browser only. No OS control, no filesystem access via the browser, no control of other applications. Muniment takes the browser slice of computer control because that is where governed knowledge work happens; general computer use stays out of scope. Required capability, not the frontier bet.

---

## 7. Control plane

Operated by us as a multi-tenant cloud (Docker Compose for dev, K8s in production; the stack stays compose-deployable to preserve the unadvertised enterprise self-host card). Components: API service, admin web app, Postgres, LiteLLM, Flue runtime, object storage (S3-compatible), Redis (websocket fanout, LiteLLM rate limiting). Per-org isolation enforced at the schema (org_id everywhere), the gateway (per-user virtual keys), and object-storage prefixes; noisy-neighbor controls via the budget system. Customer org endpoints (their own vLLM/TGI) are reachable via IP allowlist or the muniment outbound connector agent — a roadmap item the gateway design must accommodate.

### 7.1 Admin web app (web-only is fine)

- **Owner:** provider credentials, model registry + tiers, routing policy editor (label→model matrix with per-group overrides), budgets, org security posture (sandbox policy, local model policy, local stdio allowlist), audit log explorer.
- **Admin:** users/groups (manual + IdP-sourced read-only), grants editor with effective-permissions preview ("what can this user touch and why"), capability review queue (signed, versioned manifests and their subunits), workflow oversight within the containing capability, artifact library moderation.
- Effective-permissions preview is not optional polish; it is the debugging tool for every entitlement support ticket.

### 7.2 LiteLLM gateway

- All cloud and org-hosted model traffic flows through it. Org-hosted open-weight endpoints (vLLM/TGI on rented or owned GPU) register as custom OpenAI-compatible providers.
- Virtual key per user, regenerated on entitlement/budget change (4.5).
- Budget enforcement per `budget_policies`; spend telemetry feeds the owner dashboard and the routing log.

### 7.3 Flue runtime (scheduled workflows, v1)

- Capability workflow subunits are Flue agents/procedures referencing skills in the same reviewed manifest. Triggers: cron, webhook, manual.
- `run_as='owner_identity'`: the run uses the owner's virtual key and entitlements, resolved at run time (4.6). `run_as='service'`: explicit service principal with its own grants, owner-approved.
- Durable streams give crash recovery for long runs; interrupted work resumes on runtime restart.
- Results delivered to a desktop inbox (websocket push + notification) and stored as artifacts/files under the owner's entitlements.

### 7.4 Capability registry / marketplace (v1)

> **§11 anchor:** "package" is retired as a product noun. This section describes the Pi distribution/installation substrate for capabilities; users and admins grant and list capabilities, never package kinds or subunits independently. Follow-up implementation work will align the registry schema and APIs.

- Distribution format: Pi packages (installable from npm/git). Do not invent a format. This is internal substrate terminology only: each distribution carries one capability manifest whose reviewed subunits are skills (including former prompts), extensions, connections, workflows, and model requirements. Server-side skills for Flue use the same skill format.
- Registry service: capability submission and admin review, signing (manifest hash + org signing key), versioning, org allowlist, and capability entitlements. `packages.submit`, `package_versions`, and `install`/`use` package grants are pre-§11 API/schema baseline names pending the follow-up migration; they are not product UI vocabulary.
- Desktop installs only entitled, signed capability versions. Capability grants track a channel under the strict pin / gated / pure `:latest` policy in §11.2; the existing package-level and `package_versions` grant mechanics are implementation baseline only.

### 7.5 MCP connection registry

- Records: name, remote URL, transport, auth config reference (OAuth client credentials held server-side, secrets in a vault/KMS), enabled flag.
- Group assignment via grants. MintMCP (or similar) endpoints are just entries here; their internal governance is theirs, ours is which groups see the connection at all.

### 7.6 Artifact library

- Versioned artifacts, group sharing via grants (`read`/`edit`), org-wide publish gated by `artifacts.publish_org` + admin review option.
- Rendered in the desktop side panel; storage in object store, metadata in Postgres.

### 7.7 Audit

Append-only `audit_log` for every privileged decision: entitlement checks that deny, grant changes, key regenerations, capability publishes, workflow runs, owner policy changes, local-stdio MCP invocations. Export stream to SIEM (stdout JSON lines is enough for v1).

---

## 8. Security summary

- No provider API keys on laptops; short-lived session tokens + scoped LiteLLM virtual keys only.
- Deny-wins entitlements; server-side enforcement; client snapshots are display hints.
- Gateway refuses out-of-entitlement model calls even from a compromised client.
- Voice fully on-device.
- Sandbox honesty: permission gates by default, real isolation opt-in, Windows full-auto punts to WSL2 or server-side sandbox.
- Label spoofing mitigated by budgets + sampled server-side re-classification.
- Local stdio MCP servers allowlisted and owner-killable.
- Capability supply chain: review + signing of the versioned manifest before any group can install it; Pi packages are the distribution substrate.
- Owner deprovisioning cannot happen via SCIM alone.

---

## 9. Build order (dependency edges)

Phases are dependency layers, not sprints. Within a phase, tracks run in parallel.

**Phase 0 — Foundations (everything depends on this)**
1. Postgres schema (section 4.1) + migrations. Blocks: all.
2. Control plane API skeleton + websocket. Blocks: client, admin app.
3. LiteLLM deployment with 2-3 providers + virtual key issuance wired to a stub user. Blocks: routing, client chat.

**Phase 1 — Identity and enforcement**
4. better-auth OIDC login + local auth. Depends: 2.
5. Grants CRUD + resolution engine + snapshot/versioning. Depends: 1, 2.
6. Virtual key regeneration on entitlement change. Depends: 3, 5.
7. SCIM 2.0 endpoint (Users/Groups). Depends: 4, 5. (Parallel with 6.)

**Phase 2 — Client core**
8. Tauri shell + auth handshake + entitlement snapshot consumption. Depends: 4, 5.
9. Pi sidecar integration (RPC), chat against virtual key. Depends: 6, 8.
10. Local model sidecar (llama.cpp + Gemma). Depends: 8. (Parallel with 9.)
11. Attachments pipeline + file store. Depends: 8; storage from Phase 0 infra.

**Phase 3 — Routing and voice**
12. Classifier prompt/finetune on Gemma + label metadata. Depends: 9, 10.
13. Gateway label→model policy + owner policy editor. Depends: 6, 12.
14. Routing log + eval harness + RouteLLM bootstrap. Depends: 13.
15. Voice: Parakeet capture → Gemma polish → insert; Kokoro read-aloud; hotkeys. Depends: 10. (Parallel with 12-14.)

**Phase 4 — Org surface**
16. MCP connection registry + client remote MCP consumption. Depends: 5, 9.
17. Local stdio MCP allowlist + kill switch. Depends: 16.
18. Capability registry + signing + client install flow (Pi package distribution substrate). Depends: 5, 9.
19. Artifact library + sharing + side-panel rendering. Depends: 5, 11.
20. Admin web app consolidation (grants editor, effective-permissions preview, review queues). Depends: 5, 13, 16, 18.

**Phase 5 — Autonomy**
21. Flue runtime deployment + first capability workflow + run-time entitlement resolution + inbox delivery. Depends: 5, 6, 18 (skills from the capability manifest).
22. Sandbox modes (permission gates already in 9; bubblewrap/Seatbelt full-auto; server-side sandbox fallback). Depends: 9, 21.
23. Sampled re-classification audit + trained-router swap path. Depends: 14 (and traffic).

---

## 10. Open items and eval tasks

| Item | Action |
|---|---|
| Qwen3-ASR | Verify open weights, license, CPU latency vs Parakeet |
| Kokoro validation | Run ADR 0015's CPU latency, real-time-factor, peak-memory, cancellation, and English ear-test matrix before claiming the read-aloud UX bar |
| Gemma terms | Legal read of Gemma Terms of Use before commercial sale |
| Parakeet validation | Run ADR 0004's CPU latency, real-time-factor, peak-memory, and multilingual quality matrix before claiming the live-dictation UX bar |
| Classifier taxonomy | Define the label set (task types x difficulty tiers) before Phase 3 |
| Capability channel grants | Replace the package-level/per-version baseline with strict pin / gated / pure `:latest` capability grants in the follow-up migration |
| MCP proxy long-term | Customers may bring gateway services (e.g. MintMCP); decide later whether to build a native group-filtering proxy |
| Windows full-auto | WSL2 detection UX vs server-side-only stance |
| Naming/branding | Muniment (muniment.ai) — decided; trademark knockout pending |
| Design partner | 5–8 orgs from founder network per monetization-and-marketing.md Phase 0 |
| Org endpoint connectivity | Design the outbound connector agent (customer vLLM/TGI reachable from muniment cloud) |

---

## 11. The capability — canonical definition (harness-spec §11, owner decision 2026-07-10; add this VERBATIM as §11 of the vendored harness-spec in docs/spec/)

Muniment combines what the industry ships as four loose nouns — connectors, skills, plugins, workflows — into ONE governed primitive: **the capability**. PydanticAI v2 named the composition side (a capability "bundles an agent's instructions, tools, lifecycle hooks, and model settings into a single, composable unit" — pydantic.dev/articles/pydantic-ai-v2); muniment's capability is the same unit made *governable*: the thing an org reviews, grants, meters, and sees in receipts. Composition is theirs; entitlement is ours.

**§11.1 Anatomy (subunits):** a capability is a signed, versioned manifest over:

- **skills** — instruction content: prompts, procedures, bundled reference assets. The former "prompt" package kind COLLAPSES into skills; there is no separate prompt noun.
- **extensions** — code that executes: lifecycle hooks and local tools (in the spirit of Claude's filesystem/Chrome-control extensions). Most audit-sensitive subunit: hooks rewrite what the model sees, so they live INSIDE the reviewed unit.
- **connections** — bindings BY NAME to MCP-registry / stdio-allowlist entries. The registries are SUBSTRATE — they own endpoints and auth; a capability only references approved entries.
- **workflows** — Flue procedures (muniment's extension beyond the PydanticAI bundle).
- **model requirements** — declared needs (tier/effort/modality); declarations only, routing policy decides.

**"Package" is retired as a product noun.** The Pi package format survives as the distribution/installation format of a capability; the old package kinds become subunits, never granted or listed independently.

**§11.2 Grants and versioning:** the capability is THE end-user grant target (resource_type='capability', already reserved in §4.1). Versioning: grants track a channel, **gated on surface change** by default — every version declares its surface (connections bound, extensions/hooks present, model requirements, scopes); content-only updates flow automatically (:latest semantics); surface-EXPANDING updates park until re-approved, grantees stay on the last approved version meanwhile ("you granted a shape, not a snapshot — when the shape grows, we ask again"). Per-org policy knob: strict pin / gated (default) / pure :latest. Deferred loading is a governance feature: the one-line description shown before a capability loads is the description the admin approved.

**§11.3 Receipts:** receipts name the capabilities in the loop — route · model · cost · time · capability@version[, ...]. Emitted from day one of the capability schema (retrofitting provenance into an append-only log is a known trap).

For desktop runs, the append-only local SQLite run journal defined by desktop
ADR 0002 is authoritative. Receipts are rebuildable projections of its
versioned events, not independently maintained state. The contract is decided;
implementation remains Phase 2 work.

**§11.4 Vocabulary:** end users see "capabilities" by that name on every surface. Admin LIBRARY regroups to Capabilities + Artifacts. Copy law unaffected.

---

## 12. Mobile companion app (owner decision 2026-07-10)

Amends the §1 non-goal "No mobile client". Mobile enters scope as a **phased
companion client** — TestFlight/internal-track distribution first, selective
version shipping; launch timing and any public announcement remain owner-only.
03-mobile-app.md stays the design ground truth for the eight mockup screens;
its "mockup only — do not engineer" scope line is superseded by this section.

### 12.1 What mobile is (and is not)

Mobile is a governed window onto the same control plane: chat through the
org's entitled models, watch and steer serious agent work running on the
user's desktop, read artifacts, see receipts, peek entitlements. It is NOT a
fourth execution surface: no local models on the phone, no local stdio MCP
servers, no local sandbox, no artifact editing (full-screen viewer + "Edit on
desktop", per 03 §4). The no-keys-on-clients rule applies doubly: phones hold
only short-lived session tokens; all model traffic proxies through the
control plane.

### 12.2 Enabling primitives (control-plane + desktop work; each also serves desktop)

1. **Server-side thread store.** Threads/messages become first-class
   control-plane records (org_id-scoped, entitlement-checked). Already
   implied by shared threads (03 §3.6, share-to-project) and cross-device
   continuity; mobile just makes it non-optional. The desktop remains the
   execution surface and syncs its threads up; mobile reads and appends
   through the store. Design provenance in from day one (receipts §11.3 —
   retrofitting provenance is a known trap).
2. **Chat completion endpoint.** The control plane exposes a chat API that
   resolves the caller's LiteLLM virtual key **server-side** (§4.5), so
   entitlements and budgets enforce identically for clients that cannot hold
   a virtual endpoint. Routing labels (§5): mobile has no resident local
   classifier, so the label is produced server-side (heuristic tier first;
   sampled re-classification already exists as a pattern, §5.3).
3. **Session relay + remote approvals.** The desktop publishes full-fidelity
   Pi session events projected from its authoritative local run journal over
   its existing control-plane websocket (§6.2 today
   sends summaries); the control plane relays steer / queued follow-up /
   interrupt commands back down, and — the flagship — **ask/allow/deny
   permission gates (§6.5) can be answered from the phone**, receipt-visible.
   The relay works only while the desktop is online with a live run; the
   offline story is a server-side Flue session (§7.3), later (M3). Relay
   cursors and control-plane copies are consumers, not a competing history;
   journal-backed publication is not yet implemented.

### 12.3 Voice on mobile (owner decision 2026-07-10)

The desktop on-device voice stack (§6.7) does **not** port to phones and is
not required for mobile v1. When mobile voice is built it uses
**platform-native speech APIs** (iOS Speech framework / Android
SpeechRecognizer or system dictation) or a purpose-chosen mobile alternative
stack — decided by ADR at that phase. Copy honesty is law: the desktop claim
"voice never leaves this machine" is **desktop-scoped** and is never asserted
for mobile unless the chosen mobile stack actually guarantees it
(platform dictation may transit vendor cloud). 03 §3.3's "all on-device"
line is amended accordingly; the hold-to-talk interaction design stands.

### 12.4 Phases

- **M0 — companion read/chat:** OIDC login, thread list/view + chat (thread
  store + chat endpoint), inbox read-only, entitlement peek, push
  notifications. TestFlight/internal track only.
- **M1 — live session mirror:** watch a desktop run, steer / queued
  follow-up, answer permission gates remotely. Requires a ninth screen
  (live session view) — not in the current eight mockups; owner supplies the
  design before M1 engineering (mockups are ground truth; do not invent it).
- **M2 — voice input** per §12.3; artifact viewer polish.
- **M3 — cloud-sandbox runs from mobile:** start/steer server-side Flue
  sessions (§7.3); no new mobile-specific primitives.

### 12.5 Open items

| Item | Action |
|---|---|
| Repo strategy | ADR before M0: mobile targets inside the muniment-desktop Tauri v2 workspace (shared webview UI + Rust core) vs a separate repo + factory lane. |
| Stack | Default assumption Tauri v2 mobile (stable API; plugin surface thinner than desktop; tauri-action has no mobile automation yet). ADR may choose otherwise. |
| Mobile CI | iOS builds on the macOS CI template, Android SDK on the Linux template; extend the desktop-ci driver. Before M0 scaffolding. |
| Push notifications | APNs/FCM relay from control-plane events (workflow complete/failed, shared-thread mention, permission gate pending). Design at M0. |
| Apple developer account | Owner acquires; TestFlight first, selective version shipping. |

## 13. Companion work surfaces: CLI and editor extension (owner decision 2026-07-12)

Amends §1 the way §12 did for mobile. The non-goal "No multiple UI modes"
STANDS — these are not modes; they are additional governed windows onto the
same single mode, in the places work already happens.

### 13.1 What they are (and are not)

The **muniment CLI** (terminal) and the **muniment editor extension**
(VS Code and license-compatible forks; JetBrains later) are companion
surfaces over the SAME per-user runtime service. Like the desktop UI, they
attach to the service's managed Pi process and run journal over the local
IPC/attach protocol; they never spawn a second ungoverned runtime, never
hold provider keys, and never bypass the virtual-key path. Everything a
user does in them carries the same entitlements, permission gates,
receipts, and journal entries as the desktop surface. Any execution-surface
installer installs the shared per-user service if absent and upgrades an older
version that does not meet its declared minimum in place; a compatible
installed service is reused, so there is never a per-surface or second runtime.
The CLI is an interactive human
surface — headless/scripted agents remain the server-side agent access
layer's job, never the CLI's. Execution surfaces may invoke every
runtime capability, including browser control (§6.8); the browser
extension is an actuator of the runtime, not a companion surface, and is
outside this section's scope.

### 13.2 Enabling primitives

1. **Attach protocol:** local IPC exposing session open/stream/steer,
   permission-gate answers, and artifact fetch — a thin projection of the
   Pi RPC + run-journal surfaces the runtime service maintains. Accepted
   [ADR 0009](../decisions/0009-companion-attach-protocol.md) defines the
   transport, local-process authorization, and journal-backed crash semantics.
2. **Shared device session:** the runtime service holds one authenticated
   device session for all surfaces. Sign-in does not require the desktop: the
   CLI can initiate the existing §2.8 system-browser PKCE round-trip through
   the service, and every surface reuses the resulting session rather than
   storing tokens or creating a per-surface session.
3. **Entitlement snapshot reuse:** same signed snapshot, same
   display-hints-only rule (§4.4).

### 13.3 Phases

- **E0 — DONE:** accepted [attach-protocol ADR 0009](../decisions/0009-companion-attach-protocol.md).
  The first unblocked slice is pure-core protocol types, strict codecs,
  negotiation, authorization state, cursor/idempotency semantics, and tests;
  native listeners and companion UIs remain unimplemented.
- **E0.5 — DONE:** owner-ratified [ADR 0011](../decisions/0011-companion-surface-repo-strategy.md)
  on 2026-07-17 selects this repository for E1 and E2 and defines how each is
  built, tested, and released.
- **E1 — CLI (OPEN):** first scaffold the Rust workspace binary, its independent
  CI lane, and its ADR 0009 attach-protocol client; then implement threads
  list/open, chat/run with inline tool stream,
  permission gates answered in-terminal, receipts printout. These first slices
  may attach to the current app-managed owner because the protocol is
  identical; service extraction is a separate build line and does not block E1.
- **E2 — VS Code extension:** independently sequenced in its scoped TypeScript
  package lane, implement thread view + composer, editor-context
  attach (send selection/file within workspace scope), permission gates
  in-editor. Marketplace publishing is a public act — owner-gated like
  all launch/publicity.
- Distribution rides the existing channels (brew/winget once signed;
  marketplace at owner go).

### 13.4 Open items

| Item | Action |
|---|---|
| Fork coverage | Decide which VS Code forks (Cursor/Windsurf) are officially claimed vs expected-to-work. |
| JetBrains | Later; outside this amendment's build scope. |
| Marketplace publisher account | Owner acquires alongside launch prep. |

## 14. Remote Control (owner decision 2026-07-17)

Remote Control extends the M1 live-session view (§12.4) from mirroring and
remote approvals to **takeover**: an authenticated mobile user can send prompts
and commands to, stop, or queue a follow-up for a live local session, while
continuing to see the mirror banner, tool stream, permission-gate cards, and
receipts. The session may be owned by any execution surface — desktop, CLI
(E1), or editor extension (E2) (§13) — on Windows or macOS. Linux follows where
the shared runtime integration works without platform-specific scope.

### 14.1 Runtime and transport

The per-user runtime service defined by [ADR 0012](../decisions/0012-user-level-runtime-service.md)
remains the single local executor. Desktop, CLI, and editor-extension sessions
are peers over the E0 attach protocol defined by ADR 0009 and ADR 0011,
so Remote Control is **one runtime-to-cloud relay integration**, not a tunnel
implemented separately by each surface. The runtime opens an outbound-only
HTTPS session leg to `api.muniment.ai`; Remote Control requires no inbound port
or other inbound connectivity on the user's machine. Messages route through
the relay encrypted, and the relay does not receive filesystem access,
environment access, tool execution, or provider credentials. Filesystem,
environment, tool execution, and the authoritative run journal remain entirely
on the user's machine. Model inference continues through the existing governed
LiteLLM/model-routing path (§4.5 and §5), including the optional local-model
exception in §5.3.

The service owns that outbound leg independently of every surface lifecycle.
Remote Control therefore remains available for a live local session when no
desktop or editor window is open and no CLI process is attached.

Only one remote-controlled session may be active per local runtime at a time.
The runtime service must remain alive. If network connectivity is lost, the
runtime allows a bounded reconnection window of approximately ten minutes,
then ends the remote leg cleanly without ending or transferring execution of
the local session. A new remote client must pair again; stale clients cannot
resume control from relay state alone.

### 14.2 Entry points and session state

- CLI: a command that starts Remote Control and an in-session slash command.
- Editor extension: a command that starts Remote Control, an open-in-browser
  action, and a persistent **Remote controlled** banner while attached.
- Desktop: the equivalent session action and persistent banner while attached.
- Handoff: an expiring session URL and QR code that open the authenticated
  mobile app; cloud and mobile expose a list of eligible live sessions.
- Disconnect: whichever execution surface owns the session shows the persistent
  banner and a disconnect affordance. Mobile also exposes Stop and detach.

Session presence is runtime-authoritative. A relay or client must not present a
stale session as controllable after the runtime disconnects, the owner uses the
kill switch, entitlement is revoked, or the network-loss window expires.

### 14.3 Governance, identity, and receipts

Remote Control is gated by an explicit entitlement flag set per organization
and/or user. Session pairing is phishing-resistant: every QR code or URL is
short-lived, single-use, and bound to the intended session and the
authenticated organization user; possession of a handoff link alone never
grants attach authority. The runtime validates authorization before accepting
the attach and continuously enforces entitlement and policy for the remote leg.
An organization-owner kill switch disables new attaches and disconnects active
remote legs.

Remote-issued commands enter the same governed runtime path as commands from
the owning surface. They never bypass entitlements, workspace scope, policy, or
the §6.5 ask/allow/deny permission gates. Those gates are answerable from the
phone, and every answer is recorded in the run journal and projected into the
run receipt. The journal additionally records a receipt event for remote
attach, every remote-issued command (including its authenticated actor and
origin), and detach with its reason. Duplicate or replayed relay messages must
not produce a second command or approval.

This preserves the §6.8 taxonomy: mobile remains a companion drive surface,
not an execution surface or runtime capability. It cannot host or execute
browser control locally; a command issued from mobile may ask the local runtime
to invoke browser control or another capability, but the runtime does so only
through its ordinary entitlement, permission, journal, and receipt path.

### 14.4 Non-goals and failure boundaries

- Mobile never executes tools, models, MCP servers, browser control, or
  sandboxes locally; §12 continues to apply.
- Remote Control does not move execution into the relay or provide an offline
  local-session surrogate. Server-side Flue sessions remain the separate M3
  path (§12.4).
- Remote Control never bypasses runtime governance, creates per-surface
  tunnels, or requires inbound connectivity on the user's machine.
- Relay failure, mobile disconnect, or remote detach cannot silently approve a
  pending gate or interrupt the still-local session. Stop is an explicit,
  authenticated, journaled command.

## 15. Onboarding, memory, and Muniment Home (owner decision 2026-07-19)

Muniment gives users a visible, portable memory system and a surface-appropriate
first run. One required on-device model supports that onboarding and remains the
front door for routing every query after setup; folder setup itself never depends
on successful model acquisition.

### 15.1 Muniment Home and the memory model

**Muniment Home** is a user-visible directory containing `memory/`, `agents/`,
`projects/<name>/`, and `sessions/`. Its contents are plain, human-editable
Markdown files, and those files are the source of truth. Any SQLite or embedding
index is a disposable, rebuildable cache. The primary store is **never** a hidden
dot-directory, and there is no proprietary memory database.

Muniment does not build a sync service for Home. The directory rides the user's
existing filesystem sync when the user chooses one. Session transcripts use
semantic filenames containing the date and topic, not opaque identifiers, and
the product exposes a visible retention policy for them.

`AGENTS.md` remains the repository-instruction layer: honor the open standard and
resolve the nearest file for each path in a monorepo. Instructions govern how the
agent works in a repository; memory records user and project context. They are
distinct layers, and neither silently replaces or absorbs the other.

### 15.2 Per-surface defaults

- **Desktop:** first run has an explicit, unskippable directory picker, defaulted
  to `Documents/Muniment`. The location remains changeable later.
- **CLI and VS Code extension:** the directory in which the surface was opened
  **is** the workspace memory location by default, without asking again. Both
  surfaces expose a visible option to change it.
- The user-level Muniment Home is created lazily, on the first cross-project
  need; opening the CLI or extension in a repository does not eagerly create it.

### 15.3 Required on-device onboard and query-router model

The working pick is **Qwen3.5-4B Instruct, Q4 GGUF (Apache 2.0)**. It ships as a
pinned, checksummed descriptor through the verified runtime-acquisition path in
[ADR 0008](../decisions/0008-pi-runtime-distribution.md) and is served by the
`llama-server` distribution line in
[ADR 0014](../decisions/0014-llama-server-distribution.md). A descriptor bump can
swap the model without changing this contract. Artifact download uses a Muniment
proxy or redirect; this requirement has a corresponding MUNICLOUD implementation
ripple.

The model has two continuing roles: onboarding/import triage and the per-query
front-door router. For each query it selects a route class — **cloud model**,
**proxy**, or **local** — before the selected route's own governed model policy
and entitlements apply. Voice and audio models remain optional. The Qwen
onboard/router model is the only required on-device model artifact.

### 15.4 Consent-gated onboarding and import

Onboarding may detect candidate sources including `~/.claude`, `~/.codex`,
Cursor, Cline memory banks, and assistant data-export ZIPs. Detection leads to a
consent checklist with a preview. Muniment reads only sources the user approves;
it never performs a background disk walk.

Every imported memory carries provenance frontmatter with its source and import
date. The original text is **always** retained verbatim; summaries, rewrites, or
other transformations are additive and never replace it.

Triage produces a Markdown onboarding report containing a user-type
classification, the proposed Home layout, and two or three starter agents as
Markdown files proposed for `agents/`. The user confirms the report **before**
Muniment scaffolds Home or writes those agents.

### 15.5 Safeguards, boundaries, and open items

- Folder setup fails open. If the required model cannot download, onboarding
  degrades to manual setup and retries the download in the background; a failed
  download never bricks first run. Only features that depend on AI fail closed.
- The headless/server CLI stance remains open pending an owner ruling.
- Gemma 4 E2B is a tracked alternate pending llama.cpp PLE support
  ([ggml-org/llama.cpp issue 22243](https://github.com/ggml-org/llama.cpp/issues/22243)).
- The final model choice is gated on the MUNIQA routing evaluation, with
  [arXiv 2604.02367](https://arxiv.org/abs/2604.02367) as its reference.

## 16. Agent system prompt model (owner decision 2026-07-19)

Muniment's agent system prompt is a small, governed contract: a static,
hand-written base plus a runtime-generated per-session tail. The following five
rules are binding.

### 16.1 Binding rules

1. **Contracts, not taste.** The system prompt states harness facts,
   communication behavior, when to act versus ask, and governance behavior. It
   contains zero aesthetic or domain prescriptions; those belong in `AGENTS.md`,
   memory, or skills and load lazily when relevant.
2. **Nothing always-on that is not always true.** Every line must pass this
   admission test: it improves behavior in the **majority** of turns. No statement
   may be factually false.
3. **Static base plus generated tail.** A hand-written static base is followed by
   a runtime-**generated** per-session tail rendered as labeled data fields. The
   tail never uses prose to re-describe tools that already carry schemas.
4. **Hand-written, reviewed, versioned, and evaluated.** Prompt text is
   hand-written, read in full, versioned per model, and gated on an evaluation
   before any change ships.
5. **No negative fixation lists.** Fix causes upstream rather than accumulating
   lists of prohibited mistakes in the prompt.

### 16.2 Base prompt v0

The owner accepted the following base prompt v0 verbatim on 2026-07-19. The
static base should remain well under 500 tokens.

```text
You are an agent in Muniment, a governed AI workspace. You help the user
with their real work — writing, analysis, research, operations, and
software tasks.

# Harness
- Text you output outside tool calls is your message to the user. Tool
  activity may be visible to them, but never rely on it to communicate:
  anything the user needs from this turn must be in your text.
- Your capabilities are listed below this prompt, one line each. Full
  instructions load when you invoke one. Tools describe their own inputs.
- Every action you take is recorded in the workspace's audit log.

# Governance
- Your capabilities are granted by the user's organization. A denied
  action is a decision, not an obstacle: say what was denied and what
  granting it would enable, and never attempt to work around it.
- Never state that an action succeeded when it did not, and never act
  outside your granted capabilities.

# Working with the user
- When the user asks a question or is thinking out loud, answer it —
  briefly, with the main trade-offs — and stop. Do not start changing
  things until they ask.
- When they ask for reversible work within scope, do it without asking
  step-by-step permission. Confirm first for destructive or
  outward-facing actions (sending, publishing, deleting).
- Report outcomes plainly, including failures and partial results.

# Memory and files
- The user's durable context is visible markdown: the Home folder
  (memory/, agents/, projects/, sessions/) and, in repos, AGENTS.md and
  workspace memory. Read what is relevant before working; when you learn
  a durable fact worth keeping, save it there — never anywhere hidden.
- Prefer editing existing files to creating new ones. Match the
  conventions already around you.
```

### 16.3 Generated per-session tail

At runtime, Muniment produces the tail anew for each session. It renders these
items as labeled data fields:

- surface and OS
- workspace path
- Home path
- date
- organization name
- user role
- organization working-context blurb
- granted capabilities, one line each
- hard policy constraints
- memory index

The organization working-context blurb is **org-admin-editable** and
length-capped. It is the organization-scope sibling of user memory and
`AGENTS.md`: three scopes, one philosophy. All organization-authored fields are
length-capped and rendered as labeled data so administrator text cannot
masquerade as system instructions. Injected organization fields appear in the
audit trail.

### 16.4 Boundaries

- Everything beyond the listed tail fields is **pull** through capabilities —
  including the directory, full policy documents, and team structure — and is
  never pushed into the tail.
- Tool schemas remain authoritative; the prompt does not duplicate them in
  prose.
- Aesthetic preferences, domain guidance, and task-specific context remain in
  the lazily loaded `AGENTS.md`, memory, and skill layers.
