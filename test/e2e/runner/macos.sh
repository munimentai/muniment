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
runtime_state=${MUNIMENT_E2E_RUNTIME_STATE:-"$HOME/.local/share/ai.muniment.desktop"}
runtime_config=${MUNIMENT_E2E_RUNTIME_CONFIG:-"$HOME/.config/ai.muniment.desktop"}
runtime_log="$HOME/Library/Logs/Muniment/runtime.log"
cleanup_log="$run_root/cleanup.log"
cleanup_status_ledger=
cleanup_status_entries=
first_failed_step=none
redaction_report="$run_root/redaction-failure.txt"
window_probe="$run_root/window-count"
installed_bundle=/Applications/muniment.app
status=0
cleanup_status=0
installed=0
runtime_touched=${MUNIMENT_E2E_FINALIZER_TEST_MODE:-0}
app_pid=
process_name=
finalized=0

# shellcheck source=../support/cleanup-ledger.sh
source test/e2e/support/cleanup-ledger.sh
# shellcheck source=../support/runner-failure.sh
source test/e2e/support/runner-failure.sh

cleanup_absent() { [[ ! -e $1 ]]; }
process_absent() { [[ -z $process_name ]] || ! pgrep -x "$process_name" >/dev/null; }
runtime_process_absent() {
  local escaped_bundle=${installed_bundle//./[.]}
  ! pgrep -f "^$escaped_bundle/Contents/Library/LaunchServices/muniment-runtime$" >/dev/null
}
runtime_job_stopped() {
  local target="gui/$(id -u)/ai.muniment.runtime" job_status
  job_status=$(/bin/launchctl print "$target" 2>/dev/null || true)
  ! grep -Eq 'state = running|pid = [1-9][0-9]*' <<<"$job_status"
}

# shellcheck source=../support/macos-runtime-probe.sh
source test/e2e/support/macos-runtime-probe.sh

stop_runtime() {
  local target="gui/$(id -u)/ai.muniment.runtime"
  /bin/launchctl kill SIGTERM "$target" 2>/dev/null || true
  for _ in {1..20}; do
    if runtime_process_absent && runtime_job_stopped; then return; fi
    sleep 0.25
  done
  local escaped_bundle=${installed_bundle//./[.]}
  pkill -f "^$escaped_bundle/Contents/Library/LaunchServices/muniment-runtime$" 2>/dev/null || true
  runtime_process_absent && runtime_job_stopped
}

payload_failure() {
  printf '%s\n' "$1" >>"$raw/payload.log"
  runner_failure "$1"
  return 1
}

verify_installed_payload() {
  local runtime="$installed_bundle/Contents/Library/LaunchServices/muniment-runtime"
  local agent="$installed_bundle/Contents/Library/LaunchAgents/ai.muniment.runtime.plist"
  local plist_buddy=${MUNIMENT_E2E_PLIST_BUDDY:-/usr/libexec/PlistBuddy}
  local otool=${MUNIMENT_E2E_OTOOL:-/usr/bin/otool}
  local runtime_version rpaths label bundle_program throttle unsuccessful_exit

  [[ -x $runtime ]] || payload_failure 'installed runtime is unavailable or not executable' || return
  rpaths=$("$otool" -l "$runtime" 2>>"$raw/payload.log" | awk '$1 == "cmd" { rpath = ($2 == "LC_RPATH"); next } rpath && $1 == "path" { print $2; rpath = 0 }') || payload_failure 'installed runtime load commands are unavailable' || return
  grep -Fxq '@executable_path/../../Resources/asr-runtime' <<<"$rpaths" || payload_failure 'installed runtime ASR rpath is unavailable' || return
  runtime_version=$("$runtime" --version 2>>"$raw/payload.log") || payload_failure 'installed runtime version probe failed' || return
  [[ -n ${runtime_version//[[:space:]]/} ]] || payload_failure 'installed runtime version is empty' || return
  [[ -f $agent ]] || payload_failure 'installed runtime LaunchAgent is unavailable' || return
  label=$("$plist_buddy" -c 'Print :Label' "$agent" 2>>"$raw/payload.log") || payload_failure 'installed runtime LaunchAgent label is unavailable' || return
  [[ $label == ai.muniment.runtime ]] || payload_failure 'installed runtime LaunchAgent label is invalid' || return
  bundle_program=$("$plist_buddy" -c 'Print :BundleProgram' "$agent" 2>>"$raw/payload.log") || payload_failure 'installed runtime LaunchAgent executable path is unavailable' || return
  [[ $bundle_program == Contents/Library/LaunchServices/muniment-runtime ]] || payload_failure 'installed runtime LaunchAgent executable path is invalid' || return
  throttle=$("$plist_buddy" -c 'Print :ThrottleInterval' "$agent" 2>>"$raw/payload.log") || payload_failure 'installed runtime LaunchAgent throttle is unavailable' || return
  [[ $throttle == 5 ]] || payload_failure 'installed runtime LaunchAgent throttle is invalid' || return
  unsuccessful_exit=$("$plist_buddy" -c 'Print :KeepAlive:SuccessfulExit' "$agent" 2>>"$raw/payload.log") || payload_failure 'installed runtime LaunchAgent unsuccessful-exit policy is unavailable' || return
  [[ $unsuccessful_exit == false ]] || payload_failure 'installed runtime LaunchAgent unsuccessful-exit policy is invalid' || return
  printf 'runtime_version=%s\npayload=verified\n' "$runtime_version" >"$raw/payload.log"
}

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
  local runner_status=$?
  (( finalized == 0 )) || return
  finalized=1
  trap - EXIT INT TERM
  # The runner finishes its stderr log before redaction reads it.
  exec 2>&3
  if (( runner_status != 0 )); then status=1; fi
  if (( status != 0 )); then first_failed_step=runner; fi
  if (( runtime_touched )); then
    cleanup_step collect-runtime-diagnostics collect_macos_runtime_diagnostics "gui/$(id -u)/ai.muniment.runtime" "$runtime_log" "$raw"
  fi
  cleanup_step stop-app stop_app
  if (( runtime_touched )); then cleanup_step stop-runtime stop_runtime; fi
  if (( installed )); then cleanup_step remove-bundle rm -rf -- "$installed_bundle"; fi
  if (( runtime_touched )); then cleanup_step remove-runtime-state rm -rf -- "$runtime_state"; fi
  cleanup_step remove-state rm -rf -- "$state_root"
  cleanup_step bundle-gone cleanup_absent "$installed_bundle"
  cleanup_step processes-gone process_absent
  if (( runtime_touched )); then cleanup_step runtime-process-gone runtime_process_absent; fi
  if (( runtime_touched )); then cleanup_step runtime-job-stopped runtime_job_stopped; fi
  if (( runtime_touched )); then cleanup_step runtime-state-gone cleanup_absent "$runtime_state"; fi
  cleanup_step state-gone cleanup_absent "$state_root"

  cleanup_step stage-cleanup-log cp "$cleanup_log" "$raw/cleanup.log"
  cleanup_step index-failure-artifacts index_failure_artifacts
  cleanup_step redact-artifacts node test/e2e/support/redact.mjs "$raw" "$safe" "$redaction_report"
  redaction_status=$cleanup_last_status
  cleanup_step remove-raw rm -rf -- "$raw"
  cleanup_step remove-archive rm -f -- "$archive"
  cleanup_step remove-expanded rm -rf -- "$expanded"
  cleanup_step remove-window-probe rm -f -- "$window_probe"
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
    mkdir -p "$artifacts" || record_cleanup_status prepare-minimal-artifacts failed
    printf 'envelope: minimal\nwithheld: guest artifacts\nreason: redaction-failed\n' >"$artifacts/envelope-reason.txt" || record_cleanup_status write-envelope-reason failed
    if [[ -s $redaction_report ]]; then
      cp -- "$redaction_report" "$artifacts/redaction-failure.txt" || record_cleanup_status write-redaction-report failed
    else
      printf 'file: unknown\ncategory: redactor-process\n' >"$artifacts/redaction-failure.txt" || record_cleanup_status write-redaction-report failed
    fi
  fi
  cleanup_step remove-safe rm -rf -- "$safe"
  cleanup_step raw-gone cleanup_absent "$raw"
  cleanup_step archive-gone cleanup_absent "$archive"
  cleanup_step expanded-gone cleanup_absent "$expanded"
  cleanup_step window-probe-gone cleanup_absent "$window_probe"
  cleanup_step safe-gone cleanup_absent "$safe"
  cleanup_step remove-redaction-report rm -f -- "$redaction_report"
  local log_to_remove=$cleanup_log
  # Later steps must not recreate the log inside the run root.
  cleanup_log=/dev/null
  cleanup_step remove-cleanup-log rm -f -- "$log_to_remove"
  cleanup_step remove-run-root rmdir "$run_root"

  mkdir -p "$artifacts" || record_cleanup_status prepare-exit-diagnostics failed
  if (( redaction_status != 0 || ${collection_status:-0} != 0 )); then
    printf 'withheld: guest stderr\n' >"$artifacts/runner-stderr.log" || record_cleanup_status write-stderr-placeholder failed
  fi
  # These files contain only runner-owned labels and statuses, never guest output.
  printf '%s' "$cleanup_status_entries" >"$artifacts/cleanup-status.log" || record_cleanup_status write-cleanup-status failed
  printf 'status=%s\ncleanup_status=%s\nredaction_status=%s\nfirst_failed_step=%s\n' \
    "$status" "$cleanup_status" "$redaction_status" "$first_failed_step" >"$artifacts/exit-reason.txt" || {
    record_cleanup_status write-exit-reason failed
    printf '%s' "$cleanup_status_entries" >"$artifacts/cleanup-status.log"
  }
  if (( status != 0 || cleanup_status != 0 || redaction_status != 0 )); then exit 1; fi
}

handle_signal() {
  status=1
  finalize
}

exec 3>&2
trap finalize EXIT
trap 'handle_signal' INT TERM
mkdir -p "$raw" "$safe" "$expanded" "$state_root" || { status=1; exit; }
: >"$cleanup_log" || { status=1; exit; }
exec 2>>"$raw/runner-stderr.log" || { status=1; exit; }

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

if [[ ${MUNIMENT_E2E_PAYLOAD_TEST_MODE:-0} == 1 ]]; then
  installed_bundle=${MUNIMENT_E2E_PAYLOAD_TEST_BUNDLE:?test bundle is required}
  installed=1
  verify_installed_payload || status=1
  exit
fi

sha=${MUNIMENT_E2E_SOURCE_SHA:-}
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { runner_failure 'invalid source SHA'; exit; }
[[ -n ${GH_TOKEN:-} && -n ${GITHUB_REPOSITORY:-} ]] || { runner_failure 'required injected environment is unavailable'; exit; }
gui_user=$(stat -f '%Su' /dev/console)
[[ -n $gui_user && $gui_user != root && $gui_user != loginwindow && $(id -un) == "$gui_user" ]] || {
  runner_failure 'runner is not executing as the active GUI test user'; exit;
}
release=$(run_setup gh api "repos/${GITHUB_REPOSITORY}/releases/tags/nightly") || { status=1; exit; }
asset_id=$(run_setup node test/e2e/support/asset-identity.mjs "$sha" macos <<<"$release") || { status=1; exit; }
run_setup gh api -H 'Accept: application/octet-stream' "repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" >"$archive" || { status=1; exit; }

run_setup ditto -x -k "$archive" "$expanded" >>"$raw/install.log" 2>&1 || { status=1; exit; }
bundles=()
while IFS= read -r -d '' bundle; do bundles+=("$bundle"); done < <(find "$expanded" -type d -name '*.app' -prune -print0)
(( ${#bundles[@]} == 1 )) || { runner_failure 'archive does not contain exactly one application bundle'; exit; }
source_bundle=${bundles[0]}
plist="$source_bundle/Contents/Info.plist"
[[ -f $plist ]] || { runner_failure 'application bundle metadata is unavailable'; exit; }
process_name=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist" 2>>"$raw/install.log")
[[ -n $process_name && $process_name != */* && -x "$source_bundle/Contents/MacOS/$process_name" ]] || { runner_failure 'application bundle does not identify one executable'; exit; }

installed=1
run_setup ditto "$source_bundle" "$installed_bundle" >>"$raw/install.log" 2>&1 || { status=1; exit; }
verify_installed_payload || { status=1; exit; }
mkdir -p "$state_root/tmp"

# The window probe reads CoreGraphics window metadata, which macOS grants with
# no privacy consent. The build runs before the launch, so a broken toolchain
# fails the job here and never reads as "the app opened no window yet".
run_setup clang -std=gnu17 -O2 -Wall -Wno-deprecated-declarations \
  -framework CoreFoundation -framework CoreGraphics \
  -o "$window_probe" test/e2e/support/macos-window-count.c >>"$raw/window-probe-build.log" 2>&1 || {
  echo 'window probe did not compile' >&2; status=1; exit;
}

TMPDIR="$state_root/tmp" "$installed_bundle/Contents/MacOS/$process_name" >"$raw/driver-app.log" 2>&1 &
app_pid=$!
runtime_touched=1

runtime_endpoint="$runtime_state/muniment/attach-v1.sock"
runtime_target="gui/$(id -u)/ai.muniment.runtime"
if probe_macos_runtime "$runtime_target" "$app_pid" "$raw/driver-app.log" "$runtime_endpoint" "$raw/runtime-connection.log"; then
  node test/e2e/support/probe-companion-pairing.mjs "$runtime_endpoint" >"$raw/companion-pairing.log" 2>&1 || status=1
  node test/e2e/support/probe-run-start.mjs "$installed_bundle/Contents/MacOS/$process_name" \
    "$runtime_endpoint" "$runtime_state" "$runtime_config" "$raw/driver-app.log" >"$raw/run-start.log" 2>&1 || status=1
else
  status=1
fi

window_ready=0
window_wait_seconds=120
window_deadline=$((SECONDS + window_wait_seconds))
while (( SECONDS < window_deadline )); do
  kill -0 "$app_pid" 2>/dev/null || { echo 'application exited before opening a window' >&2; status=1; break; }
  window_count=$("$window_probe" "$app_pid" 2>>"$raw/window.log" || true)
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
