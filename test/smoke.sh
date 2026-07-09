#!/usr/bin/env bash
# M0 structure smoke (runs on the shared runners — no GUI/build here).
set -eu
grep -q '"identifier": "ai.muniment.desktop"' src-tauri/tauri.conf.json
grep -q 'tauri_build::build' src-tauri/build.rs
grep -q 'generate_context' src-tauri/src/main.rs
test -f src-tauri/icons/icon.ico
test -f src-tauri/icons/icon.icns
test -f src/index.html
echo "smoke OK"
