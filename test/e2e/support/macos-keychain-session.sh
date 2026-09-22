#!/usr/bin/env bash
# The disposable CI login keychain stays locked after an SSH login. Give each
# runner its own unlocked keychain, without changing access to existing items.
set -euo pipefail
set +x
umask 077
[[ $# -gt 0 ]] || exit 2
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
security create-keychain -p "$password" "$keychain"
created=1
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
unset password
security list-keychains -d user -s "$keychain"
security default-keychain -d user -s "$keychain"
"$@"
