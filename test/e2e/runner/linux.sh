#!/usr/bin/env bash
set -uo pipefail
set +x
umask 077

artifacts=/tmp/dci-artifacts
raw=$(mktemp -d /tmp/muniment-e2e-raw.XXXXXX)
safe=$(mktemp -d /tmp/muniment-e2e-safe.XXXXXX)
deb=$(mktemp /tmp/muniment-nightly.XXXXXX.deb)
auth_url_file=$(mktemp /tmp/muniment-e2e-auth-url.XXXXXX)
state_root=$(mktemp -d /tmp/muniment-e2e-state.XXXXXX)
cleanup_log=$(mktemp /tmp/muniment-e2e-cleanup.XXXXXX.log)
installer_log="$raw/installer.log"
status=0
cleanup_status=0
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

finalize() {
  trap - EXIT INT TERM
  # A crashed run may leave the official driver session owning its ports.
  # Clear stale automation before opening the bounded recovery session, while
  # retaining the app, browser driver, and state that recovery needs.
  cleanup_step stop-wdio stop_matching '[w]dio.*test/e2e/wdio.conf.js'
  cleanup_step stop-driver stop_matching '[t]auri-driver'
  # Launch a fresh official-driver session against the same app state. This is
  # bounded and idempotent, and still runs if the main WDIO process crashed.
  if (( ready )); then cleanup_step revoke-session timeout 45 env MUNIMENT_E2E_CLEANUP_ONLY=1 xvfb-run -a npm run test:e2e; fi
  cleanup_step stop-browser-driver stop_matching '[c]hromedriver.*9515'
  cleanup_step stop-app bash -c 'pkill -x muniment 2>/dev/null || true; ! pgrep -x muniment >/dev/null'
  if (( installed )); then cleanup_step remove-package sudo apt-get remove -y muniment; fi
  cleanup_step remove-state rm -rf -- "$state_root"
  cleanup_step package-gone package_absent
  cleanup_step processes-gone bash -c '! pgrep -x muniment && ! pgrep -f "[t]auri-driver" && ! pgrep -f "[c]hromedriver.*9515" && ! pgrep -f "[w]dio.*test/e2e/wdio.conf.js"'
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
    cleanup_step suppress-artifacts rm -rf -- "$artifacts"
  fi
  cleanup_step remove-safe rm -rf -- "$safe"
  cleanup_step raw-gone cleanup_absent "$raw"
  cleanup_step package-file-gone cleanup_absent "$deb"
  cleanup_step auth-url-gone cleanup_absent "$auth_url_file"
  cleanup_step safe-gone cleanup_absent "$safe"
  cleanup_step remove-cleanup-log rm -f -- "$cleanup_log"
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
app_binary=$(command -v muniment)
chromedriver --port=9515 --allowed-ips=127.0.0.1 >>"$raw/chromedriver.log" 2>&1 &
export MUNIMENT_E2E_APP_BINARY="$app_binary" MUNIMENT_E2E_RAW_DIR="$raw"
export MUNIMENT_E2E_AUTH_URL_FILE="$auth_url_file" BROWSER="$PWD/test/e2e/support/browser-launcher.sh"
export XDG_DATA_HOME="$state_root/data" XDG_CONFIG_HOME="$state_root/config" XDG_CACHE_HOME="$state_root/cache"
ready=1
xvfb-run -a npm run test:e2e >"$raw/wdio.log" 2>"$raw/driver-app.log" || status=1
exit
