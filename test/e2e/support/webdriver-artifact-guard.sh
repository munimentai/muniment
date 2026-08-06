#!/usr/bin/env bash
set -euo pipefail

[[ $# == 2 ]] || { echo 'usage: webdriver-artifact-guard.sh <absent|present> <artifact>' >&2; exit 2; }
expectation=$1
artifact=$2
[[ -e $artifact ]] || { echo "artifact does not exist: $artifact" >&2; exit 2; }
artifact=$(realpath "$artifact")

work=$(mktemp -d "${RUNNER_TEMP:-/tmp}/muniment-webdriver-guard.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

# DEB files contain compressed tar payloads. Extract the shipped filesystem
# instead of scanning the compressed payload as opaque bytes.
mkdir "$work/outer"
if [[ $artifact == *.deb ]]; then
  (cd "$work/outer" && ar x "$artifact")
  payload=$(find "$work/outer" -maxdepth 1 -type f \( -name 'data.tar' -o -name 'data.tar.*' \) -print -quit)
  [[ -n $payload ]] || { echo "DEB data payload is unavailable: $artifact" >&2; exit 1; }
  mkdir "$work/payload"
  tar -xf "$payload" -C "$work/payload"
else
  7z x -y "-o$work/outer" "$artifact" >/dev/null
fi

node test/e2e/support/webdriver-release-guard.mjs "$expectation" "$work"
