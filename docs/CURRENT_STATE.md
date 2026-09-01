# CURRENT STATE — Forge CI/CD

> **Производный снимок текущего состояния.** Сгенерирован из кода; authority — код и коммит, не этот файл. Обновлять при каждом изменении capability.
> Снято: `2026-09-01`, ветка `main`; visual evidence: 46 screenshots в `docs/assets/screens/manifest.md`.

## Что работает сейчас (Current verified)

| Capability | Статус | Границы |
|---|---|---|
| Проекты CRUD | ✅ | name/repository_url/default_branch; удаление CASCADE |
| Git hosting (bare + Smart HTTP) | ✅ | public/private fetch ACL, optional token-protected push, code tree/blob, tags/releases; Smart HTTP RPC body limit 100 MiB с gzip post-inflate check; push → post-receive(old/new SHA) → idempotent pipeline per pushed object |
| Pipeline/стадии/джобы | ✅ | `.forge-ci.yml` legacy `stages/jobs/image/command` или v1 `version: 1` + top-level `jobs.commands/needs/tags/secrets`; fallback-шаблон при отсутствии config; trigger пытается читать config по resolved commit SHA; ручной trigger поддерживает `Idempotency-Key`; отмена/повтор в текущей job-модели |
| Pipeline plan snapshot | ✅ MVP | `pipeline_plans` хранит immutable snapshot: raw config/fallback template, `config_sha256`, parser version, normalised plan JSON, `plan_sha256`, dependency edges и v1 `required_tags`/`required_secrets`/`artifact_paths`. Current форматы: `legacy-linear` (`forge-legacy-linear/1`) и `v1-dag` (`forge-dsl/1.0.0`); v1 DAG исполняется через топологические `dag-*` стадии, а policy diagnostics/job-level dispatcher остаются target |
| Embedded runner | ✅ | Docker (`forge-job-<id>`) или host shell; lease-aware claim, стриминг stdout → attempt-owned `job_logs`; cancel через PID-map; можно отключить `CICD_EMBEDDED_RUNNER_ENABLED=false` |
| Execution attempts | ✅ MVP | `execution_attempts` создаются для каждой job; retry job/pipeline создаёт новую attempt и не удаляет старые логи |
| Job queue | ✅ MVP | `job_queue` материализует dispatch row на current queued attempt и копирует `required_tags`; trigger/retry/manual start enqueue-ят non-manual work, embedded claim берёт только untagged rows, external runner claim берёт совместимую работу через queue row + `SKIP LOCKED` + `required_tags ⊆ runner.tags` + current `shell` executor compatibility, unacknowledged external offer после `ackDeadline` requeue-ится, dispatch-eligible queued job без compatible execution path после `CICD_RUNNER_QUEUE_TIMEOUT_SECONDS` завершается с diagnostic, terminal/cancel/lease expiry закрывают row |
| Job leases | ✅ MVP | embedded runner создаёт active `job_leases` при claim, закрывает lease при terminal result/cancel и reconciler переводит expired/missing lease в failed; внешний runner protocol MVP выдаёт lease token, ack/renew/control/logs/complete, requeue-ит unacknowledged offer после `ackDeadline`, доставляет cancel signal через `cancel_requested_at`, проверяет fencing generation и защищает stale-runner offline reconciliation живой unexpired lease |
| External runner protocol + `forge-runner` | ✅ MVP | `/api/v1/runner/register`, heartbeat, `work:poll` с optional `waitSeconds` `0..30`, ack/renew/control/`secrets:resolve`/artifact upload/logs/complete; credential и lease token хранятся только hash-ами; `work:poll` claim-ит compatible durable `job_queue` row по `required_tags ⊆ runner.tags` и current `shell` executor compatibility (`capabilities.executorKinds` отсутствует или содержит `shell`), умеет bounded long-poll wakeup после enqueue/разблокировки стадии через in-process signal и PostgreSQL `LISTEN/NOTIFY` channel `runner_work_available`, отдаёт `workspace.checkoutUrl`, declared `attempt.secrets` и `attempt.artifacts`; отдельный `forge-runner` shell process умеет checkout, scoped secret resolve после ack, renew, active-lease heartbeat во время выполнения, polling cancel signal, declared artifact upload, stdout/stderr log append с masking и terminal completion; runtime maintenance requeue-ит unacknowledged offer после `ackDeadline`, fail-ит просроченную dispatch-eligible очередь без compatible runner-а и переводит stale online runner без unexpired active lease в `offline`, в том числе при `CICD_EMBEDDED_RUNNER_ENABLED=false`; richer log chunks, protected tags/pools, advanced capability matching, fairness policy и Docker/Kubernetes sandbox остаются target |
| Логи | ✅ | append-only внутри attempt, sequence per attempt, совместимый REST array shortcut, bounded `/logs/page` с `limit/after/q`, SSE stream текущей/последней attempt, latest attempt diagnostic на terminal job card в деталях pipeline и явный 1 MiB body limit на append endpoints |
| Артефакты | ✅ MVP | upload/download ≤50 MiB, route-level body limit 50 MiB, локальный `CICD_ARTIFACTS_DIR`; новые metadata привязаны к active/latest attempt, содержат SHA-256 и `expires_at`; download проверяет canonical path containment, checksum drift и не отдаёт expired/purged записи; retention worker удаляет expired local files и ставит `purged_at` |
| Секреты проектов | ✅ | AES-256-GCM at rest; значение не возвращается user API; execution выдаёт только job-declared имена секретов |
| Environments/deployments | ✅ MVP | metadata + append-only deployment history; protected environments require approval before backend starts the linked deployment pipeline, decisions are stored in `deployment_approvals`, and rollback creates a separate `rollback_of_id` deployment record. Richer policy rules, multi-approver workflows and rollback orchestration remain target |
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
| Auth/RBAC | ✅ conditional | если `CICD_AUTH_SECRET` задан непустым: argon2id+JWT+PAT, session-bound access invalidation, refresh rotate/logout/revoke, route role-политики, project memberships, Git Smart HTTP project checks, configurable CORS allowlist, аудит login/logout/denied; без секрета trusted-network mode |
| Secret injection | ✅ MVP | embedded runner передаёт в env только `jobs.required_secrets`; external runner получает declared secrets через lease-scoped `secrets:resolve` после ack; stdout/stderr masking best-effort |
| Error envelope + request_id | ✅ | {error:{code,message,request_id}} + x-request-id |
| Pagination | ✅ | limit/offset (cap 200) на проектах/пайплайнах |
| Rate limiting / body limits | ✅ MVP | in-process per-client fixed-window: auth, API read/write, Git Smart HTTP, internal hook и artifact upload возвращают `429`; явные body limits покрывают artifact uploads, Git RPC, JUnit upload и log append |
| Health/readiness/metrics | ✅ | `/api/v1/health` liveness без БД; `/api/v1/readiness` проверяет PostgreSQL и SQLx migration versions/checksums; `/metrics` Prometheus text |
| Compose packaging smoke | ✅ | CI job `compose-smoke` выполняет `docker compose config -q`, production image build, `docker compose up --build -d`, backend health/readiness и frontend nginx smoke с cleanup |
| Browser E2E / accessibility / performance smoke | ✅ MVP | CI job `e2e` поднимает собранный Docker Compose stack, запускает deterministic `frontend/scripts/seed-evidence.mjs`, проверяет Playwright Chromium critical journeys (Dashboard → project pipelines → pipeline plan/logs/artifacts, repository code browser, mobile drawer Escape/focus), axe smoke без `serious`/`critical` violations на representative pages, seeded API read p95 и Dashboard route ready-time against MVP regression budgets; Lighthouse, full keyboard audit, all-route a11y, load test и 30-day SLO evidence остаются target |
| RU/EN i18n contract | ✅ MVP | `frontend/src/shared/i18n/i18n-contract.test.ts` проверяет parity ключей `ru`/`en`, непустые значения, отсутствие raw-key fallback, runtime switch i18next и динамические переводы для стабильных API contract values: pipeline/change/PR/runner/delivery/notification statuses, PR actions и PAT scopes; full locale E2E и полный stable identifier contract suite остаются target |
| CLI | ✅ MVP | `cicd-cli` остаётся HTTP-only и покрывает runtime/platform операции: projects/pipelines/jobs/logs/attempts, runners, secrets, artifacts, environments/deployments, schedules, webhooks/outbox, notifications, reports/audit, users, project members и API tokens; поддержаны `CICD_API_TOKEN`/`--token`, `CICD_OUTPUT`/`--output json|table`, `--limit`/`--offset` для projects/pipelines; profile/keyring/YAML/NDJSON/real-API CLI integration gate остаются target |
| Backup/restore helper | ✅ MVP | `scripts/forge_backup.py` + wrappers создают/проверяют/restoring local Docker Compose backup: PostgreSQL custom dump, Git/artifact volume copy, `SHA256SUMS`, `manifest.json`; off-site/PITR/monthly drill остаются target |
| Dependency audit / secret scan / SBOM hygiene | ✅ MVP | CI запускает OpenAPI backward compatibility diff, SQLx optional MySQL/RSA feature guard, `cargo build --release --workspace`, `cargo audit --ignore RUSTSEC-2023-0071`, `pnpm audit --audit-level high`, `scripts/scan_secrets.py`, `scripts/generate_sbom.py --check` и pinned Trivy critical container image scan через `scripts/scan_container_images.sh`; `docs/assets/sbom.json` синхронизирован с Cargo/npm inventory; Cargo resolved graph не содержит deprecated `serde_yaml`/`unsafe-libyaml`; OpenAPI examples validation, `cargo-deny`, deeper history/container secret scan и release SBOM publication остаются target |

## Не реализовано (Target approved — см. ADR + contracts)

Production-grade runner dispatch fairness, resumable/chunked artifact sessions, idempotent chunked log upload, Docker/Kubernetes sandbox, pool/protected-tag policy и advanced capability matching (ADR-0007), policy-aware pipeline planner поверх v1 DAG (`on`, retry, `artifacts.expire_in`, line/column diagnostics, job-level dispatcher), full secret redaction/rotation/environment policy, richer protected-environment policy rules, multi-approver delivery workflows и rollback orchestration, general idempotency storage for all retryable mutations, command spans/stream classification для диагностических логов, artifact object storage/tenant isolation/legal hold, off-site/PITR backup platform и verified restore drill, external notification channel adapters (email/Slack), inbound provider webhook handlers, tenant isolation, service-account tokens, scoped Git credentials, production cookie/CSRF/session-family policy, schedule IANA timezone/DST/misfire и multi-replica leases, outbox lease/fencing/crash recovery, full dead-letter operator policy/metrics, OpenAPI examples validation/deprecation lifecycle, full locale E2E/stable identifier suite, Lighthouse/load reports и 30-day SLO evidence, `cargo-deny` license/source policy, broader container/image policy beyond current critical CVE scan, deeper history/container secret scan, release SBOM publication и distributed/proxy rate/time/concurrency limiting (сейчас in-process окно по forwarded client key и route-level body limits).

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
├── migrations/         # versioned SQLx migrations incl. 0023 artifact retention
├── domain/             # cicd-domain: чистые типы + JobStatus
├── cli/                # cicd-cli: HTTP-only runtime/platform commands
├── tests/              # api_contract, domain_transitions, integration_db (+ sql/init-roles.sql)
├── cli/tests/          # cli_contract
└── docker-compose.test.yml
```

## Известные dev-only риски

- Без непустого `CICD_AUTH_SECRET` API и Dashboard полностью открыты в trusted-network режиме. Пустой `CICD_CORS_ALLOWED_ORIGINS` оставляет permissive CORS для isolated development; shared deployment обязан задать allowlist origins.
- PostgreSQL в compose опубликован только на `127.0.0.1`, но API/Dashboard host ports нельзя открывать в недоверенную сеть.
- `CICD_GIT_INTERNAL_TOKEN` пустой по умолчанию только для isolated local development; shared-деплой обязан задать уникальный токен, а legacy `forge-internal-dev-token` отклоняется при старте.
- Auth/RBAC пока без tenant isolation, service-account tokens, scoped Git credentials и production-grade cookie/CSRF/session-family policy; session-bound access invalidation, refresh rotate/logout/revoke, project membership, scoped PAT, configurable CORS allowlist и Git read/write checks реализованы как MVP-слой поверх глобальных ролей.
- Execution attempts / job queue / job leases — MVP-слой: old `/jobs/{id}/logs` читает текущую или последнюю attempt, bounded `/jobs/{id}/logs/page` поддерживает `limit/after/q`, полный аудит попыток доступен через `/jobs/{id}/attempts`, `job_queue` переживает restart и является источником claim для embedded/external runners, embedded берёт только untagged rows, внешний runner protocol уже проверяет runner credential, lease token, fencing generation, tag compatibility и current `shell` executor compatibility по `capabilities.executorKinds`, принимает stdout/stderr log append, выдаёт только declared secrets после ack, принимает declared artifact upload, поддерживает bounded long-poll `work:poll` через in-process + PostgreSQL `LISTEN/NOTIFY` wakeup, requeue-ит unacknowledged offer после `ackDeadline`, fail-ит dispatch-eligible queue timeout без compatible execution path, доставляет cancel signal через lease `control` и переводит stale online runner без unexpired active lease в `offline`. `forge-runner` даёт отдельный shell process со scoped secret env + masking + artifact upload + active-lease heartbeat + cancel polling, но production sandbox, richer log chunks, protected tags/pools/advanced capabilities, fairness и расширенная restart/race suite ещё target.
- Scheduler/outbox — MVP: есть строгий 5-польный UTC cron и уникальные fire slots, но нет IANA timezone/DST/misfire, lease/fencing/crash-safe dispatcher-а, full dead-letter operator policy/metrics и внешних notification adapters; bounded delivery history/requeue и `in_app`/`sse` local outbox projection уже работают.

## Верификационные команды

```bash
docker compose config -q
docker compose -f backend/docker-compose.test.yml config -q
just readiness
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test --workspace'
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test -p cicd-cli --test cli_contract'
cd frontend && pnpm test && pnpm build
cd frontend && pnpm e2e   # requires running seeded Compose stack
cd frontend && pnpm lint
python3 scripts/generate_sbom.py --check
bash scripts/scan_container_images.sh forge-cicd-backend:ci forge-cicd-frontend:ci
python3 scripts/verify_docs.py --canonical --links --current-state
```

## Frontend: 21 маршрут / 20 рабочих страниц + /login

Полный список базовых страниц — `docs/architecture/frontend-boundaries.md`; визуальный реестр — `docs/assets/screens/manifest.md`. Исполняемый route smoke — `frontend/src/app/router.test.tsx`: production `appRoutes` поднимаются в memory router, а 20 рабочих Dashboard-страниц + `/login` проверяются на первый рендер с mocked API DTO. Real-browser baseline — `frontend/e2e/critical-flows.spec.ts` и `frontend/e2e/accessibility.spec.ts` против собранного Compose stack с deterministic seed.
