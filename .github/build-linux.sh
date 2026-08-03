#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
asr_runtime="$repo_root/src-tauri/third-party/sherpa-onnx-v1.13.2/link"

export LD_LIBRARY_PATH="$asr_runtime${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

for attempt in 1 2; do
  if npm run tauri build "$@"; then
    exit 0
  fi
  if [ "$attempt" -eq 2 ]; then
    exit 1
  fi
  echo "Linux bundle build failed. Retrying once."
done
