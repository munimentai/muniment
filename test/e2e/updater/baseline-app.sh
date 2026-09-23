#!/usr/bin/env bash
set -euo pipefail
umask 077
: "${MUNIMENT_E2E_REAL_APP_BINARY:?}"
: "${MUNIMENT_E2E_DRIVER_APP_LOG:?}"
: "${MUNIMENT_UPDATE_PROOF_PID_FILE:?}"
printf '%s\n' "$$" > "$MUNIMENT_UPDATE_PROOF_PID_FILE"
exec "$MUNIMENT_E2E_REAL_APP_BINARY" "$@" --probe-runtime-notice >> "$MUNIMENT_E2E_DRIVER_APP_LOG" 2>&1
