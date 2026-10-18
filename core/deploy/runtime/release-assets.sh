#!/usr/bin/env bash
# Build and sign the server files every vX.Y.Z release carries: the
# muniment-server archives for Linux x86_64 and aarch64, SHA256SUMS over them
# and the release's other files, and SHA256SUMS.minisig. The release workflow
# calls it after it publishes the desktop files of the same version.
#
# It installs the Rust and zig toolchains and minisign when they are missing,
# empties <dir>, then runs package.sh for each architecture and
# sign-release.sh, which needs MUNIMENT_RELEASE_SIGNING_KEY and merges
# [release-sums], the release's SHA256SUMS, into the signed list.
#
# Usage: deploy/runtime/release-assets.sh <X.Y.Z> <dir> [release-sums]
set -euo pipefail

version=$1
directory=$2
release_sums=${3:-}
context=$(cd "$(dirname "$0")/../.." && pwd)

bash "$context/ci/rust-toolchain.sh"
# Sourced, so zig and cargo are on PATH for package.sh.
# shellcheck source=../../ci/zig-toolchain.sh
source "$context/ci/zig-toolchain.sh"
command -v minisign >/dev/null || { apt-get update && apt-get install -y --no-install-recommends minisign; }

rm -rf "$directory"
for arch in x86_64 aarch64; do
  "$context/deploy/runtime/package.sh" "$version" "$arch" "$directory"
done
"$context/deploy/runtime/sign-release.sh" "$version" "$directory" ${release_sums:+"$release_sums"}
