# muniment-desktop — Design standard

1. **Owner mockups (ground truth): [docs/mockups/desktop/](docs/mockups/desktop/)**
   — adhere closely. Mobile mockups ([docs/mockups/mobile/](docs/mockups/mobile/))
   complete the brand story; never engineered.
2. **The written system:** [docs/spec/design-spec.md](docs/spec/design-spec.md)
   §1 (tokens/laws/identity) + §2 (desktop app) and
   [docs/spec/02-desktop-app.md](docs/spec/02-desktop-app.md).
3. **The ring:** [docs/design-reference/ring/muniment-ring-pulse-spin.html](docs/design-reference/ring/muniment-ring-pulse-spin.html)
   is the reference geometry + thinking-animation engine (§1.8). At rest:
   static ink. Thinking: verdigris breath/spin/trace, all visible instances
   in sync. ≤20px: solid two-edge reduction.

Key grammar (short form; design-spec §2 is authoritative): sidebar /
thread / artifact rail (⌘J) layout; user messages right in `faint` bubbles,
responses plain on `paper` (no bubble, no avatar); streaming = 2px signal
underline + caret, never dots; tool activity = inline mono cards with
status-dot pulse while running; provenance line under every response (mono,
11.5px, route in signal); composer focus shifts border to `muted`, never
signal. Radius 2/6/10. Both themes first-class; `prefers-reduced-motion`
respected everywhere.
