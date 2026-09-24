#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
asr_runtime="$repo_root/src-tauri/third-party/sherpa-onnx-v1.13.2/link"

export LD_LIBRARY_PATH="$asr_runtime${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# linuxdeploy must resolve CEF from Cargo output before it copies dependencies.
export LD_LIBRARY_PATH="$repo_root/src-tauri/target/release:$LD_LIBRARY_PATH"

tauri_cache="${XDG_CACHE_HOME:-$HOME/.cache}/tauri"
mkdir -p "$tauri_cache"

# Tauri uses these tools from its cache when they exist, so each one is fetched
# from a fixed source and checked against its SHA-256 before the bundler sees
# it. The release asset URLs name the asset by its ID, so a replaced asset
# fails the download instead of serving new bytes. A cached copy that does not
# match is fetched again.
fetch_tool() {
  local url="$1"
  local sha256="$2"
  local destination="$3"
  local temporary="$destination.download"

  if [ -f "$destination" ] && printf '%s  %s\n' "$sha256" "$destination" | sha256sum --check --quiet 2>/dev/null; then
    return 0
  fi
  curl --fail --location --retry 3 --retry-delay 2 --retry-all-errors \
    --header 'Accept: application/octet-stream' \
    --output "$temporary" "$url"
  printf '%s  %s\n' "$sha256" "$temporary" | sha256sum --check --quiet
  chmod +x "$temporary"
  mv "$temporary" "$destination"
}

# tauri-apps/binary-releases apprun-old, asset AppRun-x86_64.
fetch_tool \
  "https://api.github.com/repos/tauri-apps/binary-releases/releases/assets/274691722" \
  f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f \
  "$tauri_cache/AppRun-x86_64"
# tauri-apps/binary-releases linuxdeploy, asset linuxdeploy-x86_64.AppImage.
fetch_tool \
  "https://api.github.com/repos/tauri-apps/binary-releases/releases/assets/182515537" \
  e762bea85c8eb0d4b3508d46e5c1f037f717d0f9303ae3b4aafc8b04991fa1ef \
  "$tauri_cache/linuxdeploy-x86_64.AppImage"
fetch_tool \
  "https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gtk/dda522bce37387f1b853d9095713bfaa924c8423/linuxdeploy-plugin-gtk.sh" \
  7804c9eef13e59bf2783aad9882ef9db8f3f3f9e8d631874b1d348d550a3693f \
  "$tauri_cache/linuxdeploy-plugin-gtk.sh"
fetch_tool \
  "https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gstreamer/2a2e67491c32995a3f279ad0ecbe77abd512b42a/linuxdeploy-plugin-gstreamer.sh" \
  c107b49d84edbffc6ab226ed1007e0626a4f7aa2c3a36b7782bef62351d49e94 \
  "$tauri_cache/linuxdeploy-plugin-gstreamer.sh"

if [ "${MUNIMENT_SIDECARS_BUILT:-0}" != 1 ]; then
  cargo build --manifest-path "$repo_root/src-tauri/Cargo.toml" --package muniment-acp --release --locked
  cargo build --manifest-path "$repo_root/src-tauri/Cargo.toml" --package muniment-runtime --package muniment-cli --release --locked
  # The reader sidecar is Go, static, and lands beside the Rust binaries.
  bash "$repo_root/.github/build-reader.sh" src-tauri/target/release/muniment-reader
fi

npm run tauri build -- --verbose --no-bundle "$@"
node "$repo_root/scripts/stage-cef-linux.mjs"
npm run tauri bundle -- --verbose "$@" --config '{"bundle":{"resources":{"target/release/cef-resources/":"cef/"}}}'
node scripts/package-appimage-linux.mjs
