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
cleanup_status=0
cleanup_log="$raw/cleanup.log"

# shellcheck source=../support/cleanup-ledger.sh
source test/e2e/support/cleanup-ledger.sh
# shellcheck source=../support/runner-failure.sh
source test/e2e/support/runner-failure.sh

harness_processes_gone() {
  ! pgrep -f '(^|/)muniment-desktop( |$)' >/dev/null &&
    ! pgrep -x muniment >/dev/null &&
    ! pgrep -f '(^|/)muniment-runtime( |$)' >/dev/null &&
    ! pgrep -x tauri-driver >/dev/null &&
    ! pgrep -x safaridriver >/dev/null &&
    ! pgrep -f '[w]dio.*test/e2e/wdio.conf.js|[@]wdio/local-runner/.*/run.js' >/dev/null
}

runtime_job_stopped() {
  local job_status
  job_status=$(launchctl print "gui/$(id -u)/ai.muniment.runtime" 2>/dev/null || true)
  ! grep -Eq 'state = running|pid = [1-9][0-9]*' <<<"$job_status"
}

stop_app() {
  local signal attempt
  for signal in TERM KILL; do
    # Stop automation first so it cannot restart the app during cleanup.
    pkill -"$signal" -f '[w]dio.*test/e2e/wdio.conf.js|[@]wdio/local-runner/.*/run.js' 2>/dev/null || true
    pkill -"$signal" -f '(^|/)muniment-desktop( |$)' 2>/dev/null || true
    pkill -"$signal" -x muniment 2>/dev/null || true
    pkill -"$signal" -x tauri-driver 2>/dev/null || true
    pkill -"$signal" -x safaridriver 2>/dev/null || true
    launchctl kill "SIG$signal" "gui/$(id -u)/ai.muniment.runtime" 2>/dev/null || true
    pkill -"$signal" -f '(^|/)muniment-runtime( |$)' 2>/dev/null || true
    for attempt in {1..20}; do
      if harness_processes_gone && runtime_job_stopped; then return 0; fi
      sleep 0.25
    done
  done
  harness_processes_gone && runtime_job_stopped
}

run_e2e() {
  local wdio_log=$1
  shift
  cleanup_step stop-before-spec stop_app
  if (( cleanup_last_status != 0 )); then
    runner_failure 'Spec process cleanup failed. The runner did not start the next spec.'
    return 1
  fi
  printf 'start-spec: %s\n' "${wdio_log##*/}" >>"$cleanup_log"
  npm run test:e2e "$@" >"$wdio_log" 2>&1
}

mkdir -p "$raw" "$state_root/ready" "$state_root/degraded" || exit 1

finalize() {
  trap - EXIT INT TERM
  cleanup_step stop-app stop_app
  if (( cleanup_status != 0 )); then status=1; fi
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

# Each spec starts with no app, runtime, or driver from the last spec.
export HOME="$state_root/degraded" MUNIMENT_E2E_HOME_PATH="$state_root/degraded-home"
run_e2e "$raw/wdio.log" -- --spec test/e2e/specs/local-mode-chat.spec.js || status=1
run_e2e "$raw/wdio-sign-in.log" -- --spec test/e2e/specs/real-sign-in.spec.js || status=1
export HOME="$state_root/ready" MUNIMENT_E2E_HOME_PATH="$state_root/ready-home"
export MUNIMENT_E2E_ONBOARDING_ONLY=1
run_e2e "$raw/wdio-onboarding.log" || status=1
unset MUNIMENT_E2E_ONBOARDING_ONLY
export MUNIMENT_E2E_CLEANUP_ONLY=1
run_e2e "$raw/wdio-cleanup.log" || status=1
