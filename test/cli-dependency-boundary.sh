#!/usr/bin/env bash
set -eu

if [ "$#" -gt 0 ]; then
  packages=$(printf '%s\n' "$@")
else
  packages=$(cargo tree --manifest-path src-tauri/Cargo.toml --package muniment-cli \
    --locked --target all --prefix none --format '{p}' | awk '{print $1}')
fi

unexpected=$(printf '%s\n' "$packages" | grep -Ev '^(muniment-cli|muniment-attach)$' || true)
if [ -n "$unexpected" ]; then
  printf 'muniment-cli includes dependencies outside the protocol boundary:\n%s\n' \
    "$unexpected" >&2
  exit 1
fi
