#!/usr/bin/env bash
# Run with the Linux desktop build dependencies installed.
set -euo pipefail
cd "$(dirname "$0")/.."
test "$(uname -s)" = Linux
manifest=src-tauri/Cargo.toml

# Compile the socket tests before Tauri stages the runtime over Cargo's debug binary.
artifacts=$(cargo test --manifest-path "$manifest" --package muniment-runtime --locked \
  --test attach_boundaries --no-run --message-format=json)
socket_tests=$(printf '%s\n' "$artifacts" | python3 -c '
import json, sys
artifacts = [json.loads(line) for line in sys.stdin]
matches = [a["executable"] for a in artifacts
           if a.get("reason") == "compiler-artifact"
           and a["target"]["name"] == "attach_boundaries" and a.get("executable")]
assert len(matches) == 1, "Cargo must report one socket test executable."
print(matches[0])
')
debug_directory=$(dirname "$(dirname "$socket_tests")")

cargo build --manifest-path "$manifest" --package muniment-runtime --package muniment-acp --release --locked
# The normal desktop build runs tauri_build and copies the configured release resources.
cargo build --manifest-path "$manifest" --package muniment-desktop --locked
cmp src-tauri/target/release/muniment-runtime "$debug_directory/muniment-runtime"

# Run the compiled tests directly so Cargo cannot replace Tauri's staged runtime.
# Each layout checks command and chat-event connections, stale sockets, and executable rejection.
"$socket_tests" desktop_starts_the_ --test-threads=1
cmp src-tauri/target/release/muniment-runtime "$debug_directory/muniment-runtime"
