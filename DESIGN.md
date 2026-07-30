# muniment-desktop — Design standard

1. **Owner mockups (ground truth): [docs/mockups/desktop/](docs/mockups/desktop/)**
   — adhere closely. Mobile mockups ([docs/mockups/mobile/](docs/mockups/mobile/))
   are design ground truth for the phased mobile companion app (harness-spec §12).
2. **The written system:** [docs/spec/design-spec.md](docs/spec/design-spec.md)
   §1 (tokens/laws/identity) + §2 (desktop app) and
   [docs/spec/02-desktop-app.md](docs/spec/02-desktop-app.md).
3. **The ring:** [docs/design-reference/ring/muniment-ring-pulse-spin.html](docs/design-reference/ring/muniment-ring-pulse-spin.html)
   is the reference geometry + thinking-animation engine (§1.8). At rest:
   static ink. Thinking: verdigris breath/spin/trace, all visible instances
   in sync. ≤20px: solid two-edge reduction.
4. **Remote Control:** [docs/design-reference/remote-control-ux.md](docs/design-reference/remote-control-ux.md) records the pending desktop session UX reference.

Key grammar (short form; design-spec §2 is authoritative): sidebar /
thread / artifact rail (⌘J) layout; user messages right in `faint` bubbles,
responses plain on `paper` (no bubble, no avatar); streaming = 2px signal
underline + caret, never dots; tool activity = inline mono cards with
status-dot pulse while running; provenance line under every response (mono,
11.5px, route in signal); composer focus shifts border to `muted`, never
signal. Radius 2/6/10. Both themes first-class; `prefers-reduced-motion`
respected everywhere.

Conversation, tool, permission, and receipt state is rebuilt from the
append-only local run journal (ADR 0002). Reopen reduces committed events;
snapshots are disposable, and uncertain external effects require explicit
attention rather than silent replay.

The signed-in shell has one workspace `h1`, a headed thread list, and a transcript region named for the open thread.
