# Backup & Restore — Forge CI/CD

## 1. Что бэкапим

| Компонент | Способ | Частота | Статус |
|---|---|---|---|
| PostgreSQL | `pg_dump` | Ежедневно | Required |
| `.env` | Внешний secret manager / encrypted store | При изменении | Required |
| Artifacts (Phase 8) | `rsync` / S3 replication | Ежедневно | Planned |
| Redis (Future) | RDB snapshot / необязательно | — | Planned |

> Forge CI/CD использует только PostgreSQL как постоянное хранилище (ADR-0004). Artifacts (Phase 8) хранятся в файловой системе или S3; метаданные — в PostgreSQL.

---

## 2. Автоматический бэкап

### 2.1. Скрипт `scripts/backup.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

BACKUP_DIR="/backups"
DATE=$(date +%F)
DB_USER="${CICD_DATABASE_USER:-cicd}"
DB_NAME="${CICD_DATABASE_NAME:-cicd}"

# 1. pg_dump
docker compose exec -T postgres pg_dump -U "$DB_USER" "$DB_NAME" \
  | gzip > "$BACKUP_DIR/postgres-$DATE.sql.gz"

# 2. Artifacts (Phase 8)
if [ -d "$CICD_STORAGE" ]; then
  rsync -a --delete "$CICD_STORAGE/" "$BACKUP_DIR/artifacts/"
fi

# 3. Ротация: хранить последние 7 дневных и 4 недельных снапшота
find "$BACKUP_DIR" -name "postgres-*.sql.gz" -mtime +7 -delete
find "$BACKUP_DIR" -name "postgres-*.sql.gz" -mtime +28 -delete
```

### 2.2. Cron

```cron
0 2 * * * cd /opt/dev/CI-CD && ./scripts/backup.sh >> /var/log/cicd-backup.log 2>&1
```

Бэкап выполняется в 02:00 nightly. Лог — `/var/log/cicd-backup.log`.

---

## 3. Ручной бэкап

### 3.1. PostgreSQL

```bash
# Полный дамп в gzip
docker compose exec -T postgres pg_dump -U cicd cicd \
  | gzip > cicd-$(date +%F).sql.gz

# Только схема (без данных)
docker compose exec -T postgres pg_dump -U cicd --schema-only cicd \
  | gzip > cicd-schema-$(date +%F).sql.gz

# Только данные
docker compose exec -T postgres pg_dump -U cicd --data-only cicd \
  | gzip > cicd-data-$(date +%F).sql.gz
```

### 3.2. Artifacts (Phase 8)

```bash
# Локальная файловая система
rsync -a --delete "$CICD_STORAGE/" ./artifacts-backup/

# S3 (при использовании S3-совместимого бэкенда)
aws s3 sync s3://cicd-artifacts ./artifacts-backup/
```

---

## 4. Восстановление

### 4.1. Скрипт `scripts/restore.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

BACKUP_FILE="$1"
BACKUP_DIR="/backups"
DB_USER="${CICD_DATABASE_USER:-cicd}"
DB_NAME="${CICD_DATABASE_NAME:-cicd}"

if [ -z "$BACKUP_FILE" ]; then
  echo "Usage: $0 <backup-file>"
  echo "Example: $0 /backups/postgres-2026-08-26.sql.gz"
  exit 1
fi

# 1. Остановить backend (frontend можно оставить)
docker compose stop backend

# 2. Восстановить PostgreSQL
gunzip -c "$BACKUP_FILE" \
  | docker compose exec -T postgres psql -U "$DB_USER" -d "$DB_NAME"

# 3. Запустить backend
docker compose up -d backend

# 4. Проверка
curl -fsS http://127.0.0.1:22801/api/v1/health
```

### 4.2. Порядок восстановления

1. Остановить `backend` (чтобы не было запросов к БД во время restore).
2. Восстановить PostgreSQL из `pg_dump`:
   ```bash
   gunzip -c /backups/postgres-2026-08-26.sql.gz \
     | docker compose exec -T postgres psql -U cicd -d cicd
   ```
3. Восстановить artifacts (если применимо):
   ```bash
   rsync -a ./artifacts-backup/ "$CICD_STORAGE/"
   ```
4. Запустить `backend` и проверить `/api/v1/health`:
   ```bash
   docker compose up -d backend
   curl -fsS http://127.0.0.1:22801/api/v1/health
   ```
5. Проверить целостность данных:
   ```bash
   curl -sS http://127.0.0.1:22801/api/v1/projects | jq length
   ```

---

## 5. Point-in-time recovery

- Если включён WAL archiving — восстановление до момента времени.
- Нужен отдельный backup solution: pgBackRest, WAL-G, Barman.
- На текущий момент PITR не настроен; восстановление — из последнего `pg_dump`.

### 5.1. Включение WAL archiving (план)

```postgresql
# postgresql.conf
wal_level = replica
archive_mode = on
archive_command = 'test ! -f /backups/wal/%f && cp %p /backups/wal/%f'
```

---

## 6. Проверка бэкапов

### 6.1. Регулярная проверка

- Раз в месяц — test restore на staging-окружении.
- Скрипт `scripts/verify-backup.sh`:
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  BACKUP="/backups/postgres-$(date -d yesterday +%F).sql.gz"
  gunzip -t "$BACKUP" && echo "OK: $BACKUP" || echo "FAIL: $BACKUP"
  ```

### 6.2. Метрики

- `backup_last_success_timestamp` — время последнего успешного бэкапа.
- `backup_last_size_bytes` — размер последнего дампа.
- Alert: если `backup_last_success_timestamp` старше 26 часов.

---

## 7. RPO / RTO

| Сценарий | RPO | RTO | Действия |
|---|---|---|---|
| Потеря данных PostgreSQL | 24 часа | 1 час | Restore из последнего `pg_dump` |
| Потеря artifacts (Phase 8) | 24 часа | 30 мин | `rsync` из бэкапа или S3 replication |
| Потеря entire host | 24 часа | 4 часа | Развёртывание на новом хосте из бэкапа |
| Corruption БД | 24 часа | 2 часа | Drop + restore из `pg_dump` |

### 7.1. Определения

- **RPO** (Recovery Point Objective) — максимально допустимая потеря данных по времени.
- **RTO** (Recovery Time Objective) — максимально допустимое время восстановления.

### 7.2. Текущие значения

- RPO = 24 часа (ежедневный `pg_dump` в 02:00).
- RTO = 1 час (restore из gzip-дампа + запуск backend).

### 7.3. Целевые значения (Phase 5+)

- RPO = 1 час (WAL archiving + incremental backups).
- RTO = 15 минут (pgBackRest delta restore).

---

## 8. Disaster recovery

### 8.1. Полная потеря хоста

1. Развернуть Docker Compose на новом хосте:
   ```bash
   git clone git@github.com:FerrPOINT/CI-CD.git
   cd CI-CD
   cp .env.example .env  # восстановить CICD_ значения
   ```
2. Восстановить PostgreSQL из бэкапа (раздел 4).
3. Восстановить artifacts (если применимо).
4. Запустить сервисы:
   ```bash
   docker compose up --build -d
   curl -fsS http://127.0.0.1:22801/api/v1/health
   ```

### 8.2. Corruption БД

1. Остановить backend.
2. Подключиться к PostgreSQL, проверить состояние:
   ```bash
   docker compose exec postgres psql -U cicd -d cicd -c "SELECT count(*) FROM projects;"
   ```
3. Если corruption — drop и restore:
   ```bash
   docker compose exec postgres psql -U cicd -d postgres -c "DROP DATABASE cicd;"
   docker compose exec postgres psql -U cicd -d postgres -c "CREATE DATABASE cicd;"
   gunzip -c /backups/postgres-2026-08-26.sql.gz \
     | docker compose exec -T postgres psql -U cicd -d cicd
   ```
4. `store::migrate()` при старте создаст недостающие таблицы (`CREATE TABLE IF NOT EXISTS`).

---

## 9. Хранение бэкапов

| Тип | Количество копий | Срок хранения |
|---|---|---|
| Дневные | 7 | 7 дней |
| Недельные | 4 | 4 недели |
| Месячные | 12 | 12 месяцев (план) |

- Бэкапы хранятся вне хоста приложения (external volume / S3 / remote server).
- Минимум одна копия — на отдельном физическом устройстве.

---

## 10. Env-переменные

| Переменная | Назначение |
|---|---|
| `CICD_DATABASE_USER` | Пользователь PostgreSQL для `pg_dump` |
| `CICD_DATABASE_PASSWORD` | Пароль PostgreSQL |
| `CICD_DATABASE_NAME` | Имя базы данных |
| `CICD_DATABASE_PORT` | Порт PostgreSQL (default `22543`) |
| `CICD_STORAGE` | Директория для artifacts (Phase 8) |

> `.env` не коммитится в Git. См. `docs/SECURITY.md`.

---

## 11. References

- `docs/DEPLOYMENT.md` — развёртывание.
- `docs/OPS_RUNBOOK.md` — операционный runbook.
- `docs/MONITORING.md` — мониторинг и алерты.
- `docs/RUNTIME.md` — конфигурация runtime.
- `docs/adr/0004-postgresql-only.md` — решение о единственной СУБД.
- `docs/SECURITY.md` — безопасность и секреты.