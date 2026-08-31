# CURRENT STATE — Forge CI/CD

> **Производный снимок текущего состояния.** Сгенерирован из кода; authority — код и коммит, не этот файл. Обновлять при каждом изменении capability.
> Снято: `2026-08-31`, ветка `main`; visual evidence: 45 screenshots в `docs/assets/screens/manifest.md`.

## Что работает сейчас (Current verified)

| Capability | Статус | Границы |
|---|---|---|
| Проекты CRUD | ✅ | name/repository_url/default_branch; удаление CASCADE |
| Git hosting (bare + Smart HTTP) | ✅ | public/private fetch ACL, optional token-protected push, code tree/blob, tags/releases; push → post-receive → pipeline |
| Pipeline/стадии/джобы | ✅ | `.forge-ci.yml` (stages/jobs/image/command) или fallback-шаблон; отмена/повтор в текущей job-модели |
| Embedded runner | ✅ | Docker (`forge-job-<id>`) или host shell; стриминг stdout → attempt-owned `job_logs`; cancel через PID-map |
| Execution attempts | ✅ MVP | `execution_attempts` создаются для каждой job; retry job/pipeline создаёт новую attempt и не удаляет старые логи |
| Логи | ✅ | append-only внутри attempt, sequence per attempt, REST polling и SSE stream текущей/последней attempt |
| Артефакты | ✅ | upload/download ≤50 MiB, локальный `CICD_ARTIFACTS_DIR`; новые metadata привязаны к active/latest attempt |
| Секреты проектов | ✅ | AES-256-GCM at rest; значение не возвращается API |
| Environments/deployments | ✅ | metadata + history |
| Reports | ✅ | агрегаты success rate/duration |
| Users/roles + API-токены | ✅ | хранение + enforcement при `CICD_AUTH_SECRET`; глобальная роль ограничивает максимум прав |
| Audit log | ✅ | append-only, последние 200 |
| Schedules | ✅ MVP | enabled rows проверяются примерно раз в минуту; cron строка валидируется/хранится, но не исполняет полную cron-семантику |
| Outgoing webhooks | ✅ MVP | terminal pipeline event -> `domain_events`/`outbox_messages`; basic retry/backoff, optional HMAC |
| Notifications | ⚙️ | конфигурация каналов хранится, sender/SSE delivery нет |
| Login UI | ✅ | `/login` вызывает auth API; redirect guard включается только при `401` |
| Project membership RBAC | ✅ MVP | `project_memberships`, фильтрация списка проектов, deny-before-load для project-owned API; `admin` bypass, tenant/scoped PAT ещё target |
| Auth/RBAC | ✅ conditional | если `CICD_AUTH_SECRET` задан непустым: argon2id+JWT+PAT, route role-политики, project memberships, аудит login/denied; без секрета trusted-network mode |
| Secret injection | ✅ | project secrets передаются env в embedded job и маскируются в stdout logs |
| Error envelope + request_id | ✅ | {error:{code,message,request_id}} + x-request-id |
| Pagination | ✅ | limit/offset (cap 200) на проектах/пайплайнах |
| Rate limiting | ✅ | login 30/min → 429 |
| Metrics | ✅ | /metrics Prometheus text |

## Не реализовано (Target approved — см. ADR + contracts)

Runner protocol/leases/dispatch (внешний runner, ADR-0007), immutable pipeline plan/DAG, idempotency keys, S3 artifacts, backup scripts, notifications sender/SSE delivery, tenant isolation, scoped PAT, production session/logout policy, full cron semantics, delivery history/replay/dead letters, log pagination/search и per-IP rate limiting (сейчас per-process окно).

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
│   ├── authz.rs        # role-политики роутов + project membership enforcement
│   ├── rate_limit.rs   # login rate limiting
│   ├── metrics.rs      # /metrics Prometheus exposition
│   ├── migrations/     # versioned SQLx migrations incl. 0007 execution_attempts
│   └── domain.rs       # re-export shim → cicd-domain
├── domain/             # cicd-domain: чистые типы + JobStatus
├── cli/                # cicd-cli: HTTP-only (project/pipeline/job)
├── tests/              # api_contract, domain_transitions, integration_db (+ sql/init-roles.sql)
├── cli/tests/          # cli_contract
└── docker-compose.test.yml
```

## Известные dev-only риски

- Без непустого `CICD_AUTH_SECRET` API и Dashboard полностью открыты в trusted-network режиме; CORS permissive.
- PostgreSQL в compose опубликован только на `127.0.0.1`, но API/Dashboard host ports нельзя открывать в недоверенную сеть.
- Токен `CICD_GIT_INTERNAL_TOKEN` обязателен к смене для shared-деплоя.
- Auth/RBAC пока без tenant isolation, scoped PAT и production-grade session/logout policy; project membership реализован как MVP-слой поверх глобальных ролей.
- Execution attempts — MVP-слой без внешних leases/fencing: old `/jobs/{id}/logs` читает текущую или последнюю attempt, а полный аудит попыток доступен через `/jobs/{id}/attempts`.
- Scheduler/outbox — MVP: нет точной cron-семантики, delivery history, audited replay/dead letters и notification/SSE sender.

## Верификационные команды

```bash
docker compose config -q
docker compose -f backend/docker-compose.test.yml config -q
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test --workspace'
cd frontend && pnpm test && pnpm build
cd frontend && pnpm lint
python3 scripts/verify_docs.py --canonical --links --current-state
```

## Frontend: 21 маршрут / 20 рабочих страниц + /login

Полный список базовых страниц — `docs/architecture/frontend-boundaries.md`; визуальный реестр — `docs/assets/screens/manifest.md`.
