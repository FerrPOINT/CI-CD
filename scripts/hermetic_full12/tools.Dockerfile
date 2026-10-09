FROM python:3.12-bookworm@sha256:e91fec3d1ac69f04e4eddcd29c327e630ce34658cf31075bfa7e8b0e052bafea AS python
FROM rust:1.88.0@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0
RUN rustup component add --toolchain 1.88.0 rustfmt clippy llvm-tools-preview
COPY --from=python /usr/local/bin/python3.12 /usr/local/bin/python3
COPY --from=python /usr/local/lib/libpython3.12.so.1.0 /usr/local/lib/libpython3.12.so.1.0
COPY --from=python /usr/local/lib/python3.12 /usr/local/lib/python3.12
COPY docker docker-compose /usr/local/bin/
COPY docker-compose docker-buildx /usr/local/lib/docker/cli-plugins/
RUN ldconfig && python3 --version && rustc --version && cargo fmt --version && cargo clippy --version && docker --version && docker-compose --version && docker buildx version && git --version
