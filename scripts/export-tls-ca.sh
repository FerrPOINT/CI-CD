#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/export-tls-ca.sh [--compose-file PATH] [--project-name NAME] [--service NAME] --output PATH

Copies Caddy's internal root CA from a running Forge TLS profile. The exported
certificate is public trust material, not an application secret. Import it only
on operator-controlled clients for the named internal deployment.
EOF
}

compose_file="docker-compose.tls.yml"
project_name=""
service="caddy"
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --compose-file)
      compose_file="$2"
      shift 2
      ;;
    --project-name)
      project_name="$2"
      shift 2
      ;;
    --service)
      service="$2"
      shift 2
      ;;
    --output)
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$output" ]]; then
  printf '%s\n' "--output is required" >&2
  exit 2
fi

mkdir -p "$(dirname "$output")"
compose_files=(-f docker-compose.yml -f "$compose_file")
if [[ -n "$project_name" ]]; then
  compose_files=(-p "$project_name" "${compose_files[@]}")
fi
docker compose "${compose_files[@]}" cp \
  "${service}:/data/caddy/pki/authorities/local/root.crt" "$output"
printf 'Exported Caddy internal root CA to %s\n' "$output"
