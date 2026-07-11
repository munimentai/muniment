# 0007 — Owner-gated mobile companion repository and stack strategy

- Status: proposed — awaiting OWNER RATIFICATION
- Date: 2026-07-11
- Context: ROADMAP standing gates; harness-spec §12.5

## Context

Harness-spec §12 makes mobile a phased companion to the control plane, never a
peer of the desktop execution surface. Before any M0 scaffold exists, §12.5
requires a decision between a separate mobile repository and mobile targets in
this Tauri workspace, plus a framework and CI strategy.

This repository's code PR gate runs structure and unit checks followed by
Linux, Windows, and macOS desktop builds in `.github/workflows/ci.yml`. The
platform builds use the one-VM-at-a-time `desktop-ci` driver and therefore run
sequentially. Adding mobile targets here would couple mobile changes and their
release schedule to that three-platform desktop gate. At the same time, mobile
is prohibited from running local models, local MCP servers, sandboxes, or user
keys. It uses short-lived session tokens and control-plane APIs, so useful
reuse of `src-tauri/core` is expected to be small.

## Decision proposed for owner ratification

Create a separate **`muniment-mobile` repository**, with its own Plane/factory
lane, ownership boundary, CI gates, and release cadence. Build the iOS and
Android clients with **React Native using Expo as the framework**, retaining
platform-specific files or native modules where the product specification
requires different behavior.

This is a proposal, not an accepted decision. The owner must record explicit
ratification before any action described here becomes authorized.

### Repository strategy

**A. Separate `muniment-mobile` repository — recommended.** Mobile work gets a
clear owner and factory lane, independently gated jobs, and a release cadence
that can ship selected TestFlight/internal-track versions without waiting for
or destabilizing desktop releases. The boundary matches the architecture:
mobile consumes cloud contracts and does not embed the desktop's execution
runtime. It also prevents ordinary mobile PRs from paying this repository's
sequential Linux, Windows, and macOS desktop build cost.

The cost is deliberate duplication at the product boundary. Design tokens,
copy laws, and relevant product documentation should remain canonical here and
be exported as a small versioned, generated artifact or synchronized by an
explicit reviewed update in the mobile repository. They should not be shared
through a source-level dependency on the desktop app. API schemas may likewise
be generated from the control-plane contract. This requires version discipline
and can allow temporary drift, but keeps releases and build systems independent.

**B. Mobile targets in this Tauri workspace — rejected.** A monorepo could
share CSS/webview components, documentation, and selected Rust types directly,
and one change could update desktop and mobile together. Those benefits are
weaker than they first appear: docs/spec/03-mobile-app.md requires native
navigation and platform behavior, while harness-spec §12 removes the local
models, MCP, sandbox, and key-handling responsibilities that make the desktop
Rust core valuable. Mobile-only changes would also share ownership and queue
with the desktop lane, and code changes would be exposed to the existing
three-platform desktop PR gate unless CI classification became substantially
more complex. Independent mobile distribution would still require separate
signing and release machinery inside the same repository.

### Framework strategy

**React Native with Expo — recommended.** React Native renders platform-native
UI primitives, recommends using a framework for new applications, and permits
platform-specific implementations. Expo supplies the application framework,
development workflow, and access to native projects/modules without requiring
the product to make hosted build service use part of this decision. This fit is
important because docs/spec/03-mobile-app.md requires iOS large-title headers,
collapsing navigation and swipe-back, but Android Material navigation and
predictive back; it also calls for platform-specific new-thread controls,
native share behavior, bottom sheets, long-press actions, light/dark handling,
and later native speech integration. Shared React Native screens can preserve
Muniment's tokens and conversation grammar while platform files or modules own
those edges.

The strongest tradeoff is a TypeScript/JavaScript runtime and native dependency
surface distinct from the desktop's Svelte/Rust stack. Complex OS integrations
may still require Swift and Kotlin, Expo SDK upgrades require coordinated
maintenance, and pixel-identical behavior cannot be assumed across platforms.
Those costs are preferable to forcing a webview abstraction over a specification
that explicitly gives each platform a say at the edges.

**Tauri v2 mobile — rejected for the companion.** Tauri can reuse a web UI and
Rust where it is genuinely shared, and its mobile plugin model bridges Swift on
iOS and Kotlin on Android. It would align with the desktop toolchain. Here,
however, little execution core is eligible to move to the phone, while the most
important mobile requirements are native navigation, platform interactions,
notifications, authentication, and later speech. Choosing Tauri mainly for
reuse would optimize for code this client must not carry and leave more custom
plugin and native-shell work at the product's most visible boundary.

**Native Swift/SwiftUI plus Kotlin/Jetpack Compose — rejected for M0.** Separate
native clients offer the most direct platform behavior, accessibility, and API
access. They also duplicate screen, state, networking, authentication, receipt,
and design-token work from the first read/chat slice and require two specialist
implementation/review paths. The specification's platform differences are
material but bounded enough for React Native platform modules and files.

**Flutter — rejected.** Flutter offers one mature cross-platform codebase and
strong custom rendering, but its rendered widget system is a poorer default for
the requirement to follow native navigation idioms and controls. It also adds a
third language/toolchain without offering the desktop-web reuse of Tauri or the
native-primitive model of React Native.

## Scope and phase boundaries

The owner-provided cloud prerequisites are shipped: **MUNICLOUD-139 server-side
thread store**, **MUNICLOUD-140 chat proxy**, **MUNICLOUD-142 relay spec**, and
**MUNICLOUD-145–155 native-client auth**. They bound M0; this ADR does not reopen
or redesign that cloud work.

M0 remains companion read/chat only: OIDC login, thread list/view and chat,
read-only inbox and entitlement peek, and push notifications, distributed only
through TestFlight/internal tracks. It gains no local execution capability.
The sequence remains unchanged: M1 adds the live desktop-session mirror and
remote permission responses after the owner supplies its ninth-screen design;
M2 adds mobile voice and artifact-viewer polish; M3 starts or steers server-side
Flue sessions. At every phase, mobile is never a local execution surface: no
local models, local MCP servers, local sandboxes, or user keys run or reside
there.

## CI and distribution requirements

No CI change is made by this ADR. After ratification, mobile must have
independently gated iOS and Android jobs in the selected `muniment-mobile` repo
and factory lane, rather than extending this repository's desktop PR gate.

- iOS builds require the existing macOS `desktop-ci` template plus Xcode and
  owner-controlled signing/provisioning access.
- Android builds may use the existing Linux `desktop-ci` template plus the JDK,
  Android SDK/NDK, and pinned Android tooling.
- Both jobs must run deterministic dependency installation, lint/type checks,
  tests, and unsigned or appropriately provisioned build verification before
  any distribution job. Distribution is a separately protected action, not a
  side effect of a PR build.

The owner controls the Apple Developer account and the App Store Connect roles
that manage TestFlight. Uploading a build and making it available to testers are
owner-gated distribution actions. Production launch, store submission, public
availability, and announcement are owner-only. This ADR creates no account,
credential, repository, pipeline, signing asset, build, or release.

## Consequences and ratification gate

The recommendation gives mobile an architecture-aligned ownership boundary,
native-oriented shared UI, independent gates, and selective releases. It accepts
a separate toolchain, explicit synchronization of tokens/docs/contracts, and
occasional Swift/Kotlin modules. Desktop and mobile changes that alter a shared
cloud contract will require coordinated reviews rather than an atomic monorepo
commit.

**Until the owner records ratification, no repository scaffolding,
implementation tickets, CI edits, signing or provisioning work, account or
credential work, build upload, TestFlight/internal-track distribution, or
production distribution may begin.** Ratification would make the following
decisions actionable, without this ADR filing or implementing them now:

- create and assign the `muniment-mobile` repository and Plane/factory lane;
- pin the React Native/Expo versions and native-project workflow;
- choose application/bundle identifiers, minimum OS versions, and versioning;
- define token/docs/API-schema synchronization and dependency ownership;
- design independent iOS/Android CI jobs and protected distribution gates;
- specify M0 authentication, secure token storage, deep links, and push setup;
- assign Apple/Google account roles, signing custody, tester groups, and the
  owner-only launch checklist.

If the owner rejects either recommendation, this ADR returns to proposed with
the chosen repository and/or framework recorded before any scaffold begins.

## Sources

- [ROADMAP standing gates](../../ROADMAP.md)
- [Harness specification §12](../spec/harness-spec.md#12-mobile-companion-app-owner-decision-2026-07-10)
- [Mobile application design](../spec/03-mobile-app.md)
- [Design system and copy laws](../spec/01-design-system.md)
- [Tauri mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/)
- [React Native](https://reactnative.dev/)
- [React Native platform-specific code](https://reactnative.dev/docs/platform-specific-code.html)
- [Expo documentation](https://docs.expo.dev/)
- [Apple TestFlight](https://developer.apple.com/testflight/)
