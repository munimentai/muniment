#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
asr_runtime="$repo_root/src-tauri/third-party/sherpa-onnx-v1.13.2/link"

export LD_LIBRARY_PATH="$asr_runtime${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

tauri_cache="${XDG_CACHE_HOME:-$HOME/.cache}/tauri"
mkdir -p "$tauri_cache"

fetch_tool() {
  local url="$1"
  local destination="$2"
  local temporary="$destination.download"

  if [ ! -f "$destination" ]; then
    curl --fail --location --retry 3 --retry-delay 2 --retry-all-errors \
      --output "$temporary" "$url"
    chmod +x "$temporary"
    mv "$temporary" "$destination"
  fi
}

fetch_tool \
  "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64" \
  "$tauri_cache/AppRun-x86_64"
fetch_tool \
  "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-x86_64.AppImage" \
  "$tauri_cache/linuxdeploy-x86_64.AppImage"
fetch_tool \
  "https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh" \
  "$tauri_cache/linuxdeploy-plugin-gtk.sh"
fetch_tool \
  "https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gstreamer/master/linuxdeploy-plugin-gstreamer.sh" \
  "$tauri_cache/linuxdeploy-plugin-gstreamer.sh"

cargo build --manifest-path "$repo_root/src-tauri/Cargo.toml" --package muniment-acp --release --locked
cargo build --manifest-path "$repo_root/src-tauri/Cargo.toml" --package muniment-runtime --release --locked

exec npm run tauri build "$@"
