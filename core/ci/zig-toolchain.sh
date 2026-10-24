#!/usr/bin/env bash
# Install zig and cargo-zigbuild, which cross-compile the Linux server
# archives for x86_64 and aarch64 from one runner, and the Rust targets they
# build. Put zig on PATH for the later steps of the job.
#
# The runner container keeps /root between jobs, so each tool installs once.
# The zig archive is pinned by version and checked against the SHA-256 that
# ziglang.org publishes in its download index.
set -euo pipefail

zig_version=0.14.1
zigbuild_version=0.23.4
case "$(uname -m)" in
  x86_64) zig_arch=x86_64 zig_sha256=24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c ;;
  aarch64) zig_arch=aarch64 zig_sha256=f7a654acc967864f7a050ddacfaa778c7504a0eca8d2b678839c21eea47c992b ;;
  *) echo "zig-toolchain.sh: no pinned zig for $(uname -m)" >&2; exit 1 ;;
esac

zig_home="$HOME/.cache/zig-$zig_version-$zig_arch"
if [ ! -x "$zig_home/zig" ]; then
  archive=$(mktemp "${RUNNER_TEMP:-/tmp}/zig.XXXXXX")
  trap 'rm -f "$archive"' EXIT
  curl --proto '=https' --tlsv1.2 -sSfL -o "$archive" \
    "https://ziglang.org/download/$zig_version/zig-$zig_arch-linux-$zig_version.tar.xz"
  printf '%s  %s\n' "$zig_sha256" "$archive" | sha256sum --check --quiet
  mkdir -p "$zig_home"
  tar -xJf "$archive" -C "$zig_home" --strip-components=1
  # A script that sources this one may set its own EXIT trap, so the archive
  # goes now rather than at exit.
  rm -f "$archive"
  trap - EXIT
fi
export PATH="$zig_home:$HOME/.cargo/bin:$PATH"

if [ "$(cargo zigbuild --version 2>/dev/null | awk '{print $2}')" != "$zigbuild_version" ]; then
  cargo install --locked cargo-zigbuild --version "$zigbuild_version"
fi
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$zig_home" >> "$GITHUB_PATH"
fi
