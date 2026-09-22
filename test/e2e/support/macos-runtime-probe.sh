#!/usr/bin/env bash

collect_macos_runtime_diagnostics() {
  local target=$1 runtime_log=$2 output=$3
  local launchctl_command=${MUNIMENT_E2E_LAUNCHCTL:-/bin/launchctl}
  local launchctl_status=0
  "$launchctl_command" print "$target" >"$output/runtime-launchctl.log" 2>&1 || launchctl_status=$?
  printf '\nlaunchctl_exit_status=%s\n' "$launchctl_status" >>"$output/runtime-launchctl.log" || return 1
  if [[ -f $runtime_log ]]; then
    tail -c 262144 "$runtime_log" >"$output/runtime.log" || return 1
  else
    printf 'No runtime log exists for this user.\n' >"$output/runtime.log" || return 1
  fi
  local runtime_service_log="${runtime_log%/*}/runtime-service.log"
  if [[ -f $runtime_service_log ]]; then
    tail -c 262144 "$runtime_service_log" >"$output/runtime-service.log" || return 1
  else
    printf 'No runtime service log exists for this user.\n' >"$output/runtime-service.log" || return 1
  fi
}

probe_macos_runtime() {
  local target=$1 app_pid=$2 app_log=$3 endpoint=$4 diagnostic=$5
  local runtime_executable=${6:-} child_pid=
  local launchctl_command=${MUNIMENT_E2E_LAUNCHCTL:-/bin/launchctl}
  local wait_seconds=${MUNIMENT_E2E_RUNTIME_WAIT_SECONDS:-60}
  local deadline=$((SECONDS + wait_seconds)) job_active=false endpoint_present=false client_connected=false job_status connection_status

  while (( SECONDS < deadline )); do
    job_active=false
    job_status=$("$launchctl_command" print "$target" 2>/dev/null || true)
    if grep -Eq 'state = running' <<<"$job_status" && grep -Eq 'pid = [1-9][0-9]*' <<<"$job_status"; then
      job_active=true
    fi
    if [[ -S $endpoint ]]; then endpoint_present=true; else endpoint_present=false; fi
    connection_status=$(grep -F 'desktop runtime client connected=' "$app_log" 2>/dev/null | tail -n 1 || true)
    if [[ $connection_status == 'desktop runtime client connected=true' ]]; then
      client_connected=true
    else
      client_connected=false
    fi
    child_pid=
    if [[ -n $runtime_executable ]]; then
      child_pid=$(pgrep -P "$app_pid" -f "^${runtime_executable//./[.]}$" || true)
    fi
    if [[ ( $job_active == true || $child_pid =~ ^[1-9][0-9]*$ ) && $endpoint_present == true && $client_connected == true ]]; then
      local runtime_mode=launchd
      [[ $job_active == true ]] || runtime_mode=child
      printf 'runtime_mode=%s\njob_active=%s\nendpoint_present=true\nclient_connected=true\n' "$runtime_mode" "$job_active" >"$diagnostic"
      return 0
    fi
    kill -0 "$app_pid" 2>/dev/null || break
    sleep 1
  done

  {
    printf 'wait_seconds=%s\n' "$wait_seconds"
    printf 'job_active=%s\n' "$job_active"
    printf 'endpoint_present=%s\n' "$endpoint_present"
    printf 'client_connected=%s\n' "$client_connected"
    printf 'application_alive='
    if kill -0 "$app_pid" 2>/dev/null; then printf 'true\n'; else printf 'false\n'; fi
  } >"$diagnostic"
  printf 'installed runtime connection was unavailable within %s seconds\n' "$wait_seconds" >&2
  return 1
}
