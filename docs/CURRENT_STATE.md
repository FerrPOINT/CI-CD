# CURRENT STATE — Forge CI/CD

> **Производный снимок текущего состояния.** Сгенерирован из кода; authority — код и коммит, не этот файл. Обновлять при каждом изменении capability.
> Снято: `2026-09-01`, ветка `main`; visual evidence: 45 screenshots в `docs/assets/screens/manifest.md`.

## Что работает сейчас (Current verified)

| Capability | Статус | Границы |
|---|---|---|
| Проекты CRUD | ✅ | name/repository_url/default_branch; удаление CASCADE |
| Git hosting (bare + Smart HTTP) | ✅ | public/private fetch ACL, optional token-protected push, code tree/blob, tags/releases; push → post-receive(old/new SHA) → idempotent pipeline per pushed object |
| Pipeline/стадии/джобы | ✅ | `.forge-ci.yml` (stages/jobs/image/command) или fallback-шаблон; ручной trigger поддерживает `Idempotency-Key`; отмена/повтор в текущей job-модели |
| Embedded runner | ✅ | Docker (`forge-job-<id>`) или host shell; lease-aware claim, стриминг stdout → attempt-owned `job_logs`; cancel через PID-map |
| Execution attempts | ✅ MVP | `execution_attempts` создаются для каждой job; retry job/pipeline создаёт новую attempt и не удаляет старые логи |
| Job leases | ✅ MVP | embedded runner создаёт active `job_leases` при claim, закрывает lease при terminal result/cancel и reconciler переводит expired/missing lease в failed; внешний runner protocol/ack/renew/fencing ещё target |
| Логи | ✅ | append-only внутри attempt, sequence per attempt, совместимый REST array shortcut, bounded `/logs/page` с `limit/after/q` и SSE stream текущей/последней attempt |
| Артефакты | ✅ MVP | upload/download ≤50 MiB, локальный `CICD_ARTIFACTS_DIR`; новые metadata привязаны к active/latest attempt и содержат SHA-256; download проверяет canonical path containment и checksum drift |
| Секреты проектов | ✅ | AES-256-GCM at rest; значение не возвращается API |
| Environments/deployments | ✅ | metadata + history |
| Reports | ✅ | агрегаты success rate/duration |
| Users/roles + API-токены | ✅ | хранение + enforcement при `CICD_AUTH_SECRET`; session-bound access JWT, refresh session rotate/logout; новые PAT требуют project scope, scopes и expiry; глобальная роль ограничивает максимум прав |
| Audit log | ✅ | append-only, последние 200 |
| Schedules | ✅ MVP | строгий 5-польный UTC cron, persisted `next_fire_at`, unique `schedule_fires` slot и idempotent pipeline trigger; строки с `last_fire_error` ждут явного PATCH/исправления; IANA timezone/DST/misfire и multi-replica leases остаются target |
| Outgoing webhooks | ✅ MVP | terminal pipeline event -> `domain_events`/`outbox_messages`; basic retry/backoff, optional HMAC |
| Outbox delivery history | ✅ MVP | project-scoped `/outbox-deliveries` API показывает статус, attempts, `failed_at`/`last_error`; failed delivery можно явно requeue новой generation |
| Notifications | ✅ MVP | `in_app`/`sse` каналы создают durable local outbox event на terminal pipeline events; история доступна через `/notification-events`, live stream — через `/notifications/stream`; email/Slack adapters и inbound provider handlers остаются target |
| Login UI | ✅ | `/login` вызывает auth API; redirect guard включается только при `401` |
| Project membership RBAC | ✅ MVP | `project_memberships`, фильтрация списка проектов, deny-before-load для project-owned API и name-based repo API; `admin` bypass, tenant isolation/SAT ещё target |
| Git auth/RBAC | ✅ conditional MVP | public repo read открыт; private read и receive-pack требуют legacy `CICD_GIT_TOKEN` либо JWT/PAT + `project_memberships` + `git:*` PAT scope при `CICD_AUTH_SECRET`; без секрета и Git token остаётся trusted local mode |
| Auth/RBAC | ✅ conditional | если `CICD_AUTH_SECRET` задан непустым: argon2id+JWT+PAT, session-bound access invalidation, refresh rotate/logout/revoke, route role-политики, project memberships, Git Smart HTTP project checks, аудит login/logout/denied; без секрета trusted-network mode |
| Secret injection | ✅ | project secrets передаются env в embedded job и маскируются в stdout logs |
| Error envelope + request_id | ✅ | {error:{code,message,request_id}} + x-request-id |
| Pagination | ✅ | limit/offset (cap 200) на проектах/пайплайнах |
| Rate limiting | ✅ MVP | in-process per-client fixed-window: auth, API read/write, Git Smart HTTP, internal hook и artifact upload возвращают `429` при превышении |
| Metrics | ✅ | /metrics Prometheus text |
| Backup/restore helper | ✅ MVP | `scripts/forge_backup.py` + wrappers создают/проверяют/restoring local Docker Compose backup: PostgreSQL custom dump, Git/artifact volume copy, `SHA256SUMS`, `manifest.json`; off-site/PITR/monthly drill остаются target |

## Не реализовано (Target approved — см. ADR + contracts)

External runner protocol/dispatch (отдельный runner process, registration token, poll/ack/renew API, fencing на mutating endpoints; ADR-0007), immutable pipeline plan/DAG, general idempotency storage for all retryable mutations, command spans/stream classification для диагностических логов, artifact retention/object storage, off-site/PITR backup platform и verified restore drill, external notification channel adapters (email/Slack), inbound provider webhook handlers, tenant isolation, service-account tokens, scoped Git credentials, production cookie/CSRF/session-family policy, schedule IANA timezone/DST/misfire и multi-replica leases, outbox lease/fencing/crash recovery, full dead-letter operator policy/metrics и distributed/proxy rate limiting (сейчас in-process окно по forwarded client key).

## Текущее runtime-дерево backend

```text
backend/
├── Cargo.toml          # workspace: [".", "domain", "cli"]
├── src/                # cicd-server (монолит, мигрирует по ADR-0005)
│   ├── api.rs          # projects/pipelines/jobs/logs (+health, CORS)
│   ├── platform.rs     # runners/secrets/artifacts/… /users/tokens
│   ├── git_host.rs     # bare repos, Smart HTTP, post-receive
│   ├── pulls.rs        # refs/commits/compare/pull requests
│   ├── runner.rs       # embedded executor (+job_leases, secret injection, маскирование)
│   ├── outbox.rs       # ADR-0006: domain_events/outbox + scheduler worker
│   ├── authz.rs        # role-политики роутов + project membership enforcement
│   ├── rate_limit.rs   # in-process fixed-window route-class limiting
│   ├── metrics.rs      # /metrics Prometheus exposition
│   └── domain.rs       # re-export shim → cicd-domain
├── migrations/         # versioned SQLx migrations incl. 0015 job leases
├── domain/             # cicd-domain: чистые типы + JobStatus
├── cli/                # cicd-cli: HTTP-only (project/pipeline/job)
├── tests/              # api_contract, domain_transitions, integration_db (+ sql/init-roles.sql)
├── cli/tests/          # cli_contract
└── docker-compose.test.yml
```

## Известные dev-only риски

- Без непустого `CICD_AUTH_SECRET` API и Dashboard полностью открыты в trusted-network режиме; CORS permissive.
- PostgreSQL в compose опубликован только на `127.0.0.1`, но API/Dashboard host ports нельзя открывать в недоверенную сеть.
- `CICD_GIT_INTERNAL_TOKEN` пустой по умолчанию только для isolated local development; shared-деплой обязан задать уникальный токен, а legacy `forge-internal-dev-token` отклоняется при старте.
- Auth/RBAC пока без tenant isolation, service-account tokens, scoped Git credentials и production-grade cookie/CSRF/session-family policy; session-bound access invalidation, refresh rotate/logout/revoke, project membership, scoped PAT и Git read/write checks реализованы как MVP-слой поверх глобальных ролей.
- Execution attempts / job leases — MVP-слой без внешнего runner protocol/fencing: old `/jobs/{id}/logs` читает текущую или последнюю attempt, bounded `/jobs/{id}/logs/page` поддерживает `limit/after/q`, полный аудит попыток доступен через `/jobs/{id}/attempts`, а embedded `job_leases` закрывают crash/restart risk только на уровне локального backend worker-а.
- Scheduler/outbox — MVP: есть строгий 5-польный UTC cron и уникальные fire slots, но нет IANA timezone/DST/misfire, lease/fencing/crash-safe dispatcher-а, full dead-letter operator policy/metrics и внешних notification adapters; bounded delivery history/requeue и `in_app`/`sse` local outbox projection уже работают.

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
