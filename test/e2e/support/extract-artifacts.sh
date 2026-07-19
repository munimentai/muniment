#!/usr/bin/env bash
set -euo pipefail
input=$1 destination=$2
begin='=== DESKTOP-CI ARTIFACTS BEGIN ==='
end='=== DESKTOP-CI ARTIFACTS END ==='
payload=$(mktemp) archive=$(mktemp)
trap 'rm -f "$payload" "$archive"' EXIT
awk -v begin="$begin" -v end="$end" '
  $0 == begin { if (++begins != 1 || inside) exit 41; inside=1; next }
  $0 == end { if (!inside || ++ends != 1) exit 42; inside=0; next }
  inside { print }
  END { if (begins != 1 || ends != 1 || inside) exit 43 }
' "$input" >"$payload" || { echo 'invalid desktop-ci artifact markers' >&2; exit 1; }
[[ -s $payload ]] || { echo 'empty desktop-ci artifact payload' >&2; exit 1; }
node -e '
  const fs = require("fs");
  const encoded = fs.readFileSync(process.argv[1], "utf8").replace(/\n/g, "");
  if (!encoded || encoded.length % 4 || !/^[A-Za-z0-9+/]+={0,2}$/.test(encoded)) process.exit(1);
  const decoded = Buffer.from(encoded, "base64");
  if (decoded.toString("base64") !== encoded) process.exit(1);
  fs.writeFileSync(process.argv[2], decoded);
' "$payload" "$archive" || { echo 'invalid desktop-ci artifact base64' >&2; exit 1; }
while IFS= read -r member; do
  [[ $member != /* && $member != *'../'* && $member != '..' ]] || { echo 'unsafe archive path' >&2; exit 1; }
done < <(tar -tzf "$archive")
while IFS= read -r line; do
  case ${line:0:1} in -|d) ;; *) echo 'unsafe archive member type' >&2; exit 1;; esac
done < <(tar -tvzf "$archive")
stage=$(mktemp -d "${destination}.stage.XXXXXX")
trap 'rm -f "$payload" "$archive"; rm -rf "$stage"' EXIT
tar -xzf "$archive" --no-same-owner --no-same-permissions -C "$stage"
rm -rf "$destination"
mv "$stage" "$destination"
