#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
fixture=$(mktemp -d)
cleanup() {
  rm -rf "$fixture"
}
trap cleanup EXIT

awk '
  index($0, "- name: Build (${{ matrix.platform }}) via desktop-ci") { build = 1 }
  build && !settings && /^          set / {
    sub(/^          /, ""); print; settings = 1
  }
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
if [[ -n ${SSH_ERROR:-} ]]; then
  printf '%s\n' "$SSH_ERROR" >&2
fi
if [[ ${SSH_FAIL_ONCE:-false} = true && $count -gt 1 ]]; then
  exit 0
fi
exit "${SSH_STATUS:-1}"
EOF
chmod +x "$fixture/ssh"

export PATH="$fixture:$PATH"
export SSH_ATTEMPTS="$fixture/attempts"
export RUNNER_TEMP="$fixture"
export PLATFORM=windows
export key=unused repo_url=unused REF=unused cmd=unused

export SSH_STATUS=1
export SSH_FAIL_ONCE=true
export SSH_ERROR='failed to bundle project: `Peer disconnected`'
bash "$fixture/retry.sh" > "$fixture/output"
test "$(cat "$SSH_ATTEMPTS")" -eq 2
test "$(grep -Fc 'WiX download failed. Retrying once.' "$fixture/output")" -eq 1

rm -f "$SSH_ATTEMPTS"
export SSH_STATUS=3
export SSH_FAIL_ONCE=false
export SSH_ERROR=
if bash "$fixture/retry.sh" > "$fixture/output"; then
  echo "desktop build retry loop ignored a failed driver" >&2
  exit 1
else
  status=$?
fi
test "$status" -eq 3
test "$(cat "$SSH_ATTEMPTS")" -eq 2
