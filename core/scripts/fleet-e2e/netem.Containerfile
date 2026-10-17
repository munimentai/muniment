# A helper with `tc`, run in a fleet container's network namespace to add
# delay and loss with netem.
ARG BASE_IMAGE=debian:trixie-slim
FROM ${BASE_IMAGE}
RUN apt-get update \
    && apt-get install -y --no-install-recommends iproute2 \
    && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["tc"]
