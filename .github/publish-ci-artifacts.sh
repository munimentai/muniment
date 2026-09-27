#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

platform=${1:?platform required}
directory=${2:?artifact directory required}
case "$platform" in linux|windows|macos) ;; *) echo 'Unknown platform' >&2; exit 1 ;; esac
: "${GITHUB_RUN_ATTEMPT:?run attempt required}"
[[ "$GITHUB_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]]
[[ -d "$directory" ]]
shopt -s nullglob
reports=("$directory"/junit-*.xml)
if (( ${#reports[@]} == 0 )); then
  echo 'No JUnit report exists for this E2E job.' >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/diagnostics/attempt-$GITHUB_RUN_ATTEMPT" "$work/reports"
tar -czf "$work/diagnostics/attempt-$GITHUB_RUN_ATTEMPT/diagnostics.tar.gz" -C "$directory" .
cp "${reports[@]}" "$work/reports/"
node .github/lib/artifact-store.mjs upload "$platform-e2e" "$work/diagnostics"
# Stable report paths are the factory tester's read contract.
node .github/lib/artifact-store.mjs upload "$platform-e2e-report" "$work/reports"
prefix="s3://factory-ci-artifacts/muniment-desktop/$GITHUB_RUN_ID"
diagnostics="$prefix/$platform-e2e/attempt-$GITHUB_RUN_ATTEMPT/diagnostics.tar.gz"
printf 'Verified CI diagnostics: %s\n' "$diagnostics"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  # shellcheck disable=SC2016
  printf '\n### %s E2E evidence\n\nDiagnostics: `%s`\n\nJUnit: `%s/%s-e2e-report/`\n' \
    "$platform" "$diagnostics" "$prefix" "$platform" >>"$GITHUB_STEP_SUMMARY"
fi
