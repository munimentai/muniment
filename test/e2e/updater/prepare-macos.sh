#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077
[[ $(uname -s) == Darwin && $(id -un) == harness && $HOME == /Users/harness ]]
proof=/tmp/muniment-updater-install-proof
mkdir -p "$proof"
cp test/e2e/updater/{run-macos.sh,install.spec.mjs,baseline-app.sh,verify-macos.py} "$proof/"
clang -framework CoreGraphics -framework CoreFoundation test/e2e/support/macos-window-count.c -o "$proof/macos-window-count"
collect() {
  result=$?
  mkdir -p /tmp/dci-artifacts
  cp -R "$proof/raw" /tmp/dci-artifacts/ 2>/dev/null || true
  cp "$proof/result.json" /tmp/dci-artifacts/ 2>/dev/null || true
  exit "$result"
}
trap collect EXIT
curl --fail --silent --show-error "$MUNIMENT_BASELINE_URL" -o /tmp/update-baseline.tar.gz
curl --fail --location --silent --show-error https://github.com/munimentai/muniment/releases/download/v0.0.1/nightly-84e8c1b74dc2030449dc91de890691627f659ce9-macos-muniment.app.tar.gz -o /tmp/update-target.tar.gz
MUNIMENT_UPDATE_PROOF_DISPOSABLE=1 bash "$proof/run-macos.sh" /tmp/update-baseline.tar.gz /tmp/update-target.tar.gz aea8c2cdbf99b3920f36765b1a2184fbff61e10aef94f6861070cc89136593b1 0.0.1
