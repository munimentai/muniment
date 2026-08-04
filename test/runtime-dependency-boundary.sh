#!/usr/bin/env bash
set -eu

manifest=${MUNIMENT_CARGO_MANIFEST:-src-tauri/Cargo.toml}

if [ "$#" -gt 0 ]; then
  direct_packages=$(printf '%s\n' "$@")
  all_packages=$direct_packages
else
  direct_packages=$(cargo tree --manifest-path "$manifest" --package muniment-runtime \
    --locked --target all --depth 1 --prefix none --format '{p}' | awk '{print $1}')
  all_packages=$(cargo tree --manifest-path "$manifest" --package muniment-runtime \
    --locked --target all --prefix none --format '{p}' | awk '{print $1}')
fi

unexpected=$(printf '%s\n' "$direct_packages" | grep -Ev \
  '^(muniment-runtime|muniment-core|muniment-attach)$' || true)
forbidden=$(printf '%s\n' "$all_packages" | grep -E \
  '^(muniment-desktop|muniment-cli|muniment-acp|tauri|tauri-.*)$' || true)
if [ -n "$unexpected" ] || [ -n "$forbidden" ]; then
  printf 'muniment-runtime includes dependencies outside the runtime boundary:\n%s\n%s\n' \
    "$unexpected" "$forbidden" >&2
  exit 1
fi
