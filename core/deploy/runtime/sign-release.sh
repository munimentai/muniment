#!/usr/bin/env bash
# Write <dir>/SHA256SUMS over the server archives in <dir> and sign it as
# <dir>/SHA256SUMS.minisig with the release signing key, then check the
# signature against the public key in crates/server/src/release.rs, so a
# release never carries a signature its launchers reject.
#
# [release-sums] names the SHA256SUMS the release already carries for its
# desktop files. Its lines stay in <dir>/SHA256SUMS beside the archive lines,
# so one signed file lists every file of the release.
#
# MUNIMENT_RELEASE_SIGNING_KEY holds the minisign secret key file's contents,
# and MUNIMENT_RELEASE_SIGNING_PASSWORD its password when it has one. Without
# the key the script fails, so nothing is published unsigned.
#
# Usage: deploy/runtime/sign-release.sh <X.Y.Z> <dir> [release-sums]
set -euo pipefail

version=$1
directory=$2
release_sums=${3:+$(cd "$(dirname "$3")" && pwd)/$(basename "$3")}
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
  if [ -n "$release_sums" ]; then
    if grep -Evq '^[0-9a-f]{64}  [^/ ]+$' "$release_sums"; then
      echo "sign-release.sh: $release_sums holds a line that is not '<sha256>  <name>'" >&2
      exit 1
    fi
    # The release's own lines for the archive names give way to the new ones.
    { awk 'NR == FNR { server[$2]; next } !($2 in server)' SHA256SUMS "$release_sums"; cat SHA256SUMS; } |
      LC_ALL=C sort -k2,2 > SHA256SUMS.merged
    mv SHA256SUMS.merged SHA256SUMS
  fi
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
