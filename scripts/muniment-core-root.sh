#!/usr/bin/env bash
# Print the core tree in this repository. The shared crates, their fixture
# exporters and pins/pins.toml live there.
set -euo pipefail
cd "$(dirname "$0")/../core"
pwd
