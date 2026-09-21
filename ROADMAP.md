# muniment-desktop — ROADMAP

Muniment releases the desktop harness first. Paid cloud availability follows
while the desktop grows. This file lists remaining outcomes, not completed work.

## Phase one: release the desktop harness

- The installed app reaches the thread surface without a Muniment account,
  sends a local-provider message and receives a reply on macOS, Windows and Linux.
- Browser, Files, Terminal, editable files, projects, agents, artifacts, memory,
  model routing and voice pass installed-app checks on supported platforms.
- The direct CEF browser has release packaging on every supported platform.
  Browser sandboxing, session persistence and user/agent control pass the same
  installed-build checks. Missing platform features have explicit release limits.
- The production Pi pin has a verified rollback and passing platform evidence.
- The cloud and company-record flags remain off in the default release. Each
  feature can be enabled alone in a development build. Provider accounts remain usable.
- Public release carries FSL-1.1-Apache-2.0, dependency notices, contributor
  guidance, a security policy, signed installers and a repeatable update path.
- The public repo and its history pass the secret scan. External pull requests
  cannot reach private build credentials or an automatic merge path.
- The site and public reference docs describe the desktop users can install.
  The graph, importers, reports and paid cloud are not desktop launch prerequisites.

## Phase two: cloud availability and paid accounts

- Cloud account sign-in, entitlements, billing and recovery work without making
  local desktop use depend on a subscription or reachable cloud service.
- Paid services support shared work, hosted execution and policy-controlled
  routing. Each service defines its account tier and acceptance rules before release.
- Cloud-backed and local runs preserve their credential and permission boundaries.
- Mobile remote control has explicit account, pairing, encryption and relay rules.
  It does not gate the desktop release.

## Optional company record

- The company graph stays behind its independent flag while its Record panel,
  Companies settings, importers, proposals and reports receive focused validation.
- Unattended graph writes require an explicit, tested approval policy.
- Extraction remains optional and needs a verified model, download lifecycle and
  evidence checks before users can enable it.
- The company-record release has its own acceptance gate. Cloud availability
  neither requires nor silently enables it.

## Standing gates

- Code changes pass the structure smoke, steering check, copy lint, frontend
  tests, relevant Rust tests and applicable platform checks.
- A local macOS build proves macOS only. Windows and Linux need their own evidence.
- Markdown-only pull requests use the smoke and steering gate.
- Public-surface changes update `docs/public-evidence/` when prepared for publication.
- Release, publication, push and paid-service activation require an explicit request.
