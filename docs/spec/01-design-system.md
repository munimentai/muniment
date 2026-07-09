# Muniment Design System

**Doc 1 of 5** · Companions: desktop-app.md, mobile-app.md, marketing-site.md, admin-web-app.md
**Status:** v1 for handoff · July 2026

This document is the source of truth for tokens, laws, the identity, and shared components. The four surface docs reference it and never redefine it.

---

## 1. Principles

1. **The interface is the org's territory. The model is a visitor.** Design like institutions that hold things in trust: registries, ledgers, standards bodies. Calm, permanent, meticulous.
2. **Color means computation.** The single accent appears only while a model or tool works, or in the recorded route of work done.
3. **If it's a record, it's mono.** Conversation is grotesque; evidence is monospace.
4. **The platform has its say at the edges.** Brand tokens are invariant; chrome placement, navigation idiom, and input conventions follow the host OS or the web.
5. **Nothing to prove.** No hype vocabulary, no decoration, no motion without meaning.

Forbidden vocabulary in all UI copy: "AI", "magic", "supercharge", "unlock", "sovereign/sovereignty" (the website may use it exactly once, see marketing-site.md §3.2).

---

## 2. Color

### 2.1 Tokens

| Token | Light | Dark | Use |
|---|---|---|---|
| `paper` | `#F6F7F6` | `#141716` | App/page background |
| `surface` | `#FFFFFF` | `#1C201E` | Cards, composer, bars, popovers |
| `faint` | `#EDEFEE` | `#222624` | User bubbles, kbd chips, hover fills |
| `ink` | `#1A1D1C` | `#E8EBE9` | Text, primary buttons, icons active |
| `muted` | `#5C6461` | `#8A928E` | Secondary text, icons at rest, captions |
| `border` | `#E2E5E3` | `#2A2F2C` | Hairlines, dividers, input strokes |
| `signal` | `#2F7E6D` | `#58B39F` | Computation only (§2.2) |
| `signal-soft` | `rgba(47,126,109,.10)` | `rgba(88,179,159,.12)` | Signal-adjacent fills (rare) |
| `oxide` | `#B4483E` | `#C96A61` | Deny/critical records (admin) |
| `ochre` | `#B98A2F` | `#CBA14E` | Caution/budget records (admin) |

All neutrals carry a faint green cast tying them to signal. Never substitute pure `#FFFFFF`/`#000000` for `paper`/`ink`. Contrast floor: every text/background pair ≥ 4.5:1 both themes; verify small mono signal-on-paper, and if it misses, darken light-theme signal to `#2A7264`.

### 2.2 The color law (lintable)

`--signal` may be referenced ONLY by components on this allowlist:

1. The mark's thinking state (§5)
2. Streaming underline + caret on the active response line
3. Running-tool status (dot pulse + header text)
4. The route segment of the provenance line
5. Voice polish flash in the composer
6. Workflow-run-in-progress indicators (rows, tray)
7. Admin: live routed-requests sparkline; running-workflow row pulse
8. Website hero request trace

Everything else is ink, muted, or border — including primary buttons, links, focus rings, selection, icons, badges, charts of historical data, and the mark at rest. `oxide`/`ochre` are admin-record colors: static, desaturated, never animated, never used as alarm chrome.

Enforcement: a stylelint/custom lint rule flags any `--signal` reference outside allowlisted component files. PR review treats violations as bugs, not taste.

### 2.3 Theming

Follow the OS by default (`prefers-color-scheme`); user override persists per device. Both themes are first-class: every component, state, and doc screenshot ships in both. No third theme, no user-custom accent colors (that would break the law).

---

## 3. Typography

### 3.1 Families

- **Schibsted Grotesk** (400/500/600/700) — everything human: conversation, headings, buttons, marketing prose, form labels.
- **Commit Mono** (regular; fallback stack `"Commit Mono", ui-monospace, "SF Mono", Menlo, monospace`) — everything that is a record: provenance lines, tool activity, audit rows, policy matrices, model names, costs, timestamps, file paths, IDs, kbd chips, code.

**The register rule:** if a string is evidence (could appear in an audit log), it is mono. If it is speech (could be said aloud naturally), it is grotesque. When mixed in one line (e.g. "Routed to `glm-5.2`"), only the record token switches families.

### 3.2 Scale

Desktop/web: 12 / 13 / 15 (body) / 17 / 22 / 28. Mobile: 13 / 15 (body) / 17 / 22 / 30 (large titles per platform). Mono renders one step smaller than adjacent body text and never bold. Line-height 1.55 body, 1.3 headings, 1.45 mono blocks. Tracking: headings −1%, body 0, ALL-CAPS labels +4% (used only for tiny section labels).

No display serif. No italic except semantic emphasis inside user-authored content. Numerals: tabular lining figures in all tables and provenance lines (`font-variant-numeric: tabular-nums`).

---

## 4. Shape, space, depth, motion

- **Radius:** 2 / 6 / 10. 2 = chips, kbd. 6 = buttons, inputs, tool cards, table controls. 10 = panels, composer, bubbles, windows, modals. Nothing pill-shaped, no per-corner mixing.
- **Spacing scale:** 4 / 8 / 12 / 16 / 20 / 24 / 32 / 44 / 64. Component internals prefer 8/12; layout gaps prefer 16/24/32.
- **Hairlines do elevation.** Borders are 1px `border`. Shadows exist at exactly two levels: `shadow-window` (0 1px 2px rgba(0,0,0,.05), 0 12px 40px rgba(0,0,0,.08)) for app windows/marketing frames, and `shadow-overlay` (0 4px 24px rgba(0,0,0,.14)) for popovers/modals. No glow, no colored shadows, no inner shadows.
- **Motion inventory (complete):** the mark's thinking state (§5), streaming underline appearance, tool status pulse (1.4s ease-in-out), panel/rail slide (180ms ease-out), popover fade+2px rise (120ms), the website hero sequence (once). Durations never exceed 300ms except the mark and the hero. `prefers-reduced-motion` removes all of it (mark falls back to static verdigris when thinking). No parallax, scroll-jacking, skeleton shimmer (use static placeholder blocks), or spring physics.

### 4.1 Anti-pattern law (PR-review checklist)

No gradients on text or surfaces · no violet/purple · no glassmorphism/backdrop blur · no orbs/particles/ambient animation · no assistant avatar · no typing dots · no sparkle/wand icons · no emoji in UI copy · no pill radius · no zebra striping · no card-on-card nesting deeper than one level.

---

## 5. Identity: the milled ring

### 5.1 The mark

A ring with a milled edge: radius modulated by a uniform 22-tooth wave. Reference geometry in a 48-unit viewBox: `r(t) = 16.5 + 1.6·sin(22t)`, monoline stroke, round caps/joins. Rationale: coins are milled so a clipped coin exposes itself — verification made visible.

Companion motif — **the countermark**: the same milled seal split in two by an irregular indenture cut, matched halves that prove each other. Uses: OS app icons (macOS/iOS/Windows/Android), shared-thread and fork imagery. Never competes with the ring as primary mark.

### 5.2 States

- **At rest:** static, ink. The only form in chrome, lockups, site header, print.
- **Thinking:** verdigris, animated per §5.3. Appears only during actual work: pre-first-token, tool waits, tray during workflow runs. When streaming begins, the underline takes over; the mark and the underline never animate simultaneously.
- **Done:** current breath completes, spin eases to still, color returns to ink. Never cut mid-gesture.

### 5.3 Thinking grammar (reference implementation: `muniment-ring-pulse-spin.html`)

Two decorrelated randomized behaviors on the whole ring (never fragments), plus a rare accent:

1. **Breath:** irregular heartbeat ≈70bpm, rate jittered per beat. Each beat draws a signed depth in [−1,+1], swell favored ~3:2. Scale ±3.2%, stroke weight ±18%, milling depth ±60% flex together; swell sharpens teeth, slim smooths them. Asymmetric: quick flex, slow settle.
2. **Spin:** angular velocity eases toward a random target every 1.5–4s from {still, slow ±, medium ±, fast ±}, still weighted heaviest. Cap the fast tier below 24px if teeth strobe.
3. **Trace:** every 5–10 beats a brighter segment runs the outline once.

All simultaneously visible instances share one organism and animate in sync — one presence, not several spinners. Decorrelation of breath and spin is the anti-loop principle; do not synchronize them "for polish."

### 5.4 Reductions and lockup

≤20px the ring renders as a solid two-edge milled fill for legibility (favicon, tray, list chips). Lockup: mark + `muniment` (Schibsted 600, lowercase, −1% tracking), mark sized to cap height + overshoot, no tagline. Clearspace: one tooth-depth on all sides. Minimum sizes: mark 12px, lockup 90px wide.

---

## 6. Core components

Every component ships both themes, all states (rest/hover/active/focus/disabled), and keyboard behavior. Focus ring: 2px ink outline, 2px offset, never signal.

- **Button / primary:** ink fill, paper text, radius 6, 13.5/600, padding 7×18. Hover: 6% ink-shift. Never signal.
- **Button / quiet:** transparent, muted text, hover `faint` fill. Icon buttons pair icon+label at first exposure.
- **Input / textarea:** `surface` fill, `border` stroke, radius 6 (10 for the composer), focus stroke `muted`. Placeholder muted. Error state: 1px `oxide` stroke + mono cause line below; no red fills.
- **Chip:** radius 2, mono 11–12px, `faint` fill or hairline. Variants: kbd, attachment, model-pin, transform (voice), filter (admin).
- **Provenance line:** mono 11.5, muted, route segment in signal: `analysis/high → glm-5.2 · $0.0041 · 3.8s`. Click/tap expands the receipt (label, matched policy rule, tokens, connections touched, lineage). Screen-reader label spells it out in words.
- **Tool card:** radius 6, `surface`, mono 12.5. Header = status dot + verb + object; running = signal dot pulse + signal header; complete = muted, collapsed to header. Body collapses past 8 lines.
- **Table (admin grammar):** real `<table>`, 36px rows, hairline row separators, no zebra, sticky header, mono for identifiers/figures/timestamps, grotesque for names. Deny rows: strikethrough + `oxide` text + the word "deny" (never color alone). Row expands inline; entities with URLs navigate.
- **Query bar (admin):** mono input accepting `key:value` filters with clickable chips for common cases.
- **Toast:** bottom-center, `surface`, hairline, radius 6, ink text, auto-dismiss 5s, one action max. Never signal, never icons-only.
- **Modal / confirm:** radius 10, `shadow-overlay`. Destructive confirms require typing the object's name; the button then reads the consequence ("Revoke 3 grants").
- **Popover / menu:** radius 6, 120ms fade+rise, full keyboard nav.
- **Empty state:** one grotesque sentence inviting action, optional quiet button. No illustration.
- **Kbd chip:** radius 2, `faint`, mono 11.

---

## 7. Voice and copy rules

Sentence case everywhere. Buttons say what happens ("Fork thread", "Revoke grant", "Publish policy"). Errors: what happened + the next step, no apology, mono for the technical cause. Numbers in records are exact, not rounded prose ("$0.0041", not "less than a cent" — except in screen-reader labels, which prefer words). Time: relative under 7 days ("3h ago"), absolute mono date after. Never exclamation marks. Never "please wait."

---

## 8. Accessibility floor (all surfaces)

Contrast ≥ 4.5:1 · full keyboard operability, visible focus · `prefers-reduced-motion` respected including the mark and hero · color never the sole carrier (deny = strike + word; running = pulse + verb) · hit targets ≥ 44px touch, ≥ 24px pointer · provenance and tool cards fully labeled for screen readers · zoom to 200% without horizontal scroll on web surfaces.

---

## 9. Implementation

Distribute as: `@muniment/tokens` (CSS custom properties + Tailwind preset, light/dark via `[data-theme]`), `@muniment/mark` (the ring: static SVG exports + the animation engine as a framework-agnostic component with `state: rest|thinking`, shared-organism context), `@muniment/ui` (components above). The signal-allowlist lint ships inside `@muniment/tokens`. Fonts self-hosted (Schibsted Grotesk OFL, Commit Mono OFL); no third-party font CDNs in product surfaces.
