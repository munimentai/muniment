#!/usr/bin/env bash
# Give each runner a disposable Keychain without changing access to existing items.
set -euo pipefail
set +x
umask 077
report() { printf 'macos-keychain-session: reason=%s\n' "$1" >&2; }
trap 'report setup-failed' ERR
[[ $# -gt 0 ]] || { report missing-command; exit 2; }
session_uid=$(id -u)
# A GUI LaunchAgent uses the console audit session without root access.
# An SSH-only unlock leaves Spotlight's console session locked.
launchctl print "gui/$session_uid" >/dev/null
prior_default=$(security default-keychain -d user | sed 's/^[[:space:]]*"//;s/"[[:space:]]*$//')
prior_list=$(security list-keychains -d user | sed 's/^[[:space:]]*"//;s/"[[:space:]]*$//')
[[ -n $prior_default && -n $prior_list ]] || { report setup-failed; exit 1; }
previous=()
while IFS= read -r entry; do previous+=("$entry"); done <<<"$prior_list"
work=$(mktemp -d "${TMPDIR:-/tmp}/muniment-keychain.XXXXXX")
keychain="$work/e2e.keychain-db"
label="ai.muniment.e2e-keychain.${work##*.}"
agent_requested=0
cleanup() {
  result=$?
  trap - EXIT INT TERM ERR
  # Stop the agent before restoring the Keychains or removing its files.
  if (( agent_requested )); then
    launchctl bootout "gui/$session_uid/$label" >/dev/null 2>&1 || { report cleanup-agent; result=1; }
  fi
  if [[ -f "$work/created" || -f "$keychain" ]]; then
    security default-keychain -d user -s "$prior_default" >/dev/null 2>&1 || { report cleanup-default; result=1; }
    security list-keychains -d user -s "${previous[@]}" >/dev/null 2>&1 || { report cleanup-list; result=1; }
    security delete-keychain "$keychain" >/dev/null 2>&1 || { report cleanup-delete; result=1; }
  fi
  rm -rf -- "$work" || { report cleanup-files; result=1; }
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
openssl rand -hex 32 >"$work/password"
cat >"$work/setup.sh" <<'SH'
#!/bin/bash
set -euo pipefail
set +x
umask 077
work=$1
security=$2
exec >"$work/setup.log" 2>&1
# Publish the result atomically so the caller never reads an empty status.
trap 'result=$?; printf "%s\n" "$result" >"$work/result.tmp"; mv "$work/result.tmp" "$work/result"' EXIT
password=$(<"$work/password")
keychain="$work/e2e.keychain-db"
"$security" create-keychain -p "$password" "$keychain"
touch "$work/created"
# Keep both sessions unlocked until cleanup, including across a VM sleep.
"$security" set-keychain-settings "$keychain"
"$security" unlock-keychain -p "$password" "$keychain"
SH
xml_string() {
  printf '%s' "$1" | sed 's/\&/\&amp;/g;s/</\&lt;/g;s/>/\&gt;/g;s/"/\&quot;/g;s/'"'"'/\&apos;/g'
}
cat >"$work/agent.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key><array>
    <string>/bin/bash</string>
    <string>$(xml_string "$work/setup.sh")</string>
    <string>$(xml_string "$work")</string>
    <string>$(xml_string "$(command -v security)")</string>
  </array>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
  <key>RunAtLoad</key><true/>
</dict></plist>
PLIST
# Register cleanup before bootstrap, which can fail after loading the agent.
agent_requested=1
launchctl bootstrap "gui/$session_uid" "$work/agent.plist"
for (( attempt=0; attempt<300; attempt++ )); do
  [[ ! -f "$work/result" ]] || break
  sleep 0.1
done
if [[ ! -f "$work/result" ]]; then
  report setup-timeout
  exit 1
fi
[[ $(<"$work/result") == 0 ]] || { report setup-failed; exit 1; }
security unlock-keychain -p "$(<"$work/password")" "$keychain"
rm -f -- "$work/password"
security list-keychains -d user -s "$keychain"
security default-keychain -d user -s "$keychain"
if "$@"; then
  exit 0
else
  result=$?
  report probe-failed
  exit "$result"
fi
