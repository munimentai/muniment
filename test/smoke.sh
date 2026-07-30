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
# Shared-host and guest setup failures can close SSH before desktop-ci returns
# infrastructure status 3; keep the all-platform one-time retry covering 255.
grep -Fq '{ [ "$status" -ne 3 ] && [ "$status" -ne 255 ]; }' "$ci"
test "$(grep -Fc '[ "$PLATFORM" != "windows" ]' "$ci")" -eq 0
test -f src-tauri/Cargo.lock
# installed-nightly E2E architecture remains accepted and platform-bounded
e2e_adr=docs/decisions/0013-desktop-e2e-harness.md
test -f "$e2e_adr"
grep -Fq -- '- Status: accepted' "$e2e_adr"
grep -Fq '@wdio/tauri-service' "$e2e_adr"
grep -Fq '(`tauri-driver`) for Windows and Linux' "$e2e_adr"
grep -Fq 'Windows/Linux installed launch plus real sign-in smoke.' "$e2e_adr"
grep -Fq 'macOS install/launch plus Proxmox screendump' "$e2e_adr"
grep -Fq 'Do not automate sign-in.' "$e2e_adr"
grep -Fq 'test/e2e/specs/' "$e2e_adr"
grep -Fq 'ci_gate_wait_minutes' "$e2e_adr"
grep -Fq 'non-human E2E identity' "$e2e_adr"
grep -Fq 'desktop E2E runner contract' "$e2e_adr"
grep -Fq '0013-desktop-e2e-harness.md' README.md
# The desktop must not restore the removed resident classifier.
test -z "$(grep -RilE \
  --exclude='*.test.js' \
  'Qwen3\.5|llama-server|RESIDENT_MODEL|MunimentHuggingFace|required_model_acquisition_status|dictation_polish|dictation_transform|onboarding_triage' \
  src src-tauri/src src-tauri/core/src test/probe 2>/dev/null)"
grep -Fq -- '- Status: superseded by the 2026-07-29 cloud ingress ruling' \
  docs/decisions/0017-resident-model-artifact-pin.md
grep -Fq 'The desktop sends no classification metadata.' docs/spec/harness-spec.md
# no current document may name the retired Gemma alias; ADR 0003 preserves the
# historical decision it records. No document may call the resident model Gemma.
# (`! grep` would be exempt from errexit, so assert on empty output instead)
test -z "$(grep -rl --exclude='0003-resident-gemma-model.md' 'muniment-resident-gemma' docs/)"
test -z "$(grep -ril 'resident local gemma' docs/)"
# companion workspace and its path-scoped CI lane
grep -Fq 'members = [".", "core", "attach", "cli", "acp"]' src-tauri/Cargo.toml
grep -Fq 'resolver = "2"' src-tauri/Cargo.toml
test -f src-tauri/attach/Cargo.toml
test -f src-tauri/attach/src/lib.rs
test -f src-tauri/cli/Cargo.toml
test -f src-tauri/cli/src/main.rs
grep -Fq 'muniment-attach = { path = "../attach", default-features = false, features = ["client"] }' src-tauri/cli/Cargo.toml
grep -Fq 'src-tauri/cli/*|src-tauri/cli/**' "$ci"
grep -Fq 'src-tauri/attach/*|src-tauri/attach/**)' "$ci"
grep -Fq 'protocol-fixtures/*|protocol-fixtures/**)' "$ci"
grep -Fq 'echo "companion=$companion" >> "$GITHUB_OUTPUT"' "$ci"
grep -Fq "if: steps.changes.outputs.companion == 'true'" "$ci"
grep -Fq 'cargo fmt --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-cli --package muniment-acp --check' "$ci"
grep -Fq 'cargo clippy --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-cli --package muniment-acp --all-targets --locked -- -D warnings' "$ci"
grep -Fq 'cargo test --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-cli --package muniment-acp --locked' "$ci"
grep -Fq 'run: test/cli-dependency-boundary.sh' "$ci"
test -d protocol-fixtures/muniment.attach/1
grep -Fq 'name: attach-fixtures-current' "$ci"
grep -Fq 'run: cargo run -p muniment-attach --bin export-attach-fixtures -- ../protocol-fixtures --check' "$ci"
grep -Fq 'attach-fixtures-current:' "$ci"
test -f editor-extension/package.json
test -f editor-extension/package-lock.json
test -f editor-extension/src/extension.ts
test -f editor-extension/src/protocol.ts
grep -Fq 'editor-extension/*|editor-extension/**)' "$ci"
grep -Fq 'name: extension-contract' "$ci"
grep -Fq "needs.smoke.outputs.extension == 'true'" "$ci"
grep -Fq 'run: npm run package -- --out "$RUNNER_TEMP/muniment-editor.vsix"' "$ci"
test -f protocol-fixtures/muniment.attach/1/negotiation-hello.json
test -x test/cli-dependency-boundary.sh
grep -Fq -- '--locked --target all --prefix none' test/cli-dependency-boundary.sh
# The allowlist rejects representatives of every forbidden runtime class,
# including package-name variants and implementations without category words.
for forbidden in \
  muniment-core muniment-desktop tauri tauri-plugin-dialog rusqlite \
  sherpa-onnx sherpa-onnx-sys sidecar-supervision openidconnect oauth2 \
  oidc-client keyring keyring-core secret-service dbus-secret-service \
  security-framework windows-credentials; do
  ! test/cli-dependency-boundary.sh muniment-cli muniment-attach "$forbidden" \
    >/dev/null 2>&1
done
# Exercise the real Cargo tree path with a transitive dependency hidden from
# Linux's host graph. Use an all-local fixture so this smoke check does not
# depend on which crates.io index entries happen to be cached on the runner.
boundary_fixture=$(mktemp -d)
boundary_output=$(mktemp)
restore_dependency_probe() {
  rm -rf "$boundary_fixture"
  rm -f "$boundary_output"
}
trap restore_dependency_probe EXIT
mkdir -p "$boundary_fixture/cli/src" "$boundary_fixture/attach/src" \
  "$boundary_fixture/tauri/src"
printf '[workspace]\nmembers = ["cli", "attach", "tauri"]\nresolver = "2"\n' \
  > "$boundary_fixture/Cargo.toml"
printf '[package]\nname = "muniment-cli"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\nmuniment-attach = { path = "../attach" }\n' \
  > "$boundary_fixture/cli/Cargo.toml"
printf 'fn main() {}\n' > "$boundary_fixture/cli/src/main.rs"
printf '[package]\nname = "muniment-attach"\nversion = "0.0.0"\nedition = "2021"\n\n[target.\x27cfg(windows)\x27.dependencies]\ntauri = { path = "../tauri" }\n' \
  > "$boundary_fixture/attach/Cargo.toml"
printf '' > "$boundary_fixture/attach/src/lib.rs"
printf '[package]\nname = "tauri"\nversion = "0.0.0"\nedition = "2021"\n' \
  > "$boundary_fixture/tauri/Cargo.toml"
printf '' > "$boundary_fixture/tauri/src/lib.rs"
cargo generate-lockfile --manifest-path "$boundary_fixture/Cargo.toml" --offline
if MUNIMENT_CARGO_MANIFEST="$boundary_fixture/Cargo.toml" \
    test/cli-dependency-boundary.sh >"$boundary_output" 2>&1; then
  echo "transitive target-specific forbidden dependency passed the CLI boundary" >&2
  exit 1
fi
grep -Fq 'tauri' "$boundary_output"
restore_dependency_probe
trap - EXIT
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
test -z "$(git ls-files 'protocol-fixtures/muniment.attach/**' | grep -v '^protocol-fixtures/muniment.attach/1/')"
echo "smoke OK"
