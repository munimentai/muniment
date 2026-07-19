#!/usr/bin/env bash
set -eu
set +x
umask 077
case ${1:-} in
  https://*) printf '%s' "$1" >"$MUNIMENT_E2E_AUTH_URL_FILE" ;;
  *) exit 64 ;;
esac
