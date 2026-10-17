#!/usr/bin/env bash
# Check the core boundary, then the desktop inventory, port crate paths, desktop-only modules, and dependency trees.
set -euo pipefail
cd "$(dirname "$0")/.."
core/scripts/check-core-boundary.sh "$@"
exec python3 -B scripts/check-core-boundary.py "$@"
