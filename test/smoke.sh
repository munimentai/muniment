#!/usr/bin/env bash
# M0 structure smoke (runs on the shared runners — no GUI/build here).
set -eu
grep -q '"identifier": "ai.muniment.desktop"' src-tauri/tauri.conf.json
grep -q 'tauri_build::build' src-tauri/build.rs
grep -q 'generate_context' src-tauri/src/main.rs
test -f src-tauri/icons/icon.ico
test -f src-tauri/icons/icon.icns
test -f src/index.html
# frontend toolchain: tauri drives vite (CI runs plain `npm run tauri build`)
grep -q '"beforeBuildCommand": "npm run build"' src-tauri/tauri.conf.json
grep -q '"frontendDist": "../dist"' src-tauri/tauri.conf.json
test -f vite.config.js
# fonts are vendored (CSP is default-src 'self'; no CDN requests)
test -f src/fonts/SchibstedGrotesk-latin.woff2
test -f src/fonts/CommitMono-VF.woff2
# desktop CI: native checks gate bundles, while docs-only PRs and main pushes
# remain smoke-only
ci=.github/workflows/ci.yml
grep -Fq 'name: Desktop compile preflight (${{ matrix.platform }})' "$ci"
grep -Fq "if: github.event_name == 'pull_request' && needs.smoke.outputs.desktop == 'true'" "$ci"
grep -Fq 'platform: [linux, windows, macos]' "$ci"
grep -Fq "cmd='cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets'" "$ci"
test "$(grep -Fc 'apt-get install -y -qq --no-install-recommends libasound2-dev' "$ci")" -eq 2
test -f src-tauri/Cargo.lock
# companion workspace and its path-scoped CI lane
grep -Fq 'members = [".", "core", "attach", "cli"]' src-tauri/Cargo.toml
grep -Fq 'resolver = "2"' src-tauri/Cargo.toml
test -f src-tauri/attach/Cargo.toml
test -f src-tauri/attach/src/lib.rs
test -f src-tauri/cli/Cargo.toml
test -f src-tauri/cli/src/main.rs
grep -Fq 'muniment-attach = { path = "../attach", default-features = false, features = ["client"] }' src-tauri/cli/Cargo.toml
grep -Fq "if: steps.changes.outputs.companion == 'true'" "$ci"
grep -Fq 'cargo tree --manifest-path src-tauri/Cargo.toml --package muniment-cli' "$ci"
grep -Fq "needs.smoke.outputs.desktop == 'true'" "$ci"
test -f src-tauri/tauri.machine.conf.json
grep -Fq '"upgradeCode": "c75b4a56-7d8b-5b99-9fc7-61ef0aabe84b"' src-tauri/tauri.machine.conf.json
grep -Fq '"template": "./windows/per-machine.wxs"' src-tauri/tauri.machine.conf.json
test "$(grep -Fc 'Root="HKLM" Key="Software\\{{manufacturer}}\\{{product_name}}"' src-tauri/windows/per-machine.wxs)" -eq 5
grep -Fq '<Directory Id="CommonDesktopFolder"' src-tauri/windows/per-machine.wxs
grep -Fq '<Directory Id="CommonProgramsFolder"' src-tauri/windows/per-machine.wxs
! grep -Fq '<Directory Id="DesktopFolder"' src-tauri/windows/per-machine.wxs
! grep -Fq '<Directory Id="ProgramMenuFolder"' src-tauri/windows/per-machine.wxs
! grep -Fq 'Root="HKCU" Key="Software\\{{manufacturer}}\\{{product_name}}"' src-tauri/windows/per-machine.wxs
grep -Fq 'build-windows-installers.mjs' .github/workflows/nightly.yml
grep -Fq 'windows-installers.ps1' .github/workflows/nightly.yml
test -f docs/windows-installers.md
grep -Fq 'needs: [smoke, desktop-compile]' "$ci"
test "$(grep -Fc "if: github.event_name == 'pull_request' && needs.smoke.outputs.desktop == 'true'" "$ci")" -eq 2
echo "smoke OK"
