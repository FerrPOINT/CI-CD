# syntax=docker/dockerfile:1.27.1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# Umbrella build for the SDLC fleet root compose (context = /opt/dev/sdlc).
# Stages the sibling services-base checkout so the pdlc-auth-core path
# dependency (../../../services-base/...) resolves.
FROM rust:1.88.0-slim-bookworm AS build
ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
WORKDIR /app
# backend/Cargo.toml: pdlc-auth-core = { path = "../../../services-base/..." }
# /app/../../services-base == /services-base (WORKDIR is repo backend/).
COPY services-base/Cargo.toml /services-base/Cargo.toml
COPY services-base/base-modules/rust/pdlc-auth-core /services-base/base-modules/rust/pdlc-auth-core
COPY services-base/base-modules/rust/pdlc-shared /services-base/base-modules/rust/pdlc-shared
COPY services-base/base-modules/rust/pdlc-telemetry /services-base/base-modules/rust/pdlc-telemetry
# Keep unselected CLI dependency metadata without its implementation.
COPY services-base/base-modules/rust/pdlc-cli-core/Cargo.toml /services-base/base-modules/rust/pdlc-cli-core/Cargo.toml
COPY CI-CD/backend/Cargo.toml CI-CD/backend/Cargo.lock* ./
COPY CI-CD/backend/domain/Cargo.toml domain/Cargo.toml
COPY CI-CD/backend/app/Cargo.toml app/Cargo.toml
COPY CI-CD/backend/infra/Cargo.toml infra/Cargo.toml
COPY CI-CD/backend/api/Cargo.toml api/Cargo.toml
# Unselected standalone CLI: retain workspace metadata and reject accidental compilation.
COPY CI-CD/backend/cli/Cargo.toml cli/Cargo.toml
COPY CI-CD/backend/migration/Cargo.toml migration/Cargo.toml
RUN mkdir -p src/bin domain/src app/src infra/src api/src cli/src migration/src /services-base/base-modules/rust/pdlc-cli-core/src \
 && printf '%s\n' 'compile_error!("metadata-only pdlc-cli-core must not be compiled");' > /services-base/base-modules/rust/pdlc-cli-core/src/lib.rs \
 && printf 'fn main() {}\n' > src/main.rs && printf '' > src/lib.rs \
 && printf 'fn main() {}\n' > src/bin/openapi-dump.rs && printf 'fn main() {}\n' > src/bin/forge-runner.rs \
 && printf 'fn main() {}\n' > src/bin/forge-delivery.rs && printf 'fn main() {}\n' > src/bin/forge-pg-migrate.rs \
 && printf '\n' > domain/src/lib.rs \
 && printf '\n' > app/src/lib.rs \
 && printf '\n' > infra/src/lib.rs \
 && printf '\n' > api/src/lib.rs \
 && printf '%s\n' 'compile_error!("metadata-only cicd-cli must not be compiled");' > cli/src/main.rs \
 && printf 'fn main() {}\n' > migration/src/main.rs \
 && /usr/local/cargo/bin/cargo build --locked --release -p cicd-server --bin cicd-server --bin forge-runner \
 && /usr/local/cargo/bin/cargo clean -p cicd-server -p cicd-domain -p cicd-app -p cicd-infra -p cicd-api -p cicd-cli -p cicd-migrate
COPY --exclude=**/src/bin/tests/forge_runner.rs --exclude=**/src/bin/tests/forge_delivery.rs CI-CD/backend/src ./src
COPY CI-CD/backend/domain/src domain/src
COPY CI-CD/backend/app/src app/src
COPY CI-CD/backend/infra/src infra/src
COPY CI-CD/backend/api/src api/src
COPY CI-CD/backend/migration/src migration/src
COPY CI-CD/backend/migrations ./migrations
# Compile-time delivery witnesses must come from the same pinned source set.
COPY CI-CD/scripts/postgres-delivery.py /scripts/postgres-delivery.py
COPY CI-CD/scripts/workspace_deploy_admission.py CI-CD/scripts/workspace_deploy_adapter.py CI-CD/scripts/workspace_deploy_bridge.py /scripts/
COPY services-base/scripts /services-base/scripts
COPY services-base/base-operator/src /services-base/base-operator/src
RUN find src domain app infra api cli migration -name '*.rs' -exec touch {} + \
 && /usr/local/cargo/bin/cargo test --locked --release -p cicd-domain --lib \
 && /usr/local/cargo/bin/cargo build --locked --release -p cicd-server --bin cicd-server --bin forge-runner

FROM debian:bookworm-slim
# docker-cli only (DooD): embedded runner spawns job containers through the
# mounted host docker socket; no daemon runs inside this image. The static
# official CLI is pinned because bookworm's docker.io is too old for the
# host daemon API (>=1.44).
ARG DOCKER_CLI_VERSION=29.5.0
RUN apt-get update && apt-get install -y --no-install-recommends curl git git-lfs unzip xz-utils ca-certificates python3 && rm -rf /var/lib/apt/lists/* \
 && curl -fsSL https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_CLI_VERSION}.tgz \
  | tar -xz --strip-components=1 -C /usr/local/bin docker/docker \
 && useradd --create-home --uid 10001 cicd

# Official Compose v5.5.1 release checksums (not downloaded mutable checksums).
ARG TARGETARCH
RUN case "${TARGETARCH:-amd64}" in \
      amd64) compose_arch=x86_64; compose_sha=db1889184726840f75c4f9c001048430d4f25b3be3cb084d3ddd762bc0aed576 ;; \
      arm64) compose_arch=aarch64; compose_sha=732e3a84c1a0f67256ce80bc2598a24546b10ca05f9faa97efceb1171ece2ef7 ;; \
      *) exit 1 ;; esac \
 && mkdir -p /usr/local/lib/docker/cli-plugins \
 && curl -fsSL "https://github.com/docker/compose/releases/download/v5.5.1/docker-compose-linux-${compose_arch}" -o /tmp/docker-compose \
 && echo "${compose_sha}  /tmp/docker-compose" | sha256sum -c - \
 && install -m 0755 /tmp/docker-compose /usr/local/lib/docker/cli-plugins/docker-compose \
 && rm /tmp/docker-compose \
 && docker compose version

# Official Buildx v0.37.2 release checksums; daemon access stays in the backend.
ARG BUILDX_VERSION=0.37.2
RUN case "${TARGETARCH:-amd64}" in \
      amd64) buildx_arch=amd64; buildx_sha=982ca20490b45ed1ec8d99795974d3d874a358f75938c9c237305010e6b7e548 ;; \
      arm64) buildx_arch=arm64; buildx_sha=efa38cb7aa7db2dbb9ad049b00b0a9737f66f033626177b5a4e845184ad7ab29 ;; \
      *) exit 1 ;; esac \
 && curl -fsSL "https://github.com/docker/buildx/releases/download/v${BUILDX_VERSION}/buildx-v${BUILDX_VERSION}.linux-${buildx_arch}" -o /tmp/docker-buildx \
 && echo "${buildx_sha}  /tmp/docker-buildx" | sha256sum -c - \
 && install -m 0755 /tmp/docker-buildx /usr/local/lib/docker/cli-plugins/docker-buildx \
 && rm /tmp/docker-buildx \
 && docker buildx version

WORKDIR /app
COPY --from=build /app/target/release/cicd-server /usr/local/bin/cicd-server
COPY --from=build /app/target/release/forge-runner /usr/local/bin/forge-runner
COPY --from=build /app/migrations /app/migrations
RUN mkdir -p /var/lib/forge/git /var/lib/forge/artifacts /workspaces \
 && chown -R cicd:cicd /var/lib/forge /workspaces
USER cicd
EXPOSE 22801
CMD ["/usr/local/bin/cicd-server"]
