# CURRENT STATE — Forge CI/CD

> **Производный снимок текущего состояния.** Сгенерирован из кода; authority — код и коммит, не этот файл. Обновлять при каждом изменении capability.
> Снято: `2026-08-27`, ветка `main` (после `cbbf930`).

## Что работает сейчас (Current verified)

| Capability | Статус | Границы |
|---|---|---|
| Проекты CRUD | ✅ | name/repository_url/default_branch; удаление CASCADE |
| Git hosting (bare + Smart HTTP) | ✅ | token-auth опционален; push → post-receive → pipeline |
| Pipeline/стадии/джобы | ✅ | `.forge-ci.yml` (stages/jobs/image/command) или fallback-шаблон; отмена/повтор |
| Embedded runner | ✅ | Docker (`forge-job-<id>`) или host shell; стриминг stdout → `job_logs`; cancel через PID-map |
| Логи | ✅ | append-only, sequence, поллинг |
| Артефакты | ✅ | upload/download ≤50 MiB, локальный `CICD_ARTIFACTS_DIR` |
| Секреты проектов | ✅ | AES-256-GCM at rest; значение не возвращается API |
| Environments/deployments | ✅ | metadata + history |
| Reports | ✅ | агрегаты success rate/duration |
| Users/roles + API-токены | ✅ хранение | enforcement middleware отсутствует |
| Audit log | ✅ | append-only, последние 200 |
| Schedules/webhooks | ✅ | outbox-доставка + cron-scheduler (миграция 0004) |
| Login UI | ✅ | /login + refresh + гард (вкл. при CICD_AUTH_SECRET) |
| Auth/RBAC | ✅ | argon2id+JWT+PAT, role-политики, аудит login/denied |
| Secret injection | ✅ | env в job + маскирование в логах |
| Error envelope + request_id | ✅ | {error:{code,message,request_id}} + x-request-id |
| Pagination | ✅ | limit/offset (cap 200) на проектах/пайплайнах |
| Rate limiting | ✅ | login 30/min → 429 |
| Metrics | ✅ | /metrics Prometheus text |

## Не реализовано (Target approved — см. ADR + contracts)

Runner protocol/leases/dispatch (внешний runner, ADR-0007), idempotency keys, S3 artifacts, backup scripts, SSE-события, project-membership RBAC, per-IP rate limiting (сейчас per-process окно).

## Текущее runtime-дерево backend

```text
backend/
├── Cargo.toml          # workspace: [".", "domain", "cli"]
├── src/                # cicd-server (монолит, мигрирует по ADR-0005)
│   ├── api.rs          # projects/pipelines/jobs/logs (+health, CORS)
│   ├── platform.rs     # runners/secrets/artifacts/… /users/tokens
│   ├── git_host.rs     # bare repos, Smart HTTP, post-receive
│   ├── pulls.rs        # refs/commits/compare/pull requests
│   ├── runner.rs       # embedded executor (+secret injection, маскирование)
│   ├── outbox.rs       # ADR-0006: domain_events/outbox + scheduler worker
│   ├── authz.rs        # role-политики роутов (AUTHZ_CONTRACT)
│   ├── rate_limit.rs   # login rate limiting
│   ├── metrics.rs      # /metrics Prometheus exposition
│   ├── migrations/     # versioned SQLx migrations (ADR-0008, applied at startup + cicd-migrate)
│   └── domain.rs       # re-export shim → cicd-domain
├── domain/             # cicd-domain: чистые типы + JobStatus
├── cli/                # cicd-cli: HTTP-only (project/pipeline/job)
├── tests/              # api_contract, cli_contract (+ sql/init-roles.sql)
└── docker-compose.test.yml
```

## Известные dev-only риски (до Phase D security)

- API и Dashboard полностью открыты (нет auth/RBAC); CORS permissive.
- PostgreSQL в compose опубликован на все интерфейсы.
- Токен `CICD_GIT_INTERNAL_TOKEN` обязателен к смене для shared-деплоя.
- Login — UI-заглушка; API-токены не проверяются middleware.

## Верификационные команды

```bash
docker compose config -q
docker compose -f backend/docker-compose.test.yml config -q
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test --workspace'
cd frontend && pnpm test && pnpm build
python3 scripts/verify_docs.py --canonical --links --current-state
```

## Frontend: 21 маршрут / 20 страниц + /login

Полный список маршрутов и соответствие скринам — `docs/ROUTING.md`, визуальный реестр — docs/assets/screens/manifest.md (появится на Gate 5).
