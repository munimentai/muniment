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
installed=0
installed_bundle=/Applications/muniment.app
cleanup_status=0
cleanup_log="$raw/cleanup.log"
cleanup_status_entries=
first_failed_step=none
current_step=prepare-state
# Keep the login home before a spec changes HOME.
diagnostic_reports="$HOME/Library/Logs/DiagnosticReports"
crash_start="$run_root/crash-start"

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

run_step() {
  current_step=$1
  shift
  local result=0
  "$@" || result=$?
  if (( result != 0 )); then
    status=1
    if [[ $first_failed_step == none ]]; then first_failed_step=$current_step; fi
  fi
  return "$result"
}

log_command() {
  local log=$1
  shift
  "$@" >>"$log" 2>&1
}

collect_crash_reports() {
  local directory report
  # macOS can finish a crash report after the app exits.
  if (( status != 0 )); then sleep 5; fi
  for directory in "$diagnostic_reports" "$state_root/degraded/Library/Logs/DiagnosticReports" "$state_root/ready/Library/Logs/DiagnosticReports"; do
    [[ -d $directory ]] || continue
    find "$directory" -maxdepth 1 -type f -name 'muniment-desktop-*.ips' -newer "$crash_start" -print0 >"$run_root/crash-reports" || return 1
    while IFS= read -r -d '' report; do
      cp -- "$report" "$raw/${report##*/}" || return 1
    done <"$run_root/crash-reports"
  done
}

run_e2e() {
  local spec=$1 wdio_log=$2
  shift 2
  export MUNIMENT_E2E_DRIVER_APP_LOG="$raw/driver-app-$spec.log"
  run_step "prepare-log-$spec" touch "$MUNIMENT_E2E_DRIVER_APP_LOG" || return 1
  cleanup_step stop-before-spec stop_app
  if (( cleanup_last_status != 0 )); then
    runner_failure 'Spec process cleanup failed. The runner did not start the next spec.'
    return 1
  fi
  printf 'start-spec: %s\n' "${wdio_log##*/}" >>"$cleanup_log"
  run_step "spec-$spec" log_command "$wdio_log" npm run test:e2e "$@"
}

finalize() {
  local runner_status=$? redaction_status=0
  trap - EXIT INT TERM
  exec 2>&3
  if (( runner_status != 0 )); then status=1; fi
  if (( status != 0 )) && [[ $first_failed_step == none ]]; then first_failed_step=$current_step; fi
  cleanup_step stop-app stop_app
  if (( cleanup_status != 0 )); then status=1; fi
  if (( installed )); then cleanup_step remove-bundle rm -rf -- "$installed_bundle"; fi
  cleanup_step collect-crash-reports collect_crash_reports
  if node test/e2e/support/redact.mjs "$raw" "$safe" "$redaction_report"; then
    cleanup_step replace-artifacts rm -rf -- "$artifacts"
    if (( cleanup_last_status == 0 )); then
      cleanup_step publish-artifacts mv -- "$safe" "$artifacts"
    fi
  else
    redaction_status=1
    record_cleanup_status redact-artifacts failed
    rm -rf -- "$artifacts" "$safe"
    mkdir -p "$artifacts"
    printf 'envelope: minimal\nwithheld: guest artifacts\nreason: redaction-failed\n' >"$artifacts/envelope-reason.txt"
    cp -- "$redaction_report" "$artifacts/redaction-failure.txt" 2>/dev/null || printf 'file: unknown\ncategory: redactor-process\n' >"$artifacts/redaction-failure.txt"
  fi
  # Keep cleanup outcomes after removing the raw log.
  cleanup_log=/dev/null
  cleanup_step remove-raw rm -rf -- "$raw" "$state_root" "$safe"
  cleanup_step remove-run-files rm -f -- "$auth_url_file" "$redaction_report" "$crash_start" "$run_root/crash-reports"
  cleanup_step remove-run-root rmdir "$run_root"
  if (( cleanup_status != 0 || redaction_status != 0 )); then status=1; fi
  mkdir -p "$artifacts" || exit 1
  printf '%s' "$cleanup_status_entries" >"$artifacts/cleanup-status.log" || status=1
  # These fields contain only runner-owned labels and statuses.
  printf 'status=%s\ncleanup_status=%s\nredaction_status=%s\nfirst_failed_step=%s\n' \
    "$status" "$cleanup_status" "$redaction_status" "$first_failed_step" >"$artifacts/exit-reason.txt" || status=1
  exit "$status"
}
exec 3>&2
trap finalize EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$raw" "$state_root/ready/Documents" "$state_root/degraded/Documents" || exit 1
exec 2>>"$raw/runner-stderr.log" || exit 1
run_step prepare-crash-marker touch "$crash_start" || exit
current_step=validate-environment

[[ -n ${MUNIMENT_E2E_USERNAME:-} && -n ${MUNIMENT_E2E_PASSWORD:-} ]] || {
  echo 'required injected environment is unavailable' >&2
  status=1
  exit
}
run_step install-dependencies log_command "$raw/installer.log" npm ci --no-audit --no-fund || exit
run_step build-app log_command "$raw/installer.log" npm run tauri build -- --no-bundle --features e2e-webdriver --config src-tauri/tauri.e2e.conf.json || exit
app_binary="$PWD/src-tauri/target/release/muniment-desktop"
current_step=validate-app
[[ -x $app_binary ]] || { echo 'E2E application binary is unavailable' >&2; status=1; exit; }
run_step webdriver-marker node test/e2e/support/webdriver-release-guard.mjs present "$app_binary" || exit

# The installed smoke removes its bundle. Restore the pinned bundle for WDIO.
sha=${MUNIMENT_E2E_SOURCE_SHA:-}
current_step=validate-source
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo 'The source SHA is invalid.' >&2; status=1; exit; }
[[ -n ${GH_TOKEN:-} && -n ${GITHUB_REPOSITORY:-} ]] || { echo 'The release environment is unavailable.' >&2; status=1; exit; }
release=$(run_step fetch-release gh api "repos/${GITHUB_REPOSITORY}/releases/tags/nightly") || { first_failed_step=fetch-release; status=1; exit; }
asset_id=$(run_step identify-asset node test/e2e/support/asset-identity.mjs "$sha" macos <<<"$release") || { first_failed_step=identify-asset; status=1; exit; }
run_step download-bundle gh api -H 'Accept: application/octet-stream' "repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" >"$state_root/muniment.app.zip" || exit
run_step expand-bundle log_command "$raw/installer.log" ditto -x -k "$state_root/muniment.app.zip" "$state_root/expanded" || exit
run_step stop-before-install stop_app || exit
run_step validate-bundle-absent test ! -e "$installed_bundle" || exit
installed=1
run_step install-bundle log_command "$raw/installer.log" ditto "$state_root/expanded/muniment.app" "$installed_bundle" || exit
installed_desktop="$installed_bundle/Contents/MacOS/muniment-desktop"
run_step validate-installed-app test -x "$installed_desktop" || exit
run_step installed-webdriver-marker node test/e2e/support/webdriver-release-guard.mjs absent "$installed_desktop" || exit
for library in libsherpa-onnx-c-api.dylib libonnxruntime.1.24.4.dylib; do
  run_step "validate-$library" test -s "$installed_bundle/Contents/Resources/asr-runtime/$library" || exit
done
# The installed path resolves the ASR rpath and matches runtime client admission.
run_step install-webdriver-app log_command "$raw/installer.log" install -m 0755 "$app_binary" "$installed_desktop" || exit
run_step verify-webdriver-app cmp -s "$app_binary" "$installed_desktop" || exit
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH

export MUNIMENT_E2E_APP_BINARY="$PWD/test/e2e/support/macos-wdio-app.sh" MUNIMENT_E2E_RAW_DIR="$raw"
export MUNIMENT_E2E_REAL_APP_BINARY="$installed_desktop"
export MUNIMENT_E2E_AUTH_URL_FILE="$auth_url_file" BROWSER="$PWD/test/e2e/support/browser-launcher.sh"
run_step image-fixture openssl base64 -d -A -in test/e2e/fixtures/image-token.png.base64 -out "$state_root/image-token.png" || exit
export MUNIMENT_E2E_IMAGE_PATH="$state_root/image-token.png"

# Each spec starts with no app, runtime, or driver from the last spec.
export HOME="$state_root/degraded" MUNIMENT_E2E_HOME_PATH="$state_root/degraded-home"
run_e2e local-mode-chat "$raw/wdio-local-mode-chat.log" -- --spec test/e2e/specs/local-mode-chat.spec.js || status=1
run_e2e real-sign-in "$raw/wdio-sign-in.log" -- --spec test/e2e/specs/real-sign-in.spec.js || status=1
export HOME="$state_root/ready" MUNIMENT_E2E_HOME_PATH="$state_root/ready-home"
export MUNIMENT_E2E_ONBOARDING_ONLY=1
run_e2e onboarding "$raw/wdio-onboarding.log" || status=1
unset MUNIMENT_E2E_ONBOARDING_ONLY
export MUNIMENT_E2E_CLEANUP_ONLY=1
run_e2e cleanup "$raw/wdio-cleanup.log" || status=1
