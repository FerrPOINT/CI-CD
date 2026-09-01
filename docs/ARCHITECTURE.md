# Архитектура Forge CI/CD

## 1. Контекст

Self-hosted CI/CD control plane: Git-хостинг (bare-репозитории + Smart HTTP + post-receive auto-trigger), пайплайны со стадиями и джобами, embedded runner (Docker/shell), внешний `forge-runner` shell MVP поверх runner protocol, платформенные ресурсы (runners, secrets, artifacts, environments, schedules, webhooks, notifications, reports, audit, users, tokens) и React Dashboard.

> **Переходный период.** Backend мигрирует с монолитного crate на Cargo workspace со слоями (ADR-0005). Ниже описана целевая архитектура; текущее состояние каждого среза отмечено в «Статус миграции». До завершения миграции старые пути (`cicd::domain`, `backend/src/*`) работают через re-export shim и остаются источником истины для ещё не перенесённых вертикалей.

## 2. Технологический стек

### Backend

| Компонент | Библиотека | Версия |
|---|---|---|
| Язык | Rust | 1.86 (edition 2024) |
| Web framework | axum | 0.8 |
| Async runtime | tokio | 1 |
| DB | sqlx (PostgreSQL) | 0.8 |
| Git | git2 | 0.20 |
| Шифрование секретов | aes-gcm + sha2 | 0.10 |
| Config | env (`CICD_`) → typed config (в процессе) | — |
| CLI | clap + reqwest | 4 / 0.12 |
| Observability | tracing + tracing-subscriber | 0.1 / 0.3 |
| Ошибки | thiserror | 2 |

### Frontend

| Компонент | Библиотека | Версия |
|---|---|---|
| Framework | react + react-dom | 19 |
| Build | vite | 6 |
| Styling | tailwindcss + @tailwindcss/vite | 4 |
| Components | shadcn/ui | — |
| Router | react-router | 8 |
| Server state | @tanstack/react-query | 5 |
| Client state | zustand | 5 |
| i18n | i18next + react-i18next | 25 / 15 |
| Unit tests | vitest + @testing-library/react | 3 / 16 |
| Types | typescript | 5.9 |

### Infrastructure

- PostgreSQL 17, Docker Compose.
- Порты: API `22801`, Dashboard `22802`, PostgreSQL `22543`.
- Env-префикс `CICD_`.

## 3. Структура монорепозитория

```text
CI-CD/
├── backend/
│   ├── Cargo.toml            # workspace + unified deps (ADR-0005)
│   ├── domain/               # чистые бизнес-типы, port-trait'ы        [готово]
│   ├── cli/                  # cicd-cli: HTTP-клиент control plane     [готово]
│   ├── src/                  # серверный crate (мигрирует в app/infra/api/server)
│   │   ├── bin/forge-runner.rs # external shell runner process MVP
│   │   ├── api.rs            # HTTP-роуты проектов/пайплайнов/джобов
│   │   ├── platform.rs       # HTTP-роуты платформенных ресурсов
│   │   ├── git_host.rs       # bare-репо + Smart HTTP + post-receive
│   │   ├── pulls.rs          # refs/commits/compare/pull requests
│   │   ├── runner.rs         # embedded runner: Docker/shell, supervisor
│   │   ├── runner_protocol.rs # external runner protocol MVP
│   │   ├── store.rs          # shared DB helpers + next_log_sequence
│   │   └── domain.rs         # re-export shim → cicd-domain
│   ├── tests/                # integration: api_contract, domain, real-DB
│   └── target/               # build artifacts (gitignored)
├── frontend/                 # React SPA (pages, widgets, typed hooks)
│   └── src/{api,app,pages,shared,widgets}
├── docs/                     # guides, contracts, adr/ + screenshots/
├── plans/                    # committed working plans; non-normative
├── docker-compose.yml        # postgres + backend + frontend
└── justfile                  # unified commands
```

Целевая структура backend (полностью — в ADR-0005 и `plans/architecture-rebuild-plan.md`):

```text
backend/
├── domain/        # без axum/sqlx/fs
├── app/           # use-case'ы, границы транзакций
├── infra/         # PostgreSQL-репозитории, git/artifact/runner-адаптеры, миграции
├── api/           # DTO, роуты, middleware, OpenAPI
├── server/        # composition root
├── cli/           # отдельный HTTP-клиент
├── migration/     # версионные SQLx-миграции
├── tests/         # black-box real-DB тесты
└── scripts/       # test DB, backup/restore
```

## 4. Backend: слоистая архитектура (целевая)

### 4.1 Presentation (`api/`)

Тонкий HTTP-адаптер: извлечение path/query/body, валидация DTO, вызов use-case'ов из `app/`, маппинг `AppError` → HTTP. Не содержит SQL.

### 4.2 Application (`app/`)

Use-case'ы и политики: проект/пайплайн-оркестрация, lifecycle джобов и логов, git-операции, платформенные операции, RBAC-проверки, границы транзакций. Импортирует `domain`, не знает про HTTP и SQL.

### 4.3 Domain (`domain/`)

`JobStatus` + переходы (`transition_to()` — единственный источник правил), ID/newtype'ы, port-trait'ы для репозиторей. Без I/O.

### 4.4 Infrastructure (`infra/`)

PostgreSQL-реализации портов (sqlx), Git-хранилище (git2 + bare-репо), artifact storage (локальная ФС, далее S3), runner-адаптер (Docker/shell), шифрование секретов (AES-256-GCM), миграции.

### 4.5 Server (`server/`)

Composition root: чтение конфига, создание `PgPool`, репозиториев, адаптеров, запуск supervisor/scheduler, привязка роутера. Единственная точка, где собираются зависимости.

### 4.6 CLI (`cli/`)

`cicd-cli` — отдельный workspace-пакет: clap-команды `project`/`pipeline`/`job`, общается с API по HTTP (`CICD_API_URL`), не линкует серверный код.

## 5. Конфигурация

| Переменная | Назначение |
|---|---|
| `CICD_DATABASE_URL` | PostgreSQL connection string |
| `CICD_MIGRATIONS_DIR` | каталог committed SQLx migrations (`/app/migrations` в compose image) |
| `CICD_BIND` | адрес API (по умолчанию `0.0.0.0:22801`) |
| `CICD_GIT_ROOT` | путь к bare-репозиториям |
| `CICD_GIT_TOKEN` | legacy shared token Git Smart HTTP |
| `CICD_GIT_INTERNAL_TOKEN` | токен post-receive → pipeline hook |
| `CICD_CORS_ALLOWED_ORIGINS` | comma-separated allowlist browser origins для shared Dashboard/API; пусто = permissive isolated dev |
| `CICD_SECRETS_KEY` | base64 32-byte ключ AES-256-GCM |
| `CICD_ARTIFACTS_DIR` | локальное хранилище артефактов |
| `CICD_ARTIFACT_RETENTION_DAYS` | TTL новых артефактов; default 30, диапазон `1..3650` |
| `CICD_EMBEDDED_RUNNER_ENABLED` | `true` по умолчанию; `false` отключает embedded execution в backend |
| `CICD_RUNNER_MODE` | `host` в local compose; `docker`/`host` в backend binary |
| `CICD_RUNNER_REGISTRATION_TOKEN` | bootstrap token для `/api/v1/runner/register`; пусто отключает external registration |
| `CICD_RUNNER_CREDENTIAL` | bearer credential для уже зарегистрированного `forge-runner` |
| `CICD_RUNNER_NAME` / `CICD_RUNNER_TAGS` / `CICD_RUNNER_TOTAL_SLOTS` | identity/capacity внешнего `forge-runner` |
| `CICD_RUNNER_POLL_INTERVAL_SECONDS` / `CICD_RUNNER_NO_CHECKOUT` | empty-poll cadence с server `waitSeconds` cap 30s и dev/debug режим без Git checkout для внешнего `forge-runner` |
| `CICD_RUNNER_WORK_DIR` | workspace root внешнего `forge-runner` |
| `CICD_RUNNER_KEEP_WORKSPACE` | `1` — не удалять workspace джоба |

Целевая модель — typed config (группы Database/Http/Git/Artifacts/Runner/Auth) по образцу task-tracker; сейчас — прямое чтение env.

## 6. Ключевые механизмы

### 6.1 Pipeline lifecycle

`projects → pipelines → pipeline_plans` и `projects → pipelines → stages → jobs → execution_attempts → job_logs` (CASCADE). Статусы `queued → running → success/failed/canceled`; правила — `JobStatus::transition_to()`. Агрегация job → stage → pipeline (`refresh_statuses`). Ручной trigger pipeline поддерживает `Idempotency-Key`, а `pipeline_triggers` хранит replay/fingerprint для защиты от duplicate run при повторе запроса. Текущий `pipeline_plans` фиксирует immutable snapshot с raw config/template, parser version, config/plan SHA-256 и dependency edges: `legacy-linear` для старого `stages/jobs` и `v1-dag` для `version: 1` + top-level `jobs.needs`/`tags`. V1 DAG пока исполняется через топологические runtime-стадии `dag-*`; policy diagnostics, triggers и job-level dispatcher остаются target.

### 6.2 Git-хостинг

Bare-репозитории в `CICD_GIT_ROOT`, Smart HTTP (`/git/<name>.git`), public read для public repo, legacy `CICD_GIT_TOKEN` и JWT/PAT project-membership auth при `CICD_AUTH_SECRET`, auto-generated `post-receive` hook → internal endpoint → pipeline по pushed ref. Новый hook передаёт `old_rev/new_rev`; повтор same `repository/ref/new_rev` дедуплицируется. `.forge-ci.yml` из репозитория читается best-effort по resolved commit SHA и задаёт legacy stages/jobs либо v1 `jobs.needs`; если файл недоступен, используется `legacy_template` build/test/deploy, а выбранный raw config фиксируется в `pipeline_plans`.

### 6.3 Embedded runner

Supervisor-полл queued-джобов, атомарный lease-aware claim (`queued → running`) с active `execution_attempt` и `job_leases`, клонирование репо в workspace, выполнение в Docker (имя контейнера `forge-job-<id>`, volume workspace) или host shell, построчный стриминг stdout в attempt-owned `job_logs`, сбор declared artifact files из `jobs.artifact_paths`, bounded page/search API для длинных логов, kill-on-cancel через PID-map, закрытие lease на terminal result/cancel и reconciliation expired/missing lease, cleanup workspace (кроме `CICD_RUNNER_KEEP_WORKSPACE=1`).

### 6.4 External runner protocol MVP

`/api/v1/runner/*` реализует control-plane protocol slice для внешних runner-ов: register через `CICD_RUNNER_REGISTRATION_TOKEN`, heartbeat с tags/capacity/capabilities, immediate или bounded `work:poll`, lease `ack`, `renew`, `control`, `secrets:resolve`, artifact upload, `logs` и `complete`. Сервер хранит только hash runner credential и lease token, а mutating lease endpoints проверяют runner identity, token, `job_leases.generation` и expiry. `work:poll` claim-ит compatible durable `job_queue` row с `required_tags ⊆ runner.tags` и текущим executor `shell`, если `capabilities.executorKinds` отсутствует или явно содержит `shell`; long-poll просыпается по process-local signal и PostgreSQL `LISTEN/NOTIFY` после enqueue/unblock; затем отдаёт `workspace.checkoutUrl`, declared `attempt.secrets` и `attempt.artifacts`. `forge-runner` умеет зарегистрироваться, heartbeat-ить, подтвердить lease, получить только declared secrets после ack, продлевать lease, держать active-lease heartbeat во время выполнения, poll-ить cancel control signal, выполнить команды в shell workspace, загрузить declared artifact files, отправить stdout/stderr в attempt-owned `job_logs` с masking и отправить terminal result. Runtime reconciliation requeue-ит unacknowledged offer после `ackDeadline`, закрывает expired/missing leases и переводит stale online runner без unexpired active lease в `offline`; при отключённом embedded runner этот maintenance loop всё равно запускается. Resumable artifact sessions, richer log chunks, Docker/Kubernetes sandbox isolation и advanced pool/protected-tag/capability/fairness policy остаются target.

### 6.5 Платформенные ресурсы (MVP)

Runners (registry + heartbeat), execution attempts/retry history, secrets (AES-256-GCM at rest + job-declared embedded/external env injection/masking), artifacts (upload/download + локальное хранилище, 50 MiB лимит, SHA-256, `expires_at` и retention purge worker), environments/deployments, schedules MVP (strict 5-field UTC cron, `next_fire_at`, unique fire slots), outgoing webhooks MVP (terminal pipeline events через outbox/basic retry/HMAC), `in_app`/`sse` notifications MVP (local outbox history + Dashboard stream), reports (агрегаты success rate/duration), audit log (последние 200 событий), users/roles, project memberships, argon2id credentials, session-bound access JWT, refresh sessions с rotate/logout, project-scoped PAT enforcement и Git Smart HTTP project checks при `CICD_AUTH_SECRET`.

## 7. Frontend архитектура

- **pages/** — 20 рабочих экранов + login: dashboard, projects/project-members, repositories/browser/compare/pulls, pipelines/detail, runners, secrets, artifacts, environments, schedules, webhooks, reports, audit-log, users, settings, login.
- **shared/** — ui-kit (shadcn), i18n (ru/en), theme (dark/gray/light).
- **widgets/** — AppShell (sidebar + header + Outlet).
- **api/** — типизированный клиент/wrappers + generated OpenAPI schema `schema.d.ts`.
- Целевое: generated transport boundary после стабилизации API-слоя; текущие DTO уже генерируются из `openapi/openapi.yaml`.

## 8. Testing

- Domain: unit-тесты переходов статусов (`domain/src/lib.rs`, `tests/domain_transitions.rs`).
- API contract: no-DB тесты health/readiness/503/валидации (`tests/api_contract.rs`).
- CLI: contract-тест help-групп (`cli/tests/cli_contract.rs`).
- Real-PostgreSQL integration tests уже есть для migrations/readiness/project/auth paths, trigger idempotency, immutable `pipeline_plans`, v1 DAG config из bare repo с `required_tags`/`required_secrets`/`artifact_paths`, runner protocol register/heartbeat/poll/ack/control/`secrets:resolve`/artifact upload/renew/logs/complete с basic tag + executor capability matching, scoped secret delivery, attempt-owned artifacts, artifact expiry/purge и PostgreSQL `LISTEN/NOTIFY` long-poll wakeup. Playwright/axe current MVP покрывает representative browser journeys на собранном Compose stack; target остаётся для full auth/RBAC/CLI E2E, coverage gate и широких failure/protocol tests.

## 9. Статус миграции (ADR-0005)

| Срез | Статус |
|---|---|
| Workspace + `domain` пакет | ✅ готово |
| `cli` отдельный пакет | ✅ готово |
| typed config + `AppError` | ◩ частично: `AppError`/error envelope current, full typed config target |
| SQLx версионные миграции | ✅ current: `backend/migrations/*.sql` + runtime `sqlx::migrate::Migrator`/`cicd-migrate` |
| app/infra/api/server пакеты | ⬜ Phase C (strangler по вертикалям) |
| OpenAPI + генерация клиента | ✅ current: `openapi/openapi.yaml` + `frontend/src/api/schema.d.ts` |
| Auth/RBAC/token middleware | ◩ current conditional: JWT/scoped PAT/global roles + project memberships + session-bound access invalidation + refresh rotate/logout/revoke при `CICD_AUTH_SECRET`; tenant scope, service-account/scoped Git credentials и production cookie/CSRF/session-family policy target |
| Distributed runner protocol | ◩ current MVP: `/api/v1/runner/*` register/heartbeat/poll/ack/control/`secrets:resolve`/artifact upload/renew/logs/complete + durable `job_queue` + ack-timeout requeue + basic tag/executor capability matching + hashed credentials/lease tokens + fencing generation + scoped secret delivery + long-poll + active runner heartbeat + stale offline reconciliation + `forge-runner` shell process; resumable artifact sessions, richer log chunks, advanced runner policy и sandbox boundary остаются Phase D |

## 10. Dev workflow

```bash
just up             # docker compose up --build -d
just health         # curl /api/v1/health
just readiness      # curl /api/v1/readiness
just test-backend   # cargo test --workspace (в rust:1.86-bookworm)
just test-frontend  # pnpm test
just build-frontend # pnpm build
```

Backend-проверки без cargo на хосте описаны в `docs/DEVELOPMENT_GUIDE.md`; полный gate: fmt + clippy `--workspace --all-targets -D warnings` + test workspace + release build.

## 11. Deployment

Docker Compose: postgres + backend + frontend (nginx static + `/api`,`/git` proxy). Current MVP включает local backup/verify/restore helper для PostgreSQL, Git storage и artifacts. Restart-policy, monitoring, off-site/PITR backups и full restore drill — Phase E (см. `docs/ROADMAP.md`, `docs/DEPLOYMENT.md`).

## References

- `docs/adr/0005-workspace-layered-architecture.md` — workspace и слои
- `docs/adr/0003-manual-job-transitions.md` — история эволюции исполнения
- `plans/architecture-rebuild-plan.md` — поэтапный план миграции
- `docs/DATA_MODEL.md`, `docs/API.md`, `docs/GIT_HOSTING.md`
