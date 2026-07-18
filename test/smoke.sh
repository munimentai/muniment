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
grep -Fq 'src-tauri/cli/*|src-tauri/cli/**' "$ci"
grep -Fq 'src-tauri/attach/*|src-tauri/attach/**)' "$ci"
grep -Fq 'protocol-fixtures/*|protocol-fixtures/**)' "$ci"
grep -Fq 'echo "companion=$companion" >> "$GITHUB_OUTPUT"' "$ci"
grep -Fq "if: steps.changes.outputs.companion == 'true'" "$ci"
grep -Fq 'cargo fmt --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-cli --check' "$ci"
grep -Fq 'cargo clippy --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-cli --all-targets --locked -- -D warnings' "$ci"
grep -Fq 'cargo test --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-cli --locked' "$ci"
grep -Fq 'run: test/cli-dependency-boundary.sh' "$ci"
test -d protocol-fixtures/muniment.attach/1
grep -Fq 'name: attach-fixtures-current' "$ci"
grep -Fq 'run: cargo run -p muniment-attach --bin export-attach-fixtures -- ../protocol-fixtures --check' "$ci"
grep -Fq 'attach-fixtures-current:' "$ci"
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
# Linux's host graph. The boundary must inspect every dependency pulled in by
# muniment-attach for every target platform.
attach_manifest=src-tauri/attach/Cargo.toml
lockfile=src-tauri/Cargo.lock
attach_manifest_backup=$(mktemp)
lockfile_backup=$(mktemp)
boundary_output=$(mktemp)
cp "$attach_manifest" "$attach_manifest_backup"
cp "$lockfile" "$lockfile_backup"
restore_dependency_probe() {
  cp "$attach_manifest_backup" "$attach_manifest"
  cp "$lockfile_backup" "$lockfile"
  rm -f "$attach_manifest_backup" "$lockfile_backup" "$boundary_output"
}
trap restore_dependency_probe EXIT
printf '\n[target.\x27cfg(windows)\x27.dependencies]\ntauri = "2"\n' >> "$attach_manifest"
cargo generate-lockfile --manifest-path src-tauri/Cargo.toml --offline
if test/cli-dependency-boundary.sh >"$boundary_output" 2>&1; then
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
echo "smoke OK"
