# Database Indexes — Forge CI/CD

## 1. Overview

Индексы PostgreSQL для обеспечения производительности и целостности данных Forge CI/CD. Схема создаётся при старте приложения через `store::migrate()` (`backend/src/store.rs`).

> **Source of truth:** актуальная схема в `backend/src/store.rs`. При расхождении приоритет у исходного кода.

## 2. Текущие индексы

### 2.1 Реализованные индексы

Primary/unique indexes создаются через `CREATE TABLE` constraints. Явные supporting indexes создаются отдельными `CREATE INDEX IF NOT EXISTS` в `store::migrate()`.

| Table | Index / Constraint | Type | Columns | Purpose |
|-------|-------------------|------|---------|---------|
| `projects` | `projects_pkey` | B-tree PRIMARY KEY | `id` | PK lookup |
| `projects` | `projects_name_key` | B-tree UNIQUE | `name` | Уникальность имени проекта |
| `pipelines` | `pipelines_pkey` | B-tree PRIMARY KEY | `id` | PK lookup |
| `stages` | `stages_pkey` | B-tree PRIMARY KEY | `id` | PK lookup |
| `stages` | `stages_pipeline_id_position_key` | B-tree UNIQUE | `(pipeline_id, position)` | Уникальность позиции стадии в пайплайне |
| `jobs` | `jobs_pkey` | B-tree PRIMARY KEY | `id` | PK lookup |
| `jobs` | `jobs_stage_id_position_key` | B-tree UNIQUE | `(stage_id, position)` | Уникальность позиции задачи в стадии |
| `job_logs` | `job_logs_pkey` | B-tree PRIMARY KEY | `id` | PK lookup (BIGSERIAL) |
| `job_logs` | `job_logs_job_id_sequence_key` | B-tree UNIQUE | `(job_id, sequence)` | Уникальность sequence лога в рамках job |
| `runners` | `idx_runners_status` | B-tree | `status` | Выборка registry по статусу |
| `project_secrets` | `idx_project_secrets_project` | B-tree | `project_id` | Секреты проекта |
| `artifacts` | `idx_artifacts_job` | B-tree | `job_id` | Артефакты job |
| `deployments` | `idx_deployments_environment` | B-tree | `environment_id` | История окружения |
| `schedules` | `idx_schedules_project` | B-tree | `project_id` | Расписания проекта |
| `webhooks` | `idx_webhooks_project` | B-tree | `project_id` | Webhook-конфигурации проекта |
| `audit_log` | `idx_audit_log_created` | B-tree DESC | `created_at` | Последние события аудита |
| `pipelines` | `idx_pipelines_project_id` | B-tree | `project_id` | Список pipeline проекта |

### 2.2 Карта индексов по таблицам

#### projects

```sql
-- PRIMARY KEY
CREATE UNIQUE INDEX projects_pkey ON projects (id);

-- UNIQUE CONSTRAINT
CREATE UNIQUE INDEX projects_name_key ON projects (name);
```

#### pipelines

```sql
-- PRIMARY KEY
CREATE UNIQUE INDEX pipelines_pkey ON pipelines (id);

-- TODO: добавить idx_pipelines_project_id
```

#### stages

```sql
-- PRIMARY KEY
CREATE UNIQUE INDEX stages_pkey ON stages (id);

-- UNIQUE CONSTRAINT
CREATE UNIQUE INDEX stages_pipeline_id_position_key ON stages (pipeline_id, position);

-- TODO: добавить idx_stages_pipeline_id
```

#### jobs

```sql
-- PRIMARY KEY
CREATE UNIQUE INDEX jobs_pkey ON jobs (id);

-- UNIQUE CONSTRAINT
CREATE UNIQUE INDEX jobs_stage_id_position_key ON jobs (stage_id, position);

-- TODO: добавить idx_jobs_stage_id
```

#### job_logs

```sql
-- PRIMARY KEY
CREATE UNIQUE INDEX job_logs_pkey ON job_logs (id);

-- UNIQUE CONSTRAINT
CREATE UNIQUE INDEX job_logs_job_id_sequence_key ON job_logs (job_id, sequence);

-- TODO: добавить idx_job_logs_job_id
```

## 3. Планируемые индексы (Phase 2)

> **Текущее состояние:** FK-колонки (`project_id`, `pipeline_id`, `stage_id`, `job_id`) не имеют явных индексов. PostgreSQL не создаёт индексы для FK автоматически. Для оптимизации запросов `list_pipelines`, `pipeline_detail` и `list_logs` требуются дополнительные индексы.

| Table | Index | Type | Columns | Query | Priority |
|-------|-------|------|---------|-------|----------|
| `pipelines` | `idx_pipelines_project_id` | B-tree | `project_id` | `GET /projects/{id}/pipelines` | ✅ Реализован |
| `pipelines` | `idx_pipelines_project_created` | B-tree | `(project_id, created_at DESC)` | `GET /projects/{id}/pipelines` LIMIT 50 | Medium |
| `pipelines` | `idx_pipelines_status` | B-tree | `status` | Фильтр по статусу (Phase 2) | Low |
| `stages` | `idx_stages_pipeline_id` | B-tree | `pipeline_id` | `GET /pipelines/{id}` (pipeline_detail) | High |
| `jobs` | `idx_jobs_stage_id` | B-tree | `stage_id` | `GET /pipelines/{id}` (pipeline_detail) | High |
| `job_logs` | `idx_job_logs_job_id` | B-tree | `job_id` | `GET /jobs/{id}/logs` | High |
| `job_logs` | `idx_job_logs_job_sequence` | B-tree | `(job_id, sequence)` | `GET /jobs/{id}/logs` ORDER BY sequence | Covered by UNIQUE |

### Migration SQL (Phase 2)

```sql
-- Pipelines
CREATE INDEX IF NOT EXISTS idx_pipelines_project_id
  ON pipelines (project_id);

CREATE INDEX IF NOT EXISTS idx_pipelines_project_created
  ON pipelines (project_id, created_at DESC);

-- Stages
CREATE INDEX IF NOT EXISTS idx_stages_pipeline_id
  ON stages (pipeline_id);

-- Jobs
CREATE INDEX IF NOT EXISTS idx_jobs_stage_id
  ON jobs (stage_id);

-- Job logs
CREATE INDEX IF NOT EXISTS idx_job_logs_job_id
  ON job_logs (job_id);
```

> Индекс `idx_job_logs_job_sequence` уже покрыт unique constraint `job_logs_job_id_sequence_key` на `(job_id, sequence)`.

## 4. Composite Index Strategy

### 4.1 Частые запросы

| Query pattern | Index | Обоснование |
|---------------|-------|-------------|
| `WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50` | `(project_id, created_at DESC)` | Покрывающий для list_pipelines |
| `WHERE pipeline_id = $1 ORDER BY position` | `(pipeline_id, position)` | Покрыт UNIQUE constraint |
| `WHERE stage_id = $1 ORDER BY position` | `(stage_id, position)` | Покрыт UNIQUE constraint |
| `WHERE job_id = $1 ORDER BY sequence` | `(job_id, sequence)` | Покрыт UNIQUE constraint |

### 4.2 Агрегация статусов

`refresh_statuses()` выполняет `SELECT status FROM jobs WHERE stage_id = $1` и `SELECT status FROM stages WHERE pipeline_id = $1`. Индексы на FK-колонки (`idx_jobs_stage_id`, `idx_stages_pipeline_id`) оптимизируют эти запросы.

## 5. Index Maintenance

- `REINDEX CONCURRENTLY` во время low-traffic window для обслуживания bloat.
- Мониторинг bloat через `pg_stat_user_indexes`:
  ```sql
  SELECT schemaname, relname, indexrelname, idx_scan, idx_tup_read, idx_tup_fetch
  FROM pg_stat_user_indexes
  ORDER BY idx_scan DESC;
  ```
- `ANALYZE` после массовых операций (миграций, импорта данных).
- При объёме `job_logs` > 1M строк — рассмотреть партиционирование по `created_at` (range).

## 6. JSONB Indexes

В текущей схеме нет JSONB-колонок. Запланированы в Phase 5+:
- `pipelines.config` — JSONB GIN для конфигурации пайплайна из YAML.
- `projects.settings` — JSONB GIN для настроек проекта.

## 7. Unique Constraints как бизнес-инварианты

| Constraint | Table | Columns | Бизнес-правило |
|------------|-------|---------|----------------|
| `projects_name_key` | `projects` | `name` | Имя проекта уникально глобально |
| `stages_pipeline_id_position_key` | `stages` | `(pipeline_id, position)` | Позиция стадии уникальна в рамках пайплайна |
| `jobs_stage_id_position_key` | `jobs` | `(stage_id, position)` | Позиция задачи уникальна в рамках стадии |
| `job_logs_job_id_sequence_key` | `job_logs` | `(job_id, sequence)` | Sequence лога уникален в рамках задачи |

## References

- `docs/DATA_MODEL.md` — полная физическая модель БД.
- `docs/DATABASE_STANDARDS.md` — стандарты БД (типы, именование, FK).
- `docs/PERFORMANCE.md` — оптимизация запросов и connection pooling.
- `backend/src/store.rs` — исходный код схемы БД.