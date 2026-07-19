cleanup_step() {
  local label=$1; shift
  if [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE:-0} == 1 ]]; then
    { printf '%s\t' "$label"; printf '%q ' "$@"; printf '\n'; } >>"$MUNIMENT_E2E_FINALIZER_TEST_LEDGER"
    if [[ $label == "${MUNIMENT_E2E_FINALIZER_TEST_FAIL:-}" ]]; then
      cleanup_last_status=1
      printf '%s: failed\n' "$label" >>"$cleanup_log"
      cleanup_status=1
    else
      cleanup_last_status=0
      printf '%s: ok\n' "$label" >>"$cleanup_log"
    fi
  elif "$@" >>"$cleanup_log" 2>&1; then
    cleanup_last_status=0
    printf '%s: ok\n' "$label" >>"$cleanup_log"
  else
    cleanup_last_status=1
    printf '%s: failed\n' "$label" >>"$cleanup_log"
    cleanup_status=1
  fi
}
