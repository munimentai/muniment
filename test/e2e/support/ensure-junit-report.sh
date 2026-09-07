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
if (( ${#reports[@]} != 0 )) && { (( run_status == 0 && extract_status == 0 )) || [[ ! -s "$artifacts_dir/runner-failure.txt" ]]; }; then
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

failure_message=$(node --input-type=module - "$artifacts_dir" "$failure_message" <<'NODE'
import fs from 'node:fs'
import path from 'node:path'

const [directory, fallback] = process.argv.slice(2)
let message = fallback
for (const name of ['runner-failure.txt', 'envelope-reason.txt']) {
  const file = path.join(directory, name)
  if (!fs.existsSync(file)) continue
  const cause = fs.readFileSync(file, 'utf8')
    .replace(/[^\u0009\u000a\u000d\u0020-\ud7ff\ue000-\ufffd\u{10000}-\u{10ffff}]/gu, '')
    .replace(/\s+/gu, ' ').trim()
  if (cause) {
    message = cause
    break
  }
}
// Cap the decoded message before escaping, so the limit cannot split an XML entity.
const escaped = Array.from(message).slice(0, 1000).join('')
  .replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
  .replaceAll('"', '&quot;').replaceAll("'", '&apos;')
process.stdout.write(escaped)
NODE
)

printf '%s\n' \
  '<?xml version="1.0" encoding="UTF-8"?>' \
  "<testsuites tests=\"1\" failures=\"1\"><testsuite name=\"$suite_name\" tests=\"1\" failures=\"1\"><testcase name=\"desktop-ci infrastructure\"><failure message=\"$failure_message\"/></testcase></testsuite></testsuites>" \
  >"$artifacts_dir/junit-infrastructure.xml"
