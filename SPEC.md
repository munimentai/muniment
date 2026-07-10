# muniment-desktop — SPEC

The muniment desktop client: Tauri v2 shell + Pi sidecar + local model
sidecar + on-device voice. Thin client to the muniment cloud — **no
serverless/solo mode exists** (harness-spec non-goal). Closed source.

**The canonical spec is vendored, verbatim, in [docs/spec/](docs/spec/):**

- [harness-spec.md](docs/spec/harness-spec.md) §6 (desktop client — THE spec
  for this repo), §5 (routing: the local model classifies, it never routes),
  §2 (architecture), §9 (build order this repo follows).
- [02-desktop-app.md](docs/spec/02-desktop-app.md) +
  [design-spec.md](docs/spec/design-spec.md) §2 (layout, thread grammar,
  composer, voice states, projects) and §1 (brand foundation, color law,
  tokens, the milled ring §1.8).
- [03-mobile-app.md](docs/spec/03-mobile-app.md) — **mockup only, never
  engineered** (non-goal; screens exist for the brand story).

**Design ground truth = the owner-built mockups in
[docs/mockups/desktop/](docs/mockups/desktop/)** (adhere closely). Ring
reference implementation: [docs/design-reference/ring/](docs/design-reference/ring/).

## Operating Constraints (LAW — reviewer enforces on every PR)

1. **The app requires the control plane.** No offline mode, no fake-degraded
   mode; server-unreachable shows the honest full-surface notice
   (design-spec §2.6). Feature work that needs cloud endpoints not yet
   shipped is BLOCKED, not mocked — coordinate via the ROADMAP phase gates.
2. **No provider API keys on this machine, ever.** Short-lived session
   tokens + the user's LiteLLM virtual endpoint only (harness-spec §8).
   Entitlement snapshots are display hints — the server enforces.
3. **Color law (design-spec §1.2):** color means computation. `--signal`
   only on: mark thinking state, running-tool pulse, streaming underline +
   caret, provenance route segment, voice polish flash, workflow-run
   indicators. Ink-on-paper everything else. The §1.6 anti-patterns are
   hard PR fails (no gradients, violet, glassmorphism, typing dots,
   avatars, sparkles, emoji, pill radius).
4. **If it's a record, it's mono.** Provenance lines, tool cards, costs,
   model names, paths → Commit Mono; conversation → Schibsted Grotesk.
5. **Forbidden vocabulary in all UI copy:** no "AI", no "magic"/
   "supercharge"/"unlock", no "sovereignty". Errors state what happened +
   next step, never apologize (§1.7).
6. **The provenance line ships in v1 and is never optional** (§2.2) — under
   every response, with an expandable receipt defined as
   `route · model · cost · time · capability@version[, ...]`.
7. **Sandbox honesty (harness-spec §6.5):** permission gates by default;
   full-auto only with `sandbox.full_auto` + real isolation (bubblewrap /
   Seatbelt); NEVER promise laptop isolation on Windows.
8. **Voice is fully on-device** (§6.7) — zero voice bytes leave the machine.
   Any PR that routes audio to a network endpoint is wrong by construction.
9. **Platform chrome follows the OS** (design-spec §2 platform note); brand
   tokens identical across platforms.
10. **Monetization and launch/publicity are owner-only.**

## CI

Desktop builds run on ephemeral pve01 VM clones via the `desktop-ci` driver
(one VM at a time; linux 10012 / windows 10011 / macos 10013 templates —
homelab docs/infra-notes.md "Desktop-CI GLUE built"). CI-only gate. The
lint/structure smoke runs on the shared self-hosted runners (no Docker, no
GUI there — real builds happen in the VMs).
