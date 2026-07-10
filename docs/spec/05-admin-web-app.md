# Muniment Admin Web App — Design Document

**Doc 5 of 5** · Tokens, laws, identity, components (incl. table grammar): see 01-design-system.md
**Audience:** admins and owners · **Aesthetic:** the ledger, fully expressed · **Density:** compact · **Status:** v1 · July 2026

---

## 1. Web-app stance (the platform has its say)

Real URLs for every view and every entity (`/access/effective/u_123`, `/models/routing`, `/records/audit?query=...`); browser back always works; filters serialize to the query string; tables are semantic `<table>`s; everything keyboard-navigable; no SPA transitions fancier than instant. Sessions ride the same OIDC as the product. This is where `oxide`/`ochre` live; signal appears in exactly two places (§6.3, §6.8).

## 2. Information architecture

Persistent left nav, grouped, mono section labels:

- **People:** Users · Groups
- **Access:** Grants · Effective permissions
- **Models:** Providers · Model registry · Routing policy · Budgets
- **Connections:** MCP registry · Local-stdio allowlist
- **Library:** Capabilities · Artifacts
- **Records:** Audit · Usage

Role gating: admins see People, Access, Library, and Records read-only; owners see everything plus §7. **Hidden, not disabled:** sections a role lacks do not render. Header: lockup, org name, environment tag (mono chip: `prod`), account menu.

## 3. Global patterns

- **Table grammar** per design-system §6 everywhere: 36px rows, hairlines, no zebra, mono identifiers/figures/timestamps, deny = strikethrough + oxide + the word.
- **Query bar** on every list: mono `key:value` syntax (`group:contractors effect:deny resource:model`) + clickable chips for the common cases; state in the URL.
- **Inline expansion** for row detail; entities with URLs navigate on the name.
- **Drafts and publish:** anything with blast radius (routing policy, allowlists, org posture) edits as a draft with a plain-English preview and a single versioned Publish.
- **Destructive confirms:** type the object's name; the confirm button states the consequence ("Revoke 3 grants across 2 groups").
- **Every mutation lands in Audit** with actor, before/after, and reason where required.

## 4. People

### 4.1 Users
Columns: name · email (mono) · role · groups (chips) · status · last active (mono). Row expand: IdP subject, entitlement version, virtual-key age, quick actions (view effective permissions — the primary action, deliberately first). SCIM-sourced users show a mono `scim` chip; manual edits to SCIM-owned fields are blocked with an explanatory line, not a silent failure.

### 4.2 Groups
Manual and IdP-sourced (`source` chip). IdP groups are read-only membership with "managed by <IdP>" line. Group page: members, grants held (a filtered view of the grants table), and "What this group can touch" — a group-level effective view.

## 5. Access

### 5.1 Grants
The raw ledger. Columns: principal (type chip + name) · resource (type + id or `*`) · action · effect (word) · granted by · created (mono) · expires (mono, ochre when < 7d). Create = one-row inline form: principal → resource (wildcard allowed) → action → effect → expiry. **Deny requires a reason** (lands in audit). Bulk operations only via filter-then-act with a full preview list.

### 5.2 Effective permissions (the flagship — gets the most design time)
The title search. Input: a user (or group). Output: every resource type as a section; within each, what the principal can do and **which grant says so**, the resolution chain rendered in mono:

`org default allow use model:* → group:contractors deny use model:opus-4.8 (wins)`

Header controls: "as of" timestamp (defaults now) and **diff against any prior date** (reads from audit) — changed rows marked added/removed with words, not color alone. Footer: entitlement version, last snapshot push, "Regenerate virtual key" (owner). Deep link from every user row, support ticket, and audit entry. This screen closes every access ticket; if a designer has one week, it goes here.

## 6. Models

### 6.1 Providers (owner)
Rows: provider · endpoint (mono) · auth status · health (word + last check). Add-provider flow covers cloud APIs and org-hosted OpenAI-compatible endpoints identically. Credentials write-only (never redisplayed); rotation logged.

### 6.2 Model registry
Rows: display name · `litellm_model_name` (mono) · provider · tier chip · caps (context, attachments) · enabled. Tier feeds routing and budgets.

### 6.3 Routing policy (owner)
The label→model matrix: rows = classifier labels (task × difficulty), columns = target model · fallback · notes. Per-group override tabs across the top. Edits are drafts; each rule renders a plain-English preview line ("code-plan/high routes to glm-5.2 via openrouter; falls back to opus-4.8"). Publish versions the policy; version history with diff and one-click rollback. **Signal, allowed use #1:** a live routed-requests sparkline per rule (last hour) — computation, so it may be green and moving. Pin-capability note links to the `router.override` capability grant.

### 6.4 Budgets
Per principal: period · limit · current burn (mono figure; ochre ≥ 80%, oxide at limit — static words+color, no gauges). Row expand: burn by model tier, key regeneration timestamp. Org-level default budget at top.

## 7. Connections

### 7.1 MCP registry
Rows: name · URL (mono) · transport · auth status · groups granted (chips) · enabled. Row expand: OAuth client config reference (secrets vaulted, write-only), inline **Test connection** (result as a mono record), grant shortcuts. MintMCP-style endpoints are ordinary rows; their internal governance is theirs, ours is which groups see the connection.

### 7.2 Local-stdio allowlist (owner) — deliberately austere
Top: the org **kill switch** — a labeled toggle whose confirm states blast radius in plain words ("Disables filesystem and browser MCP servers on every desktop immediately"). Below: allowlist rows — command/binary pattern (mono) · platforms · groups · added by. Nothing decorative on this page at all.

## 8. Library

### 8.1 Capabilities
Tabs: **Review queue · Published · Rejected**. Queue row expand: signed, versioned capability-manifest diff (mono); subunit inventory (skills, including former prompts · extensions, including hooks and local tools · connections · workflows · model requirements); declared surface diff (connections, extensions/hooks, model requirements, scopes); signature status; submitter. Actions: "Approve and sign" (states the signing key) · "Reject with note". Approval covers the capability's one-line deferred-loading description as well as its declared surface. Published rows: versions, channel policy (strict pin / gated / pure `:latest`), group entitlements, install count. Surface-expanding versions park in the review queue under the default gated policy while grantees remain on the last approved version. The Pi package format appears only in technical distribution details, never as a Library noun or independently grantable item.

### 8.2 Artifacts
Org library moderation: title · owner · shared-with chips · versions · flags. Org-wide publish queue if the org enables review.

Workflow operations live inside the containing capability's detail: workflow · owner · trigger (mono cron/webhook) · run-as (owner_identity/service chip) · last run · state. **Signal, allowed use #2:** running rows carry the pulse. Workflow row expand → run history ledger (trigger, duration, result, artifacts, entitlement-resolution note when access changed between runs — "salesforce connection unavailable at run time: grant revoked", oxide record). Service principals link to their own grants view. Workflows are subunits and never receive an independent Library listing or grant.

## 9. Records

### 9.1 Audit explorer
Pure mono, reverse-chronological, the query bar (`actor:`, `action:`, `resource:`, `decision:`, date range), infinite scroll with absolute-date landmarks. Row expand: full record incl. before/after JSON. Export: JSONL of the current filter. **No charts on this screen** — it is the record, not the dashboard. Append-only stated in the header.

### 9.2 Usage
The one place charts live: spend by model/group/day, routed-tier mix, request volume. Monochrome bars, ink on paper, tabular-numeral axes; **signal never appears in charts** (history is a record, not live computation). Every chart has a "view as table" toggle and CSV export.

## 10. Owner-only surfaces

**Org security posture:** sandbox policy per group (default / always-ask / full-auto-permitted) · local-model policy · voice cloud-cleanup toggle (default off) · pin-capability overview. Each control carries a one-line consequence in plain language.
**Identity:** SCIM tokens (issue/rotate/revoke, last-seen), OIDC config, **break-glass owner management** — deactivating an owner requires a second owner or the documented break-glass procedure; either path is logged loudly.
**Providers** (§6.1) and policy Publish rights.

## 11. States and empties

Empty tables: one sentence + the creating action ("No grants yet. Everything is denied by default — add the org's first allow."). Loading: static placeholder rows, no shimmer. Errors: mono record with cause and retry; failed mutations never leave optimistic UI behind. Session expiry: return-to-URL preserved through re-auth.

## 12. Build order for design

1. Table grammar kit + query bar (everything else derives).
2. Effective permissions (§5.2).
3. Routing policy (§6.3).
4. Grants, Users, MCP registry.
5. Capability workflow operations + Audit.
6. Remainder follows the kit with minimal bespoke work.
