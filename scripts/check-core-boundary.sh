#!/usr/bin/env bash
# Check the inventory, shared crate sources, desktop-only modules, and dependency trees.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 -B scripts/check-core-boundary.py "$@"
