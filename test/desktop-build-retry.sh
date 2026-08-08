#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
fixture=$(mktemp -d)
cleanup() {
  rm -rf "$fixture"
}
trap cleanup EXIT

awk '
  /^  desktop-build:/ { build = 1 }
  build && /^          attempt=1$/ { loop = 1 }
  loop { last = /^          done$/; sub(/^          /, ""); print }
  loop && last { exit }
' "$root/.github/workflows/ci.yml" > "$fixture/retry.sh"

cat > "$fixture/ssh" <<'EOF'
#!/usr/bin/env bash
count=0
if [[ -f $SSH_ATTEMPTS ]]; then
  read -r count < "$SSH_ATTEMPTS"
fi
count=$((count + 1))
printf '%s\n' "$count" > "$SSH_ATTEMPTS"
if [[ $count -eq 1 ]]; then
  printf '%s\n' 'failed to bundle project: `Peer disconnected`' >&2
  exit 1
fi
EOF
chmod +x "$fixture/ssh"

export PATH="$fixture:$PATH"
export SSH_ATTEMPTS="$fixture/attempts"
export RUNNER_TEMP="$fixture"
export PLATFORM=windows
export key=unused repo_url=unused REF=unused cmd=unused

bash -e -o pipefail "$fixture/retry.sh" > "$fixture/output"
test "$(cat "$SSH_ATTEMPTS")" -eq 2
test "$(grep -Fc 'WiX download failed. Retrying once.' "$fixture/output")" -eq 1
