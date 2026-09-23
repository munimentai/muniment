#!/usr/bin/env bash
set -uo pipefail
set +x
umask 077

: "${MUNIMENT_E2E_REAL_APP_BINARY:?The app binary is required.}"
: "${MUNIMENT_E2E_DRIVER_APP_LOG:?The app log is required.}"

: "${MUNIMENT_E2E_LOGIN_HOME:?The login home is required for Keychain access.}"
# Security.framework reads its keychain preferences from the login home.
# MUNIMENT_STATE_DIR still selects the isolated spec state through its symlink.
export HOME="$MUNIMENT_E2E_LOGIN_HOME"

# Capture startup output before WDIO attaches its log handlers.
# exec keeps the app PID and exit signal under WDIO control.
export RUST_BACKTRACE=1
exec "$MUNIMENT_E2E_REAL_APP_BINARY" "$@" >>"$MUNIMENT_E2E_DRIVER_APP_LOG" 2>&1
