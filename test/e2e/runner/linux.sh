#!/usr/bin/env bash
set -uo pipefail
set +x
umask 077

artifacts=${DCI_ARTIFACTS_DIR:-/tmp/dci-artifacts}
raw=$(mktemp -d /tmp/muniment-e2e-raw.XXXXXX)
safe=$(mktemp -d /tmp/muniment-e2e-safe.XXXXXX)
deb=$(mktemp /tmp/muniment-nightly.XXXXXX.deb)
auth_url_file=$(mktemp /tmp/muniment-e2e-auth-url.XXXXXX)
state_root=$(mktemp -d /tmp/muniment-e2e-state.XXXXXX)
image_fixture="$state_root/image-token.png"
cleanup_log=$(mktemp /tmp/muniment-e2e-cleanup.XXXXXX.log)
cleanup_status_ledger=$(mktemp /tmp/muniment-e2e-cleanup-status.XXXXXX.log)
installer_log="$raw/installer.log"
status=${MUNIMENT_E2E_FINALIZER_TEST_STATUS:-0}
cleanup_status=0
collection_status=0
installed=0
ready=0

# shellcheck source=../support/cleanup-ledger.sh
source test/e2e/support/cleanup-ledger.sh

cleanup_absent() { [[ ! -e $1 ]]; }
package_absent() { ! dpkg-query -W -f='${db:Status-Status}' muniment 2>/dev/null | grep -q 'installed'; }

stop_matching() {
  pkill -f "$1" 2>/dev/null || true
  ! pgrep -f "$1" >/dev/null
}

run_e2e() {
  local wdio_log=$1 driver_log=$2 run_timeout=${3:-0} run_status=0
  tauri-driver --port 4444 >"$driver_log" 2>&1 &
  if ! timeout 30 bash -c 'until (: >/dev/tcp/127.0.0.1/4444) 2>/dev/null; do sleep 0.2; done'; then
    stop_matching '[t]auri-driver'
    return 1
  fi
  if (( run_timeout > 0 )); then
    timeout "$run_timeout" xvfb-run -a npm run test:e2e >"$wdio_log" 2>&1 || run_status=$?
  else
    xvfb-run -a npm run test:e2e >"$wdio_log" 2>&1 || run_status=$?
  fi
  stop_matching '[t]auri-driver' || run_status=1
  return "$run_status"
}

run_cleanup_e2e() {
  MUNIMENT_E2E_CLEANUP_ONLY=1 run_e2e "$raw/wdio-cleanup.log" "$raw/driver-cleanup.log" 45
}

emit_artifacts() {
  local source=$1 archive envelope
  archive=$(mktemp /tmp/muniment-e2e-artifacts.XXXXXX.tar.gz) || return 1
  envelope=$(mktemp /tmp/muniment-e2e-envelope.XXXXXX) || { rm -f -- "$archive"; return 1; }
  if [[ ${MUNIMENT_E2E_FINALIZER_TEST_FAIL:-} == publish-envelope ]] ||
    ! tar -czf "$archive" -C "$source" . ||
    ! tar -tzf "$archive" >/dev/null ||
    ! {
      printf '%s\n' '=== DESKTOP-CI ARTIFACTS BEGIN ==='
      base64 -w 0 "$archive"
      printf '\n%s\n' '=== DESKTOP-CI ARTIFACTS END ==='
    } >"$envelope"; then
    rm -f -- "$archive" "$envelope"
    return 1
  fi
  rm -f -- "$archive"
  cat "$envelope"
  local emit_status=$?
  rm -f -- "$envelope"
  return "$emit_status"
}

# Redaction or publication failed, so nothing the guest wrote may leave the VM.
# Publish instead the one bundle that is safe by construction -- the fixed
# cleanup labels with their ok/failed words -- so the lane still names the step
# that aborted rather than reporting an evidence-free infrastructure failure.
emit_minimal_artifacts() {
  local reason=$1 minimal emit_status
  minimal=$(mktemp -d /tmp/muniment-e2e-minimal.XXXXXX) || return 1
  printf 'envelope: minimal\nwithheld: guest artifacts\nreason: %s\n' "$reason" >"$minimal/envelope-reason.txt" || {
    rm -rf -- "$minimal"; return 1
  }
  cp -- "$cleanup_status_ledger" "$minimal/cleanup-status.log" 2>/dev/null || : >"$minimal/cleanup-status.log"
  emit_artifacts "$minimal"
  emit_status=$?
  rm -rf -- "$minimal"
  return "$emit_status"
}

finalize() {
  trap - EXIT INT TERM
  # A crashed run may leave the official driver session owning its ports.
  # Clear stale automation before opening the bounded recovery session, while
  # retaining the app, browser driver, and state that recovery needs.
  cleanup_step stop-wdio stop_matching '[w]dio.*test/e2e/wdio.conf.js'
  cleanup_step stop-driver stop_matching '[t]auri-driver'
  # Launch a fresh external-driver session against the same app state. This is
  # bounded and idempotent, and still runs if the main WDIO process crashed.
  if (( ready )); then cleanup_step revoke-session run_cleanup_e2e; fi
  cleanup_step stop-browser-driver stop_matching '[c]hromedriver.*9515'
  cleanup_step stop-app bash -c "pkill -f '(^|/)muniment-desktop( |$)' 2>/dev/null || true; pkill -x muniment 2>/dev/null || true; ! pgrep -f '(^|/)muniment-desktop( |$)' >/dev/null && ! pgrep -x muniment >/dev/null"
  if (( installed )); then cleanup_step remove-package sudo apt-get remove -y muniment; fi
  cleanup_step remove-state rm -rf -- "$state_root"
  cleanup_step package-gone package_absent
  cleanup_step processes-gone bash -c "! pgrep -f '(^|/)muniment-desktop( |$)' && ! pgrep -x muniment && ! pgrep -f '[t]auri-driver' && ! pgrep -f '[c]hromedriver.*9515' && ! pgrep -f '[w]dio.*test/e2e/wdio.conf.js'"
  cleanup_step state-gone cleanup_absent "$state_root"

  cleanup_step stage-cleanup-log cp "$cleanup_log" "$raw/cleanup.log"
  cleanup_step redact-artifacts node test/e2e/support/redact.mjs "$raw" "$safe"
  redaction_status=$cleanup_last_status
  cleanup_step remove-raw rm -rf -- "$raw"
  cleanup_step remove-package-file rm -f -- "$deb"
  cleanup_step remove-auth-url rm -f -- "$auth_url_file"
  if (( redaction_status == 0 )); then
    cleanup_step replace-artifacts rm -rf -- "$artifacts"
    collection_status=$cleanup_last_status
    if (( collection_status == 0 )); then
      cleanup_step publish-artifacts mv -- "$safe" "$artifacts"
      collection_status=$cleanup_last_status
    fi
    # Let the failure-path test exercise suppression itself without performing
    # any filesystem operations or changing production control flow.
    if [[ ${MUNIMENT_E2E_FINALIZER_TEST_FAIL:-} == suppress-artifacts ]]; then collection_status=1; fi
    if (( collection_status != 0 )); then cleanup_step suppress-artifacts rm -rf -- "$artifacts"; fi
  else
    collection_status=1
    cleanup_step suppress-artifacts rm -rf -- "$artifacts"
  fi
  cleanup_step remove-safe rm -rf -- "$safe"
  cleanup_step raw-gone cleanup_absent "$raw"
  cleanup_step package-file-gone cleanup_absent "$deb"
  cleanup_step auth-url-gone cleanup_absent "$auth_url_file"
  cleanup_step safe-gone cleanup_absent "$safe"
  cleanup_step remove-cleanup-log rm -f -- "$cleanup_log"
  # Redaction is the gate, not cleanup. Requiring a clean cleanup here meant any
  # early guest abort -- the failure most in need of evidence -- published
  # nothing at all, and the lane could only report "desktop-ci infrastructure".
  if (( redaction_status == 0 && collection_status == 0 )); then
    emit_artifacts "$artifacts" || cleanup_status=1
  elif (( redaction_status != 0 )); then
    emit_minimal_artifacts redaction-failed || cleanup_status=1
  else
    emit_minimal_artifacts publication-failed || cleanup_status=1
  fi
  rm -f -- "$cleanup_status_ledger"
  if (( status != 0 || cleanup_status != 0 || redaction_status != 0 )); then exit 1; fi
}

if [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE:-0} == 1 ]]; then
  ready=${MUNIMENT_E2E_FINALIZER_TEST_READY:-1}
  installed=${MUNIMENT_E2E_FINALIZER_TEST_INSTALLED:-1}
  finalize
  exit
fi
trap finalize EXIT INT TERM

sha=${MUNIMENT_E2E_SOURCE_SHA:-}
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo 'invalid source SHA' >&2; status=1; exit; }
[[ -n ${GH_TOKEN:-} && -n ${MUNIMENT_E2E_USERNAME:-} && -n ${MUNIMENT_E2E_PASSWORD:-} ]] || { echo 'required injected environment is unavailable' >&2; status=1; exit; }
base64 --decode test/e2e/fixtures/image-token.png.base64 >"$image_fixture" || { status=1; exit; }
# Resolve and validate identity before package installation. Missing/duplicate
# assets and a release pointing elsewhere fail shut.
release=$(gh api "repos/${GITHUB_REPOSITORY}/releases/tags/nightly") || { status=1; exit; }
asset_id=$(node test/e2e/support/asset-identity.mjs "$sha" <<<"$release") || { status=1; exit; }
gh api -H 'Accept: application/octet-stream' "repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" >"$deb" || { status=1; exit; }
[[ $(dpkg-deb -f "$deb" Package) == muniment ]] || { echo 'package identity mismatch' >&2; status=1; exit; }

sudo apt-get update -qq >>"$installer_log" 2>&1 || { status=1; exit; }
installed=1
sudo apt-get install -y -qq webkit2gtk-driver xvfb chromium chromium-driver "$deb" >>"$installer_log" 2>&1 || { status=1; exit; }
npm ci --no-audit --no-fund >>"$installer_log" 2>&1 || { status=1; exit; }
command -v tauri-driver >/dev/null || cargo install tauri-driver --version 2.0.5 --locked >>"$installer_log" 2>&1 || { status=1; exit; }
app_binary=$(command -v muniment-desktop || command -v muniment) || { echo 'installed application binary is unavailable' >&2; status=1; exit; }
chromedriver --port=9515 --allowed-ips=127.0.0.1 >>"$raw/chromedriver.log" 2>&1 &
export MUNIMENT_E2E_APP_BINARY="$app_binary" MUNIMENT_E2E_RAW_DIR="$raw"
export MUNIMENT_E2E_AUTH_URL_FILE="$auth_url_file" BROWSER="$PWD/test/e2e/support/browser-launcher.sh"
export MUNIMENT_E2E_IMAGE_PATH="$image_fixture"
ready=1
export XDG_DATA_HOME="$state_root/ready/data" XDG_CONFIG_HOME="$state_root/ready/config" XDG_CACHE_HOME="$state_root/ready/cache"
export MUNIMENT_E2E_ONBOARDING_ONLY=1 MUNIMENT_E2E_MODEL_READY=1 MUNIMENT_E2E_HOME_PATH="$state_root/ready-home"
run_e2e "$raw/wdio-onboarding.log" "$raw/driver-onboarding.log" || status=1
unset MUNIMENT_E2E_ONBOARDING_ONLY MUNIMENT_E2E_MODEL_READY
export XDG_DATA_HOME="$state_root/degraded/data" XDG_CONFIG_HOME="$state_root/degraded/config" XDG_CACHE_HOME="$state_root/degraded/cache"
export MUNIMENT_E2E_HOME_PATH="$state_root/degraded-home"
export MUNIMENT_E2E_FORCE_MANUAL=1
run_e2e "$raw/wdio.log" "$raw/driver-app.log" || status=1
exit
