#!/usr/bin/env bash
# Build the unsigned muniment-server archive for Linux aarch64 from this
# runner, through the same pinned zig toolchain, target library selection,
# release link and archive checks that deploy/runtime/release-assets.sh uses,
# and print the ELF machine of each packed file. A failed link or a file
# built for another machine fails the script.
#
# Nothing is signed or published. The archive lands in [out], or in a
# temporary directory that the script removes when it ends.
#
# Usage: ci/server-cross-build.sh [out]
set -euo pipefail

context=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-}
unpacked=$(mktemp -d)
cleanup=("$unpacked")
if [ -z "$out" ]; then
  out=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/server-cross-build.XXXXXX")
  cleanup+=("$out")
fi
version=$(sed -n '/^\[workspace\.package\]$/,/^\[/s/^version = "\(.*\)"$/\1/p' "$context/Cargo.toml")

# Sourced, so zig and cargo-zigbuild are on PATH for package.sh.
# shellcheck source=zig-toolchain.sh
source "$context/ci/zig-toolchain.sh"
# zig-toolchain.sh sets its own EXIT trap, so this one goes after it.
trap 'rm -rf "${cleanup[@]}"' EXIT

"$context/deploy/runtime/package.sh" "$version" aarch64 "$out"
archive="$out/muniment-server-$version-linux-aarch64.tar.gz"
tar -xzf "$archive" -C "$unpacked"
for file in "$unpacked"/*; do
  machine=$(od -An -t u2 -j 18 -N 2 "$file" | tr -d ' ')
  echo "server-cross-build.sh: $(basename "$file") has ELF machine $machine"
  if [ "$machine" != 183 ]; then
    echo "server-cross-build.sh: $(basename "$file") is not an aarch64 ELF file" >&2
    exit 1
  fi
done
echo "server-cross-build.sh: built and checked $archive"
