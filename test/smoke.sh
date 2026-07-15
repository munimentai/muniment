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
# Offline ASR runtime identity and redistribution notices are release gates.
asr_runtime=src-tauri/asr-runtime.toml
grep -Fq 'version = "1.13.2"' "$asr_runtime"
grep -Fq 'commit = "13d0ae6c539d2809d32f5eaa3ef1db0c459d0b24"' "$asr_runtime"
for target in x86_64-unknown-linux-gnu x86_64-pc-windows-msvc x86_64-apple-darwin aarch64-apple-darwin; do
  grep -Fq "[targets.$target]" "$asr_runtime"
done
test -f docs/third-party/sherpa-onnx-NOTICE.md
grep -Fq 'Apache License 2.0' docs/third-party/sherpa-onnx-NOTICE.md
grep -Fq 'Microsoft ONNX Runtime' docs/third-party/sherpa-onnx-NOTICE.md
grep -Fq '"asr-runtime/*": "asr-runtime/"' src-tauri/tauri.conf.json
grep -Fq 'unsupported desktop ASR target' src-tauri/build.rs
test -x test/stage-asr-runtime.sh
grep -Fq 'stage-asr-runtime.sh' .github/workflows/ci.yml
# fonts are vendored (CSP is default-src 'self'; no CDN requests)
test -f src/fonts/SchibstedGrotesk-latin.woff2
test -f src/fonts/CommitMono-VF.woff2
# desktop CI: native checks gate bundles, while docs-only PRs and main pushes
# remain smoke-only
ci=.github/workflows/ci.yml
grep -Fq 'name: Desktop compile preflight (${{ matrix.platform }})' "$ci"
grep -Fq "if: github.event_name == 'pull_request' && needs.smoke.outputs.docs_only != 'true'" "$ci"
grep -Fq 'platform: [windows, macos]' "$ci"
grep -Fq 'cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets' "$ci"
test -f src-tauri/Cargo.lock
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
test "$(grep -Fc "if: github.event_name == 'pull_request' && needs.smoke.outputs.docs_only != 'true'" "$ci")" -eq 2
echo "smoke OK"
