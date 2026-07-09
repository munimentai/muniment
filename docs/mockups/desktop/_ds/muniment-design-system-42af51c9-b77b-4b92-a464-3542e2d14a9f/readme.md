# Muniment Design System

The source of truth for muniment's tokens, laws, identity, and shared components. This is Doc 1 of 5; the four surface docs (desktop-app, mobile-app, marketing-site, admin-web-app) reference this and never redefine it.

Status: v1 for handoff · July 2026.

## Company & product context

**muniment** builds an interface where an organization routes work to AI models under its own policy. A *muniment* is a document held as evidence of a right — and that is the product's whole posture: **the interface is the org's territory; the model is a visitor.** The design language is that of institutions that hold things in trust — registries, ledgers, standards bodies. Calm, permanent, meticulous.

Every piece of work the org routes leaves a **record**: a provenance line (route → model → cost → time), the tools it touched, the policy rule it matched. Those records are the product's real content, and they are typeset as evidence (monospace), distinct from conversation (grotesque).

Surfaces represented across the five docs: **desktop app** (chat), **mobile app**, **marketing site**, **admin web app** (the org's ledger of routed work, policies, grants). This project ships UI kits for the **desktop app** and **admin web app**; the mobile app and marketing site are described in their companion docs and are not yet built here.

### Sources given
- Brand/system spec (the muniment design system doc, v1, July 2026) — transcribed into this project.
- Fonts: `SchibstedGrotesk-VariableFont_wght.ttf`, `CommitMono-400-Regular.otf`, `CommitMono-700-Regular.otf` (uploaded, self-hosted under `assets/fonts/`).
- No codebase, Figma, logo asset, or slide deck was provided. The mark is built to the spec's published geometry (§Identity), not copied from an asset.

---

## CONTENT FUNDAMENTALS

How muniment writes.

- **Sentence case everywhere.** Headings, buttons, labels, table headers (the ALL-CAPS table header is a tracked type treatment, not a casing choice for copy).
- **Buttons name the consequence**, as verbs: "Fork thread", "Revoke grant", "Publish policy". Destructive confirm buttons state the count/effect: "Revoke 3 grants".
- **Voice is second person, plain.** "Start one to see its route recorded here." No first-person assistant persona — there is no assistant character, avatar, or "I".
- **Errors: what happened + the next step, no apology.** The technical cause is set in mono ("cap must be ≥ current spend ($4.12)"). Never "Oops", never "please wait", never "!".
- **Numbers in records are exact, not rounded prose:** "$0.0041", not "less than a cent". The exception is screen-reader labels, which prefer words.
- **Time:** relative under 7 days ("3h ago"), absolute mono date after ("2026-07-07").
- **No exclamation marks. No hype.** Forbidden vocabulary in all UI copy: *AI, magic, supercharge, unlock, sovereign/sovereignty* (the marketing site may use "sovereignty" exactly once, per its doc).
- **No emoji in UI copy.** None, anywhere in product.
- **Register rule (the copy-and-type law):** if a string is evidence (could appear in an audit log), it is mono; if it is speech (could be said aloud), it is grotesque. Mixed in one line, only the record token switches families.

The vibe: understated, exact, unhurried. Nothing to prove.

---

## VISUAL FOUNDATIONS

- **Color.** Neutrals carry a faint green cast tying them to the accent. `paper`/`surface`/`faint`/`ink`/`muted`/`border` do ~all the work. Never pure `#FFFFFF`/`#000000`. **`signal` (verdigris) means computation** and is lint-restricted to an allowlist (see Color law below) — it is *not* used for buttons, links, focus, selection, badges, or historical charts. `oxide` (deny) and `ochre` (caution) are static admin-record colors: desaturated, never animated, never alarm chrome. No violet/purple, ever. No gradients on text or surfaces.
- **Type.** Two families, split by register. **Schibsted Grotesk** (400/500/600/700) for everything human. **Commit Mono** for everything that is a record — and it renders one step *smaller* than adjacent body text and **never bold**. Tabular lining figures in all tables and provenance lines. Scale (desktop/web): 12 / 13 / 15 (body) / 17 / 22 / 28; mobile adds a 30 large-title. Line-height 1.55 body, 1.3 headings, 1.45 mono. Tracking: headings −1%, body 0, tiny ALL-CAPS labels +4%. No display serif; no italic except semantic emphasis in user content.
- **Shape & radius.** Three radii only: **2** (chips, kbd), **6** (buttons, inputs, tool cards, table controls), **10** (panels, composer, bubbles, windows, modals). Nothing pill-shaped, no per-corner mixing.
- **Spacing.** 4 / 8 / 12 / 16 / 20 / 24 / 32 / 44 / 64. Internals prefer 8/12; layout gaps prefer 16/24/32.
- **Depth = hairlines.** Borders are 1px `border`. Shadows exist at exactly two levels: `shadow-window` (app windows, marketing frames) and `shadow-overlay` (popovers, modals). No glow, no colored shadows, no inner shadows.
- **Cards** are `surface`, 1px `border`, radius 6 or 10, no shadow unless they are a floating window/overlay. Card-on-card nesting never goes deeper than one level.
- **Backgrounds** are flat `paper`. No imagery washes, no patterns/textures, no ambient orbs/particles, no glassmorphism/backdrop blur.
- **Motion (complete inventory):** the mark's thinking state, the streaming underline appearance, the tool-status pulse (1.4s ease-in-out), panel/rail slide (180ms ease-out), popover fade + 2px rise (120ms), and the marketing hero sequence (once). Durations never exceed 300ms except the mark and the hero. **`prefers-reduced-motion` removes all of it** (the mark falls back to static verdigris). No parallax, scroll-jacking, skeleton shimmer (use static placeholder blocks), or spring physics.
- **Hover / press.** Primary button hover = 6% ink-shift (not opacity). Quiet controls hover to a `faint` fill. Press nudges 0.5px, no scale bounce. Rows and menu items hover to `faint`.
- **Focus.** 2px `ink` outline, 2px offset — **never signal.**
- **Transparency/blur:** used only for the modal scrim (a low-opacity ink wash). No backdrop blur anywhere.

---

## The color law (lintable)

`--signal` may be referenced **only** by these components:
1. The mark's thinking state
2. Streaming underline + caret on the active response line
3. Running-tool status (dot pulse + header text)
4. The route segment of the provenance line
5. Voice-polish flash in the composer (the `transform` chip)
6. Workflow-run-in-progress indicators
7. Admin: live routed-requests sparkline; running-workflow row pulse
8. Website hero request trace

Everything else — primary buttons, links, focus rings, selection, icons, badges, charts of historical data, the mark at rest — is ink, muted, or border. Treat violations as bugs, not taste.

---

## ICONOGRAPHY

muniment is deliberately icon-light; the type system and hairlines carry most of the load. Icons **accentuate** — they mark specific features, services, or concepts where the copy (often a bullet) doesn't quite land, and they differentiate sections (e.g. on the website). Use them **sparingly**: the icon is the exclamation mark at the end of the sentence, there to make people remember what we said, not to decorate. Simple by design — never complex, so they emphasize the message rather than distract from it.

- **No icon set is bundled or referenced in the spec.** Where product UI needs a glyph, it uses the smallest possible vocabulary: a status **dot** (running/complete/error), a caret for expand/collapse, `×` for remove/dismiss, `⌄`/`↵` for menu and enter. These are drawn as simple shapes or unicode, sized to the mono scale, colored ink/muted (signal only when a dot indicates live computation).
- **The one true mark is the milled ring** (see Identity). It is not an icon — it is the brand's presence, and its only animated appearance is the thinking state.
- **No sparkle/wand icons, no assistant avatar, no typing dots, no emoji.** These are explicitly forbidden.
- **kbd chips** render keys as mono text in a radius-2 faint chip, not as icon art.
### The icon set: Lucide

The standard icon set is **[Lucide](https://lucide.dev/icons)** — simple, single-weight line icons that match the system's hairlines. Never invent SVG icons or use emoji; pick the closest Lucide name. Use the `Icon` component (`components/icon/Icon.jsx`), which renders Lucide glyphs as inline SVG so `currentColor` applies.

- Stroke **1.75**, sizes **16–22px** inline. Don't mix stroke weights.
- Colored `ink` or `muted` via `currentColor` — **never signal** (signal means computation only).
- Load Lucide once on the host page: `<script src="https://unpkg.com/lucide@latest/dist/umd/lucide.min.js"></script>`.
- Still forbidden: sparkle/wand icons, assistant avatar, typing dots, emoji.

### Company logos: svgl

For third-party **company/brand logos**, pull from **[svgl](https://svgl.app)** — never redraw a company mark by hand. Logos are content, not iconography, and keep their own brand color where a mark requires it.

**Logo flag:** no muniment logo asset was provided, so none was invented. The wordmark is set in plain Schibsted Grotesk 600 lowercase; the mark is generated from the spec's geometry (see Identity).

---

## Identity: the milled ring

A ring with a milled edge — radius modulated by a 22-tooth wave (`r(t) = 16.5 + 1.6·sin(22t)` in a 48-unit viewBox), monoline stroke, round caps. Coins are milled so a clipped coin exposes itself: verification made visible.

- **At rest:** static, ink. The only form in chrome, lockups, headers, print.
- **Thinking:** verdigris, animated — an irregular ~70bpm breath, decorrelated eased spin, and a rare brighter trace. Appears **only during actual work**. All simultaneously visible thinking marks share one organism and animate in sync. Reduced-motion → static verdigris.
- **Lockup:** mark + `muniment` (Schibsted 600, lowercase, −1% tracking), no tagline.

Implemented in `components/mark/Mark.jsx` (`state="rest" | "thinking"`).

---

## Components

Built to the spec's §6 inventory. All ship both themes and full states; focus ring is 2px ink.

- **Mark** (`components/mark/`) — the milled ring, rest + thinking.
- **Button** (`components/controls/`) — primary (ink), quiet, outline. Never signal.
- **Input** (`components/controls/`) — input + textarea, composer variant, mono error line.
- **Chip** — model-pin, attachment, filter, kbd, transform (voice, the one signal chip).
- **Kbd** — keyboard key chip.
- **ProvenanceLine** (`components/records/`) — the recorded route; signal route segment + expandable receipt.
- **ToolCard** — a tool invocation as a record; running pulses signal, complete collapses.
- **DataTable** — the admin table grammar; deny rows strike through in oxide.
- **QueryBar** — mono `key:value` filter bar.
- **Toast** (`components/feedback/`) — bottom-center notice, one action, never signal.
- **Modal** — radius-10 confirm; destructive confirms gate on typing the object name.
- **Popover** — menu with 120ms fade + rise.
- **EmptyState** — one inviting grotesque sentence, optional quiet button.
- **Icon** (`components/icon/`) — Lucide wrapper; simple line icons used sparingly, ink/muted only.

### Intentional additions
- **Kbd** is split out as its own export alongside `Chip` (the spec lists it as a chip variant) so consumers can drop keys into prose without the chip wrapper. Same tokens, no new visual language.
- **Icon** wraps [Lucide](https://lucide.dev/icons) as the standard glyph set (the spec bans invented SVGs and emoji but names no set). Line-only, `currentColor`, never signal — no new visual language.

---

## Index / manifest

Root:
- `styles.css` — the single entry point consumers link; `@import`s the token + font files only.
- `tokens/` — `colors.css`, `typography.css`, `space-motion.css`, `fonts.css`, `base.css`.
- `assets/fonts/` — the three self-hosted webfonts.
- `components/` — `mark/`, `controls/`, `records/`, `feedback/`, `icon/` (each `Name.jsx` + `Name.d.ts` + `Name.prompt.md`, plus one `@dsCard` HTML per directory).
- `guidelines/` — foundation specimen cards (Colors, Type, Spacing, Brand).
- `ui_kits/desktop-app/` — interactive chat window (`index.html` + `Sidebar`/`Conversation`/`Composer`).
- `ui_kits/admin-web-app/` — routed-requests audit view (`index.html` + `Sparkline`).
- `SKILL.md` — Agent-Skills entry point.

The Design System tab renders every `@dsCard`-tagged file, grouped: Colors, Type, Spacing, Brand, Components, Desktop app, Admin web app.

---

## Caveats
- No logo asset was provided; the wordmark is plain type and the mark is generated from the spec geometry.
- Mobile app and marketing site UI kits are not built (their companion docs weren't provided).
- Signal-allowlist enforcement is documented as a lint rule but not shipped as executable lint config here.
