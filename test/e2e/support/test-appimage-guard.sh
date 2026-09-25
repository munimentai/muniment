#!/usr/bin/env bash
set -euo pipefail

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
mkdir "$work/payload"
printf 'TAURI_WEBDRIVER_PORT\n' > "$work/payload/muniment-desktop"
mksquashfs "$work/payload" "$work/payload.squashfs" -noappend -no-progress -comp zstd -processors 1 >/dev/null
node --input-type=module - "$work" <<'JS'
import fs from 'node:fs'
const directory = process.argv[2]
const runtime = Buffer.alloc(512)
Buffer.from('7f454c460201', 'hex').copy(runtime)
Buffer.from('414902', 'hex').copy(runtime, 8)
runtime.writeBigUInt64LE(128n, 40)
runtime.writeUInt16LE(64, 58)
runtime.writeUInt16LE(1, 60)
runtime.writeUInt32LE(1, 132)
runtime.writeBigUInt64LE(256n, 152)
runtime.writeBigUInt64LE(256n, 160)
fs.writeFileSync(`${directory}/fixture.AppImage`, Buffer.concat([runtime, fs.readFileSync(`${directory}/payload.squashfs`)]))
JS
bash test/e2e/support/webdriver-artifact-guard.sh present "$work/fixture.AppImage"
if bash test/e2e/support/webdriver-artifact-guard.sh absent "$work/fixture.AppImage" >"$work/result" 2>&1; then
  echo 'The release guard accepted a WebDriver marker in compressed AppImage data' >&2
  exit 1
fi
grep -q 'wdio-webdriver marker must be absent' "$work/result"
echo 'Compressed AppImage marker guard passed'
