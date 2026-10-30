#!/usr/bin/env bash
# Factory wrapper for the CI lane classifier.
# Selects the subset of the factory contract commands the changed paths need,
# prints the lane decision, then runs that subset. GATE_DRY_RUN=1 prints the
# selected commands instead of running them.
set -eu

usage() {
  echo "usage: scripts/ci/gate.sh <lint|unit_tests>" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
case "$1" in
  lint|unit_tests) gate=$1 ;;
  *) usage ;;
esac

# The script lives in the checkout it classifies. GIT_DIR lets a test point
# the same script at another repository's refs.
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$repo_root"
git_dir=${GIT_DIR:-$repo_root/.git}

if git --git-dir="$git_dir" rev-parse --verify --quiet origin/main >/dev/null 2>&1; then
  base=$(git --git-dir="$git_dir" merge-base HEAD origin/main)
  event=pull_request
else
  # No origin/main ref: every lane runs.
  base=
  event=workflow_dispatch
  echo "gate: no origin/main ref; selecting every lane"
fi

lanes=$(GIT_DIR=$git_dir GITHUB_EVENT_NAME=$event "$script_dir/classify.sh" "$base" HEAD)
printf '%s\n' "$lanes"

value() {
  printf '%s\n' "$lanes" | sed -n "s/^$1=//p"
}
desktop=$(value desktop)
companion=$(value companion)
core_shared=$(value core_shared)
core_support=$(value core_support)

always_lint=(
  "bash scripts/check-steering.sh ."
  "node test/ui-copy-lint.mjs src src-tauri browser-control"
  "bash scripts/check-private-addresses.sh"
  "python3 -B scripts/version.py check"
  "CARGO_HOME=\$HOME/.cargo bash test/smoke.sh"
)
tauri_lint=(
  "CARGO_HOME=\$HOME/.cargo bash test/cli-dependency-boundary.sh"
  "CARGO_HOME=\$HOME/.cargo bash test/runtime-dependency-boundary.sh"
  "RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo cargo fmt --manifest-path src-tauri/Cargo.toml --all --check"
  "RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo cargo clippy --manifest-path src-tauri/Cargo.toml --package muniment-desktop-integration --package muniment-acp --package muniment-cli --package muniment-runtime --all-targets --locked -- -D warnings"
)
reader_lint=(
  'cd src-tauri/reader && test -z "$(gofmt -l .)" && go vet ./...'
)
core_lint=(
  "cd core && RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo cargo fmt --all --check"
  "cd core && RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo cargo clippy --workspace --all-targets --locked --features muniment-pins/cli -- -D warnings"
  "cd core && RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo cargo clippy -p muniment-router --all-targets --features server --locked -- -D warnings"
  "cd core && TMPDIR=/tmp RUST_TEST_THREADS=1 RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo scripts/check-core-boundary.sh"
)
always_tests=(
  "CARGO_HOME=\$HOME/.cargo npx vitest run --root . --exclude 'test/e2e/**' --exclude 'prototypes/**' --exclude 'browser-control/**' --exclude '**/*.browser.test.js' --retry=1"
  "node --test test/ci-routing.node.mjs"
  "node --test test/subscription-acceptance.node.mjs"
  "python3 -B scripts/test_check_steering.py && python3 -B scripts/test_private_addresses.py && python3 -B scripts/test_version.py"
  "npm run browser-control"
  "cd core && python3 -B scripts/test_release.py"
  "cd core && python3 -B scripts/pins/test_update.py"
  "cd core && python3 -B scripts/test_desktop_ci.py"
)
tauri_tests=(
  "TMPDIR=/tmp CARGO_HOME=\$HOME/.cargo cargo test --manifest-path src-tauri/Cargo.toml --locked --package muniment-desktop-integration --package muniment-cli --package muniment-acp --package muniment-runtime --features muniment-desktop-integration/keyring -- --test-threads=1 --skip real_diagnostic_selects_the_browser_client_half"
)
reader_tests=(
  "cd src-tauri/reader && go test ./..."
)
core_tests=(
  "cd core && TMPDIR=/tmp RUSTUP_HOME=\$HOME/.rustup CARGO_HOME=\$HOME/.cargo cargo test --workspace --locked --features muniment-pins/cli -- --test-threads=1"
)

selected=()
skipped=()
take() {
  local reason=$1
  shift
  if [ "$1" = run ]; then
    shift
    selected+=("$@")
  else
    shift
    local command
    for command in "$@"; do
      skipped+=("$reason: $command")
    done
  fi
}

if [ "$gate" = lint ]; then
  take always run "${always_lint[@]}"
  # The classifier sets companion for the src-tauri cli, runtime, reader,
  # integration and acp trees, and for a shared core change. A desktop change
  # that leaves companion false is the frontend and skips these crates.
  if [ "$companion" = true ]; then
    take companion run "${tauri_lint[@]}"
  else
    take companion skip "${tauri_lint[@]}"
  fi
  if [ "$companion" = true ]; then
    take companion run "${reader_lint[@]}"
  else
    take companion skip "${reader_lint[@]}"
  fi
  if [ "$core_shared" = true ] || [ "$core_support" = true ]; then
    take "core_shared or core_support" run "${core_lint[@]}"
  else
    take "core_shared or core_support" skip "${core_lint[@]}"
  fi
else
  take always run "${always_tests[@]}"
  if [ "$companion" = true ]; then
    take companion run "${tauri_tests[@]}"
  else
    take companion skip "${tauri_tests[@]}"
  fi
  if [ "$companion" = true ]; then
    take companion run "${reader_tests[@]}"
  else
    take companion skip "${reader_tests[@]}"
  fi
  if [ "$core_shared" = true ] || [ "$core_support" = true ]; then
    take "core_shared or core_support" run "${core_tests[@]}"
  else
    take "core_shared or core_support" skip "${core_tests[@]}"
  fi
fi

if [ "${#skipped[@]}" -gt 0 ]; then
  echo "gate: skipped"
  printf 'skip: %s\n' "${skipped[@]}"
fi
echo "gate: run"
if [ "${#selected[@]}" -eq 0 ]; then
  echo "gate: no commands selected"
  exit 0
fi
printf 'run: %s\n' "${selected[@]}"

if [ "${GATE_DRY_RUN:-}" = 1 ]; then
  exit 0
fi

for command in "${selected[@]}"; do
  bash -c "$command"
done
