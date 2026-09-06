#!/usr/bin/env bash

runner_failure() {
  printf '%s\n' "$1" >&2
  printf '%s\n' "$1" >>"$raw/runner-failure.txt"
  status=1
}

run_setup() {
  local result=0 detail='' helper
  helper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/failure-summary.mjs"
  "$@" >"$raw/setup-stdout.log" 2>"$raw/setup-stderr.log" || result=$?
  cat "$raw/setup-stdout.log"
  cat "$raw/setup-stderr.log" >&2
  if (( result != 0 )); then
    detail=$(node "$helper" "$raw/setup-stdout.log" "$raw/setup-stderr.log" "$1" "$result") || detail="setup command failed (exit code $result), diagnostic summary unavailable"
    runner_failure "$detail"
  else
    rm -f "$raw/setup-stdout.log"
  fi
  return "$result"
}
