# Database Standards — Forge CI/CD

## 1. СУБД

- **PostgreSQL** 17.6+.
- Тип UUID — `UUID` (через `gen_random_uuid()` в SQLx).
- PK по умолчанию — `UUID` v4, генерируется на стороне приложения (`Uuid::new_v4()`).
- Исключение: `job_logs.id` — `BIGSERIAL` (автоинкрементный integer), т.к. логи — append-only и могут быть многочисленными.
- Кодировка БД — UTF-8.

## 2. Миграции

### 2.1 Текущий подход

Схема создаётся при старте приложения через `store::migrate()` — `CREATE TABLE IF NOT EXISTS` для всех таблиц. Миграции — idempotent, безопасны при повторном запуске.

```rust
// backend/src/store.rs
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS projects ...").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS pipelines ...").execute(pool).await?;
    // ...
    Ok(())
}
```

### 2.2 Планируемый подход (Phase 2+)

- Переход на `sqlx::migrate!` макрос или `refinery` для версионированных миграций.
- Имя файла: `YYYYMMDDHHMMSS_description.sql`.
- Каждая миграция:
  - оборачивается в транзакцию `BEGIN; ... COMMIT;`
  - имеет `ROLLBACK` стратегию
  - не удаляет данные без `WHERE` и бэкапа
- Запрещено:
  - изменять уже применённую миграцию
  - удалять столбцы с данными без explicit migration step

## 3. Именование

| Объект | Конвенция | Пример |
|---|---|---|
| Таблица | snake_case, множественное число | `job_logs` |
| Столбец | snake_case | `created_at` |
| PK | `id` | `id UUID PRIMARY KEY` |
| FK | `<table_singular>_id` | `project_id`, `stage_id` |
| Индекс | `idx_<table>_<columns>` | `idx_pipelines_project_id` |
| Unique constraint | `<table>_<columns>_key` | `stages_pipeline_id_position_key` |
| Check constraint | `<table>_<column>_check` | `pipelines_status_check` |
| FK constraint | `<table>_<column>_fkey` | `pipelines_project_id_fkey` |

## 4. Типы данных

| Назначение | Тип | Примечание |
|---|---|---|
| ID (сущности) | `UUID` | PK, `Uuid::new_v4()` на стороне приложения |
| ID (логи) | `BIGSERIAL` | `job_logs.id`, автоинкремент |
| Timestamp | `TIMESTAMPTZ` | всегда UTC, `now()` default |
| Текст | `TEXT` | для всех строковых полей |
| Порядок | `INTEGER` | `position` в stages/jobs, `sequence` в job_logs |
| Статус | `TEXT` + `CHECK` | enum через CHECK constraint |

### Почему TEXT + CHECK вместо native ENUM

- `TEXT` + `CHECK` проще в миграциях: добавление нового статуса — `ALTER TABLE ... DROP CONSTRAINT ... ADD CONSTRAINT ...`.
- Native `ENUM` требует `ALTER TYPE ... ADD VALUE`, что блокирует транзакции в некоторых версиях PostgreSQL.
- Значения статусов зафиксированы в коде (`JobStatus` enum в `domain.rs`) — БД-уровень CHECK дублирует защиту.
- `serde` с `rename_all = "snake_case"` обеспечивает консистентность JSON ↔ БД.

### CHECK constraints

```sql
-- pipelines, stages, jobs
CHECK (status IN ('queued', 'running', 'success', 'failed', 'canceled'))
```

## 5. Foreign Keys

### 5.1 ON DELETE CASCADE

Все FK используют `ON DELETE CASCADE` — удаление родителя автоматически удаляет всех потомков.

| FK | Parent → Child | Cascade |
|----|----------------|---------|
| `pipelines.project_id` | `projects` → `pipelines` | CASCADE |
| `stages.pipeline_id` | `pipelines` → `stages` | CASCADE |
| `jobs.stage_id` | `stages` → `jobs` | CASCADE |
| `job_logs.job_id` | `jobs` → `job_logs` | CASCADE |

```
projects ──CASCADE──→ pipelines ──CASCADE──→ stages ──CASCADE──→ jobs ──CASCADE──→ job_logs
```

### 5.2 Обоснование CASCADE

- Пайплайн не имеет смысла без проекта.
- Стадия не имеет смысла без пайплайна.
- Задача не имеет смысла без стадии.
- Логи не имеют смысла без задачи.
- Нет soft delete в текущей версии (запланирован в Phase 9 для audit_log).

### 5.3 Планируемые FK (Phase 2+)

| FK | Strategy | Обоснование |
|----|----------|-------------|
| `runners` → `projects` | `SET NULL` | Runner может пережить удаление проекта |
| `secrets.project_id` | `CASCADE` | Секреты удаляются с проектом |
| `webhooks.project_id` | `CASCADE` | Webhooks удаляются с проектом |

## 6. Структура таблиц — сводка

| Table | PK | Колонки | FK (CASCADE) | Constraints |
|-------|----|---------|--------------|-------------|
| `projects` | `id UUID` | `name`, `repository_url`, `default_branch`, `created_at` | — | `name UNIQUE` |
| `pipelines` | `id UUID` | `project_id`, `git_ref`, `status`, `created_at`, `started_at`, `finished_at` | `project_id → projects` | `status CHECK` |
| `stages` | `id UUID` | `pipeline_id`, `name`, `position`, `status` | `pipeline_id → pipelines` | `(pipeline_id, position) UNIQUE`, `status CHECK` |
| `jobs` | `id UUID` | `stage_id`, `name`, `image`, `command`, `position`, `status`, `started_at`, `finished_at` | `stage_id → stages` | `(stage_id, position) UNIQUE`, `status CHECK` |
| `job_logs` | `id BIGSERIAL` | `job_id`, `sequence`, `message`, `created_at` | `job_id → jobs` | `(job_id, sequence) UNIQUE` |

## 7. SQL Style

- Ключевые слова — uppercase: `SELECT`, `FROM`, `WHERE`, `INSERT`, `UPDATE`, `DELETE`.
- Идентификаторы — lowercase.
- Все запросы — parameterized: `$1`, `$2`, ...
- Запросы форматировать с переносами:

```sql
SELECT id, project_id, git_ref, status, created_at, started_at, finished_at
FROM pipelines
WHERE project_id = $1
ORDER BY created_at DESC
LIMIT 50;
```

## 8. Defaults

| Колонка | Default | Уровень |
|---------|---------|---------|
| `id` (UUID) | `Uuid::new_v4()` | Приложение |
| `id` (BIGSERIAL) | `nextval(...)` | БД |
| `created_at` | `now()` | БД |
| `default_branch` | `'main'` | БД |
| `status` | `'queued'` | Приложение (при создании) |
| `started_at` | `NULL` | Приложение (при переходе в `running`) |
| `finished_at` | `NULL` | Приложение (при терминальном переходе) |

## 9. Connection Pool

- **SQLx** `PgPoolOptions` — настраивается при старте приложения.
- `max_connections = 10` (текущее значение, см. `docs/PERFORMANCE.md`).
- `acquire_timeout = 30s` (default SQLx).
- Пул хранится в `AppState` как `Option<PgPool>`:
  - `Some(pool)` — нормальный режим.
  - `None` — режим без БД (health-check работает, остальные endpoint возвращают `503`).

## 10. Seeds и fixtures

- Seed-данные для dev — через `justfile` команды или curl-скрипты.
- Fixtures для тестов — в `backend/tests/` (integration tests).
- Тестовая БД — отдельная database `cicd_test` (запланировано в Phase 2).

## 11. Безопасность

- SQL-инъекции исключены: все запросы — parameterized (`sqlx::query` с `$1`, `$2`).
- Никаких string-concatenated SQL.
- Доступ к БД — через отдельного пользователя `cicd` с минимальными правами.
- Пароль БД — через env var `CICD_DATABASE_PASSWORD`.

## References

- `docs/DATA_MODEL.md` — полная физическая модель и ER-диаграмма.
- `docs/DATABASE_INDEXES.md` — перечень индексов и стратегия.
- `docs/PERFORMANCE.md` — connection pooling и оптимизация запросов.
- `backend/src/store.rs` — исходный код схемы БД.