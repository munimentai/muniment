# 0013 — Installed-nightly desktop E2E harness and platform contract

- Status: accepted
- Date: 2026-07-18
- Context: ROADMAP desktop QA direction; nightly release workflow; canonical
  `services/ai-orchestrator/docs/projects/muniment/muniment-testing.md`

## Context

The repository has Vitest and Rust tests, and its nightly workflow builds six
artifacts on ephemeral Linux, Windows, and macOS VMs. It does not install or
exercise those artifacts. The accepted QA direction adds an installed-nightly
lane which must validate the production authentication path with a non-human
real account; mocked commands, mocked identity providers, and test credentials
compiled into the app are forbidden.

Tauri recommends WebdriverIO (WDIO) with `@wdio/tauri-service`. The service can
use official `tauri-driver` on Windows/Linux, an embedded provider on all three
platforms, or a paid cross-platform provider. Direct official `tauri-driver`
supports only Windows/Linux. Although the embedded provider now makes macOS
automation technically possible, it requires WebDriver instrumentation in the
application. The product contract remains narrower: full automation on Windows
and Linux; macOS install/launch plus Proxmox screendumps and owner smoke; no
paid provider.

Cross-repository runner and fixture operations are canonical in
`services/ai-orchestrator/docs/projects/muniment/muniment-testing.md`. This
repository cannot change that document. The prerequisite named **desktop E2E
runner contract** is unresolved until that document defines the pve01 commands
for artifact transfer, guest install/uninstall, process/log collection,
screendumps, secret injection, and artifact retention described below. An
implementation must consume that contract and fail closed when a required
operation is absent; it must not infer VM paths, credentials, or hypervisor
commands.

## Decision

Use **WebdriverIO with `@wdio/tauri-service` and its `official` driver provider
(`tauri-driver`) for Windows and Linux**. Tests drive the installed application
binary, not Vite, a development build, or browser mode. Windows uses the
matching Edge WebDriver managed by the service; Linux provides a template-pinned
`WebKitWebDriver`. Versions of WDIO, the service, `tauri-driver`, Node, and the
guest WebDriver packages are locked by the implementation PR and updated only
in reviewed dependency changes.

macOS does not run WDIO. It installs and launches the pinned `.app`, waits for a
healthy first window, and records a Proxmox screendump for owner smoke. We
reject the paid CrabNebula provider. We also reject the free embedded provider
for this lane: enabling `tauri-plugin-wdio` or
`tauri-plugin-wdio-webdriver` in the shipped nightly would add an execute,
mocking, logging, or in-process HTTP surface to production bytes, while a
separate instrumented build would no longer test the pinned release artifact.
The narrower macOS scope is a deliberate product constraint, not an upstream
capability claim.

### Test-only instrumentation boundary

WDIO configuration, selectors/helpers, secret handling, and runner-side log and
screenshot collectors are test-only. They may observe the installed process and
use WebDriver's normal element, screenshot, and browser-log facilities. The
nightly artifact contains neither WDIO Tauri plugin, embedded WebDriver server,
test capability/feature, mock transport, auth bypass, fixture credential, nor
test-only IPC command. Tests may not call `browser.tauri.mock`, replace OIDC or
API endpoints, intercept production invokes, or seed production state through
an internal command. The sign-in flow uses the same UI, system webview/deep-link
callback, backend, token storage, and entitlement checks as a user.

### Harness layout and sequence

Later implementation puts configuration in `test/e2e/wdio.conf.*`, flows in
`test/e2e/specs/`, and runner lifecycle/redaction helpers in
`test/e2e/support/`. Platform shell/PowerShell entry points live under
`test/e2e/runner/`. Generated reports always go to a runner-provided temporary
directory outside the checkout. No E2E dependency or generated report is
introduced by this ADR.

Implementation PRs are ordered and independently reviewable:

1. Windows/Linux installed launch plus real sign-in smoke.
2. Streamed chat plus file attach.
3. macOS install/launch plus Proxmox screendump.
4. Failure reporting, redaction verification, retention, and operator summary.

The first flow asserts a signed-out first window, completes real sign-in, and
asserts a stable authenticated marker. The second sends a unique non-secret
prompt, observes at least one incremental streaming update and the completed
response, attaches a generated harmless fixture, and confirms the attachment
is represented. It never asserts model prose verbatim.

### Pinned-nightly lifecycle and serialization

The lane accepts an exact 40-character `source_sha`, never the moving `nightly`
tag alone. Before acquiring pve01 it resolves the private nightly release,
requires exactly the six assets named with `nightly-${source_sha}-`, verifies
their recorded checksums/asset identities through the desktop E2E runner
contract, and stages only those bytes. A missing, duplicate, mismatched, or
partially published set fails before any guest mutation.

All platform cases use the existing pve01 one-VM-at-a-time lock. The named
input **`ci_gate_wait_minutes` defaults to 90** and is the maximum time waiting
to acquire that lock. It must be a positive integer. Invalid input fails
immediately; expiry reports `pve01_gate_timeout` and fails without creating or
altering a VM. Once acquired, the runner holds the lock through guest cleanup
and releases it in an unconditional finalizer. Platform cases run sequentially,
never in a parallel matrix.

Each platform case starts from the named clean template and creates one
ephemeral platform clone. Before each artifact, including the first, the runner
reverts that clone to the named verified clean snapshot and injects secrets as
described below. The clone may be reused only after that verified revert; no
dirty guest state or signed-in profile crosses artifact boundaries. It then
performs:

- **Windows:** download the pinned per-user MSI, per-machine MSI, and NSIS
  assets inside the guest; for each installer in a fresh snapshot, install
  silently with an explicit bounded timeout, locate the installed executable
  from installer metadata rather than a guessed path, launch it as the test
  user, and assert one healthy first window. Run WDIO launch+real-sign-in on the
  per-user MSI (the canonical Windows E2E artifact). Terminate the app, run the
  matching silent uninstall, assert its product registration and installed
  files are gone, and revert before the next installer.
- **Linux:** download the pinned `.deb` and `.AppImage`; in separate clean
  snapshots install the `.deb` with the template package manager and make the
  AppImage executable without extracting or rebuilding it. Launch each as the
  unprivileged test user and assert one healthy first window. Run WDIO
  launch+real-sign-in on the `.deb` (the canonical Linux E2E artifact), then
  terminate it. Remove the package or AppImage plus per-run application state
  and assert no test process remains before reverting.
- **macOS:** download the pinned `.app.zip`, expand it with metadata preserved,
  copy the `.app` to `/Applications`, launch it as the GUI test user, assert a
  healthy first window, and take a Proxmox screendump. Do not automate sign-in.
  Terminate the app, remove the copied bundle and per-run application state,
  and assert no test process remains.

Every success or failure, including timeout or partial setup, enters the same
finalizer: stop WDIO/driver and app processes; collect and redact diagnostics;
best-effort sign out and revoke any fixture session; uninstall/remove the tested
artifact, secrets, and per-run profile; destroy the platform clone; release the
fixture lease; then release the pve01 lock. Session disposal and lease release
are attempted even if an earlier cleanup step fails. Cleanup is idempotent and
continues after individual cleanup errors; cleanup errors are reported without
fixture identifiers and make the job fail. A platform clone is destroyed after
its platform case and is never shared across platforms or nightly runs; within
a case it may be reset to the verified clean snapshot between artifacts, but
dirty guest state and signed-in profiles are never reused.

### Real-auth fixture and diagnostic contract

The product owner owns the dedicated, non-human E2E identity, its least-privilege
tenant membership and entitlement, and its reset/revocation procedure. The
desktop QA operator owns its health check and exclusive nightly lease so two
runs cannot rotate or use the fixture concurrently. It contains no human data,
production documents, administrative role, billing authority, or reusable
user session. The canonical testing document must name the secret-store entries
and lease operation as part of the **desktop E2E runner contract**; this ADR
does not invent their values.

The runner passes the username and password (and any refresh/session material)
to the guest only through the desktop-ci secret-stdin/guest-secret mechanism,
as masked environment or permission-0600 files on an in-memory or encrypted
temporary location. Secrets never appear in argv, URLs, repository files,
WDIO config, shell tracing, process listings, screenshots, screendumps, logs, or
artifact names. Logging is disabled around secret entry. A redactor seeded with
the injected values plus token/cookie/header patterns processes every text
artifact before upload; an unredacted staging directory is destroyed even when
collection or redaction fails. A redaction scan finding a value blocks upload,
fails the run, revokes the fixture session, and emits only the finding category.

On every run, collect WDIO/driver logs, frontend console logs exposed by the
official WebDriver, the app's normal backend stdout/stderr and platform logs,
installer/uninstaller output, process exit status, and a screenshot at each
flow boundary. On failure, additionally capture the current window immediately
and a Proxmox screendump before cleanup; collection failures do not skip cleanup.
If a WebDriver cannot expose frontend console logs on a template, record that
channel as `unavailable` and fail the prerequisite check rather than adding
production instrumentation. Screenshots use neutral fixture data and are
still treated as sensitive.

Upload only the redacted diagnostic bundle to the private CI run. Retain
successful bundles for **7 days** and failed bundles for **30 days**; the
non-uploaded secret staging area and VM are destroyed at job end. Access follows
private-repository CI permissions. The final summary identifies source SHA,
artifact identity, platform, flow step, status, cleanup status, and available
diagnostic channels without printing credentials, tokens, prompts containing
secrets, or local secret paths.

## Considered alternatives

- **Direct Selenium/WebDriver against `tauri-driver`: rejected.** It supports
  the selected platforms, but WDIO plus the Tauri service is the upstream
  recommended orchestration layer and manages Windows driver matching and
  application lifecycle while retaining normal WebDriver operations.
- **WDIO embedded provider on every platform: rejected for installed-nightly.**
  It is free and would unify macOS automation, but requires instrumentation in
  the tested application; conditional or separate builds would not be the exact
  six production artifacts.
- **Paid CrabNebula provider: rejected.** It would automate macOS but violates
  the no-paid-provider constraint and expands the ratified scope.
- **Playwright or renderer-only browser tests as the installed lane: rejected.**
  They remain useful cheap coverage but do not prove native installation,
  production webview behavior, deep-link auth, secure storage, or cleanup.
- **Mocked auth or a human account: rejected.** Mocking does not validate the
  production contract; human credentials create privacy, ownership, and
  concurrency hazards.

## Consequences

Windows and Linux gain a reproducible path to full installed-nightly flows and
all six artifacts receive at least install/launch coverage. macOS intentionally
has a weaker, owner-visible smoke contract. Exact production bytes remain free
of test control surfaces, at the cost of limiting diagnostics to facilities
available outside the app. The first implementation is blocked on the clearly
named desktop E2E runner contract in the canonical cross-repository document;
after it exists, the lifecycle and PR order above require no further harness or
platform choice.

## Sources

- [ROADMAP desktop QA direction](../../ROADMAP.md)
- `services/ai-orchestrator/docs/projects/muniment/muniment-testing.md`
- [Tauri WebDriver guidance](https://v2.tauri.app/develop/tests/webdriver/)
- [WebdriverIO Tauri plugin setup](https://webdriver.io/docs/desktop-testing/tauri/plugin-setup/)
- [WebdriverIO Tauri platform support](https://webdriver.io/docs/desktop-testing/tauri/platform-support/)
