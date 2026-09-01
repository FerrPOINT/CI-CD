# CURRENT STATE — Forge CI/CD

> **Производный снимок текущего состояния.** Сгенерирован из кода; authority — код и коммит, не этот файл. Обновлять при каждом изменении capability.
> Снято: `2026-09-01`, ветка `main`; visual evidence: 45 screenshots в `docs/assets/screens/manifest.md`.

## Что работает сейчас (Current verified)

| Capability | Статус | Границы |
|---|---|---|
| Проекты CRUD | ✅ | name/repository_url/default_branch; удаление CASCADE |
| Git hosting (bare + Smart HTTP) | ✅ | public/private fetch ACL, optional token-protected push, code tree/blob, tags/releases; push → post-receive(old/new SHA) → idempotent pipeline per pushed object |
| Pipeline/стадии/джобы | ✅ | `.forge-ci.yml` legacy `stages/jobs/image/command` или v1 `version: 1` + top-level `jobs.commands/needs/tags`; fallback-шаблон при отсутствии config; trigger пытается читать config по resolved commit SHA; ручной trigger поддерживает `Idempotency-Key`; отмена/повтор в текущей job-модели |
| Pipeline plan snapshot | ✅ MVP | `pipeline_plans` хранит immutable snapshot: raw config/fallback template, `config_sha256`, parser version, normalised plan JSON, `plan_sha256`, dependency edges и v1 `required_tags`. Current форматы: `legacy-linear` (`forge-legacy-linear/1`) и `v1-dag` (`forge-dsl/1.0.0`); v1 DAG исполняется через топологические `dag-*` стадии, а policy diagnostics/job-level dispatcher остаются target |
| Embedded runner | ✅ | Docker (`forge-job-<id>`) или host shell; lease-aware claim, стриминг stdout → attempt-owned `job_logs`; cancel через PID-map; можно отключить `CICD_EMBEDDED_RUNNER_ENABLED=false` |
| Execution attempts | ✅ MVP | `execution_attempts` создаются для каждой job; retry job/pipeline создаёт новую attempt и не удаляет старые логи |
| Job queue | ✅ MVP | `job_queue` материализует dispatch row на current queued attempt и копирует `required_tags`; trigger/retry/manual start enqueue-ят non-manual work, embedded claim берёт только untagged rows, external runner claim берёт совместимую работу через queue row + `SKIP LOCKED` + `required_tags ⊆ runner.tags`, terminal/cancel/expiry закрывают row |
| Job leases | ✅ MVP | embedded runner создаёт active `job_leases` при claim, закрывает lease при terminal result/cancel и reconciler переводит expired/missing lease в failed; внешний runner protocol MVP выдаёт lease token, ack/renew/logs/complete и проверяет fencing generation |
| External runner protocol + `forge-runner` | ✅ MVP | `/api/v1/runner/register`, heartbeat, immediate `work:poll`, ack/renew/logs/complete; credential и lease token хранятся только hash-ами; `work:poll` claim-ит compatible durable `job_queue` row по `required_tags ⊆ runner.tags` и отдаёт `workspace.checkoutUrl`; отдельный `forge-runner` shell process умеет checkout, renew, stdout/stderr log append и terminal completion; secrets/artifacts protocol, richer log chunks, protected tags/pools, capability matching, Docker/Kubernetes sandbox и long-poll остаются target |
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
| Health/readiness/metrics | ✅ | `/api/v1/health` liveness без БД; `/api/v1/readiness` проверяет PostgreSQL и SQLx migration versions/checksums; `/metrics` Prometheus text |
| Backup/restore helper | ✅ MVP | `scripts/forge_backup.py` + wrappers создают/проверяют/restoring local Docker Compose backup: PostgreSQL custom dump, Git/artifact volume copy, `SHA256SUMS`, `manifest.json`; off-site/PITR/monthly drill остаются target |

## Не реализовано (Target approved — см. ADR + contracts)

Production-grade runner dispatch policy, long-poll/wakeup, protocol endpoints для secrets/artifacts, idempotent chunked log upload, Docker/Kubernetes sandbox, pool/protected-tag policy и capability matching (ADR-0007), policy-aware pipeline planner поверх v1 DAG (`on`, retry/artifacts/secrets, line/column diagnostics, job-level dispatcher), general idempotency storage for all retryable mutations, command spans/stream classification для диагностических логов, artifact retention/object storage, off-site/PITR backup platform и verified restore drill, external notification channel adapters (email/Slack), inbound provider webhook handlers, tenant isolation, service-account tokens, scoped Git credentials, production cookie/CSRF/session-family policy, schedule IANA timezone/DST/misfire и multi-replica leases, outbox lease/fencing/crash recovery, full dead-letter operator policy/metrics и distributed/proxy rate limiting (сейчас in-process окно по forwarded client key).

## Текущее runtime-дерево backend

```text
backend/
├── Cargo.toml          # workspace: [".", "domain", "cli"]
├── src/                # cicd-server (монолит, мигрирует по ADR-0005)
│   ├── api.rs          # projects/pipelines/jobs/logs (+health/readiness, CORS)
│   ├── platform.rs     # runners/secrets/artifacts/… /users/tokens
│   ├── git_host.rs     # bare repos, Smart HTTP, post-receive
│   ├── pulls.rs        # refs/commits/compare/pull requests
│   ├── runner.rs       # embedded executor (+job_leases, secret injection, маскирование)
│   ├── runner_protocol.rs # external runner protocol MVP
│   ├── bin/forge-runner.rs # external shell runner process MVP
│   ├── outbox.rs       # ADR-0006: domain_events/outbox + scheduler worker
│   ├── authz.rs        # role-политики роутов + project membership enforcement
│   ├── rate_limit.rs   # in-process fixed-window route-class limiting
│   ├── metrics.rs      # /metrics Prometheus exposition
│   └── domain.rs       # re-export shim → cicd-domain
├── migrations/         # versioned SQLx migrations incl. 0019 runner tag matching
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
- Execution attempts / job queue / job leases — MVP-слой: old `/jobs/{id}/logs` читает текущую или последнюю attempt, bounded `/jobs/{id}/logs/page` поддерживает `limit/after/q`, полный аудит попыток доступен через `/jobs/{id}/attempts`, `job_queue` переживает restart и является источником claim для embedded/external runners, embedded берёт только untagged rows, внешний runner protocol уже проверяет runner credential, lease token, fencing generation и tag compatibility, а также принимает stdout/stderr log append. `forge-runner` даёт отдельный shell process, но production sandbox, protocol secrets/artifacts, richer log chunks, protected tags/pools/capabilities и lost-heartbeat policy ещё target.
- Scheduler/outbox — MVP: есть строгий 5-польный UTC cron и уникальные fire slots, но нет IANA timezone/DST/misfire, lease/fencing/crash-safe dispatcher-а, full dead-letter operator policy/metrics и внешних notification adapters; bounded delivery history/requeue и `in_app`/`sse` local outbox projection уже работают.

## Верификационные команды

```bash
docker compose config -q
docker compose -f backend/docker-compose.test.yml config -q
just readiness
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test --workspace'
cd frontend && pnpm test && pnpm build
cd frontend && pnpm lint
python3 scripts/verify_docs.py --canonical --links --current-state
```

## Frontend: 21 маршрут / 20 рабочих страниц + /login

Полный список базовых страниц — `docs/architecture/frontend-boundaries.md`; визуальный реестр — `docs/assets/screens/manifest.md`.
