# syntax=docker/dockerfile:1.27.1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
ARG OCI_TOOLS_IMAGE=sdlc-build-forge-oci-tools:20261008
FROM docker:27.5.1-dind-rootless AS client
ARG PDLC_RUN_SLOT_SHARING=locked
FROM python:3.12-bookworm AS python
ARG PDLC_RUN_SLOT_SHARING=locked
FROM ${OCI_TOOLS_IMAGE}
ARG PDLC_RUN_SLOT_SHARING=locked
COPY --from=client /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/libexec/docker/cli-plugins/docker-compose
COPY --from=python /usr/local/bin/python3.12 /usr/local/bin/python3
COPY --from=python /usr/local/lib/libpython3.12.so.1.0 /usr/local/lib/libpython3.12.so.1.0
COPY --from=python /usr/local/lib/python3.12 /usr/local/lib/python3.12
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} ldconfig && python3 --version
