#!/usr/bin/env bash
# Complete coherent workspace backup; no deletion or guessed legacy volumes.
set -euo pipefail
umask 077
if [ "$#" -ne 1 ]; then echo 'Specify exactly one protected output archive' >&2; exit 2; fi
: "${SDLC_TASK:?Set the maintenance owner task}"
: "${SDLC_WORKSPACE_DIR:?Select the initialized workspace}"
: "${SDLC_PROJECT:?Set sdlc1 or sdlc2}"
: "${SDLC_DOCKER_CONTEXT:?Select the Docker context}"
: "${SDLC_SIGNING_KEY:?Select the preserved Auth signing key}"
case "$SDLC_PROJECT" in sdlc1|sdlc2) ;; *) echo 'Unknown workspace project' >&2; exit 2 ;; esac
exec python3 "$SDLC_WORKSPACE_DIR/services-base/scripts/platform_backup.py" backup \
  --project "$SDLC_PROJECT" --workspace-profile "$SDLC_WORKSPACE_DIR/workspace.local.json" \
  --compose-file "$SDLC_WORKSPACE_DIR/docker-compose.local.yml" \
  --project-directory "$SDLC_WORKSPACE_DIR" --docker-context "$SDLC_DOCKER_CONTEXT" \
  --task "$SDLC_TASK" --layout auto --quiesce --signing-key "$SDLC_SIGNING_KEY" --output "$1"
