cleanup_step() {
  local label=$1; shift
  if "$@" >>"$cleanup_log" 2>&1; then
    cleanup_last_status=0
    printf '%s: ok\n' "$label" >>"$cleanup_log"
  else
    cleanup_last_status=1
    printf '%s: failed\n' "$label" >>"$cleanup_log"
    cleanup_status=1
  fi
}
