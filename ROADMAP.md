# muniment-desktop — ROADMAP

Each phase is an outcome the desktop owns. A phase is deleted when it holds.
Tickets in Plane carry the slices. No dates.

## The desktop works

An installed build on all three platforms launches, enters local mode on
the thread surface, sends a message and sees a reply begin, proven by a green
nightly rather than asserted.

- The production pin moves to the version the factory runs, with its Node
  floor, and keeps one verified predecessor for rollback.
- The nightly is green on Linux, Windows and macOS with a readable evidence
  envelope on each.

## The repo goes public under FSL

This repo is the public core and the one app. At the FSL release it becomes
`munimentai/muniment` by transfer, history included, and the MUNICORE lane
takes over. Before the transfer: no workflow names the artifact bucket, the
registry host or a runner label; the whole history passes the secret scan;
`CONTRIBUTING.md`, `SECURITY.md` and a code of conduct exist; releases are
tagged with a changelog; installers are signed on every platform. Outside pull
requests never reach the reviewer's merge path, and a fork gets no preview
build.

## The graph runs in the runtime

The runtime opens a company's eight-table SQLite graph through rusqlite with no
window open, seeds the catalogue, and holds any number of companies with one
current. `sql`, `propose` and `commit` reach Claude Code and the desktop's Pi
as one MCP server over stdio under the safety controls in `SPEC.md`. propose
and commit are the only write path. The first corpus is the owner's own
portfolio.

## The record panel

The Record control opens the panel beside the thread. Its table view, record
view, board and saved views generate from the `kind` table with no hand-written
screen per kind, an edit runs through propose and commit, and Ask sends the
open view's SQL to the composer.

## Readers fill the graph

The JSON file reader joins the CSV reader in the runtime. The Go reader
sidecar takes Stripe first, then GitHub, Gmail metadata, Slack and the
helpdesk, behind Objects, Describe, Page and Delta, through the mapping, cursor
and resolve queue the CSV reader runs on. The agent proposes a mapping from
the thread, and the resolve queue is a view in the panel with one action per
row.

## The extractor downloads on request

Extraction is off until the user enables it. Enabling it downloads the pinned
extractor through the resurrected lifecycle machinery: staging, resumable
`.part` files, the `current`, `previous` and `rejected` pointers, and the
install lock. The gate writes `start_char` and `end_char` from the verbatim
quote and rejects a quote it cannot find. The router stays bundled and its
result stays off the cloud-bound wire.

## The local report

The panel renders the grooming report computed over the local graph as a table
of proposals with accept and deny. This is the harness download the waitlist
is waiting for, and the beta opens when it exists.

## Local workflows run unattended

Scheduled local workflows run in the runtime under an unattended-write policy
built from the per-thread permission policy: versioned ledger events, exact
resource matching, and resolution inside the coordinate loop alone. A step is
import, agent, act or export, and act calls a tool of an MCP server the user
connected, never a muniment connector. A model proposes, the policy or a
person approves, and code commits.

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
