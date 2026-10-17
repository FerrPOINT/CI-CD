# syntax=docker/dockerfile:1.27.1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
FROM docker:27.5.1-dind-rootless AS client
ARG PDLC_RUN_SLOT_SHARING=locked
FROM rust:1.88.0-bookworm
ARG PDLC_RUN_SLOT_SHARING=locked
COPY --from=client /usr/local/bin/docker /usr/local/bin/docker
COPY --from=client /usr/local/libexec/docker/cli-plugins/docker-buildx /usr/local/libexec/docker/cli-plugins/docker-buildx
COPY --from=client /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/bin/docker-compose
