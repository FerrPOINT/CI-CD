# Полное техническое задание Forge CI/CD

## 1. Общее описание

Self-hosted CI/CD control plane (GitLab CI / Jenkins-like). Хранит состояние проектов-репозиториев, пайплайнов, стадий, задач и append-only логов. В MVP задачи переводятся вручную через API, CLI или Dashboard; удалённые runner-агенты, webhooks, secrets, artifacts, YAML-парсинг и RBAC отложены по фазам roadmap.

**Стек:** Rust 2024 (Axum 0.8, SQLx 0.8, PostgreSQL 17), React 19 (Vite 6, Tailwind CSS 4, shadcn/ui). Env prefix `CICD_`. Порты: API `22801`, Dashboard `22802`, PostgreSQL `22543`.

> **Source of truth:** актуальная реализация в `backend/src/` и `frontend/src/`. При расхождении приоритет у исходного кода. См. `docs/ARCHITECTURE.md`, `docs/DATA_MODEL.md`, `docs/API.md`.

---

## 2. Пользователи и роли

### 2.1. Глобальные роли

| Роль | Описание |
|------|----------|
| **Админ** | Управление пользователями, системными настройками, audit log, инстансом в целом |
| **Разработчик** | Создание/редактирование проектов, запуск пайплайнов, управление статусами задач, добавление логов |
| **Viewer** | Только просмотр проектов, пайплайнов, стадий, задач и логов (read-only) |

> **Текущий статус:** аутентификация и RBAC не реализованы (Phase 0). Все endpoint публичны. Роли — целевая модель, вводится в Phase 1 (Auth) и Future (RBAC). См. `docs/ROADMAP.md`, `docs/SECURITY.md`.

### 2.2. Permissions (целевые)

| Permission | Админ | Разработчик | Viewer |
|-----------|-------|-------------|--------|
| Administer system | ✅ | ❌ | ❌ |
| Create project | ✅ | ✅ | ❌ |
| Edit project | ✅ | ✅ | ❌ |
| Delete project | ✅ | ✅ | ❌ |
| Trigger pipeline | ✅ | ✅ | ❌ |
| Transition job status | ✅ | ✅ | ❌ |
| Append job log | ✅ | ✅ | ❌ |
| View projects/pipelines/jobs/logs | ✅ | ✅ | ✅ |
| Manage users | ✅ | ❌ | ❌ |
| View audit log | ✅ | ❌ | ❌ |

---

## 3. Проекты

### 3.1. Атрибуты проекта

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | UUID v4 | Первичный ключ |
| `name` | TEXT | Уникальное имя проекта (non-empty) |
| `repository_url` | TEXT | URL Git-репозитория |
| `default_branch` | TEXT | Ветка по умолчанию, default `"main"` |
| `created_at` | TIMESTAMPTZ | Время создания, default `now()` |

### 3.2. Операции

| Метод | Путь | Назначение | Статус |
|-------|------|-----------|--------|
| `GET` | `/api/v1/projects` | Список всех проектов | ✅ Phase 0 |
| `POST` | `/api/v1/projects` | Создание проекта | ✅ Phase 0 |
| `GET` | `/api/v1/projects/{id}` | Детали проекта | Planned Phase 2 |
| `PATCH` | `/api/v1/projects/{id}` | Редактирование | Planned Phase 2 |
| `DELETE` | `/api/v1/projects/{id}` | Удаление (CASCADE) | Planned Phase 2 |

### 3.3. Валидация

- `name` — non-empty, unique. Duplicate → 500 (unique constraint violation).
- `repository_url` — non-empty, Git URL format (план: валидация формата в Phase 2).
- `default_branch` — optional, default `"main"`.

### 3.4. Удаление проекта

- `DELETE /projects/{id}` удаляет проект и каскадно все дочерние пайплайны, стадии, задачи и логи (`ON DELETE CASCADE`).
- Плановое: подтверждение в UI, soft-delete / архивация (Future).

---

## 4. Пайплайны (Pipelines)

### 4.1. Атрибуты

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | UUID v4 | PK |
| `project_id` | UUID | FK → `projects.id`, CASCADE |
| `git_ref` | TEXT | Git-реф (ветка, тег, SHA) |
| `status` | TEXT | `queued` / `running` / `success` / `failed` / `canceled` |
| `created_at` | TIMESTAMPTZ | Время создания |
| `started_at` | TIMESTAMPTZ? | Время начала (при `running`) |
| `finished_at` | TIMESTAMPTZ? | Время завершения (терминальный статус) |

### 4.2. Операции

| Метод | Путь | Назначение | Статус |
|-------|------|-----------|--------|
| `GET` | `/api/v1/projects/{project_id}/pipelines` | Последние 50 пайплайнов | ✅ Phase 0 |
| `POST` | `/api/v1/projects/{project_id}/pipelines` | Запуск пайплайна (template) | ✅ Phase 0 |
| `GET` | `/api/v1/pipelines/{pipeline_id}` | Детали (stages + jobs) | ✅ Phase 0 |

### 4.3. Template pipeline (Phase 0)

При триггере создаётся 3 стадии с одной задачей в каждой:

| Position | Stage | Job | Image | Command |
|---|---|---|---|---|
| 0 | `build` | `checkout` | `alpine/git:latest` | `git fetch --all` |
| 1 | `test` | `unit-tests` | `rust:1.86` | `cargo test` |
| 2 | `deploy` | `deploy` | `alpine:3.21` | `echo deploy` |

Все задачи — статус `queued`. В будущем конфигурация загружается из YAML (Phase 3).

### 4.4. Плановое (Phase 3)

- YAML-конфиг `.forge-ci.yml` — парсинг stages/jobs из репозитория.
- Custom stages: произвольное количество, произвольные имена, произвольные jobs.
- Параллельные jobs внутри стадии.
- Зависимости между stages (DAG вместо линейной последовательности).
- Frontend: визуализация DAG stages, collapsible job details.

---

## 5. Стадии (Stages)

### 5.1. Атрибуты

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | UUID v4 | PK |
| `pipeline_id` | UUID | FK → `pipelines.id`, CASCADE |
| `name` | TEXT | Название (`build`, `test`, `deploy`) |
| `position` | INTEGER | Порядок выполнения (0, 1, 2, ...) |
| `status` | TEXT | Агрегированный статус из jobs |

### 5.2. Констрейнты

- `UNIQUE(pipeline_id, position)` — позиция уникальна в рамках пайплайна.
- `CHECK (status IN ('queued','running','success','failed','canceled'))`.

### 5.3. Агрегация статусов

Статус stage вычисляется из статусов всех её jobs (см. раздел 10 и `docs/WORKFLOW.md`).

---

## 6. Задачи (Jobs)

### 6.1. Атрибуты

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | UUID v4 | PK |
| `stage_id` | UUID | FK → `stages.id`, CASCADE |
| `name` | TEXT | Название (`checkout`, `unit-tests`, `deploy`) |
| `image` | TEXT | Docker-образ (`rust:1.86`, `alpine:3.21`) |
| `command` | TEXT | Команда (`cargo test`, `git fetch --all`) |
| `position` | INTEGER | Порядок внутри стадии |
| `status` | TEXT | `queued` / `running` / `success` / `failed` / `canceled` |
| `started_at` | TIMESTAMPTZ? | Время начала |
| `finished_at` | TIMESTAMPTZ? | Время завершения |

### 6.2. Констрейнты

- `UNIQUE(stage_id, position)` — позиция уникальна в рамках стадии.
- `CHECK (status IN ('queued','running','success','failed','canceled'))`.

### 6.3. Операции

| Метод | Путь | Назначение | Статус |
|-------|------|-----------|--------|
| `POST` | `/api/v1/jobs/{job_id}/status` | Смена статуса (валидация transition) | ✅ Phase 0 |
| `GET` | `/api/v1/jobs/{job_id}/logs` | Список логов | ✅ Phase 0 |
| `POST` | `/api/v1/jobs/{job_id}/logs` | Добавление строки лога | ✅ Phase 0 |

### 6.4. Переходы статусов

Реализовано в `domain.rs`, `JobStatus::transition_to()` — единственный источник правды.

| From | To | Результат |
|---|---|---|
| `queued` | `running` | ✅ Ok, `started_at = now()` |
| `queued` | `canceled` | ✅ Ok, `finished_at = now()` |
| `queued` | `success` | ❌ InvalidTransition |
| `queued` | `failed` | ❌ InvalidTransition |
| `running` | `success` | ✅ Ok, `finished_at = now()` |
| `running` | `failed` | ✅ Ok, `finished_at = now()` |
| `running` | `canceled` | ✅ Ok, `finished_at = now()` |
| terminal | * | ❌ TerminalStatus |

После обновления статуса job каскадно пересчитываются статусы stage и pipeline (`refresh_statuses`).

---

## 7. Логи (Job Logs)

### 7.1. Атрибуты

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | BIGSERIAL | Автоинкрементный PK |
| `job_id` | UUID | FK → `jobs.id`, CASCADE |
| `sequence` | INTEGER | Порядковый номер в рамках job |
| `message` | TEXT | Текстовая строка лога |
| `created_at` | TIMESTAMPTZ | Время записи, default `now()` |

### 7.2. Констрейнты

- `UNIQUE(job_id, sequence)` — последовательность уникальна в рамках job.
- `sequence` вычисляется сервером: `COALESCE(MAX(sequence), 0) + 1`.
- Append-only: редактирование и удаление логов не поддерживается.

### 7.3. Плановое (Phase 4)

- Streaming логов через SSE (`GET /jobs/{id}/logs/stream`).
- Pagination `?since_sequence=N`.
- Truncation для длинных логов (chunked storage).
- Frontend: real-time log viewer с авто-скроллом, search, фильтрация по уровню.
- CLI: `job logs --follow` (streaming).

---

## 8. Runner (Phase 5)

**Цель:** выполнение задач в реальных контейнерах.

### 8.1. Функциональные требования

- Runner registration: `runners` таблица, `POST /runners/register` (token-based).
- Runner agent: отдельный процесс, подключается к API, забирает jobs из очереди.
- Job execution: `docker run` с указанным `image` и `command`, стриминг stdout/stderr в `job_logs`.
- Job lifecycle: API автоматически переводит `queued → running → success/failed` через runner.
- Runner heartbeat: `POST /runners/{id}/heartbeat`.
- Concurrency: несколько runners, job locking (`SELECT FOR UPDATE SKIP LOCKED`).
- Frontend: runner status в admin, real-time job progress.

### 8.2. Критерии приёмки

- Runner e2e test: запуск реального `alpine:3.21 echo hello` → `job.status = success`, логи содержат `hello`.

---

## 9. Webhooks (Phase 6)

**Цель:** уведомление внешних систем о событиях + входящие webhooks от Git-провайдеров.

### 9.1. Исходящие webhooks

- `webhooks` таблица: URL, events, secret, project_id.
- `webhook_deliveries` таблица: попытки доставки, статус, response code.
- Events: `pipeline.started`, `pipeline.finished`, `job.started`, `job.finished`, `job.failed`.
- Endpoint `POST /projects/{id}/webhooks` — регистрация webhook.
- Delivery: async background task, retry с exponential backoff (3 attempts).
- HMAC-SHA256 подпись payload'а.

### 9.2. Входящие webhooks

- `POST /api/v1/webhooks/incoming` — приём push/PR/tag от GitHub/GitLab/Gitea.
- Проверка подписи (HMAC-SHA256 / plain token).
- Маппинг `repository_url` → проект → запуск пайплайна.

### 9.3. Критерии приёмки

- Webhook delivery test, retry test, signature verification.
- Incoming webhook от GitHub push → pipeline создан.

> См. `docs/WEBHOOKS.md`, `docs/NOTIFICATIONS.md`.

---

## 10. Статусная модель и агрегация

### 10.1. Статусы

Все три сущности (pipeline, stage, job) используют один набор:

| Статус | Описание | Терминальный |
|---|---|---|
| `queued` | Создан, ожидает выполнения | Нет |
| `running` | Выполняется | Нет |
| `success` | Завершён успешно | Да |
| `failed` | Завершён с ошибкой | Да |
| `canceled` | Отменён | Да |

### 10.2. Агрегация: jobs → stage → pipeline

| Условие | Результат |
|---|---|
| Все потомки `queued` | `queued` |
| Хотя бы один `running`, остальные `queued`/`running` | `running` |
| Все `success` | `success` |
| Хотя бы один `failed`, остальные терминальные | `failed` |
| Хотя бы один `canceled`, остальные терминальные (без `failed`) | `canceled` |
| `failed` и `canceled` одновременно | `failed` (приоритет ошибки) |

**Приоритет:** `failed` > `canceled` > `running` > `queued` > `success`.

> См. `docs/WORKFLOW.md`, `docs/DOMAIN_MODEL.md`.

---

## 11. Secrets (Phase 7)

**Цель:** безопасное хранение секретов проектов.

- `secrets` таблица: `project_id`, `key`, `encrypted_value`, `created_at`.
- Шифрование: AES-256-GCM, ключ из `CICD_SECRETS_KEY` env var.
- Endpoints: `GET/POST/DELETE /projects/{id}/secrets`.
- Secrets доступны runner'ам через env vars при выполнении job.
- Маскирование секретов в логах (replace на `***`).

> См. `docs/SECRETS_MGMT.md`.

---

## 12. Artifacts (Phase 8)

**Цель:** хранение артефактов сборки.

- `artifacts` таблица: `job_id`, `filename`, `size_bytes`, `storage_key`, `created_at`.
- Storage: локальная файловая система (`CICD_STORAGE` dir) или S3-compatible.
- Endpoints: `POST /jobs/{id}/artifacts` (multipart upload), `GET /artifacts/{id}/download`.
- Лимит размера (default 100 МБ), TTL для автоматической очистки.

> См. `docs/ARTIFACTS.md`.

---

## 13. Отчёты (Phase 9)

**Цель:** сбор и визуализация CI/CD метрик.

- Pipeline success rate за период.
- Average duration, percentiles (p50, p90, p95, p99).
- Deployment frequency.
- Failure trends.
- Frontend: charts (`recharts`), `/admin` page с tab Reports.

> См. `docs/REPORTS.md`, `docs/SYSTEM_ADMIN.md`.

---

## 14. Нефункциональные требования

### 14.1. Производительность

- P95 ответа API < 200 мс при 50 RPS на типичных запросах.
- Загрузка деталей пайплайна (stages + jobs) < 100 мс.
- Запись лога задачи (append) < 50 мс.
- 99.5% uptime на single-instance deployment.

> Цели ориентировочные — load testing запланирован в Phase 2+. См. `docs/PERFORMANCE.md`.

### 14.2. Безопасность

- Все secrets — через env vars с префиксом `CICD_`.
- SQL только через parameterized queries (`sqlx::query` с `$1`, `$2`, ...).
- Не коммитить `.env`, токены, пароли, реальные данные.
- Argon2id для паролей (Phase 1), JWT access + httpOnly refresh cookie (Phase 1).
- RBAC на всех уровнях (Future).
- Audit log для админ-действий (Phase 9).
- HMAC-SHA256 подпись webhook payload'ов (Phase 6).
- AES-256-GCM шифрование секретов (Phase 7).

> См. `docs/SECURITY.md`.

### 14.3. Надёжность

- PostgreSQL — единственное постоянное хранилище (ADR-0004).
- `ON DELETE CASCADE` на всех уровнях иерархии.
- Идемпотентная миграция схемы при старте (`store::migrate()`, `CREATE TABLE IF NOT EXISTS`).
- API не запускается без доступной БД.
- Docker Compose health checks: PostgreSQL → backend → frontend.
- Graceful shutdown: обработка `SIGTERM`/`SIGINT` (целевая доработка).
- Backup: `pg_dump` по cron, восстановление, проверка бэкапов.

> См. `docs/RESILIENCE.md`, `docs/BACKUP_RESTORE.md`, `docs/RUNTIME.md`.

### 14.4. Локализация и темы

- Языки: `ru`, `en` (i18next).
- Темы: `dark` (default), `light`, `system`.
- Date/time форматы по локали.

> См. `docs/I18N.md`, `docs/UI_UX.md`.

---

## 15. Критерии приёмки

### 15.1. Общие (каждая фаза)

- Код покрыт тестами: unit + integration + critical e2e.
- `cargo test` green, `pnpm test` green.
- `cargo clippy` и `cargo fmt --check` чистые.
- CI (GitHub Actions) green.
- Документация обновлена (`docs/API.md`, `docs/DATA_MODEL.md`, `docs/ROADMAP.md`).
- Скриншоты UI (если применимо) приложены.
- Ручная проверка через curl/UI пройдена.

### 15.2. Phase 0 (MVP) — ✅ Done

- [x] `docker compose up` → 3 сервиса healthy.
- [x] `curl /api/v1/health` → 200 OK.
- [x] CRUD проектов: create, list.
- [x] Trigger pipeline → 3 стадии, 3 задачи, все `queued`.
- [x] Job status transitions: `queued → running → success/failed/canceled`.
- [x] Append log → `sequence` автоинкремент.
- [x] Каскадная агрегация: job → stage → pipeline.
- [x] Dashboard: проекты, пайплайны, детали с stages/jobs/logs.
- [x] CLI: `project`, `pipeline`, `job` command groups.

### 15.3. Phase 1 (Auth)

- [ ] e2e login flow, token refresh, logout, protected route redirect.

### 15.4. Phase 2 (Projects)

- [ ] CRUD curl-проверки: `GET/PATCH/DELETE /projects/{id}`.
- [ ] Frontend: форма редактирования, удаление с подтверждением.
- [ ] Pagination `GET /projects?page=0&size=20`.

### 15.5. Phase 3 (Pipelines)

- [ ] YAML-парсинг тесты.
- [ ] API integration: custom stages/jobs.
- [ ] Frontend e2e: DAG visualization.

### 15.6. Phase 4 (Logs)

- [ ] SSE connection test.
- [ ] Pagination test (`?since_sequence=N`).
- [ ] Frontend e2e: real-time log viewer.

### 15.7. Phase 5 (Runner)

- [ ] Runner e2e: `alpine:3.21 echo hello` → `success`, логи содержат `hello`.

### 15.8. Phase 6 (Webhooks)

- [ ] Webhook delivery test, retry test, signature verification.
- [ ] Incoming webhook → pipeline создан.

### 15.9. Phase 7 (Secrets)

- [ ] Encryption/decryption unit tests.
- [ ] Masking test (секреты заменены на `***` в логах).
- [ ] API integration.

### 15.10. Phase 8 (Artifacts)

- [ ] Upload/download test.
- [ ] Size limit test.
- [ ] Cleanup test (TTL).

### 15.11. Phase 9 (Admin + Reports)

- [ ] Admin API integration.
- [ ] Frontend component tests.
- [ ] Report accuracy (success rate, duration).

---

## 16. Границы MVP

### 16.1. Входит в MVP (Phase 0)

- Проекты: create, list.
- Пайплайны: trigger (template), list, show detail.
- Задачи: status transitions (ручные), logs (append + list).
- Каскадная агрегация статусов.
- Dashboard: проекты, пайплайны, детали.
- CLI: `project`, `pipeline`, `job`.
- Docker Compose, CI (GitHub Actions).

### 16.2. Не входит в MVP (отложено по фазам)

- Аутентификация и RBAC (Phase 1, Future).
- Редактирование и удаление проектов (Phase 2).
- YAML-конфиг пайплайнов, DAG stages, параллельные jobs (Phase 3).
- Streaming логов, pagination, search (Phase 4).
- Реальные runner-агенты, выполнение в контейнерах (Phase 5).
- Webhooks, уведомления, SSE (Phase 6).
- Secrets, шифрование (Phase 7).
- Artifacts, S3 storage (Phase 8).
- Admin panel, audit log, отчёты, метрики (Phase 9).
- Git integration: auto-trigger на push (Future).
- Matrix builds, manual approval gates, scheduled pipelines (Future).
- OIDC/OAuth SSO, Prometheus metrics, rate limiting (Future).

---

## 17. User stories по ролям

### Админ

- Я могу управлять пользователями и системными настройками.
- Я могу просматривать audit log.
- Я могу просматривать метрики и отчёты инстанса.

### Разработчик

- Я могу создать проект и указать URL репозитория.
- Я могу запустить пайплайн для Git-рефа.
- Я могу перевести задачу в новый статус (start/pass/fail/cancel).
- Я могу добавлять логи к задаче.
- Я могу просматривать детали пайплайна, стадии, задачи и логи.

### Viewer

- Я могу просматривать проекты, пайплайны, стадии, задачи и логи (read-only).

---

## 18. CLI

`cicd-cli` — отдельный бинарник, работает через публичный API.

```bash
# Project
cicd-cli project list
cicd-cli project create --name "my-service" --repository-url "git@..." --branch main

# Pipeline
cicd-cli pipeline list --project <uuid>
cicd-cli pipeline run --project <uuid> --git-ref main
cicd-cli pipeline show --id <uuid>

# Job
cicd-cli job start --id <uuid>     # POST status=running
cicd-cli job pass --id <uuid>      # POST status=success
cicd-cli job fail --id <uuid>      # POST status=failed
cicd-cli job logs --id <uuid>      # GET logs
cicd-cli job log --id <uuid> --message "..."  # POST log
```

`CICD_API_URL` env var задаёт URL API (default `http://127.0.0.1:22801`).

> См. `docs/CLI.md`, `docs/API.md`.

---

## 19. Подход к реализации

- Каждая фаза — отдельный milestone, заканчивается рабочим коммитом и проверкой.
- Все фичи сначала проектируются в документах, затем API + тесты, затем UI.
- Код не пишется, пока не зафиксирована дата-модель и API-контракт.
- Backend: слои `api → domain → store`. HTTP-хендлеры не обращаются к БД напрямую.
- Frontend: shadcn/ui + Tailwind, типизированные API-клиенты, TanStack Query.
- При изменении API обновлять `docs/API.md`; при изменении схемы — `docs/DATA_MODEL.md`.
- Архитектурные решения фиксируются в ADR (`docs/adr/`).

---

## 20. References

- `docs/ARCHITECTURE.md` — архитектура и стек.
- `docs/DATA_MODEL.md` — дата-модель.
- `docs/API.md` — REST API спецификация.
- `docs/ROADMAP.md` — план разработки по фазам.
- `docs/WORKFLOW.md` — статусная модель и агрегация.
- `docs/DOMAIN_MODEL.md` — доменные сущности и инварианты.
- `docs/SECURITY.md` — безопасность.
- `docs/PERFORMANCE.md` — производительность.
- `docs/RESILIENCE.md` — надёжность.
- `docs/CODE_REVIEW.md` — чек-лист review.
- `docs/ADR.md` — индекс архитектурных решений.