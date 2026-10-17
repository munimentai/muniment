#!/usr/bin/env bash
# Write <dir>/SHA256SUMS over the server archives in <dir> and sign it as
# <dir>/SHA256SUMS.minisig with the release signing key, then check the
# signature against the public key in crates/server/src/release.rs, so a
# release never carries a signature its launchers reject.
#
# MUNIMENT_RELEASE_SIGNING_KEY holds the minisign secret key file's contents,
# and MUNIMENT_RELEASE_SIGNING_PASSWORD its password when it has one. Without
# the key the script fails, so nothing is published unsigned.
#
# Usage: deploy/runtime/sign-release.sh <X.Y.Z> <dir>
set -euo pipefail

version=$1
directory=$2
context=$(cd "$(dirname "$0")/../.." && pwd)
if [ -z "${MUNIMENT_RELEASE_SIGNING_KEY:-}" ]; then
  echo "::error::MUNIMENT_RELEASE_SIGNING_KEY is not set, so the release would be unsigned. Nothing is published." >&2
  exit 1
fi
public_key=$(sed -n 's/^pub const RELEASE_PUBLIC_KEY: &str = "\(.*\)";$/\1/p' \
  "$context/crates/server/src/release.rs")
if [ -z "$public_key" ]; then
  echo "::error::RELEASE_PUBLIC_KEY in crates/server/src/release.rs is empty, so no launcher could verify the release. Nothing is published." >&2
  exit 1
fi
if ! command -v minisign >/dev/null; then
  echo "sign-release.sh: minisign is not installed" >&2
  exit 1
fi

(
  cd "$directory"
  shopt -s nullglob
  archives=(muniment-server-"$version"-linux-*.tar.gz)
  if [ ${#archives[@]} -eq 0 ]; then
    echo "sign-release.sh: no muniment-server-$version archive in $directory" >&2
    exit 1
  fi
  sha256sum "${archives[@]}" > SHA256SUMS
)

key_file=$(mktemp)
trap 'rm -f "$key_file"' EXIT
chmod 600 "$key_file"
printf '%s\n' "$MUNIMENT_RELEASE_SIGNING_KEY" > "$key_file"
printf '%s\n' "${MUNIMENT_RELEASE_SIGNING_PASSWORD:-}" |
  minisign -S -s "$key_file" -m "$directory/SHA256SUMS" -x "$directory/SHA256SUMS.minisig" \
    -t "muniment-server $version SHA256SUMS" >/dev/null
minisign -V -q -P "$public_key" -m "$directory/SHA256SUMS" -x "$directory/SHA256SUMS.minisig"
echo "Signed $directory/SHA256SUMS for $version."
