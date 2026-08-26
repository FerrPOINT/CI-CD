# Ops Runbook — Forge CI/CD

## 1. Обзор

Операционный playbook для инцидентов и рутинных операций Forge CI/CD. Покрывает перезапуск сервисов, резервное копирование БД, health-check и инспекцию логов.

**Сервисы:**

| Сервис | Контейнер | Порт | Назначение |
|---|---|---|---|
| API (backend) | `backend` | `22801` | Rust/Axum REST API |
| Dashboard (frontend) | `frontend` | `22802` | React SPA (nginx) |
| PostgreSQL | `postgres` | `22543` | База данных |

**Env-переменные (префикс `CICD_`):**

| Переменная | Default | Описание |
|---|---|---|
| `CICD_DATABASE_USER` | `cicd` | Пользователь PostgreSQL |
| `CICD_DATABASE_PASSWORD` | `change-this-before-shared-deployments` | Пароль PostgreSQL |
| `CICD_DATABASE_NAME` | `cicd` | Имя БД |
| `CICD_DATABASE_PORT` | `22543` | Внешний порт PostgreSQL |
| `CICD_API_PORT` | `22801` | Внешний порт API |
| `CICD_WEB_PORT` | `22802` | Внешний порт Dashboard |

---

## 2. Health-check

### 2.1. API health-check

```bash
curl -fsS http://127.0.0.1:22801/api/v1/health
```

Ожидаемый ответ (HTTP 200):

```json
{
  "status": "ok",
  "service": "cicd"
}
```

### 2.2. PostgreSQL health-check

```bash
docker compose exec postgres pg_isready -U cicd -d cicd
```

Ожидаемый ответ:

```
/var/run/postgresql:5432 - accepting connections
```

### 2.3. Dashboard health-check

```bash
curl -fsS http://127.0.0.1:22802/ | head -5
```

Ожидаемый ответ — HTML-страница с `<div id="root">`.

### 2.4. Все сервисы одной командой

```bash
just health
```

---

## 3. Перезапуск Docker Compose

### 3.1. Полный перезапуск

```bash
docker compose down
docker compose up --build -d
```

### 3.2. Перезапуск только backend

```bash
docker compose restart backend
```

### 3.3. Перезапуск только frontend

```bash
docker compose restart frontend
```

### 3.4. Остановка без удаления данных

```bash
docker compose stop
```

### 3.5. Остановка с удалением контейнеров (данные БД сохраняются в volume)

```bash
docker compose down
```

### 3.6. Полный сброс включая данные БД

> **ВНИМАНИЕ:** удаляет все данные. Использовать только в dev-окружении.

```bash
docker compose down -v
docker compose up --build -d
```

---

## 4. Резервное копирование БД (pg_dump)

### 4.1. Полный дамп

```bash
docker compose exec -T postgres pg_dump \
  -U cicd \
  -d cicd \
  --format=custom \
  --file=/tmp/cicd_backup_$(date +%Y%m%d_%H%M%S).dump
```

### 4.2. Дамп в локальный файл

```bash
docker compose exec -T postgres pg_dump \
  -U cicd \
  -d cicd \
  --format=custom \
  > backups/cicd_$(date +%Y%m%d_%H%M%S).dump
```

### 4.3. Восстановление из дампа

```bash
docker compose exec -T postgres pg_restore \
  -U cicd \
  -d cicd \
  --clean \
  --if-exists \
  < backups/cicd_20260826_120000.dump
```

### 4.4. Автоматический ежедневный backup (cron)

```bash
# crontab -e
0 2 * * * docker compose -f /opt/dev/CI-CD/docker-compose.yml exec -T postgres pg_dump -U cicd -d cicd --format=custom > /opt/backups/cicd_$(date +\%Y\%m\%d).dump 2>> /opt/backups/backup.log
```

### 4.5. Проверка backup'а

```bash
pg_restore --list backups/cicd_20260826_120000.dump | head -20
```

---

## 5. Инспекция логов

### 5.1. Все сервисы (follow)

```bash
docker compose logs -f
```

### 5.2. Только backend

```bash
docker compose logs -f backend
```

### 5.3. Только frontend

```bash
docker compose logs -f frontend
```

### 5.4. Только PostgreSQL

```bash
docker compose logs -f postgres
```

### 5.5. Последние N строк

```bash
docker compose logs --tail 100 backend
```

### 5.6. Логи за определённое время

```bash
docker compose logs --since 30m backend
docker compose logs --since 2026-08-26T10:00:00 backend
```

### 5.7. Фильтрация по уровню (RUST_LOG)

Логи backend управляются переменной `RUST_LOG`. Уровни: `error`, `warn`, `info`, `debug`, `trace`.

```bash
# В docker-compose.yml или .env:
RUST_LOG=debug    # подробные логи для отладки
RUST_LOG=info     # стандартный уровень (default)
RUST_LOG=error    # только ошибки
```

После изменения — перезапуск backend:

```bash
docker compose restart backend
```

---

## 6. Реакция на инциденты

### 6.1. API не отвечает (503 / connection refused)

1. Проверить статус контейнеров:
   ```bash
   docker compose ps
   ```
2. Проверить health-check:
   ```bash
   curl -fsS http://127.0.0.1:22801/api/v1/health
   ```
3. Проверить логи backend:
   ```bash
   docker compose logs --tail 200 backend
   ```
4. Если БД недоступна — проверить PostgreSQL:
   ```bash
   docker compose ps postgres
   docker compose logs --tail 100 postgres
   docker compose exec postgres pg_isready -U cicd -d cicd
   ```
5. Перезапустить backend:
   ```bash
   docker compose restart backend
   ```
6. Если не помогло — полный перезапуск:
   ```bash
   docker compose down
   docker compose up --build -d
   ```

### 6.2. Dashboard не загружается

1. Проверить контейнер frontend:
   ```bash
   docker compose ps frontend
   ```
2. Проверить логи:
   ```bash
   docker compose logs --tail 100 frontend
   ```
3. Проверить доступность API из frontend-контейнера:
   ```bash
   docker compose exec frontend curl -fsS http://backend:22801/api/v1/health
   ```
4. Перезапустить frontend:
   ```bash
   docker compose restart frontend
   ```

### 6.3. PostgreSQL недоступна

1. Проверить контейнер:
   ```bash
   docker compose ps postgres
   ```
2. Проверить логи:
   ```bash
   docker compose logs --tail 200 postgres
   ```
3. Проверить volume:
   ```bash
   docker volume ls | grep cicd_postgres
   ```
4. Перезапустить PostgreSQL:
   ```bash
   docker compose restart postgres
   ```
5. Дождаться healthcheck:
   ```bash
   docker compose exec postgres pg_isready -U cicd -d cicd
   ```
6. Перезапустить backend (чтобы переподключиться):
   ```bash
   docker compose restart backend
   ```

### 6.4. Зависший пайплайн (статус `running` без прогресса)

1. Найти зависший пайплайн:
   ```bash
   curl -sS http://127.0.0.1:22801/api/v1/pipelines | jq '.[] | select(.status == "running")'
   ```
2. Проверить логи связанных jobs:
   ```bash
   curl -sS http://127.0.0.1:22801/api/v1/pipelines/{id} | jq '.stages[].jobs[]'
   ```
3. Перевести job в `canceled` через API или UI.
4. Агрегация автоматически пересчитает статус stage/pipeline.

### 6.5. Закончилось место на диске

1. Проверить место:
   ```bash
   df -h
   ```
2. Очистить старые образы:
   ```bash
   docker image prune -a
   ```
3. Очистить неиспользуемые volume:
   ```bash
   docker volume prune
   ```
4. Проверить размер логов Docker:
   ```bash
   docker system df
   ```
5. Очистить систему:
   ```bash
   docker system prune -a --volumes
   ```

> **ВНИМАНИЕ:** `docker system prune --volumes` удалит `cicd_postgres_data` если контейнеры остановлены. Перед очисткой убедиться, что контейнеры запущены, или сделать backup БД.

---

## 7. Рутинные операции

### 7.1. Обновление приложения

```bash
git pull
docker compose down
docker compose up --build -d
```

### 7.2. Применение миграций

Миграции применяются автоматически при старте backend через `store::migrate()` (`CREATE TABLE IF NOT EXISTS`). Дополнительных действий не требуется.

### 7.3. Проверка версии приложения

```bash
docker compose exec backend cicd-server --version
```

### 7.4. Сборка без запуска

```bash
docker compose build
```

---

## 8. Контакты и эскалация

| Уровень | Действие | Когда |
|---|---|---|
| L1 | Перезапуск контейнера | Сервис упал, логи не показывают критических ошибок |
| L2 | Полный перезапуск docker compose | L1 не помог, проблема в нескольких сервисах |
| L3 | Восстановление БД из backup | Данные повреждены или потеряны |
| L4 | Эскалация разработчикам | L1–L3 не решили проблему |

---

## References

- `docs/ARCHITECTURE.md` — архитектура и стек
- `docker-compose.yml` — конфигурация сервисов
- `.env.example` — переменные окружения
- `justfile` — команды-шорткаты