#!/usr/bin/env bash

runner_failure() {
  printf '%s\n' "$1" >&2
  printf '%s\n' "$1" >>"$raw/runner-failure.txt"
  status=1
}

run_setup() {
  local result=0 detail='' stream text budget=850 command=$1
  "$@" >"$raw/setup-stdout.log" 2>"$raw/setup-stderr.log" || result=$?
  cat "$raw/setup-stdout.log"
  cat "$raw/setup-stderr.log" >&2
  if (( result != 0 )); then
    # Reserve space for both streams before JUnit applies its 1000-character cap.
    if [[ -s "$raw/setup-stdout.log" && -s "$raw/setup-stderr.log" ]]; then budget=400; fi
    for stream in stdout stderr; do
      text=$(cat "$raw/setup-$stream.log")
      if (( ${#text} > budget )); then
        text="${text:0:budget/2} ... ${text: -$((budget/2-5))}"
      fi
      if [[ -n "$text" ]]; then detail="${detail}${detail:+$'\n'}$text"; fi
    done
    runner_failure "${command:0:100} failed (exit code $result)${detail:+: $detail}"
  else
    rm -f "$raw/setup-stdout.log"
  fi
  return "$result"
}
