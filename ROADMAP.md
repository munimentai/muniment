# muniment-desktop — ROADMAP

Each phase is an outcome the desktop owns. A phase is deleted when it holds.
Tickets in Plane carry the slices. No dates.

## The desktop works

An installed build on all three platforms launches, enters local mode on
the thread surface, sends a message and sees a reply begin, proven by a green
nightly rather than asserted. This lands before any graph work starts.

- The production pin moves to the version the factory runs, with its Node
  floor, and keeps one verified predecessor for rollback.
- The nightly is green on Linux, Windows and macOS with a readable evidence
  envelope on each.

## The crates port into the public core

The runtime service, attach, the run journal, the Pi sidecar integration, the
per-thread permission policy, the CAS store, the memory index, the code-diff
crates, ACP interop, and the ASR and read-aloud stack live in the public core
under FSL with their tests, and the desktop consumes them from there. The
cloud-native auth flow, the entitlement projection UI, the browser-control
extension and the remote-control design study stay behind.

## The graph runs in the runtime

The runtime service opens the eight-table SQLite graph through rusqlite with no
window open. One read-only SQL tool over curated views is served to a harness,
Claude Code first, under the safety controls in `SPEC.md`. propose and commit
are the only write path. The first corpus is the owner's own portfolio.

## The extractor downloads on request

Extraction is off until the user enables it. Enabling it downloads the pinned
extractor through the resurrected lifecycle machinery: staging, resumable
`.part` files, the `current`, `previous` and `rejected` pointers, and the
install lock. The gate writes `start_char` and `end_char` from the verbatim
quote and rejects a quote it cannot find. The router stays bundled and its
result stays off the cloud-bound wire.

## The local report and the generated UI

The shell renders the grooming report computed over the local graph, and it
renders every kind from the `kind` table with no hand-written screen per kind.
This is the harness download the waitlist is waiting for, and the beta opens
when it exists.

## Local workflows run unattended

Scheduled local workflows run in the runtime under an unattended-write policy
built from the per-thread permission policy: versioned ledger events, exact
resource matching, and resolution inside the coordinate loop alone. A model
proposes, the policy or a person approves, and code commits.

## Mobile drives the runtime

The runtime holds one outbound HTTPS leg to MUNICLOUD, direct-first with relay
fallback and end-to-end encrypted, with no inbound ports. Pairing binds to a
free MUNICLOUD account, one device pair per install, with rate limits on the
fallback path. Mobile sees the live tool stream, Stop, queued follow-ups, and
the permission-gate card, and approving a proposal from the phone is that card.

## Release

- macOS builds are signed, notarized and stapled. Owner-gated on Apple
  enrollment, and switching signing on is secrets-only.
- WinGet publication, Homebrew tap seeding, distribution accounts, store
  publishing, launch and publicity are owner-gated.

## Standing gates

- Code pull requests gate on the structure smoke, the steering check, the
  frontend and Rust tests, and the applicable path-scoped checks or desktop
  builds.
- Markdown-only pull requests gate on the structure smoke and the steering
  check alone. Pushes to `main` run the smoke only.
- Build desktop targets only, unless an accepted ADR changes the companion
  checks.
- Every pull request and every push to `main` runs the gitleaks secret scan in
  `.github/workflows/secret-scan.yml`.
- `SPEC.md` carries the release gate and the folder hierarchy standard, and
  each criterion names the check that enforces it.
- No user-facing text carries an em dash, in any form. `npm run lint:copy`
  fails on the character and on every escaped spelling of it across `src`,
  `src-tauri` and `browser-control`.
