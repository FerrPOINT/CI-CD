# Deployment — Forge CI/CD

## 1. Overview

Self-hosted CI/CD control plane. MVP поставляется как Docker Compose: backend (Rust / Axum), frontend (Vite static, раздаётся через nginx), PostgreSQL 17. Reverse proxy — по желанию.

Это control plane с текущим embedded runner: сервер забирает queued jobs, выполняет Docker/shell команды, стримит stdout в job logs и поддерживает cancel/retry. Это не безопасный distributed runner pool: API/container не должен получать Docker socket в production; внешний runner protocol, leases и sandbox boundary описаны в `docs/RUNNER_ARCHITECTURE.md`.

## 2. System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | 2 cores | 4+ cores |
| RAM | 2 GB | 4+ GB |
| Disk | 10 GB SSD | 50+ GB SSD |
| OS | Linux x86_64 | Ubuntu 22.04 LTS |
| Docker | 24.0+ | 27.0+ |
| Docker Compose | 2.20+ | 2.27+ |

## 3. Services

| Service | Image | Port | Description |
|---------|-------|------|-------------|
| `backend` | build from `backend/Dockerfile` | `22801` | Axum API сервер |
| `frontend` | build from `frontend/Dockerfile` | `22802` | Dashboard (nginx static) |
| `postgres` | `postgres:17.6-alpine` | `22543` | PostgreSQL 17.6 |

Порты настраиваются через переменные окружения в `.env` (см. раздел 5).

## 4. Quick Start

```bash
git clone git@github.com:FerrPOINT/CI-CD.git
cd CI-CD

cp .env.example .env
# отредактируйте пароль БД перед shared-деплоем

docker compose up -d --build

# проверка
curl -sS http://localhost:22801/api/v1/health
# => {"status":"ok","service":"cicd"}

# dashboard
open http://localhost:22802
```

## 5. Environment Variables

Все переменные имеют префикс `CICD_`. Полный файл — `.env.example`.

```env
# PostgreSQL
CICD_DATABASE_USER=cicd
CICD_DATABASE_PASSWORD=change-this-before-shared-deployments
CICD_DATABASE_NAME=cicd
CICD_DATABASE_PORT=22543

# Порты
CICD_API_PORT=22801
CICD_WEB_PORT=22802

# Backend (внутренние, задаются в docker-compose.yml)
# CICD_DATABASE_URL — формируется из user/password/name
# CICD_BIND=0.0.0.0:22801
# RUST_LOG=info
```

### 5.1 Переменные backend

| Variable | Default | Description |
|----------|---------|-------------|
| `CICD_DATABASE_URL` | (из compose) | PostgreSQL connection string |
| `CICD_BIND` | `0.0.0.0:22801` | Адрес привязки API сервера |
| `RUST_LOG` | `info` | Уровень логирования (`debug`, `info`, `warn`, `error`) |

### 5.2 Переменные PostgreSQL

| Variable | Default | Description |
|----------|---------|-------------|
| `CICD_DATABASE_USER` | `cicd` | Пользователь БД |
| `CICD_DATABASE_PASSWORD` | `cicd_local_only` | Пароль БД |
| `CICD_DATABASE_NAME` | `cicd` | Имя БД |
| `CICD_DATABASE_PORT` | `22543` | Внешний порт PostgreSQL |

### 5.3 Переменные портов

| Variable | Default | Description |
|----------|---------|-------------|
| `CICD_API_PORT` | `22801` | Внешний порт API |
| `CICD_WEB_PORT` | `22802` | Внешний порт Dashboard |

## 6. Health Checks

| Endpoint | Service | Check |
|----------|---------|-------|
| `GET /api/v1/health` | backend | `{"status":"ok","service":"cicd"}` |
| `pg_isready -U cicd` | postgres | Docker healthcheck |

Docker Compose healthchecks:

- **postgres**: `pg_isready` каждые 5s, 10 попыток.
- **backend**: `curl -fsS http://localhost:22801/api/v1/health` каждые 10s, 10 попыток.
- **frontend**: зависит от backend (`condition: service_healthy`).

## 7. Docker Compose

Файл `docker-compose.yml` определяет три сервиса:

```yaml
services:
  postgres:
    image: postgres:17.6-alpine
    # ... healthcheck, volume cicd_postgres_data

  backend:
    build: ./backend
    depends_on:
      postgres: { condition: service_healthy }
    # healthcheck через curl

  frontend:
    build: ./frontend
    depends_on:
      backend: { condition: service_healthy }

volumes:
  cicd_postgres_data:
```

### Команды

```bash
# Сборка и запуск
docker compose up -d --build

# Только инфраструктура (для локальной разработки)
docker compose up -d postgres

# Пересоздать контейнеры после изменений
docker compose build
docker compose up -d

# Логи
docker compose logs -f backend
docker compose logs -f frontend

# Остановить
docker compose down

# Остановить и удалить данные
docker compose down -v
```

## 8. Production Notes

### 8.1 Перед деплоем

- **Сменить пароль БД**: `CICD_DATABASE_PASSWORD` в `.env`.
- **Ограничить CORS**: в текущей версии `CorsLayer::permissive()` — для production ограничить whitelist доменов (см. `docs/SECURITY.md`).
- **Закрыть порт PostgreSQL**: не экспонировать `22543` наружу, оставить только в internal Docker network.
- **HTTPS**: использовать reverse proxy (nginx / Caddy / Traefik) с TLS-сертификатом.
- **Backup**: настроить регулярный `pg_dump`.

### 8.2 Reverse Proxy (nginx)

```nginx
server {
  listen 443 ssl http2;
  server_name cicd.example.com;

  ssl_certificate     /etc/ssl/certs/cicd.crt;
  ssl_certificate_key /etc/ssl/private/cicd.key;

  location /api/ {
    proxy_pass http://127.0.0.1:22801;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
  }

  location / {
    proxy_pass http://127.0.0.1:22802;
    proxy_set_header Host $host;
  }
}
```

### 8.3 Backup

```bash
docker compose exec -T postgres pg_dump -U cicd cicd > cicd-$(date +%Y%m%d).sql
```

Восстановление:

```bash
docker compose exec -T postgres psql -U cicd cicd < cicd-20260826.sql
```

### 8.4 Update

```bash
git pull origin main
docker compose build
docker compose up -d
```

При изменениях схемы БД (миграции `store::migrate()` — `CREATE TABLE IF NOT EXISTS`) достаточно перезапустить backend: схема обновится автоматически.

### 8.5 Resource Limits

В `docker-compose.yml` можно добавить ограничения:

```yaml
backend:
  deploy:
    resources:
      limits:
        memory: 512M
        cpus: '1.0'
```

## 9. Architecture Notes

- Backend запускается с `Option<PgPool>`: если БД недоступна, health-check всё равно отвечает, остальные endpoint возвращают `503 Service Unavailable`.
- Схема БД создаётся при старте через `store::migrate()` — `CREATE TABLE IF NOT EXISTS` для всех таблиц. Отдельный migrator-бинар не требуется.
- Frontend собирается в статические файлы и раздаётся через nginx (порт 80 внутри контейнера, маппится на `22802`).

## References

- `docs/ARCHITECTURE.md`
- `docs/LOCAL_SETUP.md`
- `docs/SECURITY.md`
- `.env.example`
- `docker-compose.yml`