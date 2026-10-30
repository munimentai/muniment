#!/usr/bin/env bash
# Classify a diff between BASE and HEAD into the CI lane flags.
# Prints the ten key=value lines. The workflow step appends them to
# GITHUB_OUTPUT. A pull request and a push with a known base classify their
# diff. A dispatch, a new branch or a base outside the clone runs every lane.
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: scripts/ci/classify.sh BASE HEAD" >&2
  exit 2
fi

BASE_SHA=$1
HEAD_SHA=$2

docs_only=true
companion=false
attach_fixtures=false
code_diff_fixtures=false
desktop=false
core_shared=false
core_support=false
installer=false
release_tooling=false
release_only=true
# A pull request and a push with a known base classify their diff. A
# dispatch, a new branch or a base outside the clone runs every lane.
classify=true
case "${GITHUB_EVENT_NAME:-pull_request}" in
  pull_request) ;;
  push)
    if [ -z "${BASE_SHA//0/}" ] || ! git cat-file -e "$BASE_SHA^{commit}" >/dev/null 2>&1; then
      classify=false
    fi
    ;;
  *) classify=false ;;
esac
if [ "$classify" != true ]; then
  docs_only=false
  companion=true
  attach_fixtures=true
  code_diff_fixtures=true
  desktop=true
  core_shared=true
  core_support=true
  installer=true
  release_only=false
fi
while IFS= read -r path; do
  # Only these standalone helpers bypass native checks. Unknown files
  # and signing or packaging helpers keep the full native gate.
  case "$path" in
    .github/lib/release-promotion.mjs|.github/lib/release-promotion.test.js|.github/lib/update-feed.mjs|.github/lib/update-feed.test.js|.github/lib/winget-manifest.mjs|.github/lib/winget-manifest.test.js|.github/lib/winget-release.mjs)
      docs_only=false
      release_tooling=true
      continue
      ;;
  esac
  release_only=false
  case "$path" in
    core/docs/*|core/docs/**|core/deploy/*|core/deploy/**|core/scripts/*|core/scripts/**|core/ci/*|core/ci/**|core/test_support/*|core/test_support/**|core/Cargo.lock|.forgejo/workflows/core*.yml)
      # Before *.md, so a core document is never documentation-only:
      # core/docs/core-boundary.md is an input of the boundary check.
      # No desktop build reads these, but core.yml does not run for a
      # push made with the job token, so the core workspace runs here.
      docs_only=false
      core_support=true
      ;;
    *.md) ;;
    src-tauri/cli/*|src-tauri/cli/**|src-tauri/runtime/*|src-tauri/runtime/**)
      docs_only=false
      companion=true
      ;;
    src-tauri/reader/*|src-tauri/reader/**)
      # The Go reader sidecar ships beside the runtime in every bundle.
      docs_only=false
      companion=true
      desktop=true
      ;;
    core/protocol-fixtures/code-diff/*|core/protocol-fixtures/code-diff/**)
      docs_only=false
      companion=true
      core_shared=true
      code_diff_fixtures=true
      ;;
    core/protocol-fixtures/*|core/protocol-fixtures/**)
      docs_only=false
      companion=true
      core_shared=true
      attach_fixtures=true
      ;;
    core/crates/*|core/crates/**|core/Cargo.toml|core/pins/*|core/pins/**|core/third-party/*|core/third-party/**)
      # The desktop builds these shared crates by path. They get the
      # Linux checks without the three-platform preflight.
      docs_only=false
      companion=true
      core_shared=true
      ;;
    src-tauri/integration/*|src-tauri/integration/**)
      docs_only=false
      companion=true
      desktop=true
      ;;
    src-tauri/acp/*|src-tauri/acp/**)
      docs_only=false
      companion=true
      ;;
    src-tauri/Cargo.toml|src-tauri/Cargo.lock)
      docs_only=false
      companion=true
      desktop=true
      attach_fixtures=true
      code_diff_fixtures=true
      ;;
    *)
      docs_only=false
      desktop=true
      ;;
  esac
  case "$path" in
    src-tauri/tauri*.conf.json|src-tauri/windows/*|src-tauri/packaging/*|src-tauri/capabilities/*|.github/build-*.mjs|.github/build-*.sh|.github/lib/*|.github/upload-nightly-assets.mjs|.github/workflows/ci.yml|.forgejo/workflows/ci.yml|test/e2e/*|test/windows-installers.ps1)
      # The installer, packaging, workflow and e2e paths are the ones
      # the nightly cannot prove one merge later without a red run,
      # so only these run the three-platform build on the PR.
      installer=true
      ;;
  esac
done < <(if [ "$classify" = true ]; then git diff --name-only --diff-filter=ACDMRT "$BASE_SHA" "$HEAD_SHA"; fi)
[ "$release_tooling" = true ] || release_only=false
# The workflow step appends these lines to GITHUB_OUTPUT. A direct call prints
# the same lines, so either path yields one copy of each key.
echo "release_tooling=$release_tooling"
echo "release_only=$release_only"
echo "docs_only=$docs_only"
echo "companion=$companion"
echo "attach_fixtures=$attach_fixtures"
echo "code_diff_fixtures=$code_diff_fixtures"
echo "desktop=$desktop"
echo "core_shared=$core_shared"
echo "core_support=$core_support"
echo "installer=$installer"
