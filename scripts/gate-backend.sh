#!/bin/bash
# K4.x local gate: fmt + clippy + tests in pinned container (no sed on mounted files!)
set -e
cd /opt/dev/sdlc/CI-CD/backend
docker run --rm --entrypoint /bin/bash \
  -v "$PWD":/workspace \
  -v /opt/dev/sdlc/services-base:/services-base:ro \
  -v cicd-cargo-cache:/usr/local/cargo/registry \
  -v cicd-cargo-target-k4:/workspace/target \
  rust:1.88-bookworm -c '
cp -r /usr/local/rustup /tmp/rustup 2>/dev/null || true
mkdir -p /tmp/rubin && cp /usr/local/cargo/bin/rustup /tmp/rubin/rustup 2>/dev/null || true
cd /workspace
export RUSTUP_HOME=/tmp/rustup
TCBIN=$(echo /tmp/rustup/toolchains/*/bin); export PATH=$TCBIN:/tmp/rubin:$PATH
/tmp/rubin/rustup component add rustfmt clippy 2>&1 | tail -1 || true
echo "=== fmt ==="
cargo fmt --all -- --check && echo FMT_OK
echo "=== clippy ==="
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "^error(\[|:)" | head -20 || true
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
echo "=== tests ==="
cargo test --workspace 2>&1 | grep -E "test result" | tail -12
echo GATE_DONE
'
