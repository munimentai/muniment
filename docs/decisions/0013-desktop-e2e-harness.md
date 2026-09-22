# 0013 — Installed-nightly desktop E2E harness and platform contract

- Status: accepted
- Context: ROADMAP desktop QA direction; nightly release workflow; canonical
  `services/ai-orchestrator/docs/projects/muniment/muniment-testing.md`

## Context

The repository has Vitest and Rust tests, and its nightly workflow builds
artifacts on ephemeral Linux, Windows, and macOS VMs. The E2E lanes install
pinned nightly artifacts and run WDIO specs with a non-human real account.
They forbid mocked commands, mocked identity providers, and test credentials
compiled into the app.

All three lanes require chat, real sign-in, onboarding, and cleanup coverage.

The local release keeps cloud features disabled. Its sign-in phase proves that
the desktop opens without a Muniment account, provider connections remain
available, and cloud sign-in stays hidden. The production cloud authentication
case is skipped in that build and does not count as an authentication pass.
Cloud-enabled builds run that case with `MUNIMENT_E2E_CLOUD=true`.
A successful launch alone does not prove that the desktop works.
The chat spec must send a message and see a reply.

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

Use **WebdriverIO (WDIO) with `@wdio/tauri-service` and its `embedded` driver
provider on Linux, Windows, and macOS**. The shared `test/e2e/wdio.conf.js`
sets `driverProvider: 'embedded'` and launches `MUNIMENT_E2E_APP_BINARY`.
WebDriver reaches the app through the in-process server from
`tauri-plugin-wdio-webdriver`, not the official `tauri-driver`.

On macOS, `macos.sh` installs and launches the pinned `.app`, checks for a
healthy first window, and requests a Proxmox screendump through `desktop-ci`.
If the smoke passes, `macos-wdio.sh` runs `npm run test:e2e` three times:
installed chat and real sign-in, then onboarding, then cleanup.
Later phases still run after a spec failure, and any failure fails the lane.
The launches write the JUnit reports that the artifact envelope reads.

The macOS smoke and WDIO phases share the pinned source, not the same artifact.
The smoke removes its installed `.app` before WDIO starts.
`macos-wdio.sh` builds a separate binary with `--no-bundle --features e2e-webdriver`
and `src-tauri/tauri.e2e.conf.json` from that source.
It sets `MUNIMENT_E2E_APP_BINARY` to `src-tauri/target/release/muniment-desktop`.
The installed chat specs therefore exercise that E2E binary, not the released `.app`.
Linux and Windows also build E2E binaries for the shared embedded provider.

### Test-only instrumentation boundary

WDIO configuration, selectors/helpers, secret handling, and runner-side log and
screenshot collectors are test-only. They may observe the installed process and
use WebDriver's normal element, screenshot, and browser-log facilities. The
nightly artifact excludes WebDriver instrumentation, mock transport, auth bypasses,
fixture credentials, and test-only IPC commands.
The separate E2E build enables `tauri-plugin-wdio-webdriver` through the
`e2e-webdriver` feature and grants `wdio-webdriver:default` through `src-tauri/tauri.e2e.conf.json`.
Tests may not call `browser.tauri.mock`, replace OIDC or
API endpoints, intercept production invokes, or seed production state through
an internal command. The sign-in flow uses the same UI, system webview/deep-link
callback, backend, token storage, and entitlement checks as a user.

### Harness layout and sequence

The harness keeps configuration in `test/e2e/wdio.conf.js`, flows in
`test/e2e/specs/`, and runner lifecycle/redaction helpers in
`test/e2e/support/`. Platform shell/PowerShell entry points live under
`test/e2e/runner/`. Generated reports always go to a runner-provided temporary
directory outside the checkout.

The three-lane contract is:

- **Linux:** `linux.sh` installs the pinned artifact, then runs WDIO chat,
  real sign-in, onboarding, and cleanup.
- **Windows:** `windows.ps1` installs the pinned artifact, then runs WDIO chat,
  real sign-in, onboarding, and cleanup.
- **macOS:** the lane runs the pinned `.app` smoke and Proxmox screendump through `macos.sh`.
  It then runs WDIO chat, real sign-in, onboarding, and cleanup through `macos-wdio.sh`.

The sign-in spec completes real sign-in and asserts a stable authenticated marker.
The chat spec sends a unique non-secret prompt and checks for an incremental
streaming update and a completed response.
It attaches a generated harmless fixture and checks that the UI shows the attachment.
It never asserts model prose verbatim.

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
  user, and assert one healthy first window. The per-user MSI is the canonical
  Windows E2E artifact. Run WDIO against the separate E2E binary.
  Stop the app. Run the matching silent uninstall.
  Assert that its product registration and installed files are gone.
  Revert before the next installer.
- **Linux:** download the pinned `.deb` and `.AppImage`; in separate clean
  snapshots install the `.deb` with the template package manager and make the
  AppImage executable without extracting or rebuilding it. Launch each as the
  unprivileged test user and assert one healthy first window. Run WDIO
  against the E2E `.deb` installed over the pinned `.deb`, then stop it.
  Remove the package or AppImage plus per-run application state
  and assert no test process remains before reverting.
- **macOS:** `macos.sh` downloads the pinned `.app.zip` and preserves its metadata
  when it expands the archive. It copies the `.app` to `/Applications`, launches
  it as the GUI test user, and checks for a healthy first window.
  `desktop-ci --screendump` records the Proxmox screendump.
  The smoke stops the app, removes the bundle and per-run application state,
  and checks that no test process remains.
  After a successful smoke, `macos-wdio.sh` builds the separate E2E binary
  from the same pinned source and runs the installed chat and real sign-in specs.
  It then runs onboarding with separate state, followed by cleanup.
  Its finalizer stops WDIO and the app, redacts diagnostics, and removes per-run state.

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
embedded WebDriver, the app's normal backend stdout/stderr and platform logs,
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

- **Direct Selenium/WebDriver against `tauri-driver`: rejected.** It does not
  support macOS. WDIO with the embedded provider covers all three platforms.
- **Paid CrabNebula provider: rejected.** It violates the no-paid-provider constraint.
- **Playwright or renderer-only browser tests as the installed lane: rejected.**
  They remain useful cheap coverage but do not prove native installation,
  production webview behavior, deep-link auth, secure storage, or cleanup.
- **Mocked auth or a human account: rejected.** Mocking does not validate the
  production contract; human credentials create privacy, ownership, and
  concurrency hazards.

## Consequences

Linux, Windows, and macOS require WDIO chat, real sign-in, onboarding, and
cleanup in addition to pinned artifact installation coverage.
A macOS spec failure is an E2E lane failure, not an owner smoke result.
Production artifacts exclude the embedded WebDriver server, while separate
E2E builds provide the test control surface.
The WDIO results do not prove those flows against exact production bytes.

## Sources

- [ROADMAP desktop QA direction](../../ROADMAP.md)
- `services/ai-orchestrator/docs/projects/muniment/muniment-testing.md`
- [Tauri WebDriver guidance](https://v2.tauri.app/develop/tests/webdriver/)
- [WebdriverIO Tauri plugin setup](https://webdriver.io/docs/desktop-testing/tauri/plugin-setup/)
- [WebdriverIO Tauri platform support](https://webdriver.io/docs/desktop-testing/tauri/platform-support/)
