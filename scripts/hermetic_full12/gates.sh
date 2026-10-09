#!/usr/bin/env bash
set -euo pipefail
cd /work/CI-CD/backend
case "$1" in
  python)
    cd /work/CI-CD
    python3 -B -m unittest scripts.tests.test_postgres_delivery scripts.tests.test_oci_offline_fixture
    echo 'FROZEN_LINUX_PYTHON_UNITS:PASS'
    ;;
  check)
    cargo check --locked --offline --workspace --all-targets --features integration,postgres-integration,oci-integration
    echo 'LOCKED_INTEGRATION_FEATURES_CHECK:PASS'
    ;;
  clippy)
    cargo clippy --locked --offline --workspace --all-targets --features integration,postgres-integration,oci-integration -- -D warnings
    echo 'LOCKED_INTEGRATION_FEATURES_CLIPPY:PASS'
    ;;
  postgres)
    cargo build --locked --offline --bins --example postgres_target
    cargo test --locked --offline -p cicd-server --features postgres-integration --test integration_db sdlc_pg_delivery -- --test-threads=1 --nocapture
    ;;
  oci)
    cargo test --locked --offline -p cicd-server --features oci-integration --test integration_db sdlc_oci_delivery -- --test-threads=1 --nocapture
    ;;
  *) exit 2 ;;
esac
