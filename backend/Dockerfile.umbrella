# syntax=docker/dockerfile:1.27.1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# Workspace-root context with original sibling SDK.
FROM rust:1.88.0-slim-bookworm AS manifests
ARG PDLC_RUN_SLOT_SHARING=locked
ARG TARGETARCH
ARG CARGO_REGISTRY_CACHE_ID=pdlc-operator-downloads-v1-cargo188
ARG CARGO_BUILD_JOBS=2
WORKDIR /app
# backend/Cargo.toml: sdlc-auth-core = { path = "../../../services-base/..." }
# /app/../../services-base == /services-base (WORKDIR is repo backend/).
COPY services-base/Cargo.toml /services-base/Cargo.toml
COPY services-base/crates/sdlc-shared /services-base/crates/sdlc-shared
COPY services-base/crates/sdlc-auth-core /services-base/crates/sdlc-auth-core
# Unselected CLI dependency: preserve its manifest and reject accidental compilation.
COPY services-base/crates/sdlc-cli-core/Cargo.toml /services-base/crates/sdlc-cli-core/Cargo.toml
COPY services-base/crates/sdlc-telemetry /services-base/crates/sdlc-telemetry
COPY CI-CD/backend/Cargo.toml CI-CD/backend/Cargo.lock* ./
COPY CI-CD/backend/domain/Cargo.toml domain/Cargo.toml
# Unselected standalone CLI: retain workspace metadata and reject accidental compilation.
COPY CI-CD/backend/cli/Cargo.toml cli/Cargo.toml
COPY CI-CD/backend/migration/Cargo.toml migration/Cargo.toml
COPY CI-CD/backend/app/Cargo.toml app/Cargo.toml
COPY CI-CD/backend/infra/Cargo.toml infra/Cargo.toml
COPY CI-CD/backend/api/Cargo.toml api/Cargo.toml
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} \
    mkdir -p /services-base/crates/sdlc-cli-core/src \
    && printf '%s\n' 'compile_error!("metadata-only sdlc-cli-core must not be compiled");' > /services-base/crates/sdlc-cli-core/src/lib.rs \
    && mkdir -p src \
    && printf '%s\n' '' > src/lib.rs \
    && mkdir -p src \
    && printf '%s\n' 'fn main() {}' > src/main.rs \
    && mkdir -p src/bin \
    && printf '%s\n' 'fn main() {}' > src/bin/openapi-dump.rs \
    && mkdir -p src/bin \
    && printf '%s\n' 'fn main() {}' > src/bin/forge-runner.rs \
    && mkdir -p domain/src \
    && printf '%s\n' '' > domain/src/lib.rs \
    && printf '%s\n' 'fn main() {}' > domain/src/main.rs \
    && mkdir -p cli/src \
    && mkdir -p cli/src \
    && printf '%s\n' 'compile_error!("metadata-only cicd-cli must not be compiled");' > cli/src/main.rs \
    && mkdir -p migration/src \
    && printf '%s\n' '' > migration/src/lib.rs \
    && mkdir -p migration/src \
    && printf '%s\n' 'fn main() {}' > migration/src/main.rs \
    && mkdir -p app/src \
    && printf '%s\n' '' > app/src/lib.rs \
    && printf '%s\n' 'fn main() {}' > app/src/main.rs \
    && mkdir -p infra/src \
    && printf '%s\n' '' > infra/src/lib.rs \
    && printf '%s\n' 'fn main() {}' > infra/src/main.rs \
    && mkdir -p api/src \
    && printf '%s\n' '' > api/src/lib.rs \
    && printf '%s\n' 'fn main() {}' > api/src/main.rs

FROM manifests AS unit-dependencies
ARG PDLC_RUN_SLOT_SHARING=locked
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} --mount=type=cache,target=/usr/local/cargo/registry,id=${CARGO_REGISTRY_CACHE_ID}-${TARGETARCH},sharing=locked \
    cargo test --locked --release -p cicd-domain --lib --no-run \
    && cargo clean --release -p cicd-server -p cicd-domain -p cicd-cli -p cicd-migrate -p cicd-app -p cicd-infra -p cicd-api

FROM unit-dependencies AS dependencies
ARG PDLC_RUN_SLOT_SHARING=locked
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} --mount=type=cache,target=/usr/local/cargo/registry,id=${CARGO_REGISTRY_CACHE_ID}-${TARGETARCH},sharing=locked \
    cargo build --locked --release -p cicd-server --bin cicd-server --bin forge-runner \
    && cargo clean --release -p cicd-server -p cicd-domain -p cicd-cli -p cicd-migrate -p cicd-app -p cicd-infra -p cicd-api

FROM unit-dependencies AS unit
ARG PDLC_RUN_SLOT_SHARING=locked
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} rm -rf -- domain/src
COPY CI-CD/backend/domain ./domain
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} --mount=type=cache,target=/usr/local/cargo/registry,id=${CARGO_REGISTRY_CACHE_ID}-${TARGETARCH},sharing=locked cargo test --locked --release -p cicd-domain --lib && printf 'passed\n' > /tmp/rust-unit-check

FROM dependencies AS build
ARG PDLC_RUN_SLOT_SHARING=locked
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} --mount=type=bind,from=unit,source=/tmp/rust-unit-check,target=/tmp/rust-unit-check test "$(cat /tmp/rust-unit-check)" = passed
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} rm -rf -- src domain/src app/src infra/src api/src
COPY --exclude=**/src/bin/tests/forge_runner.rs CI-CD/backend/src ./src
COPY CI-CD/backend/domain ./domain
COPY CI-CD/backend/app ./app
COPY CI-CD/backend/infra ./infra
COPY CI-CD/backend/api ./api
COPY CI-CD/backend/migrations ./migrations
RUN --mount=type=cache,id=pdlc-host-run-slot-v1,target=/.__pdlc_build_slot__,sharing=${PDLC_RUN_SLOT_SHARING} --mount=type=cache,target=/usr/local/cargo/registry,id=${CARGO_REGISTRY_CACHE_ID}-${TARGETARCH},sharing=locked cargo build --locked --release -p cicd-server --bin cicd-server --bin forge-runner

FROM debian:bookworm-slim
# docker-cli only (DooD): embedded runner spawns job containers through the
# mounted host docker socket; no daemon runs inside this image. The static
# official CLI is pinned because bookworm's docker.io is too old for the
# host daemon API (>=1.44).
ARG DOCKER_CLI_VERSION=29.5.0
RUN apt-get update && apt-get install -y --no-install-recommends curl git git-lfs unzip xz-utils ca-certificates && rm -rf /var/lib/apt/lists/* \
 && curl -fsSL https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_CLI_VERSION}.tgz \
  | tar -xz --strip-components=1 -C /usr/local/bin docker/docker \
 && useradd --create-home --uid 10001 cicd

WORKDIR /app
COPY --from=build /app/target/release/cicd-server /usr/local/bin/cicd-server
COPY --from=build /app/target/release/forge-runner /usr/local/bin/forge-runner
COPY --from=build /app/migrations /app/migrations
RUN mkdir -p /var/lib/forge/git /var/lib/forge/artifacts /workspaces \
 && chown -R cicd:cicd /var/lib/forge /workspaces
USER cicd
EXPOSE 22801
CMD ["/usr/local/bin/cicd-server"]
