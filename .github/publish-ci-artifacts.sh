#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

platform=${1:?platform required}
directory=${2:?artifact directory required}
case "$platform" in linux|windows|macos) ;; *) echo 'Unknown platform' >&2; exit 1 ;; esac
: "${AWS_ACCESS_KEY_ID:?CI artifact access key required}"
: "${AWS_SECRET_ACCESS_KEY:?CI artifact secret key required}"
: "${GITHUB_RUN_ID:?run ID required}"
: "${GITHUB_RUN_ATTEMPT:?run attempt required}"
[[ "$GITHUB_RUN_ID" =~ ^[0-9]+$ && "$GITHUB_RUN_ATTEMPT" =~ ^[0-9]+$ ]]
[[ -d "$directory" ]]
shopt -s nullglob
reports=("$directory"/junit-*.xml)
if (( ${#reports[@]} == 0 )); then
  echo 'No JUnit report exists for this E2E job.' >&2
  exit 1
fi

endpoint=http://10.1.10.101:9000
prefix="s3://factory-ci-artifacts/muniment-desktop/$GITHUB_RUN_ID"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Read back each upload before declaring the evidence available.
publish() {
  aws --endpoint-url "$endpoint" s3 cp "$1" "$2" --only-show-errors
  aws --endpoint-url "$endpoint" s3 cp "$2" "$work/readback" --only-show-errors
  cmp "$1" "$work/readback"
}

archive="$work/diagnostics.tar.gz"
tar -czf "$archive" -C "$directory" .
diagnostics="$prefix/$platform-e2e/attempt-$GITHUB_RUN_ATTEMPT/diagnostics.tar.gz"
publish "$archive" "$diagnostics"
for report in "${reports[@]}"; do
  # Stable report paths are the factory tester's read contract.
  publish "$report" "$prefix/$platform-e2e-report/$(basename "$report")"
done
printf 'Verified CI diagnostics: %s\n' "$diagnostics"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  # shellcheck disable=SC2016
  printf '\n### %s E2E evidence\n\nDiagnostics: `%s`\n\nJUnit: `%s/%s-e2e-report/`\n' \
    "$platform" "$diagnostics" "$prefix" "$platform" >>"$GITHUB_STEP_SUMMARY"
fi
