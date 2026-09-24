#!/usr/bin/env bash
# Install the Rust toolchain on a runner that has none.
#
# rustup-init is pinned by version and checked against the SHA-256 that
# static.rust-lang.org publishes beside it, instead of piping sh.rustup.rs into
# a shell. rustup then checks each toolchain component against its channel
# manifest.
set -euo pipefail

rustup_version=1.29.1
if command -v cargo >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/cargo" ]; then
  exit 0
fi
case "$(uname -m)" in
  x86_64)
    target=x86_64-unknown-linux-gnu
    sha256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
    ;;
  aarch64 | arm64)
    target=aarch64-unknown-linux-gnu
    sha256=15f6e4ce9f583b929c996c91562bad6d4454f3281de858b02cdfdef615fac433
    ;;
  *)
    printf 'bootstrap-rustup: no rustup-init for %s\n' "$(uname -m)" >&2
    exit 1
    ;;
esac
installer=$(mktemp "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rustup-init.XXXXXX")
trap 'rm -f "$installer"' EXIT
curl --proto '=https' --tlsv1.2 -sSfL -o "$installer" \
  "https://static.rust-lang.org/rustup/archive/${rustup_version}/${target}/rustup-init"
printf '%s  %s\n' "$sha256" "$installer" | sha256sum --check --quiet
chmod +x "$installer"
"$installer" -y --profile minimal --default-toolchain stable
