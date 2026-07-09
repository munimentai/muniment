# Muniment Marketing Site — Design Document

**Doc 4 of 5** · Tokens, laws, identity, components: see 01-design-system.md
**Status:** v1 for handoff · July 2026

---

## 0. Business model (ground truth — overrides anything previously generated)

Muniment is a **hosted SaaS**. We run the control plane and gateway; customers do not host anything. The desktop app is a thin client to our cloud. **Closed source, commercial**: no public repo, no open source edition, no self-hosted tier, and self-hosting is never mentioned anywhere on the site. Pricing: one plan — **$15/seat/mo billed annually, $20/seat/mo billed monthly, 10-seat minimum, 30-day full-product trial**. Customers bring their own provider keys or their own model endpoints; we never mark up inference. No partner, MSP, or reseller program. The word "sovereignty" appears exactly once on the site (§3.2); "AI" never appears in copy.

## 1. Job and audience

Primary reader: the owner/admin who buys this for an org (IT, security, platform). Secondary: the end user who will live in it daily. The site's one job: make "every answer carries its receipt" tangible in the first screen, and make signing up feel like a decision that stays reversible by the last ("your records export in the open").

Structure: **Landing · Docs · Pricing · Security**. No blog at launch, no chat widget, no gated content, no demo-request theater — the 30-day trial is the demo.

## 2. Global rules

Content max-width 1080px, generous side margins, hairline section dividers, no cards-on-cards. Typography and tokens identical to product (the site is the first product surface people meet). Header: lockup (mark at rest + `muniment`) left; Docs · Pricing · Security right; primary header CTA "Start free trial" (ink); ink link underlines on hover. No GitHub link (closed source). Footer: docs, security, status, terms · privacy, licenses (crediting Pi, Flue, LiteLLM, Tauri — transparency is on-brand), the analytics statement ("Privacy-respecting analytics. No trackers."), no newsletter box. Dark mode via `prefers-color-scheme`. Performance budget: LCP < 1.5s, zero layout shift, Lighthouse a11y ≥ 95, total JS < 80KB on landing (the hero animation is hand-rolled, not a library).

## 3. Landing page

### 3.1 Hero — the one orchestrated motion moment

Paper background, hairline schematic: **prompt → classifier → policy → model**, drawn as a static ledger diagram (mono labels, 1.6px strokes, ink). On load — once, ~2.5s, skipped entirely under reduced-motion (static end-state shown) — a single signal trace travels the path and terminates by typing out a real provenance line:

`analysis/high → glm-5.2 · $0.0041 · 3.8s`

**H1 (Schibsted 700, ink):** Every answer carries its receipt.
**Sub:** One app for your whole org. Any model — theirs or yours. Your policies decide which one answers, and every reply shows its route, cost, and time.
**Actions:** primary ink button "Start free trial" · quiet button "Read the docs". No screenshot in the hero; the receipt is the product shot.

### 3.2 Section — The rule

The strict color idea turned outward. Copy: "The interface is yours. Color appears only when a model is working." Paired with a muted 6-second loop: a tool card runs (signal pulse), completes, goes quiet. **This section contains the single permitted use of the word:** "Sovereignty over your intelligence policy — expressed as product behavior, not a slide." It appears nowhere else on the site or in the product.

### 3.3 Section — Routing

The policy matrix as a real mono table (labels × models with fallbacks), captioned: "Owners set the map. Users never pick a model." One supporting line on the classifier: "A small local model classifies every request. Your gateway decides where it goes. It gets smarter on your traffic, not ours."

### 3.4 Section — Territory

Effective-permissions screenshot (deny rows struck). Copy: "Groups grant. Deny wins. The gateway enforces even if a client lies." Sub-points in prose: virtual keys scoped per user; revocation reaches scheduled work at its next run.

### 3.5 Section — Records

Audit explorer screenshot. "Append-only. Every privileged decision, exportable. SIEM-ready." One line on provenance receipts as user-visible audit.

### 3.6 Section — Voice, on device

"Zero voice bytes leave the machine." Three mono chips: `capture` · `polish` · `read-aloud`, each labeled with its local model class. One line: "Dictation that cleans itself up, powered by models on the laptop, not an endpoint."

### 3.7 Section — One mode

The simplicity claim, stated once, plainly: "No modes. No model picker. One surface that scales from a question to an agent run." Paired with a static desktop screenshot (light theme on light site, dark on dark).

### 3.8 Section — Nothing to run

Copy: "Sign up, connect your identity provider, invite your org, install the app. Your provider keys are held encrypted in our vault and never ship to a device — or route to model endpoints you already run." Visual: a 3-step mono getting-started sequence (`create org → connect IdP → install desktop app`) — no infrastructure blocks, no requirements list. CTA: "Start free trial".

### 3.9 Section order rationale

Receipt → rule → routing → territory → records → voice → one mode → nothing to run: claim, proof layers in descending abstraction, then the exit ramp. Each section is one screen or less; total page under 8 screens.

## 4. Docs

Docs are part of marketing for this buyer. Framework: any static generator; design: product tokens, mono code blocks with copy buttons, right-side on-page TOC, version switcher stub, search via local index (`/`). Every conceptual page opens with a two-sentence plain-language summary before detail.

Left nav (grouped): **Getting started** (Overview: create org → connect IdP or email auth → invite → install apps · Desktop app install for macOS/Windows/Linux · First route: watch the provenance line appear) · **Identity & SCIM** · **Entitlements** · **Routing** · **Connections** · **Org model endpoints** (advanced: routing to OpenAI-compatible endpoints the customer hosts; connectivity via IP allowlist or the muniment outbound connector agent) · **Packages** · **Workflows** · **Desktop app** · **API**.

No Deploy, Requirements, Compose, or Upgrade/rollback pages exist. Sweep all pages for "self-hosted", "docker", "compose", "your metal", "open source", "community support" — none may remain.

## 5. Pricing

One page, **one plan**, prose-first. Plan name: `muniment`. Two billing figures side by side: **$15/seat/mo annual · $20/seat/mo monthly**, with the line "10-seat minimum · 30-day full trial · every feature included". Included list: the desktop app and admin console, unlimited policies and connections, SSO/SCIM, append-only audit with export, on-device voice.

Footnote blocks (prose, two columns): **"Priced like the infrastructure it is"** (why per seat, why a minimum — this holds an org's identity, entitlements, audit, and routing policy) · **"Model spend stays yours"** (verbatim promise kept: you bring your own provider keys or weights; muniment routes and records — it never marks up inference; every cost you see is the real cost) · **"No lock-in"** ("Your records export in the open. Your keys are yours. Leaving is a decision, never a hostage negotiation.").

No tiers, no comparison matrix, no "Most popular" badge, no partner/MSP/reseller mention, no "numbers coming" placeholders. Primary CTA: "Start free trial".

## 6. Security page

The buyer's due-diligence shortcut, written in the ledger voice: architecture diagram (**thin desktop client → muniment cloud: control plane + gateway → your model providers or your endpoints**), per-org isolation stated plainly; key handling (provider keys encrypted at rest in our vault, never sent to devices; per-user scoped virtual keys; revocation reaches scheduled work at its next run); deny-wins entitlements enforced at the gateway even if a client lies; append-only audit with SIEM export; voice fully on-device; sandbox honesty for local agent runs including the Windows caveat. Compliance: claim no certification; one line — "Compliance documentation available under NDA — talk to us." Responsible-disclosure contact. PDF export of this page offered (generated from the same source).

## 7. Asset and content rules

Screenshots are real product captures, never mocked in a design tool, refreshed by a scripted capture pipeline; both themes captured. All motion on the site is the hero (once) and the §3.2 loop (muted, pausable). No stock imagery, no isometric illustrations, no logos-wall until real customers consent. Open-graph card: lockup on paper with one provenance line.
