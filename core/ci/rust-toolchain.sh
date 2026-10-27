#!/usr/bin/env bash
# Install the Rust toolchain that rust-toolchain.toml at the repo root pins,
# with the named components, on a runner, and put cargo on PATH for the later
# steps of the job.
# Usage: ci/rust-toolchain.sh [component...]
#
# The runner container keeps /root between jobs, so rustup installs once.
# rustup-init is pinned by version and checked against the SHA-256 that
# static.rust-lang.org publishes beside it.
set -euo pipefail

rustup_version=1.29.1
rustup_sha256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
rustup="$HOME/.cargo/bin/rustup"

if [ ! -x "$rustup" ]; then
  installer=$(mktemp "${RUNNER_TEMP:-/tmp}/rustup-init.XXXXXX")
  trap 'rm -f "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 -sSfL -o "$installer" \
    "https://static.rust-lang.org/rustup/archive/${rustup_version}/x86_64-unknown-linux-gnu/rustup-init"
  printf '%s  %s\n' "$rustup_sha256" "$installer" | sha256sum --check --quiet
  chmod +x "$installer"
  "$installer" -y --profile minimal --default-toolchain none --no-modify-path
fi

components=()
for component in "$@"; do
  components+=(--component "$component")
done
toolchain=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$(dirname "$0")/../../rust-toolchain.toml")
"$rustup" toolchain install "$toolchain" --profile minimal "${components[@]}"
"$rustup" default "$toolchain"
echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
