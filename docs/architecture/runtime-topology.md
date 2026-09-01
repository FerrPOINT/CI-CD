# Runtime topology

> **Статус:** объяснительный документ (ADR-0009). Текущее: single-node MVP; target — execution вынесен из control plane (ADR-0007).

## Current (verified)

```text
┌──────────────────────────── docker compose ────────────────────────────┐
│  frontend (nginx:1.27) :22802                                           │
│      │ /api proxy                                                       │
│  backend (cicd-server, axum) :22801                                     │
│      ├── public REST /api/v1/* (conditional auth; open trusted-network  │
│      │   mode when CICD_AUTH_SECRET is unset/empty; CORS permissive)     │
│      ├── Git Smart HTTP /git/<name>.git (public read or auth/project RBAC)│
│      ├── internal POST /api/v1/internal/git-push (X-Internal-Token)     │
│      └── embedded runner: Docker API / host shell                       │
│             └── workspace volume + forge-job-<id> containers            │
│  runner profile (forge-runner)                                          │
│      └── runner API poll/ack/renew/complete + shell workspace           │
│  postgres :22543 (volume)          artifacts dir (volume)               │
│  bare git repos (volume)                                                │
└─────────────────────────────────────────────────────────────────────────┘
```

Доверенная зона — весь compose-хост. Docker socket доступен backend-контейнеру (embedded runner), если включён соответствующий executor. `forge-runner` доступен через Compose profile `external-runner`; для него обычно ставят `CICD_EMBEDDED_RUNNER_ENABLED=false`, чтобы работу не забирали два локальных executors.

## Target (approved, частично реализовано)

```text
clients (browser/CLI) ── HTTPS ── control plane (api/app/infra/server)
                                      │ PostgreSQL (domain_events/outbox)
                                      │ artifact object storage (FS/S3)
                              outbox workers: webhooks/notifications/SSE
external runners ── mTLS/bearer ── runner API (/api/v1/runner/*)
                                      lease/fencing, no Docker socket in API
git clients ── Smart HTTP ── git ingress ── domain_events ── scheduler
```

Разделение доверия: current `forge-runner` уже выносит shell execution в отдельный процесс, но production-цель остаётся строже: control plane не исполняет jobs и не имеет Docker socket, runner-ы — отдельная зона с lease-токенами и sandbox, PostgreSQL не публикуется наружу.

## Наблюдаемость

- Health: `GET /api/v1/health` (без БД); readiness: `GET /api/v1/readiness` проверяет PostgreSQL и SQLx migration versions/checksums.
- Логи: tracing stdout; metrics `/metrics` — current Prometheus exposition с target-расширением набора метрик (`contracts/API_CONTRACT.md`).
