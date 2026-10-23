#!/usr/bin/env bash
# Build muniment-server for Linux on <arch> and pack it as
# <out>/muniment-server-<version>-linux-<arch>.tar.gz: the binary and the
# speech runtime libraries its rpath loads from its own directory. The
# archive holds only regular files in its root, which is the layout the
# launcher unpacks.
#
# cargo-zigbuild links against glibc 2.36, so the archive runs on Debian 12
# and later, the runtime image among them. ci/zig-toolchain.sh installs
# zig, cargo-zigbuild and both Rust targets.
#
# Usage: deploy/runtime/package.sh <X.Y.Z> <x86_64|aarch64> [out]
set -euo pipefail

version=$1
arch=$2
out=${3:-dist}
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "package.sh: $version is not an X.Y.Z version" >&2
  exit 2
fi
case "$arch" in
  x86_64 | aarch64) ;;
  *) echo "package.sh: $arch is not x86_64 or aarch64" >&2; exit 2 ;;
esac
context=$(cd "$(dirname "$0")/../.." && pwd)
# The binary announces the workspace version, so the archive must name it.
source_version=$(sed -n '/^\[workspace\.package\]$/,/^\[/s/^version = "\(.*\)"$/\1/p' "$context/Cargo.toml")
if [ "$version" != "$source_version" ]; then
  echo "package.sh: Cargo.toml names version $source_version, not $version" >&2
  exit 2
fi
target="$arch-unknown-linux-gnu"
target_dir=${CARGO_TARGET_DIR:-$context/target}

(
  cd "$context"
  CARGO_PROFILE_RELEASE_STRIP=symbols \
    cargo zigbuild --release --locked -p muniment-server --bin muniment-server \
    --target "$target.2.36"
)

built="$target_dir/$target/release"
files=("$built/muniment-server")
for library in "$built"/*.so*; do
  [ -e "$library" ] && files+=("$library")
done
if [ ${#files[@]} -lt 2 ]; then
  echo "package.sh: no speech runtime library sits beside the binary in $built" >&2
  exit 1
fi
mkdir -p "$out"
archive="$out/muniment-server-$version-linux-$arch.tar.gz"
python3 -B "$context/deploy/runtime/pack.py" "$archive" "${files[@]}"
echo "Packed $archive."
