#!/usr/bin/env bash
set -eu
case "${1:-}" in
  linux) artifact=sherpa-onnx-v1.13.2-linux-x64-shared-no-tts-lib.tar.bz2; digest=1c66f4ec57cbf6a608f09e373796346943702251f75d08c45e8f47345a960ee6 ;;
  windows) artifact=sherpa-onnx-v1.13.2-win-x64-shared-MD-Release-no-tts-lib.tar.bz2; digest=6ddd96bd875349b0580d0bbfd70fb08694ad1b7ef9f02966005aec5c7824b700 ;;
  macos-x64) artifact=sherpa-onnx-v1.13.2-osx-x64-shared-no-tts-lib.tar.bz2; digest=02762796b629b3c2897bef96f60aa39b46bf7b48741ff0554d4325bcbe068800 ;;
  macos-arm64) artifact=sherpa-onnx-v1.13.2-osx-arm64-shared-no-tts-lib.tar.bz2; digest=949bfacca8cfd9fe80d7067a27fdd2d44f874e7b8ef99736d73af609891784e6 ;;
  *) printf 'unsupported desktop ASR staging target: %s\n' "${1:-missing}" >&2; exit 2 ;;
esac
archive="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/$artifact"; extract="${archive%.tar.bz2}"
curl --proto '=https' --tlsv1.2 -fsSL "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/$artifact" -o "$archive"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$archive" | cut -d ' ' -f 1)
else
  actual=$(shasum -a 256 "$archive" | cut -d ' ' -f 1)
fi
test "$actual" = "$digest"
rm -rf "$extract"; mkdir -p "$extract" src-tauri/asr-runtime
tar -xjf "$archive" -C "$extract"
find "$extract" -type f \( -name 'libsherpa-onnx-c-api.*' -o -name 'sherpa-onnx-c-api.dll' -o -name 'libonnxruntime.*' -o -name 'onnxruntime.dll' \) -exec cp {} src-tauri/asr-runtime/ \;
test "$(find src-tauri/asr-runtime -type f | wc -l)" -ge 2
