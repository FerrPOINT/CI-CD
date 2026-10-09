#!/usr/bin/env bash
set -euo pipefail
cd /work/CI-CD/backend
case "$1" in
  workspace)
    umask 022
    echo FROZEN_WORKSPACE_UMASK=0022
    cargo fmt --all -- --check
    echo FROZEN_WORKSPACE_RUSTFMT:PASS
    cargo test --locked --offline --workspace -- --test-threads=1
    ;;
  integration)
    cargo test --locked --offline -p cicd-server --features integration --test integration_db -- --test-threads=1 --nocapture
    ;;
  cli)
    cargo test --locked --offline -p cicd-cli --features integration --test cli_real_api -- --test-threads=1 --nocapture
    cargo clippy --locked --offline -p cicd-cli --all-targets --features integration -- -D warnings
    echo CLI_INTEGRATION_CLIPPY:PASS
    ;;
  openapi)
    cargo run --locked --offline --bin openapi-dump -- /output/openapi-export.yaml
    diff --strip-trailing-cr /output/openapi-export.yaml /work/CI-CD/openapi/openapi.yaml
    echo OPENAPI_EXPORTER_EQUALITY_STRIP_TRAILING_CR:PASS
    ;;
  release)
    cargo build --locked --offline --release --workspace
    echo RELEASE_WORKSPACE_BUILD:PASS
    ;;
  *) exit 2 ;;
esac
