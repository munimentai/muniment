# The cleanup log interleaves each step's own stdout/stderr, so it is only
# publishable after redaction. This ledger records the same outcomes as a fixed
# label plus a fixed word and nothing else, which keeps it safe to publish on
# the path where redaction itself failed. Opt in by setting the variable.
record_cleanup_status() {
  if [[ $2 == failed ]]; then
    cleanup_status=1
    if [[ ${first_failed_step:-none} == none ]]; then first_failed_step=$1; fi
  fi
  # An in-memory ledger survives removal of the run root.
  if [[ ${cleanup_status_entries+x} ]]; then
    cleanup_status_entries+="$1: $2"$'\n'
  fi
  [[ -n ${cleanup_status_ledger:-} ]] || return 0
  printf '%s: %s\n' "$1" "$2" >>"$cleanup_status_ledger"
}

cleanup_step() {
  local label=$1; shift
  if [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE:-0} == 1 ]]; then
    { printf '%s\t' "$label"; printf '%q ' "$@"; printf '\n'; } >>"$MUNIMENT_E2E_FINALIZER_TEST_LEDGER"
    local failed=${MUNIMENT_E2E_FINALIZER_TEST_FAIL:-}
    local removal_failure=0
    case "$label:$failed" in
      package-gone:remove-package|state-gone:remove-state|raw-gone:remove-raw|package-file-gone:remove-package-file|auth-url-gone:remove-auth-url|safe-gone:remove-safe)
        removal_failure=1
        ;;
    esac
    cleanup_last_status=0
    if [[ $label == "$failed" || $removal_failure == 1 ]]; then
      cleanup_last_status=1
    elif [[ ${MUNIMENT_E2E_FINALIZER_TEST_EXECUTE:-0} == 1 ]]; then
      "$@" >>"$cleanup_log" 2>&1 || cleanup_last_status=1
    fi
    printf '%s\t%s\n' "$label" "$cleanup_last_status" >>"$MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER"
  elif "$@" >>"$cleanup_log" 2>&1; then
    cleanup_last_status=0
  else
    cleanup_last_status=1
  fi
  if (( cleanup_last_status == 0 )); then
    printf '%s: ok\n' "$label" >>"$cleanup_log"
    record_cleanup_status "$label" ok
  else
    printf '%s: failed\n' "$label" >>"$cleanup_log"
    record_cleanup_status "$label" failed
  fi
}
