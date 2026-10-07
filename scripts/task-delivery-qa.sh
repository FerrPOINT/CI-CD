#!/usr/bin/env bash
set -euo pipefail
case "${CICD_QA_GATE:-full}" in full|delivery|oci) ;; *) echo 'Unknown QA gate' >&2; exit 1;; esac
cargo fmt --all --check
rustfmt --edition 2024 --check tests/helpers/manifest_target.rs
rustc --edition=2024 tests/helpers/manifest_target.rs -o /delivery-qa/manifest-target
touch /delivery-qa/ready
cargo check --locked --offline --workspace --all-targets --features integration
cargo clippy --locked --offline --workspace --all-targets --features integration -- -D warnings
if [ "${CICD_QA_GATE:-full}" = oci ]; then
  export DOCKER_HOST=unix:///var/run/docker.sock
  cargo clippy --locked --offline --workspace --all-targets --features oci-integration -- -D warnings
  cargo test --locked --offline -p cicd-server --features oci-integration --test integration_db sdlc_oci_delivery -- --test-threads=1 --nocapture
elif [ "${CICD_QA_GATE:-full}" = delivery ]; then
  cargo test --locked --offline -p cicd-server --features integration --test integration_db sdlc_task_delivery -- --test-threads=1 --nocapture
else
  cargo tree --locked --offline -i rsa --target all > /tmp/active-rsa.txt
  cargo tree --locked --offline -i sqlx-mysql --target all > /tmp/active-mysql.txt
  test ! -s /tmp/active-rsa.txt
  test ! -s /tmp/active-mysql.txt
  cargo test --locked --offline --workspace -- --test-threads=1
  cargo test --locked --offline -p cicd-server --features integration --test integration_db -- --test-threads=1
  cargo test --locked --offline -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
  cargo clippy --locked --offline -p cicd-cli --all-targets --features integration -- -D warnings
fi
cargo run --locked --offline --bin openapi-dump -- /output/openapi-delivery.yaml
diff --strip-trailing-cr /output/openapi-delivery.yaml /contract/openapi.yaml
if [ "${CICD_QA_GATE:-full}" = full ]; then cargo build --locked --offline --release --workspace; fi
echo "TASK_DELIVERY_QA_GATE=${CICD_QA_GATE:-full}:PASS"
