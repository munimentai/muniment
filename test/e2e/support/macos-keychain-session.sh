#!/usr/bin/env bash
# Give each runner a disposable Keychain without changing access to existing items.
set -euo pipefail
set +x
umask 077
[[ $# -gt 0 ]] || exit 2
session_uid=$(id -u)
# Create the Keychain in the console audit session before publishing it.
# An SSH-only unlock leaves Spotlight's console session locked.
launchctl print "gui/$session_uid" >/dev/null
console_security() {
  # asuser adopts the audit session, not the UID. Drop root before touching the Keychain.
  sudo -n launchctl asuser "$session_uid" sudo -n -H -u "#$session_uid" security "$@"
}
prior_default=$(security default-keychain -d user | sed 's/^[[:space:]]*"//;s/"[[:space:]]*$//')
prior_list=$(security list-keychains -d user | sed 's/^[[:space:]]*"//;s/"[[:space:]]*$//')
[[ -n $prior_default && -n $prior_list ]] || exit 1
previous=()
while IFS= read -r entry; do previous+=("$entry"); done <<<"$prior_list"
work=$(mktemp -d "${TMPDIR:-/tmp}/muniment-keychain.XXXXXX")
keychain="$work/e2e.keychain-db"
created=0
cleanup() {
  result=$?
  trap - EXIT INT TERM
  if (( created )); then
    security default-keychain -d user -s "$prior_default" || result=1
    security list-keychains -d user -s "${previous[@]}" || result=1
    security delete-keychain "$keychain" || result=1
  fi
  rm -rf -- "$work" || result=1
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
password=$(openssl rand -hex 32)
console_security create-keychain -p "$password" "$keychain"
created=1
# Keep both sessions unlocked until cleanup, including across a VM sleep.
console_security set-keychain-settings "$keychain"
console_security unlock-keychain -p "$password" "$keychain"
security unlock-keychain -p "$password" "$keychain"
unset password
security list-keychains -d user -s "$keychain"
security default-keychain -d user -s "$keychain"
"$@"
