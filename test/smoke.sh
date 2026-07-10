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
echo "smoke OK"
