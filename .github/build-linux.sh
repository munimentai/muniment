#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
asr_runtime="$repo_root/src-tauri/third-party/sherpa-onnx-v1.13.2/link"

export LD_LIBRARY_PATH="$asr_runtime${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec npm run tauri build "$@"
