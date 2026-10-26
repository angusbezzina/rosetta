FROM ubuntu:24.04

# Image preparation may download packages; the smoke process runs separately
# with Docker --network none and a read-only mount of the checked-out source.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends git curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
