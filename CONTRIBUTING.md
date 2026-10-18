# Contributing to Muniment

Muniment is a desktop harness for models, tools and local work.
Read [SPEC.md](SPEC.md) for behavior and [DESIGN.md](DESIGN.md) for interface rules.

## Issues and pull requests

Anyone can report a reproducible bug or propose a feature by email to support@muniment.ai.
Include the app version, operating system, steps, expected result and actual result.
Remove credentials, personal files and private conversation content from attachments.
Report vulnerabilities through [SECURITY.md](SECURITY.md) instead.

The project does not accept outside code contributions or pull requests.
Bug reports and feature requests are welcome by email to support@muniment.ai.
Maintainers review issues and implement accepted changes.
Keep one focused change per pull request. Explain its behavior and verification.
Add tests for behavior changes and update the relevant contributor guides.
A maintainer reviews and merges changes. External pull requests must not run on
private runners or receive build credentials.

## Development

Use the prerequisites and commands in [README.md](README.md#build).
Frontend development and unit tests do not require signing credentials or a
Muniment cloud account. Platform packaging requires the platform's native tools.
A frontend preview does not provide the native terminal, browser or runtime.

Run `npm test`, `npm run lint:copy`, and `scripts/check-steering.sh .`.
For runtime changes, also run the Rust checks in [AGENTS.md](AGENTS.md).
A successful macOS build does not verify Windows or Linux.

## Core

`core/` holds the shared Rust core and its own Cargo workspace.
Read [core/README.md](core/README.md) for its crates and [core/AGENTS.md](core/AGENTS.md) for its rules.
Core holds the logic. The desktop is the UI layer, so put shared behavior in core.
No core crate may depend on Tauri or on desktop code.
Run `scripts/check-core-boundary.sh` after you add an import between modules or crates.
From `core/`, run `cargo fmt --all --check` and `cargo test -p <crate> -- --test-threads=1`.
Change pins only with `muniment-pins bump`, then run `muniment-pins compat`.
Scope core commits with `(core)`, such as `fix(core):`, and keep them apart from desktop commits.
A commit's prefix sets the next release version of the whole repository, so pick it with care.

## Contributions and license

Contributions use [LICENSE.md](LICENSE.md), FSL-1.1-ALv2, under the
inbound-equals-outbound rule. No separate agreement or sign-off line is required.
FSL is a Fair Source license. Each version becomes available under Apache 2.0
after two years. Third-party components retain their own licenses.
Do not submit code, data or assets you lack permission to distribute.
