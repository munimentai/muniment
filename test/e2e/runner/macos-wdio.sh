#!/usr/bin/env bash
set -uo pipefail
set +x
umask 077

artifacts=${DCI_ARTIFACTS_DIR:-/tmp/dci-artifacts}
run_root=$(mktemp -d "${TMPDIR:-/tmp}/muniment-wdio-macos.XXXXXX") || exit 1
raw="$run_root/raw"
safe="$run_root/safe"
state_root="$run_root/state"
auth_url_file="$run_root/auth-url"
redaction_report="$run_root/redaction-failure.txt"
status=0
mkdir -p "$raw" "$state_root/ready" "$state_root/degraded" || exit 1

finalize() {
  trap - EXIT INT TERM
  pkill -f '[w]dio.*test/e2e/wdio.conf.js' 2>/dev/null || true
  pkill -x muniment 2>/dev/null || true
  if node test/e2e/support/redact.mjs "$raw" "$safe" "$redaction_report"; then
    rm -rf -- "$artifacts"
    mv -- "$safe" "$artifacts" || status=1
  else
    status=1
    rm -rf -- "$artifacts" "$safe"
    mkdir -p "$artifacts"
    printf 'envelope: minimal\nwithheld: guest artifacts\nreason: redaction-failed\n' >"$artifacts/envelope-reason.txt"
    : >"$artifacts/cleanup-status.log"
    cp -- "$redaction_report" "$artifacts/redaction-failure.txt" 2>/dev/null || printf 'file: unknown\ncategory: redactor-process\n' >"$artifacts/redaction-failure.txt"
  fi
  rm -rf -- "$raw" "$state_root"
  rm -f -- "$auth_url_file" "$redaction_report"
  rmdir "$run_root" 2>/dev/null || true
  exit "$status"
}
trap finalize EXIT INT TERM

[[ -n ${MUNIMENT_E2E_USERNAME:-} && -n ${MUNIMENT_E2E_PASSWORD:-} ]] || {
  echo 'required injected environment is unavailable' >&2
  status=1
  exit
}
npm ci --no-audit --no-fund >"$raw/installer.log" 2>&1 || { status=1; exit; }
npm run tauri build -- --no-bundle --features e2e-webdriver --config src-tauri/tauri.e2e.conf.json >>"$raw/installer.log" 2>&1 || { status=1; exit; }
app_binary="$PWD/src-tauri/target/release/muniment-desktop"
[[ -x $app_binary ]] || { echo 'E2E application binary is unavailable' >&2; status=1; exit; }
node test/e2e/support/webdriver-release-guard.mjs present "$app_binary" || { status=1; exit; }

export MUNIMENT_E2E_APP_BINARY="$app_binary" MUNIMENT_E2E_RAW_DIR="$raw"
export MUNIMENT_E2E_AUTH_URL_FILE="$auth_url_file" BROWSER="$PWD/test/e2e/support/browser-launcher.sh"
openssl base64 -d -A -in test/e2e/fixtures/image-token.png.base64 -out "$state_root/image-token.png" || { status=1; exit; }
export MUNIMENT_E2E_IMAGE_PATH="$state_root/image-token.png"

export HOME="$state_root/ready" MUNIMENT_E2E_HOME_PATH="$state_root/ready-home"
export MUNIMENT_E2E_ONBOARDING_ONLY=1
npm run test:e2e >"$raw/wdio-onboarding.log" 2>&1 || status=1
unset MUNIMENT_E2E_ONBOARDING_ONLY
export HOME="$state_root/degraded" MUNIMENT_E2E_HOME_PATH="$state_root/degraded-home"
npm run test:e2e >"$raw/wdio.log" 2>&1 || status=1
export MUNIMENT_E2E_CLEANUP_ONLY=1
npm run test:e2e >"$raw/wdio-cleanup.log" 2>&1 || status=1
