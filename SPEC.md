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
- [03-mobile-app.md](docs/spec/03-mobile-app.md) — design ground truth for the
  phased mobile companion app (harness-spec §12); where mobile is engineered
  (this Tauri workspace vs a separate repo) is the §12.5 repo-strategy ADR,
  owner-gated.

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
11. **Local run state is event-sourced.** The append-only SQLite journal in
    [ADR 0002](docs/decisions/0002-event-sourced-run-journal.md) is authoritative
    for live/resumed sessions; UI, receipts, and relay are rebuildable
    projections. This contract precedes, but does not implement, item 9.
12. **Public-evidence rule (docs sync, adopted 2026-07-13).** Any PR that
    changes what an outside user can see or do — public API endpoints or wire
    behavior, install/distribution channels or flags, authentication flows, or
    other externally observable behavior — MUST update this repo's
    public-evidence documentation IN THE SAME PR: `docs/public-evidence/`.
    Evidence files are written to be lifted verbatim into the public docs site
    (muniment.ai/docs): plain factual reference prose, copy-pasteable commands
    and config, no roadmap speculation, no internal codenames, no pricing or
    monetization content (owner-only). The reviewer blocks a PR that changes a
    public surface without updating evidence. Merged evidence changes are
    picked up automatically by the site lane — do not file site tickets by hand.

## Production-ready gates (release gate)

A desktop release is production-ready when every criterion below holds.
Each criterion names its enforcing artifact. A criterion whose enforcer
reads "gap" is open work: closing it is a gate change first and a symptom
ticket second. Declaring readiness is a gate reading, never a judgment
call.

1. The nightly three-platform e2e run is green with a readable evidence
   envelope on every platform. Enforcer: .github/workflows/nightly.yml,
   with linux-e2e-report consumed by the factory tester.
2. The PR compile and build matrix is green on linux, windows, and macos.
   Enforcer: the desktop-compile and desktop-build jobs in ci.yml.
3. Windows installers are signed and verified. Enforcer:
   test/windows-installers.ps1 in the desktop-build job.
4. macOS builds are signed, notarized, and stapled once Apple clears the
   enrollment. Owner-gated. Enforcer: docs/macos-signing.md checklist plus
   the pre-staged .github/lib/macos-signing.mjs fail-fast behavior.
5. A release promotes only the exact bytes of a green nightly SHA.
   Enforcer: .github/lib/release-promotion.mjs.
6. UI copy obeys the forbidden-vocabulary law, records render in mono, and
   the provenance line is present on every reply. Enforcer: gap - no lint
   or test covers laws 4, 5, and 6 today.
7. An update path exists or is explicitly deferred by ADR. Enforcer: gap -
   no updater and no ADR records the deferral.

## CI

Desktop builds run on ephemeral pve01 VM clones via the `desktop-ci` driver
(one VM at a time; linux 10012 / windows 10011 / macos 10013 templates —
homelab docs/infra-notes.md "Desktop-CI GLUE built"). CI-only gate. The
lint/structure smoke runs on the shared self-hosted runners (no Docker, no
GUI there — real builds happen in the VMs).

## Journal-idea notes (cross-lane content pipeline)

`docs/journal-ideas/` holds standalone notes on shipped features,
engineering lessons, and agent-struggle patterns — raw material for the
public journal on muniment.ai (rules in docs/journal-ideas/README.md).
Merged notes flow to the site lane automatically. Notes must be safe to
publish nearly verbatim: no secrets, no security-sensitive internals.
Writing a note when a PR ships something notable is encouraged, never
blocking.
