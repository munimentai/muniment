#!/usr/bin/env bash
# Point every muniment-core git dependency at one release tag and update the
# lockfile entries of the crates that come from it.
# Usage: scripts/update-muniment-core.sh vX.Y.Z
set -euo pipefail
cd "$(dirname "$0")/.."
tag=${1:?usage: scripts/update-muniment-core.sh vX.Y.Z}
if [[ ! $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'invalid muniment-core tag: %s\n' "$tag" >&2
  exit 1
fi
repo=https://github.com/munimentai/muniment-core
manifests=$(find src-tauri -name Cargo.toml -not -path '*/target/*' -print0 \
  | xargs -0 grep -lF "git = \"$repo\"" | sort || true)
if [ -z "$manifests" ]; then
  printf 'no Cargo.toml depends on %s\n' "$repo" >&2
  exit 1
fi
# The lockfile names every crate Cargo resolves from the muniment-core repository.
# A full package id stays unique while two tags sit in the lockfile.
packages=$(awk -v source="source = \"git+$repo?" '
  /^name = / { name = $3; gsub(/"/, "", name) }
  /^version = / { version = $3; gsub(/"/, "", version) }
  index($0, source) == 1 {
    id = $3; gsub(/"/, "", id); sub(/#.*/, "", id)
    print id "#" name "@" version
  }
' src-tauri/Cargo.lock | sort -u)
if [ -z "$packages" ]; then
  printf 'src-tauri/Cargo.lock resolves no crate from %s\n' "$repo" >&2
  exit 1
fi
for manifest in $manifests; do
  sed -i.bak -E "s#(git = \"$repo\", tag = \")v[0-9]+\\.[0-9]+\\.[0-9]+\"#\\1$tag\"#g" "$manifest"
  rm -f "$manifest.bak"
done
arguments=()
for package in $packages; do
  arguments+=(--package "$package")
done
cargo update --manifest-path src-tauri/Cargo.toml "${arguments[@]}"
# The notices record every locked crate, including those a new core release adds.
python3 -B scripts/update-rust-notices.py
printf 'muniment-core %s: %s\n' "$tag" "${manifests//$'\n'/ }"
