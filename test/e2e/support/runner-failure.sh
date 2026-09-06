#!/usr/bin/env bash

runner_failure() {
  printf '%s\n' "$1" >&2
  printf '%s\n' "$1" >>"$raw/runner-failure.txt"
  status=1
}

run_setup() {
  local result=0 detail
  "$@" 2>"$raw/setup-stderr.log" || result=$?
  cat "$raw/setup-stderr.log" >&2
  if (( result != 0 )); then
    detail=$(cat "$raw/setup-stderr.log")
    runner_failure "$1 failed (exit code $result)${detail:+: $detail}"
  fi
  return "$result"
}
