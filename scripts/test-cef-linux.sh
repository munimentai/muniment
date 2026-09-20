#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export MUNIMENT_PI_CANDIDATE=1
bundle="$(pwd)/src-tauri/target/release/cef-app"
# A failed debug run can leave browser helpers or the local runtime alive.
# Limit cleanup to this disposable test bundle.
cleanup() { pkill -f "^$bundle/(muniment-cef-helper|muniment-runtime)( |$)" || true; }
trap cleanup EXIT
cleanup
sudo -n apt-get update -qq
sudo -n apt-get install -y -qq cmake ninja-build libnss3 libxss1 libasound2t64 libgbm1 xvfb
npm ci
cargo build --manifest-path src-tauri/Cargo.toml --package muniment-runtime --package muniment-cli --package muniment-acp --release --locked
bash .github/build-reader.sh src-tauri/target/release/muniment-reader
cargo build --manifest-path src-tauri/Cargo.toml --bin muniment-cef-helper --release --locked
npm run tauri build -- --no-bundle --features cef-smoke
node scripts/package-cef-linux.mjs
bundle=$(realpath src-tauri/target/release/cef-app)
sudo -n chown root:root "$bundle/chrome-sandbox"
sudo -n chmod 4755 "$bundle/chrome-sandbox"
export CHROME_DEVEL_SANDBOX="$bundle/chrome-sandbox"
export MUNIMENT_STATE_DIR
MUNIMENT_STATE_DIR=$(mktemp -d /tmp/muniment-cef-test.XXXXXX)
artifacts=${DCI_ARTIFACTS_DIR:-/tmp/dci-artifacts}
mkdir -p "$artifacts"
export GDK_BACKEND=x11
for phase in write read; do
  export MUNIMENT_CEF_SMOKE_PHASE=$phase
  timeout 90s xvfb-run -a dbus-run-session -- "$bundle/muniment-desktop" --cef-smoke > "$artifacts/cef-$phase.log" 2>&1
  cp "$MUNIMENT_STATE_DIR/browser/smoke-$phase.json" "$artifacts/"
  node -e 'const result=JSON.parse(require("fs").readFileSync(process.argv[1]));if(!result.ok){console.error(result.error);process.exit(1)}' "$artifacts/smoke-$phase.json"
  base64 -d "$MUNIMENT_STATE_DIR/browser/smoke-$phase.png.base64" > "$artifacts/cef-$phase.png"
done
echo 'CEF Linux: login, saved profile restart, input, snapshot, screenshot, app isolation, and stop control passed.'
