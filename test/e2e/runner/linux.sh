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
cleanup_log="$raw/cleanup.log"
installer_log="$raw/installer.log"
status=0
cleanup_status=0
installed=0

cleanup_step() {
  local label=$1; shift
  if "$@" >>"$cleanup_log" 2>&1; then printf '%s: ok\n' "$label" >>"$cleanup_log"; else
    printf '%s: failed\n' "$label" >>"$cleanup_log"
    cleanup_status=1
  fi
}

stop_matching() {
  pkill -f "$1" 2>/dev/null || true
  ! pgrep -f "$1" >/dev/null
}

finalize() {
  trap - EXIT INT TERM
  cleanup_step stop-wdio stop_matching '[w]dio.*test/e2e/wdio.conf.js'
  cleanup_step stop-driver stop_matching '[t]auri-driver'
  cleanup_step stop-browser-driver stop_matching '[c]hromedriver.*9515'
  cleanup_step stop-app bash -c 'pkill -x muniment 2>/dev/null || true; ! pgrep -x muniment >/dev/null'
  # auth_sign_out revokes the fixture session before the app exits when the UI
  # flow succeeds; removal below guarantees no reusable local session remains.
  if (( installed )); then cleanup_step remove-package sudo apt-get remove -y muniment; fi
  cleanup_step remove-state rm -rf -- "$state_root"
  cleanup_step processes-gone bash -c '! pgrep -x muniment && ! pgrep -f "[t]auri-driver" && ! pgrep -f "[w]dio.*test/e2e/wdio.conf.js"'

  redaction_status=0
  node test/e2e/support/redact.mjs "$raw" "$safe" || redaction_status=1
  rm -rf -- "$raw"; rm -f -- "$deb" "$auth_url_file"
  if (( redaction_status == 0 )); then
    rm -rf -- "$artifacts"; mkdir -m 700 "$artifacts"
    cp -a "$safe"/. "$artifacts"/
  fi
  rm -rf -- "$safe"
  if (( status != 0 || cleanup_status != 0 || redaction_status != 0 )); then exit 1; fi
}
trap finalize EXIT INT TERM

sha=${MUNIMENT_E2E_SOURCE_SHA:-}
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo 'invalid source SHA' >&2; status=1; exit; }
[[ -n ${GH_TOKEN:-} && -n ${MUNIMENT_E2E_USERNAME:-} && -n ${MUNIMENT_E2E_PASSWORD:-} ]] || { echo 'required injected environment is unavailable' >&2; status=1; exit; }
asset="nightly-${sha}-linux-muniment.deb"

# Resolve and validate identity before package installation. jq counts the API
# result so missing/duplicate assets and a release pointing elsewhere fail shut.
release=$(gh api "repos/${GITHUB_REPOSITORY}/releases/tags/nightly") || { status=1; exit; }
[[ $(jq -r '.target_commitish' <<<"$release") == "$sha" ]] || { echo 'release identity mismatch' >&2; status=1; exit; }
[[ $(jq --arg name "$asset" '[.assets[] | select(.name == $name)] | length' <<<"$release") == 1 ]] || { echo 'missing or duplicate Linux artifact' >&2; status=1; exit; }
asset_id=$(jq -r --arg name "$asset" '.assets[] | select(.name == $name) | .id' <<<"$release")
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
xvfb-run -a npm run test:e2e >"$raw/wdio.log" 2>"$raw/driver-app.log" || status=1
exit
