# Дата-модель Forge CI/CD

## 0. Фактическая схема реализованных таблиц

Схема создаётся при старте приложения через `store::migrate()` (`backend/src/store.rs`). Все таблицы — `CREATE TABLE IF NOT EXISTS`. При расхождении приоритет у исходного кода `store.rs`.

### ER-диаграмма (логическая)

```
projects (1) ──── (N) pipelines (1) ──── (N) stages (1) ──── (N) jobs (1) ──── (N) job_logs
   │                    │                    │                    │
   │ UUID PK            │ UUID PK            │ UUID PK            │ UUID PK
   │ name UNIQUE        │ project_id FK      │ pipeline_id FK     │ stage_id FK
   │                    │ status CHECK       │ position UNIQUE    │ status CHECK
   │                    │                    │ status CHECK       │
   │                    │                    │                    │ BIGSERIAL PK
   └── CASCADE          └── CASCADE          └── CASCADE          └── CASCADE
```

Уровень изоляции: каждый родитель CASCADE-deletes удаляет всех потомков.

---

## 1. projects

Таблица проектов-репозиториев.

```
                        Table "public.projects"
     Column      |           Type           | Nullable |      Default
-----------------+--------------------------+----------+-------------------
 id              | uuid                     | not null |
 name            | text                     | not null |
 repository_url  | text                     | not null |
 default_branch  | text                     | not null | 'main'
 created_at      | timestamp with time zone | not null | now()
Indexes:
    "projects_pkey" PRIMARY KEY, btree (id)
    "projects_name_key" UNIQUE CONSTRAINT, btree (name)
Referenced by:
    TABLE "pipelines" CONSTRAINT pipelines_project_id_fkey
        FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ, генерируется `Uuid::new_v4()` |
| `name` | TEXT | NOT NULL | — | Уникальное имя проекта |
| `repository_url` | TEXT | NOT NULL | — | URL Git-репозитория |
| `default_branch` | TEXT | NOT NULL | `'main'` | Ветка по умолчанию |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время создания |

**Индексы:**
- `projects_pkey` — PRIMARY KEY (id)
- `projects_name_key` — UNIQUE (name)

**External keys:** нет.

**Referenced by:** `pipelines.project_id` → `projects.id` ON DELETE CASCADE.

---

## 2. pipelines

Таблица запусков пайплайнов.

```
                       Table "public.pipelines"
    Column    |           Type           | Nullable |      Default
--------------+--------------------------+----------+-------------------
 id           | uuid                     | not null |
 project_id   | uuid                     | not null |
 git_ref      | text                     | not null |
 status       | text                     | not null |
 created_at   | timestamp with time zone | not null | now()
 started_at   | timestamp with time zone |          |
 finished_at  | timestamp with time zone |          |
Indexes:
    "pipelines_pkey" PRIMARY KEY, btree (id)
Check constraints:
    "pipelines_status_check" CHECK (status IN ('queued','running','success','failed','canceled'))
Foreign-key constraints:
    "pipelines_project_id_fkey" FOREIGN KEY (project_id)
        REFERENCES projects(id) ON DELETE CASCADE
Referenced by:
    TABLE "stages" CONSTRAINT stages_pipeline_id_fkey
        FOREIGN KEY (pipeline_id) REFERENCES pipelines(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ, `Uuid::new_v4()` |
| `project_id` | UUID | NOT NULL | — | FK → `projects.id`, CASCADE |
| `git_ref` | TEXT | NOT NULL | — | Git-реф (ветка, тег, SHA) |
| `status` | TEXT | NOT NULL | — | Статус: `queued` / `running` / `success` / `failed` / `canceled` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время создания |
| `started_at` | TIMESTAMPTZ | NULL | — | Время начала выполнения (при `running`) |
| `finished_at` | TIMESTAMPTZ | NULL | — | Время завершения (терминальный статус) |

**CHECK constraint:** `status IN ('queued','running','success','failed','canceled')`.

**Индексы:**
- `pipelines_pkey` — PRIMARY KEY (id)

> **TODO:** добавить индекс `idx_pipelines_project_id` на `project_id` для оптимизации `list_pipelines`.

**FK:** `project_id` → `projects(id)` ON DELETE CASCADE.

**Referenced by:** `stages.pipeline_id` → `pipelines(id)` ON DELETE CASCADE.

---

## 3. stages

Таблица стадий пайплайна (упорядоченные шаги: build, test, deploy).

```
                        Table "public.stages"
    Column    |           Type           | Nullable |      Default
--------------+--------------------------+----------+-------------------
 id           | uuid                     | not null |
 pipeline_id  | uuid                     | not null |
 name         | text                     | not null |
 position     | integer                  | not null |
 status       | text                     | not null |
Indexes:
    "stages_pkey" PRIMARY KEY, btree (id)
    "stages_pipeline_id_position_key" UNIQUE CONSTRAINT, btree (pipeline_id, position)
Check constraints:
    "stages_status_check" CHECK (status IN ('queued','running','success','failed','canceled'))
Foreign-key constraints:
    "stages_pipeline_id_fkey" FOREIGN KEY (pipeline_id)
        REFERENCES pipelines(id) ON DELETE CASCADE
Referenced by:
    TABLE "jobs" CONSTRAINT jobs_stage_id_fkey
        FOREIGN KEY (stage_id) REFERENCES stages(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ, `Uuid::new_v4()` |
| `pipeline_id` | UUID | NOT NULL | — | FK → `pipelines.id`, CASCADE |
| `name` | TEXT | NOT NULL | — | Название стадии (e.g. `build`, `test`, `deploy`) |
| `position` | INTEGER | NOT NULL | — | Порядок выполнения (0, 1, 2, ...) |
| `status` | TEXT | NOT NULL | — | Агрегированный статус из jobs |

**CHECK constraint:** `status IN ('queued','running','success','failed','canceled')`.

**UNIQUE constraint:** `(pipeline_id, position)` — позиция уникальна в рамках пайплайна.

**Индексы:**
- `stages_pkey` — PRIMARY KEY (id)
- `stages_pipeline_id_position_key` — UNIQUE (pipeline_id, position)

> **TODO:** добавить индекс `idx_stages_pipeline_id` на `pipeline_id` (для `pipeline_detail`).

**FK:** `pipeline_id` → `pipelines(id)` ON DELETE CASCADE.

**Referenced by:** `jobs.stage_id` → `stages(id)` ON DELETE CASCADE.

---

## 4. jobs

Таблица задач внутри стадии.

```
                          Table "public.jobs"
    Column    |           Type           | Nullable |      Default
--------------+--------------------------+----------+-------------------
 id           | uuid                     | not null |
 stage_id     | uuid                     | not null |
 name         | text                     | not null |
 image        | text                     | not null |
 command      | text                     | not null |
 position     | integer                  | not null |
 status       | text                     | not null |
 started_at   | timestamp with time zone |          |
 finished_at  | timestamp with time zone |          |
Indexes:
    "jobs_pkey" PRIMARY KEY, btree (id)
    "jobs_stage_id_position_key" UNIQUE CONSTRAINT, btree (stage_id, position)
Check constraints:
    "jobs_status_check" CHECK (status IN ('queued','running','success','failed','canceled'))
Foreign-key constraints:
    "jobs_stage_id_fkey" FOREIGN KEY (stage_id)
        REFERENCES stages(id) ON DELETE CASCADE
Referenced by:
    TABLE "job_logs" CONSTRAINT job_logs_job_id_fkey
        FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ, `Uuid::new_v4()` |
| `stage_id` | UUID | NOT NULL | — | FK → `stages.id`, CASCADE |
| `name` | TEXT | NOT NULL | — | Название задачи (e.g. `checkout`, `unit-tests`, `deploy`) |
| `image` | TEXT | NOT NULL | — | Docker-образ для выполнения (e.g. `rust:1.86`, `alpine:3.21`) |
| `command` | TEXT | NOT NULL | — | Команда выполнения (e.g. `cargo test`, `git fetch --all`) |
| `position` | INTEGER | NOT NULL | — | Порядок выполнения внутри стадии |
| `status` | TEXT | NOT NULL | — | Статус: `queued` / `running` / `success` / `failed` / `canceled` |
| `started_at` | TIMESTAMPTZ | NULL | — | Время начала (при переходе в `running`) |
| `finished_at` | TIMESTAMPTZ | NULL | — | Время завершения (терминальный статус) |

**CHECK constraint:** `status IN ('queued','running','success','failed','canceled')`.

**UNIQUE constraint:** `(stage_id, position)` — позиция уникальна в рамках стадии.

**Индексы:**
- `jobs_pkey` — PRIMARY KEY (id)
- `jobs_stage_id_position_key` — UNIQUE (stage_id, position)

> **TODO:** добавить индекс `idx_jobs_stage_id` на `stage_id` (для `pipeline_detail`).

**FK:** `stage_id` → `stages(id)` ON DELETE CASCADE.

**Referenced by:** `job_logs.job_id` → `jobs(id)` ON DELETE CASCADE.

---

## 5. job_logs

Append-only логи выполнения задач.

```
                       Table "public.job_logs"
    Column    |           Type           | Nullable |      Default
--------------+--------------------------+----------+-------------------
 id           | bigint                   | not null | nextval(...)
 job_id       | uuid                     | not null |
 sequence     | integer                  | not null |
 message      | text                     | not null |
 created_at   | timestamp with time zone | not null | now()
Indexes:
    "job_logs_pkey" PRIMARY KEY, btree (id)
    "job_logs_job_id_sequence_key" UNIQUE CONSTRAINT, btree (job_id, sequence)
Foreign-key constraints:
    "job_logs_job_id_fkey" FOREIGN KEY (job_id)
        REFERENCES jobs(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | BIGSERIAL | NOT NULL | `nextval(...)` | Автоинкрементный PK |
| `job_id` | UUID | NOT NULL | — | FK → `jobs.id`, CASCADE |
| `sequence` | INTEGER | NOT NULL | — | Порядковый номер лога в рамках job (вычисляется `next_log_sequence()`) |
| `message` | TEXT | NOT NULL | — | Текстовая строка лога |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время записи |

**UNIQUE constraint:** `(job_id, sequence)` — последовательность уникальна в рамках job.

**Индексы:**
- `job_logs_pkey` — PRIMARY KEY (id)
- `job_logs_job_id_sequence_key` — UNIQUE (job_id, sequence)

> **TODO:** добавить индекс `idx_job_logs_job_id` на `job_id` (для `list_logs`).

**FK:** `job_id` → `jobs(id)` ON DELETE CASCADE.

---

## 6. Status state machine

Все статусные таблицы (`pipelines`, `stages`, `jobs`) используют один набор значений:

```
 queued ──→ running ──→ success (terminal)
   │           │
   │           ├──→ failed   (terminal)
   │           │
   └──→ canceled (terminal)
                │
   running ────┘
```

**Transition rules** (реализовано в `domain.rs`, `JobStatus::transition_to()`):

| From | To | Результат |
|---|---|---|
| `queued` | `running` | ✅ Ok |
| `queued` | `canceled` | ✅ Ok |
| `queued` | `success` | ❌ InvalidTransition |
| `queued` | `failed` | ❌ InvalidTransition |
| `running` | `success` | ✅ Ok |
| `running` | `failed` | ✅ Ok |
| `running` | `canceled` | ✅ Ok |
| `success` | * | ❌ TerminalStatus |
| `failed` | * | ❌ TerminalStatus |
| `canceled` | * | ❌ TerminalStatus |

**Агрегация статусов** (job → stage → pipeline):

| Условие | Результат |
|---|---|
| Любой потомок `failed` | `failed` |
| Все потомки `success` | `success` |
| Любой потомок `running` | `running` |
| Любой потомок `canceled` | `canceled` |
| Иначе | `queued` |

---

## 7. Template pipeline

При триггере пайплайна (`POST /projects/{id}/pipelines`) создаётся 3 стадии с одной задачей в каждой:

| Position | Stage | Job | Image | Command |
|---|---|---|---|---|
| 0 | `build` | `checkout` | `alpine/git:latest` | `git fetch --all` |
| 1 | `test` | `unit-tests` | `rust:1.86` | `cargo test` |
| 2 | `deploy` | `deploy` | `alpine:3.21` | `echo deploy` |

Все задачи создаются в статусе `queued`. В будущем конфигурация будет загружаться из YAML-файла репозитория (Phase 5+).

---

## 8. Планируемые таблицы (Roadmap)

| Фаза | Таблицы | Назначение |
|---|---|---|
| Phase 1 (Auth) | `users`, `sessions` | Аутентификация, JWT |
| Phase 5 (Runner) | `runners`, `runner_registrations` | Реальные runner-агенты |
| Phase 6 (Webhooks) | `webhooks`, `webhook_deliveries` | Webhook-уведомления |
| Phase 7 (Secrets) | `secrets` | Шифрованные секреты проектов |
| Phase 8 (Artifacts) | `artifacts` | Хранилище артефактов сборки |
| Phase 9 (Admin) | `audit_log`, `system_settings` | Аудит, системные настройки |

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/API.md` — REST API спецификация.
- `backend/src/store.rs` — исходный код схемы БД.
- `backend/src/domain.rs` — доменные правила переходов статусов.
- `docs/ROADMAP.md` — план разработки.
