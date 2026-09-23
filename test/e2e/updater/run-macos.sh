#!/usr/bin/env bash
set -euo pipefail
umask 077
# Run only inside the disposable desktop-ci macOS guest, after the public release exists.
[[ ${MUNIMENT_UPDATE_PROOF_DISPOSABLE:-} == 1 && $(id -un) == harness && $HOME == /Users/harness ]]
[[ $(uname -s) == Darwin && -d /Users/harness/dci-work/src/.git ]]
[[ $# == 4 ]]
baseline=$1 expected_archive=$2 expected_archive_sha=$3 version=$4
proof=/tmp/muniment-updater-install-proof
repo=/Users/harness/dci-work/src
[[ -f $baseline && -f $expected_archive && ! -e /Applications/muniment.app && ! -e "$HOME/.muniment" ]]
python3 - "$expected_archive" "$expected_archive_sha" <<'PY'
import hashlib,re,sys
assert re.fullmatch(r'[a-f0-9]{64}',sys.argv[2])
with open(sys.argv[1],'rb') as source:
    value=hashlib.sha256()
    for chunk in iter(lambda:source.read(1024*1024),b''):value.update(chunk)
assert value.hexdigest()==sys.argv[2]
PY
mkdir -p "$proof/expected" "$proof/raw" "$proof/home" "$HOME/.muniment"
tar -xpf "$expected_archive" -C "$proof/expected"
codesign --verify --deep --strict "$proof/expected/muniment.app"
spctl --assess --type execute "$proof/expected/muniment.app"
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$proof/expected/muniment.app/Contents/Info.plist") == "$version" ]]
[[ $(shasum -a 256 "$baseline" | awk '{print $1}') == 186db597faee60ae92047f5692ea1658615239a05ee0b7dd3b68e28bcbc97c48 ]]
tar -xpf "$baseline" -C /Applications
node test/e2e/updater/sign-baseline.mjs /Applications/muniment.app
codesign --verify --deep --strict /Applications/muniment.app
spctl --assess --type execute /Applications/muniment.app
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' /Applications/muniment.app/Contents/Info.plist) == 0.0.0 ]]
export MUNIMENT_UPDATE_PROOF_INSTALLED_FILE=/Applications/muniment.app/Contents/MacOS/muniment-desktop
export MUNIMENT_UPDATE_PROOF_EXPECTED_APP="$proof/expected/muniment.app"
export MUNIMENT_UPDATE_PROOF_SHA256
MUNIMENT_UPDATE_PROOF_SHA256=$(shasum -a 256 "$MUNIMENT_UPDATE_PROOF_EXPECTED_APP/Contents/MacOS/muniment-desktop" | awk '{print $1}')
export MUNIMENT_UPDATE_PROOF_VERSION="$version"
export MUNIMENT_UPDATE_PROOF_SENTINEL="$HOME/.muniment/update-proof-sentinel"
export MUNIMENT_UPDATE_PROOF_EVIDENCE="$proof/result.json"
export MUNIMENT_UPDATE_PROOF_HOME="$proof/home"
export MUNIMENT_UPDATE_PROOF_PID_FILE="$proof/before.pid"
export MUNIMENT_UPDATE_PROOF_WINDOW_PROBE="$proof/macos-window-count"
export MUNIMENT_UPDATE_PROOF_ENDPOINT="$HOME/.muniment/muniment/attach-v1.sock"
export MUNIMENT_UPDATE_PROOF_DATABASE="$HOME/.muniment/runs.sqlite3"
export MUNIMENT_UPDATE_PROOF_REPO="$repo"
export MUNIMENT_E2E_REAL_APP_BINARY="$MUNIMENT_UPDATE_PROOF_INSTALLED_FILE"
export MUNIMENT_E2E_DRIVER_APP_LOG="$proof/raw/app.log"
export MUNIMENT_E2E_APP_BINARY="$proof/baseline-app.sh"
export MUNIMENT_E2E_RAW_DIR="$proof/raw"
unset MUNIMENT_STATE_DIR DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
printf 'preserve this isolated profile\n' > "$MUNIMENT_UPDATE_PROOF_SENTINEL"
touch "$MUNIMENT_E2E_DRIVER_APP_LOG"
chmod 700 "$proof/baseline-app.sh"
cd "$repo"
npm ci --no-audit --no-fund
bash test/e2e/support/macos-keychain-session.sh npm run test:e2e -- --mochaOpts.timeout 600000 --spec "$proof/install.spec.mjs"
python3 - "$MUNIMENT_UPDATE_PROOF_EVIDENCE" <<'PY'
import json,sys
proof=json.load(open(sys.argv[1]))
for field in ['installedBytesMatch','sentinelPreserved','restartVerified','runtimeConnectionVerified','storedThreadVerifiedAfterRestart','installedBundleMatches','signatureVerified','gatekeeperAccepted']:
    assert proof[field] is True,field
print(json.dumps(proof,indent=2))
PY
