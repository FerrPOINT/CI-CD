# Local Setup — Forge CI/CD

## 1. Требования

| Инструмент | Минимальная версия | Примечание |
|---|---|---|
| Docker + Compose | 24.x | для PostgreSQL, backend, frontend |
| Rust | 1.86+ | backend (edition 2024) |
| cargo | 1.86+ | backend |
| Node.js | 22 LTS | frontend |
| pnpm | 9.x | frontend package manager |
| just | — | task runner (опционально, см. `justfile`) |
| git | 2.40+ | — |
| curl | — | smoke-тесты API |

## 2. Быстрый старт

```bash
git clone git@github.com:FerrPOINT/CI-CD.git /opt/dev/CI-CD
cd /opt/dev/CI-CD

cp .env.example .env
# при необходимости отредактируйте .env

# Вариант A: всё через Docker Compose
docker compose up -d --build
curl -sS http://localhost:22801/api/v1/health
# => {"status":"ok","service":"cicd"}

# Вариант B: только PostgreSQL в Docker, backend и frontend локально
docker compose up -d postgres
cd backend && cargo run --bin cicd-server    # Terminal 1
cd frontend && pnpm install && pnpm dev      # Terminal 2
```

Приложение:

- API: `http://localhost:22801`
- Dashboard: `http://localhost:22802` (Docker) или `http://localhost:5173` (Vite dev server)
- PostgreSQL: `localhost:22543`

## 3. Переменные окружения

Файл `.env.example`:

```env
CICD_DATABASE_USER=cicd
CICD_DATABASE_PASSWORD=change-this-before-shared-deployments
CICD_DATABASE_NAME=cicd
CICD_DATABASE_PORT=22543
CICD_API_PORT=22801
CICD_WEB_PORT=22802
```

### 3.1 Backend (локальный запуск)

При локальном запуске backend (без Docker) укажите `CICD_DATABASE_URL`:

```env
CICD_DATABASE_URL=postgresql://cicd:cicd_local_only@localhost:22543/cicd
CICD_BIND=127.0.0.1:22801
RUST_LOG=debug
```

### 3.2 Frontend (локальный запуск)

Vite dev proxy настроен в `vite.config.ts`: `/api` → `http://localhost:22801`. Дополнительные переменные не требуются.

## 4. Backend

```bash
cd backend

# Сборка
cargo build

# Запуск API сервера
cargo run --bin cicd-server

# Запуск с горячей перезагрузкой (опционально)
cargo watch -x run --bin cicd-server

# Запуск тестов
cargo test

# CLI
cargo run --bin cicd-cli -- project list
cargo run --bin cicd-cli -- pipeline run --project <uuid> --git-ref main

# Линтеры
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Схема БД создаётся автоматически при старте (`store::migrate()` — `CREATE TABLE IF NOT EXISTS` для всех таблиц). Отдельный шаг миграции не нужен.

## 5. Frontend

```bash
cd frontend

# Установка зависимостей
pnpm install

# Dev-сервер
pnpm dev

# Типизация
pnpm build   # tsc --noEmit && vite build

# Линтер
pnpm lint

# Тесты
pnpm test
```

## 6. Docker

```bash
# Полная сборка и запуск
docker compose up -d --build

# Только инфраструктура (PostgreSQL)
docker compose up -d postgres

# Пересоздать контейнеры после изменений
docker compose build
docker compose up -d

# Логи
docker compose logs -f backend
docker compose logs -f frontend
docker compose logs -f postgres

# Остановить
docker compose down

# Остановить и удалить данные БД
docker compose down -v

# Проверка статуса
docker compose ps
```

## 7. Smoke-тесты через curl

После запуска (Docker или локальный backend):

```bash
# 1. Health-check
curl -sS http://localhost:22801/api/v1/health
# => {"status":"ok","service":"cicd"}

# 2. Создать проект
curl -sS -X POST http://localhost:22801/api/v1/projects \
  -H 'Content-Type: application/json' \
  -d '{"name":"test-project","repository_url":"https://github.com/example/repo.git","default_branch":"main"}'
# => {"id":"...","name":"test-project",...}

# 3. Список проектов
curl -sS http://localhost:22801/api/v1/projects

# 4. Запустить пайплайн (подставьте project id из шага 2)
curl -sS -X POST http://localhost:22801/api/v1/projects/<project-id>/pipelines \
  -H 'Content-Type: application/json' \
  -d '{"git_ref":"main"}'
# => {"pipeline":{...},"stages":[...]}

# 5. Показать пайплайн
curl -sS http://localhost:22801/api/v1/pipelines/<pipeline-id>

# 6. Перевести задачу в running
curl -sS -X POST http://localhost:22801/api/v1/jobs/<job-id>/status \
  -H 'Content-Type: application/json' \
  -d '{"status":"running"}'

# 7. Добавить лог
curl -sS -X POST http://localhost:22801/api/v1/jobs/<job-id>/logs \
  -H 'Content-Type: application/json' \
  -d '{"message":"Build started"}'

# 8. Прочитать логи
curl -sS http://localhost:22801/api/v1/jobs/<job-id>/logs

# 9. Завершить задачу
curl -sS -X POST http://localhost:22801/api/v1/jobs/<job-id>/status \
  -H 'Content-Type: application/json' \
  -d '{"status":"success"}'
```

## 8. Dev Workflow

### 8.1 Типичный цикл

1. `docker compose up -d postgres` — поднять БД.
2. `cargo run --bin cicd-server` — запустить backend (Terminal 1).
3. `pnpm dev` — запустить frontend dev server (Terminal 2).
4. Внести изменения, проверить через `curl` или Dashboard.
5. `cargo test` — прогнать тесты.
6. `cargo fmt && cargo clippy` — проверить стиль.
7. Закоммитить по conventional commits.

### 8.2 just

В корне проекта есть `justfile` с часто используемыми командами:

```bash
just <recipe>   # список доступных recipe
```

### 8.3 Изменение схемы БД

Схема определена в `backend/src/store.rs` в функции `migrate()`. Все таблицы создаются через `CREATE TABLE IF NOT EXISTS`. При изменении схемы — отредактировать `migrate()`, пересоздать БД:

```bash
docker compose down -v
docker compose up -d postgres
cargo run --bin cicd-server
```

## 9. IDE

Рекомендуемые расширения:

- Rust Analyzer
- Tailwind CSS IntelliSense
- ESLint
- Prettier
- GitLens
- Docker

## 10. Частые проблемы

| Проблема | Решение |
|---|---|
| Порт 22801 занят | изменить `CICD_API_PORT` в `.env` |
| Порт 22802 занят | изменить `CICD_WEB_PORT` в `.env` |
| PostgreSQL не стартует | `docker compose down -v` и пересоздать volume |
| `cargo` долго компилирует | включить `sccache`, использовать `cargo nextest` |
| Frontend не видит API | проверить что backend запущен на `22801`; Vite proxy в `vite.config.ts` |
| `503 Service Unavailable` | backend не может подключиться к БД — проверить `CICD_DATABASE_URL` |
| Ошибка `resource not found` | неверный UUID — проверить через `project list` / `pipeline show` |

Больше диагностики — в `docs/TROUBLESHOOTING.md` (планируется).

## References

- `.env.example`
- `docker-compose.yml`
- `docs/DEPLOYMENT.md`
- `docs/TESTING.md`
- `docs/CODE_STYLE.md`
- `docs/AGENTS.md`