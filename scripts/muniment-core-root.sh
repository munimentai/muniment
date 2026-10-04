#!/usr/bin/env bash
# Print the muniment-core checkout that Cargo resolves for the desktop. The
# shared crates, their fixture exporters and pins/pins.toml live there.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo metadata --manifest-path src-tauri/Cargo.toml --locked --format-version 1 \
  | python3 -c 'import json, sys
from pathlib import Path
packages = json.load(sys.stdin)["packages"]
pins = next(package for package in packages if package["name"] == "muniment-pins")
print(Path(pins["manifest_path"]).parents[2])'
