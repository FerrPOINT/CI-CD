# Database Migrations — Forge CI/CD

## 1. Overview

Схема PostgreSQL управляется двумя механизмами:

1. **Schema bootstrap** — `store::migrate()` в `backend/src/store.rs`. Выполняется при каждом старте приложения. Все таблицы создаются через `CREATE TABLE IF NOT EXISTS`.
2. **SQLx migrations** (планируется) — версионные SQL-файлы для эволюции схемы после стабилизации MVP.

В текущей версии (Phase 0) используется только schema bootstrap. Переход на версионные миграции запланирован при добавлении breaking changes в схему.

## 2. Tooling

| Tool | Purpose | Статус |
|------|---------|--------|
| `sqlx` 0.8 | Compile-time checked queries, runtime SQL execution | ✅ активно |
| `store::migrate()` | Schema bootstrap при старте приложения | ✅ активно |
| `sqlx migrate` | Версионные миграции (CLI) | 📋 планируется |
| `sqlx::query_as` | Type-safe маппинг строк в Rust-структуры | ✅ активно |

## 3. Schema Bootstrap (`store.rs`)

### 3.1 Принцип

Схема создаётся при старте приложения через функцию `migrate()` в `backend/src/store.rs`. Все DDL-операторы выполняются одним `sqlx::raw_sql(...)` вызовом:

```rust
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS projects (...);
        CREATE TABLE IF NOT EXISTS pipelines (...);
        CREATE TABLE IF NOT EXISTS stages (...);
        CREATE TABLE IF NOT EXISTS jobs (...);
        CREATE TABLE IF NOT EXISTS job_logs (...);
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}
```

### 3.2 Порядок создания таблиц

Порядок строго соответствует внешним ключам (родитель → потомок):

| Порядок | Таблица | FK-зависимости |
|---------|---------|----------------|
| 1 | `projects` | нет |
| 2 | `pipelines` | `project_id → projects(id) CASCADE` |
| 3 | `stages` | `pipeline_id → pipelines(id) CASCADE` |
| 4 | `jobs` | `stage_id → stages(id) CASCADE` |
| 5 | `job_logs` | `job_id → jobs(id) CASCADE` |

### 3.3 Характеристики

- **Идемпотентность**: `CREATE TABLE IF NOT EXISTS` — безопасно при повторном запуске.
- **Монолитный DDL**: все таблицы в одном `raw_sql` вызове — атомарное применение.
- **Без версионирования**: нет таблицы `_sqlx_migrations`, нет отката.
- **Без индексов**: индексы добавляются отдельно (планируется).

### 3.4 Когда используется

- При каждом старте `cicd-server` (`main.rs` вызывает `store::migrate(&pool)` перед запуском HTTP-сервера).
- При запуске в Docker Compose (backend-контейнер).
- При локальном запуске `cargo run --bin cicd-server`.

## 4. Полная схема

### 4.1 projects

```sql
CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    repository_url TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 4.2 pipelines

```sql
CREATE TABLE IF NOT EXISTS pipelines (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    git_ref TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);
```

### 4.3 stages

```sql
CREATE TABLE IF NOT EXISTS stages (
    id UUID PRIMARY KEY,
    pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
    UNIQUE(pipeline_id, position)
);
```

### 4.4 jobs

```sql
CREATE TABLE IF NOT EXISTS jobs (
    id UUID PRIMARY KEY,
    stage_id UUID NOT NULL REFERENCES stages(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    image TEXT NOT NULL,
    command TEXT NOT NULL,
    position INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','running','success','failed','canceled')),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    UNIQUE(stage_id, position)
);
```

### 4.5 job_logs

```sql
CREATE TABLE IF NOT EXISTS job_logs (
    id BIGSERIAL PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(job_id, sequence)
);
```

## 5. CASCADE Hierarchy

Все внешние ключи используют `ON DELETE CASCADE`:

```
projects ──CASCADE──> pipelines ──CASCADE──> stages ──CASCADE──> jobs ──CASCADE──> job_logs
```

Удаление проекта удаляет все его пайплайны, стадии, задачи и логи. Удаление пайплайна — все стадии, задачи, логи. И т.д.

## 6. Sequence Generation для job_logs

Функция `next_log_sequence` в `store.rs` вычисляет следующий `sequence` для логов задачи:

```rust
pub async fn next_log_sequence(pool: &PgPool, job_id: uuid::Uuid) -> Result<i32, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence \
         FROM job_logs WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;
    Ok(row.get("next_sequence"))
}
```

`UNIQUE(job_id, sequence)` гарантирует отсутствие дубликатов. При race condition (два append одновременно) — один получит `unique_violation` и должен retry.

## 7. Версионные миграции (планируется)

### 7.1 Переход с bootstrap на версии

При первой breaking change в схеме:

1. Создать директорию `backend/migrations/`.
2. Базовая миграция `0001_initial_schema.sql` — весь текущий DDL (без `IF NOT EXISTS`, с `CREATE TABLE`).
3. Проверка: схема уже существует → `CREATE TABLE IF NOT EXISTS` в базовой миграции.
4. Включить `sqlx::migrate!()` макрос в `store.rs` вместо `raw_sql`.
5. Создать таблицу `_sqlx_migrations` автоматически при первом запуске.

### 7.2 Структура директории

```
backend/migrations/
├── 0001_initial_schema.sql
├── 0002_add_indexes.sql
├── 0003_add_users_table.sql
└── ...
```

### 7.3 Naming Convention

```
{version}_{description}.sql
```

- Версия — целое число, строго последовательное (4 цифры с padding).
- Описание — snake_case.
- Пример: `0012_add_audit_log_table.sql`.

### 7.4 Применение

```bash
# Локально
cd backend
sqlx migrate run --database-url $CICD_DATABASE_URL

# В Docker
docker compose run --rm backend sqlx migrate run
```

## 8. Добавление новой миграции

### 8.1 Пока используется schema bootstrap

1. Отредактировать `backend/src/store.rs` → функцию `migrate()`.
2. Добавить `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE` в существующий `raw_sql` блок.
3. При добавлении новой таблицы — разместить после таблиц, от которых она зависит (FK).
4. Проверить: `cargo test`.
5. Пересоздать БД локально: `docker compose down -v && docker compose up -d postgres && cargo run --bin cicd-server`.

### 8.2 После перехода на версионные миграции

```bash
cd backend
sqlx migrate add <description>
# создаст файл backend/migrations/{NNNN}_{description}.sql
```

### 8.3 Правила

#### Must

- Каждая миграция идемпотентна в пределах своей версии (`IF NOT EXISTS` / `IF EXISTS`).
- Новые колонки — nullable или с default.
- Индексы в production — `CONCURRENTLY`.
- Обновить `docs/DATA_MODEL.md` при изменении схемы.

#### Must Not

- Нельзя удалять колонки с активными зависимостями.
- Нельзя переименовывать таблицы в одной миграции без backward-compatible alias.
- Нельзя менять тип колонки с потерей данных.
- Нельзя делать heavy ALTER на больших таблицах без отдельного runbook.

## 9. Локальная разработка

### 9.1 Сброс БД

```bash
docker compose down -v
docker compose up -d postgres
cargo run --bin cicd-server   # migrate() выполнится при старте
```

### 9.2 Проверка схемы

```bash
docker compose exec postgres psql -U cicd -d cicd -c "\dt"
docker compose exec postgres psql -U cicd -d cicd -c "\d projects"
docker compose exec postgres psql -U cicd -d cicd -c "\d pipelines"
```

## References

- `backend/src/store.rs` — исходный код `migrate()` и `next_log_sequence()`.
- `docs/DATA_MODEL.md` — полная документация схемы.
- `docs/ARCHITECTURE.md` — архитектура backend.
- `docs/ROADMAP.md` — план перехода на версионные миграции.