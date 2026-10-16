#!/usr/bin/env bash
# Build the runtime image for a release tag and push it, as `<version>` and
# `latest`, to each image repository in RUNTIME_IMAGE_REPOSITORIES (a space
# list such as `ghcr.io/owner/muniment-runtime`).
#
# The image covers each platform in RUNTIME_IMAGE_PLATFORMS, by default
# linux/amd64 and linux/arm64. For more than one platform, buildx builds each
# from the release's own signed server archive for that architecture, which
# this script downloads from RUNTIME_RELEASE_FEED and checks against
# SHA256SUMS and, when minisign is installed, against the release public key.
# It needs buildx and an emulator for every other platform. Without them, or
# without the archives, it builds and pushes the runner's own platform from
# source and warns.
#
# RUNTIME_IMAGE_LOGINS holds one `<registry> <user> <password>` line per
# registry. A repository whose registry has no login is skipped with a
# warning, so a registry can be added before its credential exists. The logins
# live in a throwaway Docker config, so none stays on the runner.
#
# Usage: deploy/runtime/publish.sh <vX.Y.Z> [revision]
set -euo pipefail

tag=$1
revision=${2:-$(git rev-parse HEAD)}
if [[ ! $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  echo "publish.sh: $tag is not a vX.Y.Z tag" >&2
  exit 2
fi
version=${BASH_REMATCH[1]}
read -r -a repositories <<<"${RUNTIME_IMAGE_REPOSITORIES:-}"
if [ ${#repositories[@]} -eq 0 ]; then
  echo "publish.sh: RUNTIME_IMAGE_REPOSITORIES names no repository" >&2
  exit 2
fi
context=$(cd "$(dirname "$0")/../.." && pwd)
local_image="muniment-runtime:publish-$$"
platforms=${RUNTIME_IMAGE_PLATFORMS:-linux/amd64,linux/arm64}
feed=${RUNTIME_RELEASE_FEED:-https://github.com/munimentai/muniment-core/releases/download}
builder="muniment-publish-$$"

DOCKER_CONFIG=$(mktemp -d)
export DOCKER_CONFIG
release=$(mktemp -d)
# A config with no auth entry makes docker pick the platform credential store,
# which outlives this directory. An entry keeps the logins in this file.
printf '{"auths":{"publish.invalid":{}}}\n' >"$DOCKER_CONFIG/config.json"
cleanup() {
  docker buildx rm "$builder" >/dev/null 2>&1 || true
  rm -rf "$DOCKER_CONFIG" "$release"
  docker image rm "$local_image" >/dev/null 2>&1 || true
}
trap cleanup EXIT

declare -A logged_in=()
while read -r registry user password; do
  [ -n "${registry:-}" ] || continue
  if [ -z "${user:-}" ] || [ -z "${password:-}" ]; then
    echo "publish.sh: the login line for $registry lacks a user or password" >&2
    exit 2
  fi
  printf '%s' "$password" | docker login "$registry" --username "$user" --password-stdin >/dev/null
  logged_in[$registry]=1
done <<<"${RUNTIME_IMAGE_LOGINS:-}"

targets=()
for repository in "${repositories[@]}"; do
  registry=${repository%%/*}
  if [ -n "${logged_in[$registry]:-}" ]; then
    targets+=("$repository")
  else
    echo "::warning::No login for $registry, so $repository is skipped."
  fi
done
if [ ${#targets[@]} -eq 0 ]; then
  echo "publish.sh: no repository has a login, so nothing is pushed" >&2
  exit 1
fi

# Downloads the release's server archives and checks them.
fetch_release() {
  local names=(SHA256SUMS SHA256SUMS.minisig)
  for arch in x86_64 aarch64; do
    names+=("muniment-server-$version-linux-$arch.tar.gz")
  done
  for name in "${names[@]}"; do
    curl --proto '=https,file' -sSfL -o "$release/$name" "$feed/$tag/$name" || return 1
  done
  (cd "$release" && sha256sum --check --strict --quiet SHA256SUMS) || return 1
  local key
  key=$(sed -n 's/^pub const RELEASE_PUBLIC_KEY: &str = "\(.*\)";$/\1/p' \
    "$context/crates/server/src/release.rs")
  if command -v minisign >/dev/null && [ -n "$key" ]; then
    minisign -V -q -P "$key" -m "$release/SHA256SUMS" -x "$release/SHA256SUMS.minisig" || return 1
  else
    echo "::warning::minisign or the release public key is missing, so only SHA256SUMS checks the archives."
  fi
}

# Whether buildx can build every platform in the list.
buildx_ready() {
  docker buildx create --name "$builder" --driver docker-container >/dev/null 2>&1 || return 1
  local supported
  supported=$(docker buildx inspect --bootstrap "$builder" 2>/dev/null | sed -n 's/^Platforms: *//p')
  local platform
  for platform in ${platforms//,/ }; do
    [[ ", $supported," == *", $platform,"* || ", $supported," == *", $platform*,"* ]] || return 1
  done
}

if [[ $platforms == *,* ]]; then
  if ! buildx_ready; then
    echo "::warning::buildx cannot build $platforms here, so only the runner's platform is published."
  elif ! fetch_release; then
    echo "::warning::The server archives of $tag could not be fetched and checked, so only the runner's platform is published."
  else
    names=()
    for repository in "${targets[@]}"; do
      names+=(-t "$repository:$version" -t "$repository:latest")
    done
    docker buildx build --builder "$builder" --platform "$platforms" \
      -f "$context/deploy/runtime/Containerfile" \
      --build-arg SERVER_STAGE=release-archive \
      --build-arg "MUNIMENT_RUNTIME_VERSION=$version" \
      --build-context "server-release=$release" \
      --label "org.opencontainers.image.version=$version" \
      --label "org.opencontainers.image.revision=$revision" \
      "${names[@]}" --push "$context"
    for repository in "${targets[@]}"; do
      echo "Pushed $repository:$version and :latest for $platforms."
    done
    exit 0
  fi
fi

docker build -f "$context/deploy/runtime/Containerfile" \
  --build-arg "MUNIMENT_RUNTIME_VERSION=$version" \
  --label "org.opencontainers.image.version=$version" \
  --label "org.opencontainers.image.revision=$revision" \
  -t "$local_image" "$context"

for repository in "${targets[@]}"; do
  for name in "$repository:$version" "$repository:latest"; do
    docker tag "$local_image" "$name"
    docker push "$name"
    docker image rm "$name" >/dev/null
  done
  echo "Pushed $repository:$version and :latest."
done
