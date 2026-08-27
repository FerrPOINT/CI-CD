# Roadmap — Forge CI/CD

## 1. Overview

План разработки от каркаса до production-ready self-hosted CI/CD control plane. Каждая фаза — отдельный milestone, заканчивается рабочим коммитом и проверкой.

## Статус (обновлено 2026-08-26)

**Phase 0 завершена.** Реализован каркас: Rust backend (Axum + SQLx), React frontend (Vite + Tailwind), Docker Compose, CLI, базовые CRUD-операции.

Реализовано в Phase 0:
- Rust workspace: `cicd-server` (API) + `cicd-cli` (CLI).
- PostgreSQL schema: `projects`, `pipelines`, `stages`, `jobs`, `job_logs`.
- REST API v1: health, projects CRUD, pipelines (list/trigger/show), jobs (status/logs).
- Доменная валидация переходов статусов (`JobStatus::transition_to`).
- Каскадная агрегация статусов job → stage → pipeline.
- React Dashboard: список проектов, пайплайнов, детали с stages/jobs/logs.
- Docker Compose: PostgreSQL 17.6 + backend + frontend.
- CLI: `project`, `pipeline`, `job` command groups.
- CI: GitHub Actions (fmt, clippy, test, build).
- Тесты: domain transitions, API contract, CLI contract, frontend unit.

Это control plane с embedded runner (Docker/shell) и platform-MVP: runners registry + heartbeat, encrypted secrets, artifacts, environments/deployments, schedules, webhooks/notifications config, reports, audit log, users/roles model, API tokens. Отложены: auth/RBAC enforcement, доставка webhook-ов, cron-scheduler execution, secret injection в runner-окружение.

---

## 2. Phase 0: Bootstrap (M0) — ✅ Done

**Цель**: рабочий каркас, CI, локальный запуск.

- [x] Rust workspace: `Cargo.toml`, edition 2024, crates `api/domain/store` (+ CLI bin).
- [x] Frontend: Vite 6 + React 19 + TypeScript + Tailwind CSS 4 + shadcn/ui.
- [x] Docker Compose: PostgreSQL 17.6-alpine, backend (rust:1.86-slim → debian), frontend (node:22 → nginx).
- [x] `.env.example` с `CICD_*` env vars, health endpoint.
- [x] CI (GitHub Actions): `cargo fmt --check`, `cargo clippy`, `cargo test`, `pnpm test`, `pnpm build`, `docker compose build`.
- [x] `README.md` с командами запуска и API workflow.
- [x] `justfile` с командами: `up`, `down`, `logs`, `test-backend`, `test-frontend`, `build-frontend`, `health`.
- [x] Verification: `docker compose up`, `curl /api/v1/health`.

---

## 3. Phase 1: Auth (M1)

**Цель**: аутентификация, пользователи, сессии.

- [ ] DB migrations: `users`, `sessions` таблицы.
- [ ] Argon2id password hashing.
- [ ] JWT access token (Bearer) + httpOnly refresh cookie.
- [ ] Endpoints: `POST /auth/register`, `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`.
- [ ] Middleware: JWT-валидация на защищённых маршрутах.
- [ ] Frontend: Login page, auth store, protected routes, user menu в topbar.
- [ ] CLI: `auth login`, `auth register` с сохранением токена.
- [ ] Verification: e2e login flow, token refresh, logout, protected route redirect.

---

## 4. Phase 2: Projects (M2) — ✅ Done (частично)

**Цель**: полноценное управление проектами.

- [x] Endpoints: `GET /projects/{id}`, `PATCH /projects/{id}`, `DELETE /projects/{id}`.
- [x] Frontend: страница Projects, форма редактирования, удаление с подтверждением.
- [ ] Валидация `name` (slug-pattern), `repository_url` (Git URL format).
- [ ] Pagination для `GET /projects` (`?page=0&size=20`).
- [ ] Verification: CRUD curl-проверки, frontend unit + e2e.

---

## 5. Phase 3: Pipelines + Stages + Jobs (M3)

**Цель**: гибкая конфигурация пайплайнов вместо template.

- [ ] YAML-конфиг пайплайна (`.forge-ci.yml` или аналог) — парсинг stages/jobs из репозитория.
- [ ] Endpoint `POST /projects/{id}/pipelines` принимает конфигурацию вместо template.
- [ ] Custom stages: произвольное количество, произвольные имена, произвольные jobs.
- [ ] Параллельные jobs внутри стадии (current: один job на stage).
- [ ] Зависимости между stages (DAG вместо линейной последовательности).
- [ ] Frontend: визуализация DAG stages, collapsible job details.
- [ ] Verification: YAML-парсинг тесты, API integration, frontend e2e.

---

## 6. Phase 4: Logs (M4)

**Цель**: полноценная работа с логами.

- [ ] Streaming логов через SSE (`GET /jobs/{id}/logs/stream`).
- [ ] Pagination для `GET /jobs/{id}/logs` (`?since_sequence=N`).
- [ ] Truncation для длинных логов (chunked storage).
- [ ] Frontend: real-time log viewer с авто-скроллом, search по логам, фильтрация по уровню.
- [ ] CLI: `job logs --follow` (streaming).
- [ ] Verification: SSE connection test, pagination test, frontend e2e.

---

## 7. Phase 5: Real Runner (M5)

**Цель**: выполнение задач в реальных контейнерах.

- [x] Runner registration: `runners` таблица, `POST /runners` + heartbeat + delete.
- [x] Embedded runner: supervisor loop внутри cicd-server, забирает jobs из очереди (`SELECT FOR UPDATE SKIP LOCKED`).
- [x] Job execution: `docker run` с указанным `image` и `command` (или shell-режим), стриминг stdout в `job_logs`.
- [x] Job lifecycle: `queued → running → success/failed` автоматически через runner.
- [x] Runner heartbeat: `POST /runners/{id}/heartbeat`.
- [x] `.forge-ci.yml` парсинг stages/jobs из репозитория.
- [ ] Отдельный runner-агент (external process) и multi-runner пулы с tags.
- [ ] Frontend: real-time job progress.
- [ ] Verification: runner e2e test (запуск реального `alpine:3.21 echo hello`).

---

## 8. Phase 6: Webhooks (M6)

**Цель**: уведомление внешних систем о событиях.

- [x] `webhooks` таблица: URL, events, project_id.
- [x] `notification_configs` таблица: каналы уведомлений (channel + target).
- [x] Endpoint `GET/POST/DELETE /projects/{id}/webhooks` — конфигурация webhook-ов.
- [x] Endpoint `GET/PUT /projects/{id}/notifications` — конфигурация уведомлений.
- [x] Frontend: страница Webhooks (webhook-и + notifications).
- [ ] `webhook_deliveries` таблица: попытки доставки, статус, response code.
- [ ] Events: `pipeline.started`, `pipeline.finished`, `job.started`, `job.finished`, `job.failed`.
- [ ] Delivery: async background task, retry с exponential backoff (3 attempts).
- [ ] HMAC-SHA256 подпись payload'а.
- [ ] Frontend: delivery history.
- [ ] Verification: webhook delivery test, retry test, signature verification.

---

## 9. Phase 7: Secrets (M7)

**Цель**: безопасное хранение секретов проектов.

- [x] `project_secrets` таблица: `project_id`, `key`, `encrypted_value`, timestamps.
- [x] Шифрование: AES-256-GCM, ключ из `CICD_SECRETS_KEY` env var.
- [x] Endpoints: `GET/POST/DELETE` (`/projects/{id}/secrets`, `/secrets/{id}`).
- [x] Frontend: страница Secrets (значения не отображаются никогда).
- [ ] Secrets доступны runner'ам через env vars при выполнении job.
- [ ] Маскирование секретов в логах (replace на `***`).
- [ ] Rotation ключа (re-encrypt при смене `CICD_SECRETS_KEY`).
- [ ] Verification: encryption/decryption unit tests, masking test, API integration.

---

## 10. Phase 8: Artifacts (M8)

**Цель**: хранение артефактов сборки.

- [x] `artifacts` таблица: `job_id`, `name`, `size_bytes`, `storage_path`, `created_at`.
- [x] Storage: локальная файловая система (`CICD_ARTIFACTS_DIR`), volume в docker-compose.
- [x] Endpoints: `GET/POST /jobs/{id}/artifacts` (raw body + `X-Artifact-Name`), `GET /artifacts/{id}/download`.
- [x] Лимит размера: 50 MiB на файл.
- [x] Frontend: страница Artifacts со скачиванием.
- [ ] TTL для автоматической очистки.
- [ ] S3-compatible storage.
- [ ] Verification: upload/download test, size limit test, cleanup test.

---

## 11. Phase 9: Admin + Reports (M9)

**Цель**: системная админка и отчёты.

- [x] `audit_log` таблица: события (runners, secrets, tokens, артефакты).
- [x] `users` таблица: username, role (admin/maintainer/developer/viewer), enabled.
- [x] `api_tokens` таблица: SHA-256 hash, hint, привязка к user.
- [x] Admin API: `GET /audit-log`, `GET/POST/PATCH /users`, `GET/POST/DELETE /api-tokens`.
- [x] Reports: `GET /projects/{id}/reports/summary` (total, success rate, avg duration).
- [x] Frontend: страницы Audit Log, Users & Tokens, Reports.
- [ ] `system_settings` таблица: key-value конфигурация инстанса.
- [ ] Charts: `recharts` для визуализации (success rate, duration histogram).
- [ ] Auth enforcement: вход по паролю, проверка ролей и токенов на запросах.
- [ ] Verification: admin API integration, frontend component tests, report accuracy.

---

## 12. Future (v1.x)

- RBAC enforcement: роли (admin, maintainer, developer, viewer), per-project permissions.
- Git integration: auto-trigger на push (GitHub/GitLab webhook listener).
- YAML pipeline editor в UI с валидацией.
- Matrix builds (параллельные jobs с параметрами).
- Manual approval gates между stages.
- Scheduled pipelines execution (cron-scheduler; хранение расписаний уже реализовано).
- Self-hosted runner pools с метками (tags).
- OIDC/OAuth SSO.
- Prometheus metrics endpoint (`/metrics`).
- Rate limiting (tower-governor).
- API token enforcement для CLI/automation (хранение уже реализовано).
- Multi-arch runner support (amd64/arm64).

---

## 13. Definitions of Done

Каждая фаза считается завершённой, когда:

- Код покрыт тестами: unit + integration + critical e2e.
- `cargo test` green, `pnpm test` green.
- `cargo clippy` и `cargo fmt --check` чистые.
- CI (GitHub Actions) green.
- Документация обновлена (`docs/API.md`, `docs/DATA_MODEL.md`, `docs/ROADMAP.md`).
- Скриншоты UI (если применимо) приложены.
- Ручная проверка через curl/UI пройдена.

## 14. References

- `docs/ARCHITECTURE.md` — архитектура и стек.
- `docs/DATA_MODEL.md` — дата-модель.
- `docs/API.md` — REST API спецификация.
- `docs/UI_UX.md` — UI/UX спецификация.
- `docs/TESTING.md` — стратегия тестирования.
- `docs/CODE_STYLE.md` — конвенции кода.
