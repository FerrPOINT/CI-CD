#!/usr/bin/env bash
set -euo pipefail
: "${SDLC_WORKSPACE_DIR:?Select the source workspace}"
: "${SDLC_DOCKER_CONTEXT:?Select the Docker context}"
: "${SDLC_TASK:?Set the task owner}"

TRIVY_IMAGE="${CICD_TRIVY_IMAGE:-aquasec/trivy@sha256:62b1e65e8869bc4b4c6aa4fa2b21595256c7c2f6018a9d9ad61caf87187c1969}"
TRIVY_SEVERITY="${CICD_TRIVY_SEVERITY:-CRITICAL}"
TRIVY_CACHE_DIR="${TRIVY_CACHE_DIR:-${RUNNER_TEMP:-/tmp}/forge-trivy-cache}"

if [ "$#" -eq 0 ]; then
  echo "usage: scripts/scan_container_images.sh <image> [<image> ...]" >&2
  exit 2
fi

mkdir -p "$TRIVY_CACHE_DIR"

for image in "$@"; do
  if ! docker --context "$SDLC_DOCKER_CONTEXT" image inspect "$image" >/dev/null 2>&1; then
    echo "container scan: image '$image' is missing; build it before scanning" >&2
    exit 2
  fi

  python3 "$SDLC_WORKSPACE_DIR/services-base/scripts/compose_probe.py" \
    --docker-context "$SDLC_DOCKER_CONTEXT" --task "$SDLC_TASK" --purpose container-vulnerability-scan -- --network isolated --rm \
    -v /var/run/docker.sock:/var/run/docker.sock \
    -v "$TRIVY_CACHE_DIR:/root/.cache/trivy" \
    "$TRIVY_IMAGE" image \
    --scanners vuln \
    --ignore-unfixed \
    --severity "$TRIVY_SEVERITY" \
    --exit-code 1 \
    --no-progress \
    "$image"
done
