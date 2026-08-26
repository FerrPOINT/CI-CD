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

- Backend: слои `api → domain → store` (HTTP-хендлеры, доменные правила переходов статусов, SQLx-хранилище).
- Состояние приложения — `AppState` с `Option<PgPool>` (бездБ режим для health-check).
- SQL-схема управляется через `store::migrate()` — `CREATE TABLE IF NOT EXISTS` при старте.
- Доменные статусы и transition-правила — `JobStatus` enum с `transition_to()`, единственный источник правды для валидации переходов.
- Frontend: компоненты на shadcn/ui + Tailwind, типизированные API-клиенты.
- Серверное состояние: `@tanstack/react-query` (целевое), клиентское — `useState`/`zustand`.
- Vite dev proxy: `/api` → `http://localhost:22801`.

### 3. Коммиты

- Conventional commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`).
- Один коммит = одна логическая единица.
- Не amend/squash без явного запроса.
- Push только после проверки `cargo test`, `cargo clippy`, `pnpm test`, `pnpm build`.

### 4. Тестирование

- Backend: `cargo test` — unit-тесты domain transitions, интеграционные тесты API contract, CLI contract.
- Frontend: Vitest для unit-тестов компонентов, Playwright для E2E (целевое).
- После UI-изменений — скриншоты full-page (375 / 1920 / 2560).
- Все новые endpoint — curl-проверка.
- Docker compose smoke: `docker compose up --build -d` + `curl /api/v1/health`.

### 5. Документация

- При изменении API обновлять `docs/API.md`.
- При изменении дата-модели обновлять `docs/DATA_MODEL.md`.
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
- Проверка: `docker compose ps` и `curl http://127.0.0.1:22801/api/v1/health`.
- Backend Dockerfile: multi-stage `rust:1.86-slim` → `debian:bookworm-slim`, runs as `uid 10001`.
- Frontend Dockerfile: multi-stage `node:22-bookworm-slim` → `nginx:1.27-alpine`.

### 8. Проверка перед завершением

- [ ] Все тесты проходят (`cargo test`, `pnpm test`).
- [ ] Линтеры (`cargo clippy`, `cargo fmt --check`) чистые.
- [ ] Документация актуальна.
- [ ] Коммиты запушены в `origin/main`.
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
