# The fleet test's runtime image: the runtime image with the sidecar test
# stub in place of Pi's executable, so runs answer without a model. The
# runtime checks Pi's pinned archive, not the executable, so the stub runs
# where Pi would. PI_RESUME_STUB_REPLY sets the reply text.
#
#   docker build -f scripts/fleet-e2e/stub.Containerfile \
#       --build-arg RUNTIME_IMAGE=muniment-runtime -t muniment-runtime-stub .

ARG RUST_IMAGE=rust:1-slim-trixie
ARG RUNTIME_IMAGE=muniment-runtime

FROM ${RUST_IMAGE} AS stub
WORKDIR /src
COPY . .
RUN CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p muniment-core --bin sidecar-test-stub

FROM ${RUNTIME_IMAGE}
COPY --from=stub --chown=65532:65532 /src/target/debug/sidecar-test-stub /opt/muniment/stub
USER 0:0
RUN pi="$(readlink -f /opt/muniment/bin/pi)" \
    && install -o 65532 -g 65532 -m 0755 /opt/muniment/stub "$pi" \
    && rm /opt/muniment/stub
USER 65532:65532
