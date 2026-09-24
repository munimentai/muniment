#!/usr/bin/env bash
# Build the Go reader sidecar to the path the caller names.
#
# The desktop-ci clones carry no Go toolchain, and the tauri bundle declares the
# reader as a resource, so every lane that compiles the desktop package needs
# this binary on disk first. Go already on the clone is used as it stands, and
# the pinned toolchain is fetched only when none is there.
set -euo pipefail

output=${1:?usage: build-reader.sh <output-path>}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
go_version=go1.24.13

export PATH="$PATH:/usr/local/go/bin:$HOME/go/bin"

if ! command -v go >/dev/null 2>&1; then
  case "$(uname -s)" in
    Linux) host_os=linux ;;
    Darwin) host_os=darwin ;;
    *) printf 'build-reader: no Go toolchain for %s\n' "$(uname -s)" >&2; exit 1 ;;
  esac
  case "$(uname -m)" in
    x86_64) host_arch=amd64 ;;
    aarch64 | arm64) host_arch=arm64 ;;
    *) printf 'build-reader: no Go toolchain for %s\n' "$(uname -m)" >&2; exit 1 ;;
  esac
  # The macOS clone has no passwordless sudo, so the toolchain lands in the
  # caller's own directory on every platform.
  # The SHA-256 values are the ones go.dev/dl publishes for each archive.
  case "$host_os-$host_arch" in
    linux-amd64) go_sha256=1fc94b57134d51669c72173ad5d49fd62afb0f1db9bf3f798fd98ee423f8d730 ;;
    linux-arm64) go_sha256=74d97be1cc3a474129590c67ebf748a96e72d9f3a2b6fef3ed3275de591d49b3 ;;
    darwin-amd64) go_sha256=6cc6549b06725220b342b740497ffd24e0ebdcef75781a77931ca199f46ad781 ;;
    darwin-arm64) go_sha256=f282d882c3353485e2fc6c634606d85caf36e855167d59b996dbeae19fa7629a ;;
  esac
  root=${XDG_CACHE_HOME:-$HOME/.cache}/muniment-go/$go_version
  if [ ! -x "$root/go/bin/go" ]; then
    tarball=$(mktemp -t go-toolchain-XXXXXX.tar.gz)
    trap 'rm -f "$tarball"' EXIT
    curl --proto '=https' --tlsv1.2 -fsSL \
      "https://go.dev/dl/${go_version}.${host_os}-${host_arch}.tar.gz" -o "$tarball"
    # GNU sha256sum answers --version. A BSD sha256sum on macOS does not, so
    # macOS uses shasum.
    if sha256sum --version >/dev/null 2>&1; then
      actual=$(sha256sum "$tarball" | awk '{print $1}')
    else
      actual=$(shasum -a 256 "$tarball" | awk '{print $1}')
    fi
    if [ "$actual" != "$go_sha256" ]; then
      printf 'build-reader: %s has SHA-256 %s, expected %s\n' "$go_version" "$actual" "$go_sha256" >&2
      exit 1
    fi
    rm -rf "$root"
    mkdir -p "$root"
    tar -C "$root" -xzf "$tarball"
  fi
  export PATH="$root/go/bin:$PATH"
fi

go version
case "$output" in
  /*) target=$output ;;
  *) target=$repo_root/$output ;;
esac
CGO_ENABLED=0 go build -C "$repo_root/src-tauri/reader" \
  -trimpath -ldflags "-s -w" -o "$target" .
