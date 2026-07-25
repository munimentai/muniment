#!/usr/bin/env bash
set -euo pipefail
input=$1 destination=$2
# desktop-ci's exit status separates the two faults that both surface here as a
# missing envelope: a driver that never ran the guest command at all, and a
# guest that ran but published a malformed one. Only a plain integer is
# recorded, so nothing transcript-derived can reach the uploaded diagnostics.
run_status=${3:-unrecorded}
[[ $run_status =~ ^[0-9]+$ ]] || run_status=unrecorded
begin='=== DESKTOP-CI ARTIFACTS BEGIN ==='
end='=== DESKTOP-CI ARTIFACTS END ==='
# The guest publishes the envelope above itself. When it cannot (an abort before
# its finalizer, or a platform runner that only fills the in-VM artifact dir),
# desktop-ci's own --collect-artifacts wraps the same base64 tgz in these fence
# lines instead (runner contract: strip the `-----` lines, base64 -d, tar xz).
# Either form is accepted, through identical strict checks; the guest form wins
# when both are present because it is the redacted, guest-verified bundle.
driver_begin='-----DESKTOP-CI-ARTIFACTS-BEGIN-----'
driver_end='-----DESKTOP-CI-ARTIFACTS-END-----'

# A malformed envelope is otherwise un-triageable: the raw desktop-ci transcript
# lives only in the runner's temp file and is discarded when this exits, so the
# nightly repeatedly surfaces an opaque "invalid artifact envelope" with no
# evidence. Record secret-free structural counts (marker occurrences, transcript
# size, failing stage) into the uploaded artifacts directory so the next
# prod nightly reveals whether desktop-ci emitted zero, duplicate, or corrupt
# markers. Only fixed labels and integer counts are written -- never a line of
# the transcript, which could carry injected credentials.
count_lines() { local matches; matches=$(grep -Fc -e "$1" "$input" 2>/dev/null || true); printf '%s' "${matches:-0}"; }
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
    "desktop_ci_exit_status=$run_status" \
    "begin_marker_count=${begin_count:-0}" \
    "end_marker_count=${end_count:-0}" \
    "transcript_line_count=${line_count// /}" \
    "transcript_byte_count=${byte_count// /}" \
    >"$destination/envelope-diagnostics.txt"
  # Presence counts for desktop-ci's own fixed log constants. These are literals
  # the driver emits, never transcript-derived values, so they name the failing
  # driver stage without any transcript content leaving the runner.
  printf '%s\n' \
    "driver_log_line_count=$(count_lines '[desktop-ci ')" \
    "driver_fatal_no_slot=$(count_lines 'FATAL: no slot')" \
    "driver_fatal_clone_failed=$(count_lines 'FATAL: clone failed')" \
    "driver_fatal_ssh_unreachable=$(count_lines 'FATAL: SSH not reachable')" \
    "driver_build_green=$(count_lines 'BUILD GREEN')" \
    "driver_build_failed=$(count_lines 'BUILD FAILED')" \
    "driver_no_artifacts_warning=$(count_lines 'WARN: no artifacts collected')" \
    "driver_artifact_marker_count=$(count_lines "$driver_begin")" \
    "driver_screendump_marker_count=$(count_lines '-----DESKTOP-CI-SCREENDUMP-BEGIN-----')" \
    >>"$destination/envelope-diagnostics.txt"
  echo "$message" >&2
  exit 1
}

payload=$(mktemp) archive=$(mktemp)
trap 'rm -f "$payload" "$archive"' EXIT
# fence=1 drops the `-----` lines the driver's Windows collector can leave inside
# its own block; the fence lines themselves stay markers, so a duplicated or
# interleaved driver envelope is still rejected rather than silently merged.
select_payload() {
  awk -v begin="$1" -v end="$2" -v fence="$3" '
    $0 == begin { if (++begins != 1 || inside) exit 41; inside=1; next }
    $0 == end { if (!inside || ++ends != 1) exit 42; inside=0; next }
    fence && inside && /^-----/ { next }
    inside { print }
    END { if (begins != 1 || ends != 1 || inside) exit 43 }
  ' "$input"
}
if grep -Fxq -e "$begin" -e "$end" "$input" 2>/dev/null; then
  select_payload "$begin" "$end" 0 >"$payload" || fail markers 'invalid desktop-ci artifact markers'
else
  select_payload "$driver_begin" "$driver_end" 1 >"$payload" || fail markers 'invalid desktop-ci artifact markers'
fi
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
