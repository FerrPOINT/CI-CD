#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${SDLC_WORKSPACE_DIR:?Select the source workspace}"
: "${SDLC_DOCKER_CONTEXT:?Select the Docker context}"
: "${SDLC_TASK:?Set the task owner}"
exec python3 "$SDLC_WORKSPACE_DIR/services-base/scripts/compose_probe.py" \
  --docker-context "$SDLC_DOCKER_CONTEXT" --task "$SDLC_TASK" --purpose cicd-rust-gate -- \
  --rm --entrypoint /bin/bash -v "$ROOT:/sources/CI-CD:ro" \
  -v "$SDLC_WORKSPACE_DIR/services-base:/sources/services-base:ro" \
  --workdir /sources/CI-CD/backend rust:1.88.0-bookworm -ceu \
  'rustup component add rustfmt clippy; export CARGO_TARGET_DIR=/tmp/target; cargo fmt --all -- --check; cargo clippy --locked --workspace --all-targets -- -D warnings; cargo test --locked --workspace'
