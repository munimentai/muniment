#!/usr/bin/env bash
set -uo pipefail
set +x
umask 077

artifacts=${DCI_ARTIFACTS_DIR:-/tmp/dci-artifacts}
run_root=$(mktemp -d "${TMPDIR:-/tmp}/muniment-e2e-macos.XXXXXX") || exit 1
raw="$run_root/raw"
safe="$run_root/safe"
archive="$run_root/muniment-nightly.app.zip"
expanded="$run_root/expanded"
state_root="$run_root/state"
cleanup_log="$run_root/cleanup.log"
installed_bundle=/Applications/muniment.app
status=0
cleanup_status=0
installed=0
app_pid=
process_name=
finalized=0

# shellcheck source=../support/cleanup-ledger.sh
source test/e2e/support/cleanup-ledger.sh

cleanup_absent() { [[ ! -e $1 ]]; }
process_absent() { [[ -z $process_name ]] || ! pgrep -x "$process_name" >/dev/null; }

stop_app() {
  if [[ -n $app_pid ]]; then kill "$app_pid" 2>/dev/null || true; fi
  if [[ -n $process_name ]]; then pkill -x "$process_name" 2>/dev/null || true; fi
  for _ in {1..20}; do process_absent && return; sleep 0.25; done
  [[ -z $process_name ]] || pkill -9 -x "$process_name" 2>/dev/null || true
  process_absent
}

index_failure_artifacts() {
  find "$raw" -maxdepth 1 -type f \( -name 'page-source-*.html' -o -name 'screenshot-*.png' \) -print \
    >"$raw/failure-artifacts.log"
}

finalize() {
  (( finalized == 0 )) || return
  finalized=1
  trap - EXIT INT TERM
  cleanup_step stop-app stop_app
  if (( installed )); then cleanup_step remove-bundle rm -rf -- "$installed_bundle"; fi
  cleanup_step remove-state rm -rf -- "$state_root"
  cleanup_step bundle-gone cleanup_absent "$installed_bundle"
  cleanup_step processes-gone process_absent
  cleanup_step state-gone cleanup_absent "$state_root"

  cleanup_step stage-cleanup-log cp "$cleanup_log" "$raw/cleanup.log"
  cleanup_step index-failure-artifacts index_failure_artifacts
  cleanup_step redact-artifacts node test/e2e/support/redact.mjs "$raw" "$safe"
  redaction_status=$cleanup_last_status
  cleanup_step remove-raw rm -rf -- "$raw"
  cleanup_step remove-archive rm -f -- "$archive"
  cleanup_step remove-expanded rm -rf -- "$expanded"
  if (( redaction_status == 0 )); then
    cleanup_step replace-artifacts rm -rf -- "$artifacts"
    collection_status=$cleanup_last_status
    if (( collection_status == 0 )); then
      cleanup_step publish-artifacts mv -- "$safe" "$artifacts"
      collection_status=$cleanup_last_status
    fi
    if [[ ${MUNIMENT_E2E_FINALIZER_TEST_FAIL:-} == suppress-artifacts ]]; then collection_status=1; fi
    if (( collection_status != 0 )); then cleanup_step suppress-artifacts rm -rf -- "$artifacts"; fi
  else
    cleanup_step suppress-artifacts rm -rf -- "$artifacts"
  fi
  cleanup_step remove-safe rm -rf -- "$safe"
  cleanup_step raw-gone cleanup_absent "$raw"
  cleanup_step archive-gone cleanup_absent "$archive"
  cleanup_step expanded-gone cleanup_absent "$expanded"
  cleanup_step safe-gone cleanup_absent "$safe"
  cleanup_step remove-cleanup-log rm -f -- "$cleanup_log"
  if [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE:-0} != 1 ]]; then
    rmdir "$run_root" 2>/dev/null || cleanup_status=1
  fi
  if (( status != 0 || cleanup_status != 0 || redaction_status != 0 )); then exit 1; fi
}

handle_signal() {
  status=1
  finalize
}

trap finalize EXIT
trap 'handle_signal' INT TERM
mkdir -p "$raw" "$safe" "$expanded" "$state_root" || { status=1; exit; }
: >"$cleanup_log" || { status=1; exit; }

if [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE:-0} == 1 ]]; then
  installed=${MUNIMENT_E2E_FINALIZER_TEST_INSTALLED:-1}
  installed_bundle="$expanded/test.app"
  mkdir -p "$installed_bundle"
  if [[ -n ${MUNIMENT_E2E_FINALIZER_TEST_READY:-} ]]; then
    : >"$MUNIMENT_E2E_FINALIZER_TEST_READY"
    while :; do sleep 1; done
  fi
  finalize
  exit
fi

sha=${MUNIMENT_E2E_SOURCE_SHA:-}
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo 'invalid source SHA' >&2; status=1; exit; }
[[ -n ${GH_TOKEN:-} && -n ${GITHUB_REPOSITORY:-} ]] || { echo 'required injected environment is unavailable' >&2; status=1; exit; }
gui_user=$(stat -f '%Su' /dev/console)
[[ -n $gui_user && $gui_user != root && $gui_user != loginwindow && $(id -un) == "$gui_user" ]] || {
  echo 'runner is not executing as the active GUI test user' >&2; status=1; exit;
}
release=$(gh api "repos/${GITHUB_REPOSITORY}/releases/tags/nightly") || { status=1; exit; }
asset_id=$(node test/e2e/support/asset-identity.mjs "$sha" macos <<<"$release") || { status=1; exit; }
gh api -H 'Accept: application/octet-stream' "repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" >"$archive" || { status=1; exit; }

ditto -x -k "$archive" "$expanded" >>"$raw/install.log" 2>&1 || { status=1; exit; }
bundles=()
while IFS= read -r -d '' bundle; do bundles+=("$bundle"); done < <(find "$expanded" -type d -name '*.app' -prune -print0)
(( ${#bundles[@]} == 1 )) || { echo 'archive does not contain exactly one application bundle' >&2; status=1; exit; }
source_bundle=${bundles[0]}
plist="$source_bundle/Contents/Info.plist"
[[ -f $plist ]] || { echo 'application bundle metadata is unavailable' >&2; status=1; exit; }
process_name=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist" 2>>"$raw/install.log")
[[ -n $process_name && $process_name != */* && -x "$source_bundle/Contents/MacOS/$process_name" ]] || { echo 'application bundle does not identify one executable' >&2; status=1; exit; }

installed=1
ditto "$source_bundle" "$installed_bundle" >>"$raw/install.log" 2>&1 || { status=1; exit; }
mkdir -p "$state_root/home" "$state_root/tmp"
HOME="$state_root/home" TMPDIR="$state_root/tmp" "$installed_bundle/Contents/MacOS/$process_name" >"$raw/app.log" 2>&1 &
app_pid=$!

window_ready=0
window_wait_seconds=120
window_deadline=$((SECONDS + window_wait_seconds))
while (( SECONDS < window_deadline )); do
  kill -0 "$app_pid" 2>/dev/null || { echo 'application exited before opening a window' >&2; status=1; break; }
  window_count=$(osascript -e 'with timeout of 2 seconds' -e "tell application \"System Events\" to count (windows of process \"$process_name\" whose visible is true)" -e 'end timeout' 2>>"$raw/window.log" || true)
  if [[ $window_count =~ ^[1-9][0-9]*$ ]]; then window_ready=1; break; fi
  sleep 1
done
if (( window_ready == 0 )); then
  {
    printf 'wait_seconds=%s\n' "$window_wait_seconds"
    printf 'process_alive='
    if kill -0 "$app_pid" 2>/dev/null; then printf 'true\n'; else printf 'false\n'; fi
    printf 'last_visible_window_count=%s\n' "${window_count:-unavailable}"
  } >"$raw/first-window-timeout.log"
  printf 'healthy first window was not visible within %s seconds\n' "$window_wait_seconds" >&2
  status=1
else
  printf 'process_alive=true\nvisible_windows=%s\nscreendump=requested-by-desktop-ci\n' "$window_count" >"$raw/smoke.log"
fi
exit
