#!/usr/bin/env bash
# Build and sign the server files every core-vX.Y.Z release carries: the
# muniment-server archives for Linux x86_64 and aarch64, SHA256SUMS over them
# and SHA256SUMS.minisig. The core.yml release job and the pins update both
# call it, so a release from either path installs through the launcher.
#
# It installs the Rust and zig toolchains and minisign when they are missing,
# empties <dir>, then runs package.sh for each architecture and
# sign-release.sh, which needs MUNIMENT_RELEASE_SIGNING_KEY.
#
# Usage: deploy/runtime/release-assets.sh <X.Y.Z> <dir>
set -euo pipefail

version=$1
directory=$2
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
"$context/deploy/runtime/sign-release.sh" "$version" "$directory"
