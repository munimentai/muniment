# muniment-desktop — ROADMAP

Phases mirror harness-spec §9. Muniment-cloud Phase 1 native auth and the cloud prerequisites for desktop chat are live as of 2026-07-11; client work that uses them must exercise the real contracts and must not introduce mocked production paths.

## M0 — Scaffold (done 2026-07-09)
- DONE — Tauri v2 desktop shell, specs, mockups, design reference, Rust/frontend test harnesses, and Linux/Windows/macOS CI gates.

## Phase 2 — Client core (§9 items 8–11)

### 8. Shell, auth, and entitlements
- DONE — Svelte 5/Vite shell, design tokens, signed-out/signed-in states, native installation-bound authorization, keychain persistence, refresh/session/revocation flows, live entitlement projection, access-device states, and server-unreachable recovery.
- The production contract is `/v1/auth/native/*`; generic OIDC remains test groundwork and is not the production handshake.

### 9. Pi sidecar and cloud chat
- DONE — verified Pi runtime acquisition/supervision, signed-in streamed chat, steering/follow-up, durable tool effects, provenance/receipts, inline tool cards, extension UI, permission-gate replay/answers, and safe interrupted-session resume.

### 10. Local Gemma sidecar
- DONE — supervised local runtime, pinned model lifecycle, verified/cancellable acquisition, rollback, native adapters, and Tauri install/status/cancel commands.
- DONE 2026-07-22 — first-use onboarding shows required local-model acquisition progress, readiness, and redacted background-retry status while keeping folder setup fail-open and AI proposal generation fail-closed.
- NEXT — release notice delivery after the approved bundled terms surface is available.

### 11. Attachments and local CAS
- DONE groundwork — pure-Rust content-addressed local store with atomic deduplication, constant-memory I/O/verification, stale-temp cleanup, journal reference accounting, retention, export, and compaction. Cloud file flow follows items 8d/9.

### Durable local run journal
- DECIDED — ADR 0002 makes a per-run append-only SQLite event journal authoritative; external effects are never silently re-executed and large bodies live in CAS.
- DONE — schema/envelope/atomic append, reducer/replay, Pi translation, deletion/collection/retention, deterministic export, crash-safe compaction, cursor-paginated run summaries, and authorized Linux companion `thread.list`/`thread.open` backed by redacted projections.
- No further journal-maintenance slice is selected.

### Capability vocabulary and provenance
- DONE — user surfaces say “capabilities” and receipts render only server-supplied route/model/cost/time/capability provenance.

## Phase 3 — Routing metadata + voice (§9 items 12, 15)
- Routing metadata carriage/policy integration follows item 9; the local classifier contract is complete.
- Voice direction remains Parakeet capture → Gemma polish → transforms, with Kokoro read-aloud and global hotkeys.
- DONE — pinned Parakeet/Silero acquisition and publication, sherpa-onnx packaging/bindings, fixed-capacity microphone capture, bounded utterance segmentation, safe VAD boundary, chunk-invariant dictation composition, desktop command/event wiring from native capture through recognition with redacted statuses, basic composer dictation controls/transcript insertion, a reproducible target-hardware evaluator, a bounded 100-utterance endurance mode, and composer press-and-hold dictation with Escape-to-cancel/restore.
- DONE 2026-07-19 — accepted ADR 0014 pins the `ggml-org/llama.cpp` release `b10068` CPU-baseline archives for all four native targets, with a complete-entry-manifest extraction contract and pre-spawn re-verification.
- DONE 2026-07-20 — ADR 0014 implementation through activation: pinned llama-server descriptors for all four targets, manifest-verified safe extraction and reusable tree verification, bounded download with staging/lock/pointer publication, pre-spawn re-verification at the spawn boundary, supervised llama-server activation with health readiness, and third-party notice content; the required Qwen3.5-4B descriptor rides the same verified path with background required-download acquisition deliberately detached from onboarding.
- DONE 2026-07-22 — the desktop dictation-polish command, composer integration, and four post-dictation transforms against the activated resident server.
- DONE 2026-07-22 — fixed system-wide hold-to-talk shortcut foundation with press/release capture, safe busy-state handling, redacted registration failure, and teardown cleanup.
- DONE 2026-07-22 — rapid double activation promotes the composer button or global shortcut to hands-free dictation until Escape or the next activation.
- DONE 2026-07-22 — users can rebind the global voice shortcut with validated modifier-plus-key capture, collision-safe rollback, local persistence, and accessible shortcut exposure.
- DONE 2026-07-22 — accepted ADR 0015 pins the CPU-only Kokoro runtime, immutable model/voice artifacts, deterministic English text-processing and segmentation boundary, packaging contract, cancellation/privacy rules, and target-hardware validation gates.
- DONE 2026-07-22 — the pinned Kokoro model and voice bundle have a bounded resumable acquisition path and conflict-safe publication/current-revision resolution with full re-verification.
- DONE 2026-07-22 — normalized Kokoro source text packs into contiguous, bounded initial synthesis ranges against an injected exact phoneme/token-count boundary, including protected-run and Unicode-safe hard-split handling, followed by ADR 0015's one-pass sub-20-token repair against the same boundary.
- DONE 2026-07-22 — final repaired source ranges carry exact phoneme strings and token IDs through the same injected accent-aware boundary.
- DONE 2026-07-25 — `pack_initial_ranges` no longer re-evaluates the injected phonemize boundary for every same-rank candidate, so boundary calls are bounded rather than quadratic in the number of natural boundaries.
- No further ungated Kokoro slice is selected. Native runtime/G2P packaging remains subject to ADR 0015's redistribution approval gate; synthesis, playback, response controls, and physical target-hardware runs remain separately deferred.

## Companion execution surfaces (§13)
- DONE E0/E0.5 — governed protocol, pairing, authorization, idempotency, bounded transport, ADR 0011 repository/lane decision, and ADR 0012 per-user runtime-service direction.
- DONE E1 — Rust CLI lane; Linux discovery/pairing; thread list/open; run start with ordered progress and flow control; typed permission handling; tool details; and receipts.
- DONE E2 — canonical fixtures and drift checks; TypeScript package/CI; cross-platform discovery, framing, pairing, authorization, thread documents, typed run streaming, workspace/current-file context, receipts, and fail-closed permission interaction.
- VSIX sideload testing is sufficient; marketplace publication and official fork-compatibility claims remain owner-gated. The runtime service is the sole runtime/session/journal owner.

## Browser-control runtime capability (§6.8)
- OWNER GO 2026-07-16 — v1 browser-only actuator line is governed runtime capability, not a companion surface.
- DONE E0–E14 — extension-only real-profile architecture through injected memory-only pairing and MV3 authorized relay.
- BLOCKED E15 — desktop-to-extension handoff waits for the owner to upload a real package to the private Chrome Web Store draft and provide its public key. Do not generate a replacement key/ID. Reserved extension ID: `cdedcfbgomnhfpifpgdlpfkkanaofkjd`.
- After the matching key lands, compose and validate E15 as one slice; runtime tools, policy, receipts, and kill switch follow. Store publication/upload are owner-gated; v1 excludes OS, filesystem, and other-app control.

## Onboarding, memory, and the Muniment Home (owner rulings 2026-07-19)
- RATIFIED — visible human-editable Markdown Home under `memory/`, `agents/`, `projects/<name>/`, `sessions/`; files are source of truth, indexes are rebuildable caches, no hidden primary store or Muniment sync, semantic transcript names and visible retention.
- RATIFIED — desktop has an unskippable first-run directory picker (default `Documents/Muniment`); CLI/VS Code use the opened directory by default and lazily create user Home for cross-project need. Honor nearest `AGENTS.md`; instructions and memory remain distinct.
- RATIFIED — required Qwen3.5-4B instruct Q4 GGUF serves onboarding triage and front-door routing, via the verified ADR 0008/0014 path. Import is consent-gated with preview, provenance, verbatim originals, and a user-confirmed report before writes; folder setup fails open and only AI-dependent features fail closed.
- RATIFIED — agent system prompt has five binding rules, hand-written/versioned/eval-gated base v0, and a bounded audited labeled-data tail.
- DONE 2026-07-20 — rulings codified in harness-spec §15/§16; required Qwen descriptor and background acquisition landed.
- DONE 2026-07-21 — first-run Home picker/scaffold; bounded ZIP preview and manifest review; bounded extraction of explicitly selected entries with verbatim text and stable provenance; consent checklist and Tauri extraction bridge retaining approved content only in transient pre-triage state. No Home writes or model calls occur in these slices.
- DONE 2026-07-21 — typed, bounded local-model triage request/report-validation contract over approved extracted entries, plus its typed Tauri invocation against the resident local model.
- DONE 2026-07-21 — structured local-triage report review, approved-source display, redacted retry handling, and explicit in-session confirmation; no Home writes occur in this slice.
- DONE 2026-07-22 — confirmed triage reports, starter-agent proposals, and provenance-bearing verbatim originals compile into a deterministic bounded Home write plan without filesystem access.
- DONE 2026-07-22 — onboarding reports required local-model progress/readiness without blocking Home setup and gates local proposal generation until AI features are available.
- DONE 2026-07-24 — a confirmed bounded Home write plan persists conflict-safely without overwriting user files, with rollback, concurrency serialization, and symlink defenses.
- DONE 2026-07-24 — compilation and persistence are exposed through one typed Tauri completion command with structured invalid-input, destination-conflict, and save-failure results.
- DONE 2026-07-24 — the confirmed-proposal frontend invokes the native completion command once, handles success/invalid input/save failure, and recovers destination conflicts by choosing another Home without repeating preview or triage.
- DONE 2026-07-25 — a failed post-publish temporary-file cleanup no longer promotes itself into the save result, so the onboarding-import files that were just written correctly survive.
- DONE 2026-07-24 — the artifact-rail control is a real signed-in frontend shell with button/platform shortcut toggle, Escape close, lifecycle reset, and an honest empty state.
- DONE 2026-07-24 — the open artifact rail is adjustable from 380–560px with an accessible pointer/keyboard splitter and viewport-aware bounds.
- DONE 2026-07-24 — the sidebar collapses with `⌘\` / `Ctrl \` in 180ms to a 52px tooltip icon rail and remembers its state per device.
- DONE 2026-07-25 — streamed replies announce one coarse string per run phase instead of re-reading the whole thread to screen readers on every chunk.
- DONE 2026-07-25 — the provenance line matches the owner mockup: `route → model · cost · time` with `--signal` bound to the route field rather than to whichever part is first, tabular numerals, and 11.5px mono.
- DONE 2026-07-25 — `docs/spec/02-desktop-app.md` §4's composer grows from two rows to a ten-line cap and then scrolls, holding the polish overlay and the thread's scroll position steady through every resize, including rail drags and sidebar collapse.
- DONE 2026-07-25 — §3.2's quiet per-message action row ships Copy alone, revealed on hover or focus, holding its place in the tab order and announcing through its own live region rather than the run-phase one. Fork, share, and retry still wait on the thread/branching contract.
- DONE 2026-07-25 — §2.3/§4's Send is the ink primary action in both composer states, and the composer takes focus once on entry to the signed-in workspace.
- DONE 2026-07-25 — §1.2's color law holds in code: one global 2px ink focus ring at offset 2, no accent on at-rest thread dots or the model-download fill, and `src/styles/signal-allowlist.test.js` is §7's promised lint over every component's styles and markup.
- DONE 2026-07-25 — §1.3/§2.1's per-device System / Light / Dark override lives in the profile popover, applied before mount and persisted per device.
- DONE 2026-07-25 — `src/styles/base.css` now carries the ground-truth bundle's element defaults (`::selection`, ink links, heading weight/leading/tracking, mono tabular numerals) and one blanket `prefers-reduced-motion` rule, so motion compliance no longer depends on a hand-maintained transition list.
- DONE 2026-07-25 — light `--signal` is the spec's `#2A7264` in both the OS-default and explicit-light blocks, with the token matrix guarded by `src/styles/contrast.test.js`.
- DONE 2026-07-25 — `syncComposerHeight` no longer uses focus as a proxy for "the user typed this", so dictated text written past the ten-line cap scrolls into view with the composer focused.
- AUDIT 2026-07-25 (code review over everything merged since the 2026-07-09 audit) — three confirmed defects filed, all three now landed: the Home-import cleanup-to-rollback promotion, the focused-composer dictation scroll, and the quadratic Kokoro packing boundary calls. Noted, not filed: `natural_candidates` (`src-tauri/core/src/kokoro/packing.rs:375-379`) suppresses the rank-3 word boundary after *any* closer, where ADR 0015 §segmentation only suppresses a closer "already covered by rank 1 or 2"; the ADR wording and the intended behaviour want an owner/author call before the one-line change.
- DONE 2026-07-26 — the signed-in workspace no longer paints the centered sign-in lockup or `shell v0.0.1` behind the conversation; `.signed-frame` is a real rule.
- DONE 2026-07-26 — the Appearance buttons take the global ink focus ring, and the §1.2/§6 guard now walks every component under `src/` rather than `base.css` alone, with `.artifact-divider` as the one documented exception.
- DONE 2026-07-26 — the onboarding surface lives in `src/lib/Onboarding.svelte`; `src/App.svelte` is down to ~1570 lines.
- DESIGN CALL 2026-07-26 (planner, §2.2 "active line") — the active line is the last line box of the streaming response's prose, and the 2px signal rule runs from that line's start to the caret, following the text as chunks arrive and leaving when the run leaves `streaming`. CSS cannot express it: measured against the built bundle at 1280×820, a three-line streaming reply reports three line rects (`x: 390`, `y: 539/563/586`) and Chromium paints `border-bottom: 2px solid rgb(42,114,100)` on every one of them — `box-decoration-break` governs the inline-direction edges only — so the rule has to be a measured overlay driven by a pure geometry helper. §2.2's 92% measure applies to the response prose; tool cards and the receipt stay at the 760px column width because they are records, not prose.
- DONE 2026-07-26 — §2.2's streaming rule is a measured overlay that marks the active line alone (`src/lib/streaming-underline.js`), and response prose takes the 92% measure while tool cards and the receipt keep the 760px record column.
- DONE 2026-07-26 — §1.5/§4 shape and depth are tokens: every component references the radius scale (`--radius-panel` included), the two improvised `color-mix` shadows are gone in favour of black-based `--shadow-window`/`--shadow-overlay` identical in both themes, and `src/styles/shape-scale.test.js` lints both scales the way `signal-allowlist.test.js` lints §1.2.
- DONE 2026-07-26 — the profile popover fits its window: `src/lib/AccessPanel.svelte` is a fixed header, one scrolling content region, and a fixed footer holding Sign out, in 02-desktop-app §2.1's order, with structural layout assertions in `src/lib/access-panel.test.js`. Planner render 2026-07-26 of the built bundle at the shipped 1100×720 default confirms the fix on screen: the popover bottoms out at 639px, Sign out renders at y≈603–629, and only `.access-content` scrolls.
- DONE 2026-07-26 — shell chrome keyboard hints: the sidebar chips derive their modifier from the same platform helpers the tooltips use, the titlebar artifact toggle carries a visible “Artifacts” label beside its chip, and every chip sits in the `kbd` mono-muted register. Confirmed in the same planner render.
- DONE 2026-07-26 — the undefined-class lint (`src/styles/class-usage.test.js`, the MUNIDESK-508 shape) walks every component's markup against its style rules, and the four pre-existing undefined classes (`voice`, `attach`, `import-card`, `revoked`) are cleared.
- OBSERVED 2026-07-26 (planner render of the built signed-in bundle) — the profile popover mis-titles itself. Its dialog `aria-label`, fixed-header `<h2>`, and close-button label all say “Your access” — with `Snapshot v7` as the header's subline — although the popover now holds Appearance, the voice shortcut, and Sign out, and 02-desktop-app §2.1 makes “Your access” one *section* of the profile popover, not its name. Inside, the entitlement section then repeats `Your access` as an `<h3>` and immediately stacks a `Your groups` sub-label beneath it, so the rendered popover shows two adjacent mono labels with no content between, both redundant with the groups list that follows. Separately, the parked §2.1 org-fallback observation is confirmed in the same render: without `organization_display_name`, `.profile-button small` wraps the raw org UUID to two lines in the 260px sidebar.
- NEXT (selected 2026-07-26; (b) also edits `src/lib/AccessPanel.svelte`, which (a) sweeps — (a) lands first, (b) rebases onto it) —
  - (a) **§1.4 type-scale sweep + lint.** The type law is the last of the three §1 laws without a guard. Current inventory (re-verified 2026-07-26; the earlier 16-count predates the popover/chrome slices and missed the `font:` shorthands): 9 raw `font-size` declarations — App.svelte 30/18/10/11/11px, AccessPanel.svelte 18/11px, Onboarding.svelte 28/13px — plus 6 raw sizes inside `font:` shorthands — App.svelte 12.5px tool card and 11.5px provenance, AccessPanel.svelte 11/10/11px, Onboarding.svelte 13px. Values that should resolve to the approved 12/13/15/17/22/28 scale become scale-token references (18→17, 30→28); sizes the spec requires off-scale become named type tokens rather than raw-value lint exceptions (§2.2's 11.5px mono provenance register at minimum), and `src/styles/type-scale.test.js` then permits component font sizes through `var(--text-*)` only, in the shape/signal lint idiom, covering both `font-size` and the `font:` shorthand.
  - (b) **§2.1 profile popover register + org fallback.** The popover's fixed header becomes the identity (display name grotesque, `org · role` mono muted — the sidebar block's register), the dialog and close labels follow, `Snapshot vN` moves into the Your-access section as the mono record it is, the `Your groups` sub-label goes away, and `.profile-button small` becomes single-line ellipsized with the full string on `title` so a UUID fallback truncates instead of wrapping.
- OBSERVED, NOT YET SELECTED (design-spec audit 2026-07-25, re-checked 2026-07-26 against the rendered bundle) — the ring's thinking state is a fixed opacity fade rather than §1.8's decorrelated breath/spin/trace with a completing done-gesture, and `src/lib/mark.js` reads a fixed path so milling depth cannot flex; this one still needs a design call on how much of the reference engine to port. `--space-*` tokens are absent, and a spacing sweep would touch nearly every declaration in the app, so it stays parked rather than half-done. The titlebar has no `⌘K` hint, and should not get one while the palette itself is deferred.
- No further artifact-rail content slice is selected. Artifact data, rendering, persistence, sharing, and cloud contracts remain deferred to Phase 4 item 19; select the first real-contract slice only when its authoritative event/data boundary is specified. CLI/VS Code defaults wait for workspace memory semantics.
- Open items: headless/server CLI stance owner ruling; final model promotion waits on MUNIQA routing eval; Gemma 4 E2B remains a future owner-gated contingency pending llama.cpp PLE support. MUNICLOUD model artifact proxy/redirect is a ripple.

## Desktop QA automation
- RATIFIED — layered frontend browser coverage plus installed-nightly real-app validation on serialized pve01 desktop-ci VMs; Windows/Linux use WebdriverIO, macOS install/launch smoke plus screendumps/manual owner pass.
- DONE — ADR 0013; canonical runner contract; Linux `.deb` real-sign-in smoke; Windows MSI real-sign-in smoke; macOS install/launch smoke; stable Linux/Windows JUnit artifacts.
- DONE 2026-07-22 — installed Linux `.deb` and Windows MSI each submit one unique prompt through authenticated production chat and verify a non-empty assistant turn plus server receipt route under a bounded deadline; rendered production conversations are excluded from uploaded screenshots.
- DONE 2026-07-22 — a failed installed-nightly platform job opens or updates one SHA-scoped triage issue containing metadata and links to the private redacted diagnostics.
- DONE 2026-07-23 — installed Linux and Windows nightly validation each attach a generated-from-source PNG, submit it through authenticated production chat, and require the model to return the image-only token; fixtures remain outside uploaded diagnostics. Test one pinned finalized installer SHA; no mocked production paths. Signing/notarization and web/API suite remain out of scope.
- OBSERVED 2026-07-25 — the signed-in-frame defect above survived every layer of this suite, because the nightly screenshots deliberately exclude rendered production conversations and the frontend suite asserts presence rather than absence.
- DECIDED 2026-07-26 — do not add a headless browser to the CI gate for it. The smoke job runs on the self-hosted runner with no browser installed, and the whole MUNIDESK-508 shape (a class in markup that no rule anywhere defines) is statically detectable, so the class-usage lint selected above buys the same coverage inside the existing vitest job with no new infra and no flake. Planning-time product inspection keeps rendering the built bundle against a stubbed signed-in `window.__TAURI__` in headless Chromium; that is a planner tool, deliberately not a CI layer.
- OBSERVED 2026-07-26 — the clipped-popover defect above is invisible to both existing layers, for the mirror image of the 2026-07-25 reason: jsdom has no layout, so three tests in `src/App.test.js` can click a `Sign out` button that never renders on screen, and the nightly screenshots exclude the signed-in surfaces it lives on. The 2026-07-26 decision stands — no browser in the CI gate — so the ticket carries a structural assertion (the action lives outside the scrolling region, in the existing style-block-parsing idiom) rather than a geometric one, and planning-time rendering stays the layer that finds this class of defect.
- VERIFIED 2026-07-26 — that split worked as designed: the popover restructure landed with its structural assertions, and the planner's stubbed-Chromium render confirmed the geometric outcome (popover inside 720px, Sign out on screen) the CI layers deliberately cannot see.

## Stable release and distribution
- DONE — rolling nightly one pinned SHA across Linux, signed Windows, unsigned macOS; owner-triggered SemVer promotion copies exact green artifacts and labels unsigned macOS.
- DONE groundwork — promotion hashes MSI, generates `Muniment.Muniment` WinGet manifest, and opens a draft fork PR. Fork/token setup, publication, Homebrew, and Apple signing remain owner-gated. No monetization or promotional surface is implied.
- DONE 2026-07-24 — macOS Developer ID signing, notarization, and stapling are pre-staged in the nightly release path behind the same desktop-ci env-injection seam Windows signing uses (`.github/build-macos-app.mjs`, `.github/lib/macos-signing.mjs`): with no Apple credentials the build stays a clean unsigned no-op, a partial credential set fails fast naming only the missing variables, and `docs/macos-signing.md` records the six vault keys plus the codesign/notarize/staple/Gatekeeper verification checklist. Apple enrollment Y5DUNHQA74 is still in review; switching signing on is secrets-only, and public download/install docs and promotion stay owner-gated.

## Phase 4+ — Org surface (§9 items 16–19)
- Remote MCP consumption, local stdio allowlist, capability install flow, artifact side panel, and projects with redaction (`output withheld · connection not granted`).

## Standing gates
- Code PRs: structure smoke, frontend/Rust tests, applicable path-scoped checks or desktop builds. Markdown-only PRs gate on structure smoke; pushes to `main` run smoke only.
- Build desktop targets only unless an accepted ADR changes companion checks.
- Distribution accounts, marketplace/store publishing, production launch, and publicity remain owner-gated.
