#!/usr/bin/env bash
# Runs the Docker fleet test: three runtime containers and the runtime of
# the test process, linked as one fleet of full members. The test
# (crates/runtime/tests/fleet_docker.rs) offloads a thread, syncs settings,
# preferences and memory, shares a model connection, partitions a container
# with `docker network disconnect`, adds delay and loss with `tc netem`,
# queues a post for a stopped container and revokes a member.
#
# Usage: scripts/fleet-e2e.sh [--keep-images]
#
# The script builds the runtime image (deploy/runtime/Containerfile), the
# test layer with the sidecar stub in place of Pi
# (scripts/fleet-e2e/stub.Containerfile) and a `tc` helper, and removes
# every container, volume, network and, unless --keep-images, image it
# made when it exits.
#
# Environment:
#   MUNIMENT_FLEET_E2E_RUNTIME_IMAGE  a runtime image to test instead of
#                                     building one; it stays after the run
#   MUNIMENT_FLEET_E2E_TIMEOUT        seconds each convergence may take (240)
#   MUNIMENT_FLEET_E2E_CLOUD_DIR      a cloud API checkout with its packages
#                                     installed: turns on the phone step,
#                                     which runs the API with its own
#                                     Postgres and Redis containers and the
#                                     Node probe
#   MUNIMENT_FLEET_E2E_CLOUD_PORT     the API's port for the phone step (58788)
#   CARGO_TARGET_DIR                  the test build's target directory
set -euo pipefail

keep_images=0
for argument in "$@"; do
  case "$argument" in
    --keep-images) keep_images=1 ;;
    *) echo "usage: $0 [--keep-images]" >&2; exit 2 ;;
  esac
done

core=$(cd "$(dirname "$0")/.." && pwd)
prefix="muniment-fleet-e2e-$$"
network="$prefix-net"
runtime_image=${MUNIMENT_FLEET_E2E_RUNTIME_IMAGE:-}
stub_image=muniment-fleet-e2e-stub
netem_image=muniment-fleet-e2e-netem
built_images=("$stub_image" "$netem_image")
containers=("$prefix-c1" "$prefix-c2" "$prefix-c3")
volumes=()
cloud_pid=
postgres="$prefix-pg"
redis="$prefix-redis"
work=$(mktemp -d "${TMPDIR:-/tmp}/muniment-fleet-e2e.XXXXXX")

say() { printf 'fleet-e2e: %s\n' "$*" >&2; }

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    for container in "${containers[@]}"; do
      if docker inspect "$container" >/dev/null 2>&1; then
        say "last log lines of $container:"
        docker logs --tail 60 "$container" 2>&1 | sed 's/^/  /' >&2 || true
      fi
    done
    [ -f "$work/cloud.log" ] && { say "last cloud API log lines:"; tail -n 30 "$work/cloud.log" >&2; }
  fi
  [ -n "$cloud_pid" ] && kill "$cloud_pid" 2>/dev/null && wait "$cloud_pid" 2>/dev/null
  docker rm -f -v "${containers[@]}" "$postgres" "$redis" >/dev/null 2>&1 || true
  [ "${#volumes[@]}" -gt 0 ] && docker volume rm "${volumes[@]}" >/dev/null 2>&1
  docker network rm "$network" >/dev/null 2>&1 || true
  if [ "$keep_images" -eq 0 ]; then
    docker image rm "${built_images[@]}" >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
  say "cleaned up (exit $status)"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

if [ -z "$runtime_image" ]; then
  runtime_image=muniment-fleet-e2e-runtime
  built_images+=("$runtime_image")
  say "building the runtime image"
  docker build -q -f "$core/deploy/runtime/Containerfile" -t "$runtime_image" "$core" >/dev/null
fi
say "building the stub layer and the tc helper"
docker build -q -f "$core/scripts/fleet-e2e/stub.Containerfile" \
  --build-arg RUNTIME_IMAGE="$runtime_image" -t "$stub_image" "$core" >/dev/null
docker build -q -f "$core/scripts/fleet-e2e/netem.Containerfile" \
  -t "$netem_image" "$core/scripts/fleet-e2e" >/dev/null

# Three free loopback ports for the containers' remote access. Each
# container advertises its Docker name for the others and its published
# port for this machine.
read -r -a ports <<<"$(python3 -c '
import socket
sockets = [socket.socket() for _ in range(3)]
for s in sockets:
    s.bind(("127.0.0.1", 0))
print(" ".join(str(s.getsockname()[1]) for s in sockets))
')"

docker network create "$network" >/dev/null
for index in 0 1 2; do
  name=${containers[$index]}
  volume="$name-data"
  volumes+=("$volume")
  docker volume create "$volume" >/dev/null
  docker run -d --name "$name" --network "$network" \
    --add-host host.docker.internal:host-gateway \
    -p "127.0.0.1:${ports[$index]}:7443" -v "$volume:/data" \
    -e MUNIMENT_REMOTE_ENDPOINTS="$name:7443,127.0.0.1:${ports[$index]}" \
    -e MUNIMENT_REMOTE_NAME="fleet-c$((index + 1))" \
    -e PI_RESUME_STUB_REPLY=" reply from fleet-c$((index + 1))" \
    --health-interval 2s --health-start-period 60s "$stub_image" >/dev/null
done
for name in "${containers[@]}"; do
  for _ in $(seq 120); do
    [ "$(docker inspect --format '{{.State.Health.Status}}' "$name")" = healthy ] && break
    sleep 1
  done
  [ "$(docker inspect --format '{{.State.Health.Status}}' "$name")" = healthy ] \
    || { say "$name did not turn healthy"; exit 1; }
done
say "containers healthy: ${containers[*]} on ports ${ports[*]}"

# On Docker Desktop the host gateway reaches loopback listeners; on Linux it
# is the bridge address, so the test runtime listens on every address there.
listen=127.0.0.1:0
[ "$(uname -s)" = Linux ] && listen=0.0.0.0:0
export MUNIMENT_FLEET_E2E=1
export MUNIMENT_FLEET_E2E_CONTAINERS
MUNIMENT_FLEET_E2E_CONTAINERS=$(IFS=,; echo "${containers[*]}")
export MUNIMENT_FLEET_E2E_NETWORK="$network"
export MUNIMENT_FLEET_E2E_NETEM_IMAGE="$netem_image"
export MUNIMENT_FLEET_E2E_HOST_LISTEN="${MUNIMENT_FLEET_E2E_HOST_LISTEN:-$listen}"
export MUNIMENT_FLEET_E2E_HOST_ENDPOINT=host.docker.internal

if [ -n "${MUNIMENT_FLEET_E2E_CLOUD_DIR:-}" ]; then
  api="$MUNIMENT_FLEET_E2E_CLOUD_DIR/api"
  cloud_port=${MUNIMENT_FLEET_E2E_CLOUD_PORT:-58788}
  database_port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
  password="fleet-e2e-$$-database"
  docker run -d --name "$postgres" -e POSTGRES_PASSWORD="$password" -e POSTGRES_DB=muniment \
    -p "127.0.0.1:$database_port:5432" postgres:17 >/dev/null
  redis_port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
  docker run -d --name "$redis" -p "127.0.0.1:$redis_port:6379" redis:7-alpine >/dev/null
  for _ in $(seq 60); do
    docker exec "$postgres" pg_isready -U postgres -d muniment >/dev/null 2>&1 && break
    sleep 1
  done
  export DATABASE_URL="postgres://postgres:$password@127.0.0.1:$database_port/muniment"
  email=owner@example.net
  account_password="fleet-e2e-$$-account"
  (cd "$api" && npx tsx scripts/prestart.ts && SEED_USER_EMAIL="$email" \
    SEED_USER_PASSWORD="$account_password" npx tsx scripts/db-seed.ts) >"$work/cloud.log" 2>&1
  # The probe signs in as a desktop client from a loopback redirect.
  (cd "$api" && PORT="$cloud_port" SERVE_SURFACES=api REDIS_URL="redis://127.0.0.1:$redis_port" \
    NATIVE_CLIENT_REGISTRATIONS='[{"client_id":"muniment-desktop","platforms":["desktop"],"redirect_uris":["http://127.0.0.1:49152/callback"],"client_role":"desktop"}]' \
    exec node --import tsx src/server.ts) >>"$work/cloud.log" 2>&1 &
  cloud_pid=$!
  for _ in $(seq 60); do
    curl -fsS "http://127.0.0.1:$cloud_port/healthz" >/dev/null 2>&1 && break
    sleep 1
  done
  curl -fsS "http://127.0.0.1:$cloud_port/healthz" >/dev/null \
    || { say "the cloud API did not answer on $cloud_port"; exit 1; }
  unset DATABASE_URL
  export MUNIMENT_FLEET_E2E_CLOUD_API="http://127.0.0.1:$cloud_port"
  export MUNIMENT_FLEET_E2E_CLOUD_EMAIL="$email"
  export MUNIMENT_FLEET_E2E_CLOUD_PASSWORD="$account_password"
  export MUNIMENT_FLEET_E2E_PHONE_PROBE="$core/scripts/fleet-e2e/phone-probe.mjs"
  say "cloud API on $cloud_port with Postgres on $database_port"
fi

say "running the fleet test"
cd "$core"
cargo test --locked -p muniment-runtime-core --no-default-features --test fleet_docker \
  -- --ignored --nocapture --test-threads=1
