#!/usr/bin/env bash
set -euo pipefail

artifacts_dir=${1:?artifacts directory is required}
suite_name=${2:?suite name is required}
run_status=${3:?run status is required}
extract_status=${4:?extract status is required}
create_success_report=${5:-0}

if (( run_status == 0 && extract_status == 0 && create_success_report == 0 )); then
  exit 0
fi

shopt -s nullglob
reports=("$artifacts_dir"/junit-*.xml)
if (( ${#reports[@]} != 0 )); then
  exit 0
fi

mkdir -p "$artifacts_dir"
if (( run_status == 0 && extract_status == 0 )); then
  printf '%s\n' \
    '<?xml version="1.0" encoding="UTF-8"?>' \
    "<testsuites tests=\"1\" failures=\"0\"><testsuite name=\"$suite_name\" tests=\"1\" failures=\"0\"><testcase name=\"installed application smoke\"/></testsuite></testsuites>" \
    >"$artifacts_dir/junit-smoke.xml"
  exit 0
fi

if (( extract_status != 0 )); then
  failure_message='desktop-ci did not return a valid artifact envelope'
else
  failure_message='desktop-ci failed before producing a JUnit report'
fi

reason_file=
for candidate in runner-failure.txt envelope-reason.txt desktop-ci-harness.log; do
  if [[ -s "$artifacts_dir/$candidate" ]]; then
    reason_file="$artifacts_dir/$candidate"
    break
  fi
done
if [[ -n $reason_file ]]; then
  captured_reason=$(tail -n 20 "$reason_file" | tr '\r\n' '  ' | sed 's/[[:space:]][[:space:]]*/ /g; s/^ //; s/ $//')
  if [[ -n $captured_reason ]]; then
    failure_message="$failure_message: $captured_reason"
  fi
fi

failure_message=$(printf '%s' "$failure_message" | sed 's/\&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g; s/"/\&quot;/g; s/'"'"'/\&apos;/g')

printf '%s\n' \
  '<?xml version="1.0" encoding="UTF-8"?>' \
  "<testsuites tests=\"1\" failures=\"1\"><testsuite name=\"$suite_name\" tests=\"1\" failures=\"1\"><testcase name=\"desktop-ci infrastructure\"><failure message=\"$failure_message\"/></testcase></testsuite></testsuites>" \
  >"$artifacts_dir/junit-infrastructure.xml"
