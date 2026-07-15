#!/usr/bin/env bash
set -eu

bundle=src-tauri/target/release/bundle
case "${1:-}" in
  linux) required='libsherpa-onnx-c-api.so libonnxruntime.so' ;;
  macos) required='libsherpa-onnx-c-api.dylib libonnxruntime.dylib' ;;
  windows)
    # Windows installers are installed and checked by windows-installers.ps1.
    exit 0
    ;;
  *) printf 'unsupported ASR bundle target: %s\n' "${1:-missing}" >&2; exit 2 ;;
esac

for file in $required; do
  test "$(find "$bundle" -type f -name "$file" | wc -l)" -ge 1
done
for file in sherpa-onnx-NOTICE.md sherpa-onnx-LICENSE.txt onnxruntime-LICENSE.txt; do
  test "$(find "$bundle" -type f -name "$file" | wc -l)" -ge 1
done
