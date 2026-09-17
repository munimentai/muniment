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
  case "$(uname -m)" in
    x86_64) arch=amd64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) printf 'build-reader: no Go toolchain for %s\n' "$(uname -m)" >&2; exit 1 ;;
  esac
  tarball=$(mktemp -t go-toolchain-XXXXXX.tar.gz)
  trap 'rm -f "$tarball"' EXIT
  curl --proto '=https' --tlsv1.2 -fsSL \
    "https://go.dev/dl/${go_version}.linux-${arch}.tar.gz" -o "$tarball"
  sudo rm -rf /usr/local/go
  sudo tar -C /usr/local -xzf "$tarball"
fi

go version
case "$output" in
  /*) target=$output ;;
  *) target=$repo_root/$output ;;
esac
CGO_ENABLED=0 go build -C "$repo_root/src-tauri/reader" \
  -trimpath -ldflags "-s -w" -o "$target" .
