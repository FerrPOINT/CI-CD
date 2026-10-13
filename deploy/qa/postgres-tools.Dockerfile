ARG OCI_TOOLS_IMAGE=sdlc-build-forge-oci-tools:20261008
FROM docker:27.5.1-dind-rootless AS client
FROM python:3.12-bookworm AS python
FROM ${OCI_TOOLS_IMAGE}
COPY --from=client /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/libexec/docker/cli-plugins/docker-compose
COPY --from=python /usr/local/bin/python3.12 /usr/local/bin/python3
COPY --from=python /usr/local/lib/libpython3.12.so.1.0 /usr/local/lib/libpython3.12.so.1.0
COPY --from=python /usr/local/lib/python3.12 /usr/local/lib/python3.12
RUN ldconfig && python3 --version
