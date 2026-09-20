#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test "$(uname -s)" = Darwin
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
clang -Werror -Wno-deprecated-declarations -framework Security -framework CoreFoundation \
  test/keychain_macos.c -o "$work/keychain-test"
"$work/keychain-test"
if [ "${1:-}" = unavailable ]; then
  "$work/keychain-test" unavailable "$work/audit.log"
fi
