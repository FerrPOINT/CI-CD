#!/usr/bin/env bash
# M5: Nightly backup for the Base stand -- all PostgreSQL databases + persistent payloads.
# Retention: 7 days. Restore procedure: see docs/BACKUP_RESTORE.md of each repo.
set -euo pipefail

BACKUP_ROOT="${BACKUP_ROOT:-/opt/dev/sdlc-backups}"
DATE="$(date +%Y-%m-%d_%H%M)"
DIR="$BACKUP_ROOT/$DATE"
RETENTION_DAYS=7

declare -A DBS=(
  [task-tracker]="sdlc-local-tt-postgres-1"
  [wiki]="sdlc-local-wiki-postgres-1"
  [fleet-control]="sdlc-local-fc-postgres-1"
  [cicd]="sdlc-local-cicd-postgres-1"
  [java-agent]="sdlc-local-ja-db-1"
)

mkdir -p "$DIR"

# PostgreSQL custom-format dumps support validation and isolated restore drills.
for name in "${!DBS[@]}"; do
  container="${DBS[$name]}"
  db="$(docker exec "$container" printenv POSTGRES_DB)"
  user="$(docker exec "$container" printenv POSTGRES_USER)"
  out="$DIR/${name}-${DATE}.dump"
  if docker exec "$container" pg_dump -U "$user" -Fc -d "$db" > "$out" 2>"$DIR/${name}.err"; then
    size="$(du -h "$out" | cut -f1)"
    echo "OK   ${name}: ${size} -> ${out}"
  else
    echo "FAIL ${name}: see $DIR/${name}.err" >&2
  fi
done

# Persistent payloads outside the databases.
if docker volume inspect sdlc-local_ja_chromium >/dev/null 2>&1; then
  docker run --rm -v sdlc-local_ja_chromium:/data:ro -v "$DIR":/backup alpine \
    tar czf "/backup/ja-chromium-${DATE}.tar.gz" -C /data . 2>/dev/null \
    && echo "OK   ja-chromium volume" || echo "FAIL ja-chromium volume" >&2
fi
if docker volume inspect sdlc-local_tt_uploads >/dev/null 2>&1; then
  docker run --rm -v sdlc-local_tt_uploads:/data:ro -v "$DIR":/backup alpine \
    tar czf "/backup/tt-uploads-${DATE}.tar.gz" -C /data . 2>/dev/null \
    && echo "OK   tt-uploads volume" || echo "FAIL tt-uploads volume" >&2
fi

find "$BACKUP_ROOT" -mindepth 1 -maxdepth 1 -type d -mtime +$RETENTION_DAYS -exec rm -rf {} \;
echo "Retention: kept last ${RETENTION_DAYS} days under $BACKUP_ROOT"
