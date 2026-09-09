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
# desktop CI: a changes job classifies the diff, the native preflights gate
# every code PR beside smoke, the installer builds run only for installer
# paths, and docs-only PRs and main pushes remain smoke-only
ci=.github/workflows/ci.yml
grep -Fq 'name: Desktop compile preflight (${{ matrix.platform }})' "$ci"
grep -Fq "if: github.event_name == 'pull_request' && needs.changes.outputs.desktop == 'true'" "$ci"
grep -Fq "needs.changes.outputs.installer == 'true'" "$ci"
grep -Fq 'platform: [linux, windows, macos]' "$ci"
grep -Fq "cmd='mkdir -p \"\$HOME/.cache/cargo-target\" && ln -sfn \"\$HOME/.cache/cargo-target\" src-tauri/target && cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets'" "$ci"
windows_preflight=$(
  awk '
    index($0, "- name: Check (${{ matrix.platform }}) via desktop-ci") { in_check = 1 }
    in_check && index($0, "if [ \"$PLATFORM\" = \"windows\" ]; then") {
      in_windows = 1
      next
    }
    in_windows && /cmd=/ { print; exit }
  ' "$ci"
)
test -n "$windows_preflight"
windows_preflight_targets=$(
  awk '{
    for (field = 1; field <= NF; field++) {
      if ($field == "--test") print $(field + 1)
    }
  }' <<<"$windows_preflight"
)
while IFS= read -r windows_test; do
  target=${windows_test##*/}
  target=${target%.rs}
  if ! grep -Fxq "$target" <<<"$windows_preflight_targets"; then
    printf 'Windows preflight omits test target: %s\n' "$windows_test" >&2
    exit 1
  fi
done < <(
  grep -RlF '#![cfg(target_os = "windows")]' \
    src-tauri/core/tests src-tauri/runtime/tests | sort
)
test "$(grep -Fc 'apt-get install -y -qq --no-install-recommends libasound2-dev' "$ci")" -eq 2
# Shared-host and guest setup failures can close SSH before desktop-ci returns
# infrastructure status 3; keep the all-platform one-time retry covering 255.
grep -Fq '[ "$status" -ne 3 ] && [ "$status" -ne 255 ]' "$ci"
test "$(grep -Fc '[ "$PLATFORM" != "windows" ]' "$ci")" -eq 0
test/desktop-build-retry.sh
test -f src-tauri/Cargo.lock
# The candidate switch leaves every production descriptor at 0.73.1.
pi_install=src-tauri/core/src/sidecar/pi_install.rs
test "$(awk '/pub const PI_ARTIFACT: / { pin = 1; next } pin && /version: "0.73.1"/ { count++; pin = 0 } END { print count }' "$pi_install")" -eq 5
grep -Fq 'https://github.com/earendil-works/pi/releases/download/v0.73.1' "$pi_install"
for lane in build linux-e2e windows-e2e macos-e2e; do
  awk -v lane="$lane" '
    /^  [a-z0-9-]+:$/ { selected = ($0 == "  " lane ":") }
    selected && /"MUNIMENT_PI_CANDIDATE=1"/ { found = 1 }
    END { exit !found }
  ' .github/workflows/nightly.yml
done
# installed-nightly E2E architecture remains accepted and platform-bounded
e2e_adr=docs/decisions/0013-desktop-e2e-harness.md
test -f "$e2e_adr"
grep -Fq -- '- Status: accepted' "$e2e_adr"
grep -Fq '@wdio/tauri-service' "$e2e_adr"
grep -Fq 'provider on Linux, Windows, and macOS' "$e2e_adr"
grep -Fq "driverProvider: 'embedded'" "$e2e_adr"
grep -Fq 'tauri-plugin-wdio-webdriver' "$e2e_adr"
grep -Fq 'The three-lane contract is:' "$e2e_adr"
grep -Fq 'pinned `.app` smoke and Proxmox screendump through `macos.sh`' "$e2e_adr"
grep -Fq 'It then runs WDIO chat, real sign-in, onboarding, and cleanup through `macos-wdio.sh`.' "$e2e_adr"
grep -Fq 'The macOS smoke and WDIO phases share the pinned source, not the same artifact.' "$e2e_adr"
grep -Fq 'builds a separate binary with `--no-bundle --features e2e-webdriver`' "$e2e_adr"
grep -Fq 'Later phases still run after a spec failure, and any failure fails the lane.' "$e2e_adr"
test -z "$(grep -nE 'smoke-only|macOS does not run WDIO|Do not automate sign-in' \
  README.md "$e2e_adr" test/e2e/EVIDENCE.md)"
grep -Fq 'test/e2e/specs/' "$e2e_adr"
grep -Fq 'ci_gate_wait_minutes' "$e2e_adr"
grep -Fq 'non-human E2E identity' "$e2e_adr"
grep -Fq 'desktop E2E runner contract' "$e2e_adr"
grep -Fq '0013-desktop-e2e-harness.md' README.md
# The restored artifact lifecycle must not restore the old runtime-specific model code.
test -z "$(grep -RilE \
  --exclude='*.test.js' \
  'Qwen3\.5|llama-server|RESIDENT_MODEL|required_model_acquisition_status|dictation_polish|dictation_transform|onboarding_triage' \
  src src-tauri/src src-tauri/core/src test/probe 2>/dev/null)"
grep -Fq -- '- Status: superseded by the 2026-07-29 cloud ingress ruling' \
  docs/decisions/0017-resident-model-artifact-pin.md
grep -Fq 'The desktop sends no classification metadata.' SPEC.md
# SPEC law 13 permits the bundled local classifier but keeps its result off the
# cloud-bound wire. Reserved classification fields remain forbidden in source.
# `chat_grant.rs` names these fields only in tests that reject them from both
# cloud-bound requests. The assertions below keep those tests present.
test -z "$(grep -RilE \
  --exclude='chat_grant.rs' \
  'routing_?label|routing_?tier|task_?tier|signals_?version|keyword-code-v1' \
  src src-tauri/src src-tauri/core/src test/probe 2>/dev/null)"
grep -Fq 'fn the_grant_request_carries_no_client_classification' \
  src-tauri/core/src/chat_grant.rs
grep -Fq 'fn the_receipt_request_carries_only_the_run_id' \
  src-tauri/core/src/chat_grant.rs
classifier_adr=docs/decisions/0028-bundled-router-classifier.md
classifier_module=src-tauri/core/src/router_classifier.rs
test -f "$classifier_adr"
test -f "$classifier_module"
grep -Fq 'This artifact has no download, no lifecycle pointer, and no user opt-in.' \
  "$classifier_adr"
for classifier_class in route.cloud route.local route.proxy; do
  grep -Fq "\`$classifier_class\`" "$classifier_adr"
  grep -Fq "\"$classifier_class\"" "$classifier_module"
done
# Release gate 7 defers the in-app updater until the first public release under FSL.
update_adr=docs/decisions/0029-update-path.md
test -f "$update_adr"
grep -Fq 'The first public release under FSL opens the in-app updater work.' \
  "$update_adr"
# SPEC and ROADMAP name that one mode identically, and neither names it a chat.
grep -Fq 'the thread surface' SPEC.md
grep -Fq 'the thread surface' ROADMAP.md
test -z "$(grep -ril 'chat mode' SPEC.md ROADMAP.md)"
# The committed note carries the routing-surface name raised with MUNICLOUD.
grep -Fq 'desktop_thread_chat' docs/desktop-single-mode.md
grep -Fq 'Ratifying MUNICLOUD ticket' docs/desktop-single-mode.md
grep -Fq 'docs/desktop-single-mode.md' SPEC.md
# no current document may name the retired Gemma alias; ADR 0003 preserves the
# historical decision it records. No document may call the resident model Gemma.
# (`! grep` would be exempt from errexit, so assert on empty output instead)
test -z "$(grep -rl --exclude='0003-resident-gemma-model.md' 'muniment-resident-gemma' docs/)"
test -z "$(grep -ril 'resident local gemma' docs/)"
# The public core boundary names both sets and runs in the smoke job.
boundary_adr=docs/decisions/0030-public-core-boundary.md
test -f "$boundary_adr"
grep -Fxq '## Port' "$boundary_adr"
grep -Fxq '## Stay' "$boundary_adr"
grep -Fq '| crate | muniment-core |' "$boundary_adr"
grep -Fq '| crate | muniment-desktop |' "$boundary_adr"
grep -Fq '| module | auth |' "$boundary_adr"
test -x scripts/check-core-boundary.sh
grep -Fq 'run: scripts/check-core-boundary.sh' "$ci"
python3 -B test/core-boundary.py
# companion workspace and its path-scoped CI lane
grep -Fq 'members = [".", "core", "attach", "code-diff", "cli", "acp", "runtime"]' src-tauri/Cargo.toml
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
grep -Fq "if: needs.changes.outputs.companion == 'true'" "$ci"
grep -Fq 'cargo fmt --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-code-diff --package muniment-cli --package muniment-acp --check' "$ci"
grep -Fq 'cargo clippy --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-code-diff --package muniment-cli --package muniment-acp --all-targets --locked -- -D warnings' "$ci"
grep -Fq 'cargo test --manifest-path src-tauri/Cargo.toml --package muniment-attach --package muniment-code-diff --package muniment-cli --package muniment-acp --locked' "$ci"
grep -Fq 'run: test/cli-dependency-boundary.sh' "$ci"
test -f src-tauri/runtime/Cargo.toml
test -f src-tauri/runtime/src/main.rs
grep -Fq 'cargo fmt --manifest-path src-tauri/Cargo.toml --package muniment-runtime --check' "$ci"
grep -Fq 'cargo clippy --manifest-path src-tauri/Cargo.toml --package muniment-runtime --all-targets --locked -- -D warnings' "$ci"
grep -Fq 'cargo test --manifest-path src-tauri/Cargo.toml --package muniment-runtime --locked' "$ci"
grep -Fq 'run: test/runtime-dependency-boundary.sh' "$ci"
test -d protocol-fixtures/muniment.attach/1
grep -Fq 'name: attach-fixtures-current' "$ci"
grep -Fq 'run: cargo run -p muniment-attach --bin export-attach-fixtures -- ../protocol-fixtures --check' "$ci"
grep -Fq 'attach-fixtures-current:' "$ci"
test -f src-tauri/code-diff/Cargo.toml
test -f src-tauri/code-diff/src/lib.rs
test -d protocol-fixtures/code-diff/1
grep -Fq 'name: code-diff-fixtures-current' "$ci"
grep -Fq 'run: cargo run -p muniment-code-diff --bin export-code-diff-fixtures -- ../protocol-fixtures --check' "$ci"
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
test -x test/runtime-dependency-boundary.sh
test/runtime-dependency-boundary.sh muniment-runtime muniment-core muniment-attach
for forbidden in muniment-desktop muniment-cli muniment-acp tauri tauri-plugin-dialog; do
  ! test/runtime-dependency-boundary.sh muniment-runtime muniment-core "$forbidden" \
    >/dev/null 2>&1
done
grep -Fq "needs.changes.outputs.desktop == 'true'" "$ci"
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
grep -Fq 'needs: [changes, desktop-compile]' "$ci"
test "$(grep -Fc "if: github.event_name == 'pull_request' && needs.changes.outputs.desktop == 'true'" "$ci")" -eq 2
test -z "$(git ls-files 'protocol-fixtures/muniment.attach/**' | grep -v '^protocol-fixtures/muniment.attach/1/')"
echo "smoke OK"
