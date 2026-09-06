#!/usr/bin/env bash

runner_failure() {
  printf '%s\n' "$1" >&2
  printf '%s\n' "$1" >>"$raw/runner-failure.txt"
  status=1
}

run_setup() {
  local result=0 detail
  "$@" >"$raw/setup-stdout.log" 2>"$raw/setup-stderr.log" || result=$?
  cat "$raw/setup-stdout.log"
  cat "$raw/setup-stderr.log" >&2
  if (( result != 0 )); then
    detail=$(cat "$raw/setup-stdout.log")
    if [[ -s "$raw/setup-stderr.log" ]]; then
      detail="${detail}${detail:+$'\n'}$(cat "$raw/setup-stderr.log")"
    fi
    runner_failure "$1 failed (exit code $result)${detail:+: $detail}"
  fi
  rm -f "$raw/setup-stdout.log"
  return "$result"
}
