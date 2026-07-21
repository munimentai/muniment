#!/usr/bin/env bash
set -euo pipefail
input=$1 destination=$2
begin='=== DESKTOP-CI ARTIFACTS BEGIN ==='
end='=== DESKTOP-CI ARTIFACTS END ==='

# A malformed envelope is otherwise un-triageable: the raw desktop-ci transcript
# lives only in the runner's temp file and is discarded when this exits, so the
# nightly repeatedly surfaces an opaque "invalid artifact envelope" with no
# evidence. Record secret-free structural counts (marker occurrences, transcript
# size, failing stage) into the uploaded artifacts directory so the next
# prod nightly reveals whether desktop-ci emitted zero, duplicate, or corrupt
# markers. Only fixed labels and integer counts are written -- never a line of
# the transcript, which could carry injected credentials.
fail() {
  local stage=$1 message=$2
  local begin_count end_count line_count byte_count
  begin_count=$(grep -Fxc -- "$begin" "$input" 2>/dev/null || true)
  end_count=$(grep -Fxc -- "$end" "$input" 2>/dev/null || true)
  line_count=$(wc -l <"$input" 2>/dev/null || echo 0)
  byte_count=$(wc -c <"$input" 2>/dev/null || echo 0)
  mkdir -p "$destination"
  printf '%s\n' \
    "stage=$stage" \
    "reason=$message" \
    "begin_marker_count=${begin_count:-0}" \
    "end_marker_count=${end_count:-0}" \
    "transcript_line_count=${line_count// /}" \
    "transcript_byte_count=${byte_count// /}" \
    >"$destination/envelope-diagnostics.txt"
  echo "$message" >&2
  exit 1
}

payload=$(mktemp) archive=$(mktemp)
trap 'rm -f "$payload" "$archive"' EXIT
awk -v begin="$begin" -v end="$end" '
  $0 == begin { if (++begins != 1 || inside) exit 41; inside=1; next }
  $0 == end { if (!inside || ++ends != 1) exit 42; inside=0; next }
  inside { print }
  END { if (begins != 1 || ends != 1 || inside) exit 43 }
' "$input" >"$payload" || fail markers 'invalid desktop-ci artifact markers'
[[ -s $payload ]] || fail payload 'empty desktop-ci artifact payload'
node -e '
  const fs = require("fs");
  const encoded = fs.readFileSync(process.argv[1], "utf8").replace(/\n/g, "");
  if (!encoded || encoded.length % 4 || !/^[A-Za-z0-9+/]+={0,2}$/.test(encoded)) process.exit(1);
  const decoded = Buffer.from(encoded, "base64");
  if (decoded.toString("base64") !== encoded) process.exit(1);
  fs.writeFileSync(process.argv[2], decoded);
' "$payload" "$archive" || fail base64 'invalid desktop-ci artifact base64'
while IFS= read -r member; do
  [[ $member != /* && $member != *'../'* && $member != '..' ]] || fail archive-path 'unsafe archive path'
done < <(tar -tzf "$archive")
while IFS= read -r line; do
  case ${line:0:1} in -|d) ;; *) fail archive-type 'unsafe archive member type';; esac
done < <(tar -tvzf "$archive")
stage=$(mktemp -d "${destination}.stage.XXXXXX")
trap 'rm -f "$payload" "$archive"; rm -rf "$stage"' EXIT
tar -xzf "$archive" --no-same-owner --no-same-permissions -C "$stage"
rm -rf "$destination"
mv "$stage" "$destination"
