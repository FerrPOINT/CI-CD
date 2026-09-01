# AGENTS.md — Forge CI/CD

## Репозиторий

- **GitHub**: `git@github.com:FerrPOINT/CI-CD.git`
- **Стек**: backend Rust 2024 (Axum 0.8 + SQLx 0.8 + PostgreSQL 17), frontend React 19 + Vite 6 + Tailwind CSS 4 + shadcn/ui
- **Env prefix**: `CICD_`
- **Публичные порты по умолчанию**: API `22801`, Dashboard `22802`, PostgreSQL `22543`

## Правила работы

### 1. Перед началом работы

1. Прочитать `docs/ARCHITECTURE.md`, `docs/DATA_MODEL.md`, `docs/API.md`.
2. Проверить текущее состояние ветки: `git status`.
3. Составить план, показать пользователю, получить подтверждение.

### 2. Код

- Backend: текущий runtime ещё монолитен, но новые изменения двигаются к `api → app/domain → infra/store` по ADR-0005.
- Состояние приложения — `AppState` с `Option<PgPool>` (`/api/v1/health` работает без БД, `/api/v1/readiness` без БД возвращает `503`).
- SQL-схема управляется committed SQLx migrations в `backend/migrations/*.sql`; backend применяет их при старте, `cicd-migrate` использует тот же набор.
- Доменные статусы и transition-правила — `JobStatus` enum с `transition_to()`, единственный источник правды для валидации переходов.
- Frontend: компоненты на shadcn/ui + Tailwind, typed API wrapper и generated OpenAPI DTO `frontend/src/api/schema.d.ts`.
- Серверное состояние: `@tanstack/react-query` (целевое), клиентское — `useState`/`zustand`.
- Vite dev proxy: `/api` → `http://localhost:22801`.

### 3. Коммиты

- Conventional commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`).
- Один коммит = одна логическая единица.
- Не amend/squash без явного запроса.
- Push только после релевантного полного gate: backend `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, integration DB tests, `cargo build --release --workspace`; frontend `pnpm openapi:check`, `pnpm openapi:compat --base-ref origin/main`, `pnpm lint`, `pnpm test`, `pnpm build`.

### 4. Тестирование

- Backend: `cargo test --workspace`, `cargo test --features integration --test integration_db -- --test-threads=1`, API/CLI/domain contract tests.
- Frontend: Vitest для unit-тестов компонентов; Playwright Chromium + axe для текущего representative E2E/a11y MVP gate.
- После UI-изменений — скриншоты full-page (375 / 1920 / 2560).
- Все новые endpoint — curl-проверка.
- Docker compose smoke: `docker compose up --build -d` + `curl /api/v1/health` + `curl /api/v1/readiness`.
- Browser E2E: running Compose stack + `cd frontend && pnpm seed:evidence && pnpm e2e`.

### 5. Документация

- При изменении API обновлять Rust OpenAPI annotations, регенерировать `openapi/openapi.yaml` и `frontend/src/api/schema.d.ts`, затем обновлять `docs/API.md`.
- При изменении дата-модели добавлять SQLx migration и обновлять `docs/DATA_MODEL.md`.
- При новом функционале добавлять/обновлять `docs/ROADMAP.md`.
- Любые неочевидные решения фиксировать в `docs/ARCHITECTURE.md`.

### 6. Безопасность

- Никогда не коммитить credentials, токены, пароли, реальные данные.
- Все secrets — через env vars с префиксом `CICD_`.
- Перед push проверять, что в diff нет чувствительных данных.
- SQL только через parameterized queries (`sqlx::query` с `$1`, `$2`, ...).

### 7. Docker

- Сборка: `docker compose build`.
- Пересоздание контейнера: `docker compose up -d` (не `docker compose restart`).
- Проверка: `docker compose ps`, `curl http://127.0.0.1:22801/api/v1/health` и `curl http://127.0.0.1:22801/api/v1/readiness`.
- Backend Dockerfile: multi-stage `rust:1.86-slim` → `debian:bookworm-slim`, runs as `uid 10001`.
- Frontend Dockerfile: multi-stage `node:22-bookworm-slim` → `nginx:1.31.4-alpine`.

### 8. Проверка перед завершением

- [ ] Backend gate чистый: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, integration DB tests, `cargo build --release --workspace`.
- [ ] Frontend gate чистый: `pnpm openapi:check`, `pnpm openapi:compat --base-ref origin/main`, `pnpm lint`, `pnpm test`, `pnpm build`.
- [ ] Документация актуальна.
- [ ] OpenAPI/generated client актуальны, если менялся API.
- [ ] Пользователь увидел результат (скриншот / curl / лог).

## Контакты

- Техлид: Александр Жуков.
- Основной язык общения и документов: русский.

## References

- `docs/ARCHITECTURE.md`
- `docs/CODE_STYLE.md`
- `docs/TESTING.md`
- `docs/API.md`
- `docs/DATA_MODEL.md`
