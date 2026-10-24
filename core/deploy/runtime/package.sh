#!/usr/bin/env bash
# Build muniment-server for Linux on <arch> and pack it as
# <out>/muniment-server-<version>-linux-<arch>.tar.gz: the binary and the
# speech runtime libraries its rpath loads from its own directory. The
# archive holds only regular files in its root, which is the layout the
# launcher unpacks.
#
# server-libraries.sh fetches the pinned sherpa-onnx libraries for the target
# and points this one Cargo invocation at them, above the desktop's forced
# SHERPA_ONNX_LIB_DIR in the repository's .cargo/config.toml. The packed
# libraries are the binary's shared-library closure, each checked to be built
# for the target. MUNIMENT_SERVER_LIBRARY_CACHE names the library cache, by
# default server-libraries under the Cargo target directory.
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
libraries="$context/deploy/runtime/server-libraries.sh"

# Each target fetches its own pinned speech runtime libraries into its own
# directory, so builds for both targets can share one workspace in turn.
cache=${MUNIMENT_SERVER_LIBRARY_CACHE:-$target_dir/server-libraries}
lib_dir=$("$libraries" fetch "$target" "$cache")
lib_dir=$(cd "$lib_dir" && pwd)
library_args_text=$("$libraries" cargo-args "$lib_dir")
mapfile -t library_args <<<"$library_args_text"
echo "package.sh: linking $target against the speech runtime libraries in $lib_dir"

(
  cd "$context"
  CARGO_PROFILE_RELEASE_STRIP=symbols \
    cargo zigbuild "${library_args[@]}" --release --locked -p muniment-server --bin muniment-server \
    --target "$target.2.36"
)

built="$target_dir/$target/release"
# The packed libraries are the ones the binary needs, read from the target's
# own library directory, never whatever else sits in the shared output.
closure=$("$libraries" closure "$target" "$lib_dir" "$built/muniment-server")
mapfile -t files <<<"$closure"
if [ ${#files[@]} -lt 2 ]; then
  echo "package.sh: muniment-server for $target needs no speech runtime library" >&2
  exit 1
fi
mkdir -p "$out"
archive="$out/muniment-server-$version-linux-$arch.tar.gz"
python3 -B "$context/deploy/runtime/pack.py" "$archive" "${files[@]}"
"$libraries" check-archive "$target" "$archive"
echo "Packed $archive."
