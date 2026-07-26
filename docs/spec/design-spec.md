# Muniment — Design Spec: Identity, Desktop App, Mobile, Website, Admin Web App

**Status:** Draft v1 for design/dev handoff
**Companion doc:** harness-spec.md (product/engineering spec)
**Date:** July 2026

---

## 1. Brand foundation

### 1.1 Name and thesis

The product is **Muniment** (`muniment.ai`). Muniments are the deeds and charters kept as proof of ownership and rights; manor houses kept them in a muniment room. The name is the product: the provenance line, the grants table, and the audit log are all muniments. Always written lowercase in the lockup (`muniment`); sentence case in prose ("Muniment").

The interface is the org's territory. The model is a visitor. The design language comes from institutions that hold things in trust: registries, standards bodies, ledgers. Calm, permanent, meticulous about records. Nothing to prove, everything documented.

The word "sovereignty" (and "sovereign") never appears in any product surface. The website may use it exactly once. The behavior carries the claim; the copy does not.

"AI" never appears in interface copy. Things are named by what they do: route, run, record, connect.

### 1.2 The strict color rule (LAW)

**Color means computation.** The chromatic accent (signal) may appear only where a model or tool is actively working or where its work is recorded:

Allowed: the mark's thinking state (§1.8), running-tool status pulse, the streaming underline and caret on the active line, the route segment of the provenance line, live voice-polish flash, workflow-run-in-progress indicators, the request trace on the website hero.

Forbidden: buttons (primary actions are ink-on-paper), links in body copy (underline in ink), focus rings (ink/muted), selection states, icons at rest, badges, empty-state illustrations, the brand mark at rest.

Semantic red and amber exist but live almost exclusively in the admin app (deny records, budget warnings), desaturated, never animated.

This rule is enforceable as a lint: the `--signal` token may only be referenced by components on the allowed list.

### 1.3 Color tokens

| Token | Light | Dark | Use |
|---|---|---|---|
| `paper` | `#F6F7F6` | `#141716` | App background |
| `surface` | `#FFFFFF` | `#1C201E` | Cards, composer, bars |
| `faint` | `#EDEFEE` | `#222624` | User bubbles, kbd chips, hover |
| `ink` | `#1A1D1C` | `#E8EBE9` | Text, primary buttons |
| `muted` | `#5C6461` | `#8A928E` | Secondary text, icons at rest |
| `border` | `#E2E5E3` | `#2A2F2C` | Hairlines |
| `signal` | `#2A7264` | `#58B39F` | Computation only (1.2) |
| `signal-soft` | `rgba(47,126,109,.10)` | `rgba(88,179,159,.12)` | Signal backgrounds |
| `oxide` | `#B4483E` | `#C96A61` | Deny/critical (admin) |
| `ochre` | `#B98A2F` | `#CBA14E` | Caution/budget (admin) |

All neutrals carry a barely-there green cast tying them to signal. Never substitute pure `#FFF`/`#000`.

Themes: respect the OS by default; both light and dark are first-class and fully built. User override persists per device.

### 1.4 Typography

Two families, two registers:

- **Schibsted Grotesk** — everything human: conversation, headings, buttons, marketing copy. Weights 400/500/600/700.
- **Commit Mono** — everything that is evidence: provenance lines, tool activity, audit entries, policy matrices, costs, model names, file paths, keyboard chips. One weight, one size class per context.

**Rule: if it's a record, it's mono.** Users learn without being told which text is conversation and which is ledger.

Type scale (desktop): 12 / 13 / 15 (body) / 17 / 22 / 28. Mono runs one step smaller than adjacent body text. Line-height 1.55 body, 1.3 headings. No display serif anywhere. No italic except semantic emphasis in user content.

### 1.5 Shape, depth, motion

- Radius scale: **2 / 6 / 10**. 2 = chips and kbd. 6 = buttons, tools, inputs. 10 = panels, composer, window. Nothing pill-shaped.
- Elevation: hairline borders do the work. Max two shadow levels (window chrome, overlays). No glow.
- Motion: purposeful and rare. The mark's thinking state (§1.8), streaming underline, tool pulse (1.4s ease), panel slide (180ms), and the one website hero sequence. `prefers-reduced-motion` kills all of it. No parallax, no scroll-jacking.

### 1.6 Anti-patterns (LAW, write into PR review)

No gradients on text or surfaces. No violet/purple anywhere. No glassmorphism or backdrop blur. No orbs, particles, or ambient animation. No assistant avatar. No typing dots (streaming = signal underline + caret). No sparkles/wand iconography. No emoji in UI copy. No pill radius. No "AI", "magic", "supercharge", "unlock" in copy.

### 1.7 Iconography and voice

Icons: 1.6px stroke, geometric, from one set (Lucide is acceptable), always paired with a label at first exposure. Sentence case everywhere. Buttons say what happens: "Send", "Fork thread", "Revoke grant". Errors state what happened and the next step; they never apologize. Empty states are invitations to act, one line, no illustration.

### 1.8 Identity: the milled ring

**The mark.** A ring with a milled edge — a circle whose radius is modulated by a uniform 22-tooth wave (reference geometry: `r(t) = 16.5 + 1.6·sin(22t)` in a 48-unit viewBox, monoline stroke, round caps). Coins are milled so a clipped or counterfeit coin exposes itself; the mark is verification made visible. Companion motif (app icon, shared-thread imagery): the **countermark**, the same milled seal split in two by an indenture cut — matched halves that prove each other.

**States.** The mark obeys the color law without exception:

- **At rest:** static, ink, no motion. This is the only form that appears in chrome, lockups, the website header, and print.
- **Thinking:** verdigris, animated (below). Appears only while a model or tool is actually working: pre-first-token, tool waits, tray/menubar during workflow runs. Once text streams, the signal underline takes over — never both at once.
- **Done:** the current breath completes, spin eases to still, color hands back to ink. Never cut off mid-gesture.

**Thinking animation grammar.** Two independent randomized behaviors layered on the whole ring (the ring never fragments or dissolves), plus a rare accent:

1. **Breath** — an irregular heartbeat (~70bpm equivalent, rate jittered per beat). Each beat draws a *signed* depth in [−1, +1], swell slightly favored (~3:2): scale (±3.2%), stroke weight (±18%), and milling depth (±60%) flex together. A swell sharpens the teeth; a slim pulse smooths them. Flex is asymmetric: quick swell, slow settle.
2. **Spin** — angular velocity eases toward a new random target every 1.5–4s, drawn from {still, slow ±, medium ±, fast ±} with still weighted heaviest. It turns, hesitates, stops, resumes, sometimes reverses; always eased, never stepped. Cap the fast tier at small sizes if the teeth strobe.
3. **Trace** — every 5–10 beats, a brighter segment runs the outline exactly once.

Breath and spin are decorrelated; that is what keeps the motion from ever reading as a loop. All simultaneously visible instances (thread chip, sidebar, tray) share one organism and animate in sync — one presence, not several spinners. `prefers-reduced-motion`: static verdigris, no gestures.

**Reductions.** ≤20px: the ring renders as a solid two-edge milled shape (fill, not stroke) for legibility; at these sizes the animation is honestly just "alive and green," which is acceptable. Favicon and tray use the solid reduction.

**Lockup.** Mark + `muniment` in Schibsted Grotesk 600, lowercase, tight tracking (−1%), mark sized to the wordmark's cap height plus overshoot. No tagline in the lockup.

**Reference implementation:** `muniment-ring-pulse-spin.html` (parametric geometry and the full behavior engine; tuning dials are tooth count, breath depth ranges, spin speed set, and gesture cadence). Production mark needs a geometry pass before print/vector export.

---

## 2. Desktop app

Platform note: Tauri renders in the native webview; let the platform have its say at the edges. macOS: traffic lights top-left, native menu bar, `⌘` shortcuts, vibrancy off (solid `surface` titlebar). Windows: caption buttons top-right, `Ctrl` shortcuts, respects Mica off. Linux: CSD with our titlebar. Native OS notifications, native file dialogs, native context menus where possible. Brand tokens stay identical across platforms; chrome placement follows the OS.

### 2.1 Layout

```
┌───────────┬──────────────────────────────────┬──────────────┐
│ SIDEBAR   │  THREAD                          │ ARTIFACT     │
│ (collaps- │                                  │ RAIL (⌘J,    │
│  ible)    │  titlebar: session name ·        │  closed by   │
│           │  artifacts ⌘J · palette ⌘K       │  default)    │
│ + New     │                                  │              │
│ Search    │  messages …                      │              │
│ ────────  │                                  │              │
│ Threads   │                                  │              │
│  (recent) │                                  │              │
│ ────────  │  ┌ composer ─────────────┐       │              │
│ Projects  │  │ input                 │       │              │
│  (if any) │  │ voice · attach · Send │       │              │
│ ────────  │  └───────────────────────┘       │              │
│ ▣ Mikey   │  hint line                       │              │
│  DNSFilter│                                  │              │
│  · owner  │                                  │              │
└───────────┴──────────────────────────────────┴──────────────┘
```

**Sidebar.** Expanded by default, state remembered, collapse animates in 180ms to a 52px icon rail. Contents top-to-bottom: New thread, Search, recent threads (title + relative time, no previews), Projects section (only rendered if the user belongs to ≥1 project; solo users never see the concept), profile block.

**Profile block (bottom).** Answers "who am I and whose territory is this": avatar-less — name, org name, role in muted mono (`mikey · dnsfilter · owner`). Click opens a popover: appearance, entitlement peek ("Your groups" → each group expands to what it grants, read-only, mono), sign out. The entitlement peek is the user-facing face of the snapshot; it is display-only.

**Command palette (⌘K).** Supplements the sidebar: threads, projects, artifacts, actions ("new thread in <project>", "toggle appearance", "open inbox"). Never the only path to anything.

### 2.2 Thread surface

Single mode. No mode switcher exists in any menu.

- **User messages:** right-aligned, `faint` bubble, radius 10, max-width 78%.
- **Responses:** left-aligned plain text on `paper`, no bubble, no avatar, max-width 92%. The absence of a bubble is deliberate: the model writes onto the org's page.
- **Streaming:** the active line carries a 2px signal underline and a signal caret. When generation ends, the color leaves. No dots, no shimmer.
- **Tool activity:** inline mono cards (radius 6, `surface`). Header: status dot + verb + object (`read dim_customers · fct_mrr_events`). Running = signal dot pulsing + signal header text; complete = muted, collapsed to header with expand. Long output collapses past 8 lines.
- **Provenance line:** under every response, mono, 11.5px, muted with the route in signal: `analysis/high → glm-5.2 · $0.0041 · 3.8s`. Click expands the full receipt: classifier label, policy rule that matched, tokens, connections touched. This is the brand's signature element; it ships in v1 and is never optional.
- **State provenance:** conversation, tool, permission, and receipt UI reduces
  from the append-only local run journal (desktop ADR 0002). Reopen rebuilds
  from it; snapshots are disposable, and uncertain external effects render
  needs-attention instead of repeating.
- **Model pin (router.override holders only):** a small mono chip adjacent to the provenance area, `auto ▾`. Pinning shows `pinned → <model>` in ink (a user decision is not computation). Users without the capability never see the chip.

### 2.3 Composer

`surface` box, radius 10, focus = border shifts to `muted` (never signal). Row: voice, attach, Send (ink button, `paper` text). Hint line below in muted 11px, rotating sparingly; default: "Routing is automatic. Every reply carries its receipt."

Attachments render as mono chips with filename + size; images get a 48px thumb. Drag-and-drop anywhere on the thread targets the composer.

### 2.4 Voice

Hold `⌥Space` (Windows: `Alt+Space` alternative binding due to system menu conflict — settle in build) anywhere in the app; also click-and-hold the voice button. States:

1. **Capturing:** composer border stays ink; a minimal 5-bar level meter replaces the hint line (muted bars, no waveform art). Verbatim transcript streams into the input in muted text.
2. **Polishing (on release):** the transcript flashes a signal underline as the local model rewrites it — the one place signal touches the composer, because computation is happening — then settles to ink. Transforms (key points / formal / short / long) appear as a transient mono chip row for 6s.
3. Escape cancels and restores prior input.

### 2.5 Projects (team surface)

A project = shared workspace root + files + artifacts + MCP set + instructions. Opening one shows tabs in the titlebar area: **Threads · Shared · Artifacts · Inbox · Connections**.

- **Threads:** your own threads inside the project scope.
- **Shared:** threads members have shared in, as live read-only views. Redaction rule: tool-output blocks produced via a connection the viewer lacks render as a struck mono placeholder — `output withheld · connection not granted` — header visible, body withheld. The reasoning text around it remains. Ledger honesty: you can see that something happened and why you can't see it.
- **Fork** (primary action on any shared thread): continues the conversation as the viewer's own thread under their own key and entitlements. Forks record lineage in the provenance receipt.
- **Inbox:** scheduled workflow results delivered here (and to the personal inbox for personal workflows). Each entry is a mono record: workflow name, run time, status, artifacts produced.
- **Connections:** read-only list of the project's MCP connections and skills with grant source shown; nothing to configure for non-admins.

No live co-typing in a thread in v1 (v2 experiment behind a driver/suggester model).

### 2.6 States

- **First run:** empty thread, one line: "Ask anything. Your org's routing decides which model answers." Composer focused. No tour, no confetti.
- **Server unreachable:** full-surface notice in mono ledger style: what happened, retrying countdown, "Copy diagnostics". App is honest that it requires the control plane; no degraded fake-offline mode.
- **Entitlement change mid-session:** toast in ink: "Your access changed. Some models or connections may differ." Provenance lines make the difference visible naturally.
- **Errors:** inline mono record under the failed message: cause + one action ("Model unavailable via provider. Retried on fallback." / "Retry").

---

## 3. Mobile app (companion app — engineering phased per harness-spec §12; amended 2026-07-10)

Purpose: originally mockup-only to complete the brand story and de-risk future scope; as of 2026-07-10 these screens are the design ground truth for the phased mobile companion app (harness-spec §12, 03-mobile-app.md v1.1). The M1 live-session view still needs a ninth mockup before engineering.

Platform has its say: iOS uses a bottom tab bar, large-title headers, system swipe-back, SF-symbol-weight icon rendering; Android uses Material navigation patterns, predictive back, and system dynamic-color is **ignored** (brand tokens win — territory doesn't recolor itself per phone). Both respect OS light/dark.

### 3.1 Screens to produce (8)

1. **Threads (home).** List of recent threads, search at top, New button. Profile chip (name · org · role) in the header, not a tab.
2. **Thread.** Same grammar as desktop: no assistant bubble, streaming signal underline, inline mono tool cards (collapsed by default on mobile), provenance line under each response (tap to expand receipt as a bottom sheet).
3. **Composer + voice.** Voice is the hero input on mobile: large hold-to-talk control, verbatim transcript → signal polish flash, transform chips. This is the Eloquent pattern in our skin.
4. **Projects list** (only when member of ≥1).
5. **Project detail** with Threads / Shared / Artifacts / Inbox as segmented tabs.
6. **Shared thread view** including the `output withheld · connection not granted` struck record, to prove the redaction pattern at small size.
7. **Inbox** with workflow-run records.
8. **Entitlement peek** bottom sheet from the profile chip.

Tabs (bottom): Threads · Projects · Inbox. Three, no more.

### 3.2 Mobile-specific rules

Provenance line truncates to route + cost; tap for the rest. Tool cards never auto-expand. Signal usage identical to desktop law. Minimum tap target 44px. No haptic flourish beyond system defaults.

---

## 4. Website (marketing)

Audience: owners and admins first (the buyer), users second. Job: explain the model-visitor idea in one screen and make the receipt tangible.

### 4.1 Structure

Single landing page + Docs + Pricing + Security page. No blog at launch. No chat widget.

### 4.2 Hero (the one orchestrated motion moment)

Monochrome diagram, paper background, hairline geometry: **prompt → classifier → policy → model**, drawn as a static ledger schematic. On load (once, 2.5s, skipped under reduced-motion), a single signal trace travels the path and terminates in a rendered provenance line that types itself out:

`analysis/high → glm-5.2 · $0.0041 · 3.8s`

Headline (Schibsted 700, ink): **"Every answer carries its receipt."**
Subline: "One app for your whole org. Any model — theirs or yours. Your policies decide which one answers, and every reply shows its route, cost, and time."

No product screenshot in the hero. The receipt is the product shot.

### 4.3 Sections (in order)

1. **The rule.** Short statement of the strict color idea turned outward: "The interface is yours. Color appears only when a model is working." Paired with a muted looping 6s capture of a tool running and going quiet. This is where the one permitted use of the word appears: a single sentence, e.g. "Sovereignty over your intelligence stack, expressed as product behavior, not a slide."
2. **Routing.** The policy matrix rendered as a real table (mono), captioned "Owners set the map. Users never pick a model."
3. **Territory.** Entitlements: the effective-permissions preview screenshot, deny rows struck. Copy: "Groups grant. Deny wins. The gateway enforces even if a client lies."
4. **Records.** Audit explorer screenshot. "Append-only. SIEM-ready."
5. **Voice, on device.** "Zero voice bytes leave the machine." Three mono chips: capture · polish · read-aloud, each labeled with the local model class.
6. **Deploy.** Docker compose block, real and copyable. Self-hosted stance stated plainly.
7. **Footer:** docs, security, licenses page (we credit the open stack: Pi, Flue, LiteLLM, Tauri — transparency is on-brand).

### 4.4 Web-specific craft

Max content width 1080px, generous margins, hairline dividers, no cards-on-cards. Typography identical to product. Dark mode honored via `prefers-color-scheme`. Lighthouse accessibility ≥ 95. No cookie banner (no tracking beyond privacy-respecting analytics, e.g. self-hosted Plausible — say so in the footer).

---

## 5. Web app (admin control plane)

Audience: admins and owners. Aesthetic: the ledger, fully expressed. Density: compact. This is where mono earns its keep and where oxide/ochre are allowed to exist.

Platform has its say: it's the web — real URLs for every view (deep-linkable grants, users, runs), browser back always works, tables are real `<table>`s, everything keyboard-navigable.

### 5.1 Information architecture

Left nav (persistent, grouped):

- **People:** Users · Groups
- **Access:** Grants · Effective permissions
- **Models:** Providers · Model registry · Routing policy · Budgets
- **Connections:** MCP registry · Local-stdio allowlist
- **Library:** Packages (review queue) · Artifacts · Workflows
- **Records:** Audit · Usage

Role gating: admins see People/Access/Library and read-only Records; owners see everything. Hidden ≠ disabled: sections a role lacks do not render.

### 5.2 Table grammar (used everywhere)

Mono for identifiers, figures, timestamps; grotesque for names. Row height 36px. Hairline row separators, no zebra. Filters as a persistent mono query bar (`group:contractors action:use effect:deny`) with clickable chips for the common cases. Every row expands inline rather than navigating away, except entities with their own URL.

**Deny rendering:** deny rows are struck-through in oxide with normal weight — a record, not an alarm. Allow rows are plain ink. Effect column uses words (`allow` / `deny`), never icons or colors alone.

### 5.3 Key screens

**Effective permissions (flagship).** The title search. Input: a user. Output: every resource type, what they can do, and *which grant says so* — grant chain rendered as `org default → group:data-team allow → user deny (wins)` in mono. Includes "as of" timestamp and a diff view against any prior date (reads from audit). This screen closes every entitlement support ticket; it gets the most design time.

**Routing policy.** The label→model matrix: rows = classifier labels (task × tier), columns = target model + fallback, per-group override tabs. Edits are drafts with a plain-English preview sentence per rule ("code-plan/high routes to glm-5.2 via openrouter; fallback opus-4.8") and a single Publish action that versions the policy. Signal appears in exactly one place here: a live "requests routed in the last hour" sparkline per rule, because that is computation.

**Grants editor.** Create grant = one row form: principal, resource (with wildcard), action, effect, expiry. Deny requires a required "reason" field that lands in the audit record.

**Budgets.** Per principal, period, limit; current burn as a mono figure with ochre past 80%, oxide at limit. No dials, no gauges.

**MCP registry.** Connection rows: name, URL, transport, auth status, groups granted. Test-connection action inline. Local-stdio allowlist is a separate, deliberately austere screen with the org kill switch at top — a labeled toggle with a confirmation that states blast radius in plain words.

**Package review queue.** Diff-style manifest view, signature status, "Approve and sign" / "Reject with note". Version pinning per group surfaces here post-v1.

**Workflows.** Run history as ledger rows: workflow, trigger, run-as identity, duration, result, artifacts. A running workflow row carries the signal pulse — the only motion on the page.

**Audit explorer.** Pure mono, reverse-chronological, the query bar, export (JSONL). No charts on this screen; it is the record, not the dashboard.

**Usage.** The one place charts live: spend by model/group/day, routed-tier mix. Monochrome bars, ink on paper; signal never appears in charts (historical data is a record, not live computation).

### 5.4 Owner-only surfaces

Providers (credentials, health), org security posture (sandbox policy per group, local-model policy, voice cloud-cleanup toggle), SCIM tokens, break-glass owner management. Destructive owner actions require typed confirmation of the object's name.

---

## 6. Accessibility and quality floor (all surfaces)

- Contrast: all text ≥ 4.5:1 in both themes (verify signal-on-paper for small mono; bump to `#2A7264` light if it misses).
- Full keyboard operability; visible focus (2px ink outline, offset 2).
- `prefers-reduced-motion` respected everywhere including the website hero.
- Color never the sole carrier of meaning (deny = strikethrough + word; running = pulse + verb).
- Screen-reader labels on provenance lines ("Routed as analysis, high tier, answered by glm-5.2, cost less than one cent, 3.8 seconds").
- Hit targets ≥ 44px on touch surfaces.

---

## 7. Deliverables checklist

1. Identity kit: production milled-ring geometry (SVG master + solid ≤20px reduction), countermark app icon (macOS/iOS/Windows sizes), lockup files, and the thinking-animation engine as a shared component (reference: `muniment-ring-pulse-spin.html`).
2. Token file (CSS custom properties + Tailwind config) with the signal-allowlist lint rule.
3. Desktop: Figma (or code-first) for layout, thread grammar, composer, voice states, project tabs, empty/error states, both themes.
4. Mobile: the 8 mockup screens, both themes, iOS and Android chrome variants.
5. Website: hero motion prototype + full page, both themes.
6. Admin: table grammar kit + the effective-permissions and routing-policy screens first, rest follow the kit.
7. Copy deck: button/error/empty-state strings following §1.7 voice rules.
