# 0029 — Use the per-platform package manager for updates

- Status: accepted

## Context

The desktop ships a rolling nightly and owner-promoted stable releases.
Stable promotion copies the exact bytes of a green nightly without a rebuild.
The roadmap puts the first public release under FSL after the graph, the SQL
tool, and the report. The desktop has no in-app updater.

## Decision

The update path is the per-platform package manager until the first public release.

- macOS uses the Homebrew tap, `mikeydiamonds/muniment`, and its `muniment-nightly` cask.
- Windows uses WinGet for stable releases.
- Linux uses the deb package or AppImage from the release assets.
  Users install the new deb through the system package manager or replace the AppImage.

The desktop checks for no update and phones no home.
The first public release under FSL opens the in-app updater work.

## Consequences

Users update outside the desktop. The desktop adds no update polling or update telemetry.
Homebrew tap seeding and WinGet publication remain owner-gated.
The package manager update path closes release gate 7 without updater code.
The structure smoke asserts this ADR exists and carries the trigger sentence.

## References

- [Production-ready gates](../../SPEC.md#production-ready-gates-release-gate) define the update criterion.
- [Roadmap](../../ROADMAP.md) defines the public core under FSL and the product outcomes.
- [Stable promotion](../../.github/lib/release-promotion.mjs) verifies and copies the nightly artifacts.
- [Homebrew](../homebrew.md) describes the macOS tap and cask.
- [Windows installers](../windows-installers.md#winget-convenience-channel) describes WinGet publication.
- [Structure smoke](../../test/smoke.sh) enforces the deferral trigger.
