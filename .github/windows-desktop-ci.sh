#!/usr/bin/env bash
set -eu

case "${1:-}" in
  preflight)
    test/stage-asr-runtime.sh windows
    cargo build --manifest-path src-tauri/Cargo.toml --locked --example sidecar-test-stub
    SHERPA_ONNX_LIB_DIR="$PWD/src-tauri/asr-runtime" \
      cargo test --manifest-path src-tauri/Cargo.toml --locked --all-targets
    ;;
  build)
    test/stage-asr-runtime.sh windows
    npm ci --no-audit --no-fund
    SHERPA_ONNX_LIB_DIR="$PWD/src-tauri/asr-runtime" \
      node .github/build-windows-installers.mjs
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File test/windows-installers.ps1
    test/bundle-asr-runtime.sh windows
    ;;
  nightly)
    test "$#" -eq 5
    git fetch --depth 1 origin "$2"
    git checkout --detach "$2"
    "$0" build
    node .github/upload-nightly-assets.mjs "$3" "$4" "$2" "$5"
    ;;
  *)
    echo "usage: $0 {preflight|build|nightly SHA TOKEN REPOSITORY PLATFORM}" >&2
    exit 2
    ;;
esac
