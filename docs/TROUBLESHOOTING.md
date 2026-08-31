# Troubleshooting — Forge CI/CD

## 1. Сборка и запуск

### 1.1 `cargo build` падает в Docker

**Симптом:** Ошибка компиляции или линковки при `docker compose build backend`.

**Решение:**

- Проверить версию Rust в Dockerfile: `rust:1.86-slim` — должна быть ≥ 1.86 (edition 2024).
- Docker multi-stage build: первый stage (`rust:1.86-slim`) компилирует, второй (`debian:bookworm-slim`) — runtime.
- Первый build — долгий (скачивание и компиляция зависимостей). Последующие — быстрее (Docker layer cache).
- Если cache сломался:

```bash
docker compose build --no-cache backend
```

- Проверить, что `Cargo.lock` не конфликтует с `Cargo.toml`:

```bash
cd backend
cargo update -p <crate>  # обновить конкретный crate
cargo build              # проверить локально
```

### 1.2 `pnpm build` — ошибка esbuild

**Симптом:**

```
esbuild: Cannot find module or permission denied
```

**Решение:**

- `esbuild` требует approval для запуска binary в некоторых окружениях (pnpm 11 security):
  ```bash
  pnpm config set enable-pre-post-scripts true
  pnpm approve-builds esbuild
  # или
  pnpm install --shamefully-hoist
  ```
- В Docker build — добавить `pnpm config set enable-pre-post-scripts true` перед `pnpm install`:
  ```dockerfile
  RUN corepack enable pnpm && pnpm config set enable-pre-post-scripts true
  RUN pnpm install --frozen-lockfile
  ```
- Проверить версию Node.js: `node --version` ≥ 22.
- Удалить `node_modules` и lockfile:
  ```bash
  rm -rf frontend/node_modules frontend/pnpm-lock.yaml
  cd frontend && pnpm install
  ```

### 1.3 Frontend dev-сервер не стартует

**Симптом:** `pnpm dev` не поднимается или белый экран.

**Решение:**

- Проверить Node.js: `node --version` ≥ 22.
- Проверить, что порт `22802` не занят:
  ```bash
  lsof -i :22802
  ```
- Удалить `node_modules`:
  ```bash
  cd frontend
  rm -rf node_modules
  pnpm install
  pnpm dev
  ```
- Проверить `VITE_API_URL` или proxy в `vite.config.ts`:
  ```typescript
  // vite.config.ts
  server: {
    proxy: {
      "/api": "http://localhost:22801",
    },
  }
  ```

### 1.4 Docker compose не поднимается

```bash
docker compose down -v
docker compose pull
docker compose up -d --build
```

- Проверить `.env`:
  ```bash
  cp .env.example .env
  ```
- Проверить статус контейнеров:
  ```bash
  docker compose ps
  ```

## 2. База данных

### 2.1 Connection refused to PostgreSQL

**Симптом:**

```
error: connection refused (os error 111)
```

**Решение:**

- Проверить, что контейнер postgres healthy:
  ```bash
  docker compose ps
  # postgres должен быть "healthy"
  ```
- Проверить `CICD_DATABASE_URL` — хост должен быть `postgres` для docker, `localhost` для локального запуска:
  ```bash
  # .env (docker)
  CICD_DATABASE_URL=postgres://cicd:cicd_local_only@postgres:5432/cicd

  # .env (local, без docker для postgres)
  CICD_DATABASE_URL=postgres://cicd:cicd_local_only@localhost:22543/cicd
  ```
- Проверить порт: PostgreSQL доступен на `22543` (host mapping), `5432` (внутри docker network).
- Проверить credentials в `.env`:
  ```bash
  CICD_DATABASE_USER=cicd
  CICD_DATABASE_PASSWORD=cicd_local_only
  CICD_DATABASE_NAME=cicd
  ```
- Подключиться вручную:
  ```bash
  psql -h localhost -p 22543 -U cicd -d cicd
  # или через docker
  docker compose exec postgres psql -U cicd -d cicd
  ```

### 2.2 Миграции не применяются

Схема применяется при старте backend через committed SQLx migrations из `backend/migrations/`. Если таблицы отсутствуют или `_sqlx_migrations` не обновляется:

- Проверить логи backend:
  ```bash
  docker compose logs backend | grep -i "migrate\|sqlx\|error\|panic"
  ```
- Подключиться к БД и проверить таблицы:
  ```bash
  docker compose exec postgres psql -U cicd -d cicd -c "\dt"
  ```
- Должны быть как минимум: `projects`, `pipelines`, `stages`, `jobs`, `job_logs`, `user_credentials`, `sessions`, `domain_events`, `outbox_messages`, `outbox_delivery_attempts`.
- Если таблиц нет — пересоздать backend-контейнер, чтобы заново пройти startup migrator:
  ```bash
  docker compose up -d --build backend
  docker compose logs -f backend
  ```

### 2.3 Медленные запросы

```sql
SELECT query, mean_exec_time, calls, total_exec_time
FROM pg_stat_statements
ORDER BY mean_exec_time DESC
LIMIT 10;
```

- Включить `pg_stat_statements` extension (если не включён):
  ```sql
  CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
  ```
- См. `docs/DATABASE_INDEXES.md` — добавить недостающие индексы.

## 3. Port conflicts

### 3.1 Порт 22801 (API)

**Симптом:** `Address already in use` при старте backend.

```bash
# Проверить, кто занимает порт
lsof -i :22801
# или
ss -tlnp | grep 22801

# Освободить порт
kill <PID>

# Или изменить порт в .env
CICD_API_PORT=22803
CICD_BIND=0.0.0.0:22803
```

### 3.2 Порт 22802 (Dashboard / Frontend)

**Симптом:** Vite dev server или nginx не стартует.

```bash
lsof -i :22802
kill <PID>

# Или изменить порт
CICD_WEB_PORT=22804
```

### 3.3 Порт 22543 (PostgreSQL)

**Симптом:** PostgreSQL контейнер не стартует.

```bash
lsof -i :22543
# Если локальный PostgreSQL уже использует 5432 — изменить host mapping
# docker-compose.yml
ports:
  - "22544:5432"  # изменить host port
```

### 3.4 Сводка портов

| Сервис | Default порт | Env var | Назначение |
|--------|-------------|---------|------------|
| Backend API | 22801 | `CICD_API_PORT` / `CICD_BIND` | REST API |
| Frontend | 22802 | `CICD_WEB_PORT` | Dashboard (nginx / Vite) |
| PostgreSQL | 22543 | `CICD_DATABASE_PORT` | БД (host mapping, внутри docker — 5432) |

## 4. Healthcheck: wget vs curl

### Проблема

Backend Dockerfile использует `wget` для healthcheck:

```dockerfile
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD wget --quiet --tries=1 --spider http://127.0.0.1:22801/api/v1/health || exit 1
```

**Почему не curl?**

- Runtime image: `debian:bookworm-slim` — не содержит `curl` по умолчанию.
- Установка `curl` увеличивает image size на ~5 MB.
- `wget` доступен в `debian:bookworm-slim` из коробки.

### Если healthcheck не работает

```bash
# Проверить вручную (с curl на хосте)
curl -sS http://127.0.0.1:22801/api/v1/health

# Проверить внутри контейнера
docker compose exec backend wget --quiet --tries=1 --spider http://127.0.0.1:22801/api/v1/health
echo $?  # 0 = OK, 1 = fail

# Проверить логи healthcheck
docker inspect --format='{{json .State.Health}}' <container_id> | jq
```

### Альтернатива: установить curl

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/*
HEALTHCHECK CMD curl -f http://127.0.0.1:22801/api/v1/health || exit 1
```

> Не рекомендуется — увеличивает image size. Использовать `wget`.

## 5. API

### 5.1 500 Internal Server Error

- Проверить логи:
  ```bash
  docker compose logs backend | grep -i error
  ```
- Частые причины:
  - БД недоступна (проверить `docker compose ps`).
  - Duplicate project name (unique constraint) — см. `docs/API_EDGE_CASES.md`.
  - Race condition на `job_logs` (duplicate sequence).
- Проверить `RUST_LOG`:
  ```bash
  RUST_LOG=debug docker compose up backend
  ```

### 5.2 503 Service Unavailable

- `AppState.pool = None` — БД не подключена при старте.
- Проверить `CICD_DATABASE_URL`.
- Проверить, что postgres healthy.

### 5.3 400 Bad Request

- Невалидный JSON body.
- `Content-Type` не `application/json`.
- Пустые обязательные поля (`name`, `repository_url`, `message`).
- Невалидный transition статуса (см. `docs/API_EDGE_CASES.md`).
- Невалидный UUID в path.

### 5.4 404 Not Found

- Project/pipeline/job не существует.
- Проверить ID:
  ```bash
  curl -sS http://127.0.0.1:22801/api/v1/projects | jq
  ```

## 6. Frontend

### 6.1 Белый экран

- Открыть DevTools → Console.
- Проверить, что API доступен:
  ```bash
  curl -sS http://127.0.0.1:22801/api/v1/health
  ```
- Проверить nginx config (`frontend/nginx.conf`):
  - SPA fallback: `try_files $uri $uri/ /index.html`.
  - API proxy: `location /api/ { proxy_pass http://backend:22801; }`.
- Проверить, что `dist/` собран:
  ```bash
  docker compose exec frontend ls /usr/share/nginx/html/
  ```

### 6.2 Tailwind стили не применяются

- Проверить `@import "tailwindcss"` в `frontend/src/index.css`.
- Перезапустить dev-сервер:
  ```bash
  cd frontend && pnpm dev
  ```
- Проверить `tailwind.config` (Tailwind 4 — CSS-based config через `@theme`).

### 6.3 API proxy не работает

- Проверить `vite.config.ts`:
  ```typescript
  server: {
    proxy: {
      "/api": {
        target: "http://localhost:22801",
        changeOrigin: true,
      },
    },
  }
  ```
- Проверить, что backend запущен на `22801`.

## 7. Тесты

### 7.1 Cargo тесты падают

```bash
cd backend
cargo test
```

- Если падают на DB — убедиться, что `CICD_DATABASE_URL` настроен.
- Если падают на API contract — проверить, что backend запущен.
- Запустить с выводом:
  ```bash
  cargo test -- --nocapture
  ```

### 7.2 Frontend тесты падают

```bash
cd frontend
pnpm test
```

- Удалить `node_modules` и переустановить:
  ```bash
  rm -rf node_modules && pnpm install && pnpm test
  ```

## 8. Диагностика

### 8.1 Health checks

```bash
# API
curl -sS http://127.0.0.1:22801/api/v1/health

# Docker
docker compose ps
docker compose logs --tail=50 backend
docker compose logs --tail=50 frontend
docker compose logs --tail=50 postgres
```

### 8.2 Логи

```bash
# Backend логи (JSON)
docker compose logs -f backend | jq

# Фильтр по ошибкам
docker compose logs backend | jq 'select(.level == "ERROR")'

# PostgreSQL логи
docker compose logs -f postgres
```

### 8.3 Состояние БД

```bash
# Подключиться к БД
docker compose exec postgres psql -U cicd -d cicd

# Проверить таблицы
\dt

# Проверить данные
SELECT COUNT(*) FROM projects;
SELECT COUNT(*) FROM pipelines;
SELECT COUNT(*) FROM job_logs;

# Проверить индексы
\di
```

### 8.4 Состояние Docker

```bash
# Все контейнеры
docker compose ps -a

# Размер образов
docker images | grep cicd

# Использование ресурсов
docker stats
```

## 9. Quick fix checklist

| Проблема | Решение |
|----------|---------|
| Backend не стартует | `docker compose logs backend` — проверить `CICD_DATABASE_URL` |
| Frontend не стартует | `lsof -i :22802` — проверить, что порт свободен |
| БД connection refused | `docker compose ps` — postgres healthy? Проверить порт `22543` |
| 503 на всех endpoint | `CICD_DATABASE_URL` невалиден или БД не запущена |
| Healthcheck fail | Проверить `wget` в контейнере, логи `docker inspect` |
| Cargo build в Docker | `docker compose build --no-cache backend` |
| pnpm esbuild error | `pnpm approve-builds esbuild` или `pnpm install --shamefully-hoist` |
| White screen | Проверить nginx SPA fallback и API proxy |
| Tailwind missing | Перезапустить `pnpm dev`, проверить `@import "tailwindcss"` |

## References

- `docs/ARCHITECTURE.md` — архитектура и стек.
- `docs/API.md` — спецификация endpoint.
- `docs/API_EDGE_CASES.md` — граничные случаи API.
- `docs/MONITORING.md` — health endpoint и логирование.
- `docs/LOGGING_STANDARDS.md` — структура логов.
- `.env.example` — шаблон env vars.
