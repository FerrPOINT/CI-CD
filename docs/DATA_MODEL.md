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

repositories (1) ──── (N) pull_requests (по repository_name)
   │
   │ UUID PK, name UNIQUE
```

Уровень изоляции: каждый родитель CASCADE-deletes удаляет всех потомков.

---

## 0.1 repositories

Реестр bare Git-репозиториев (Git-хостинг). Полное описание — `docs/GIT_HOSTING.md`.

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| id | UUID PK | нет | `gen_random_uuid()` | Идентификатор |
| name | TEXT UNIQUE | нет | — | Имя репозитория (совпадает с именем bare-репо в `CICD_GIT_ROOT`) |
| created_at | TIMESTAMPTZ | нет | `now()` | Время создания |

## 0.2 pull_requests

Pull requests между ветками bare-репозитория (создание, merge через `git merge-tree`, close/reopen).

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| id | UUID PK | нет | `gen_random_uuid()` | Идентификатор |
| repository_name | TEXT | нет | — | Репозиторий (FK по имени на `repositories.name` логически) |
| number | INTEGER | нет | — | Порядковый номер PR в рамках репозитория |
| title | TEXT | нет | — | Заголовок |
| description | TEXT | нет | `''` | Описание |
| source_branch | TEXT | нет | — | Ветка-источник |
| target_branch | TEXT | нет | — | Ветка-приёмник |
| status | TEXT CHECK | нет | `'open'` | `open` / `merged` / `closed` |
| created_by | TEXT | нет | `''` | Автор |
| created_at / updated_at | TIMESTAMPTZ | нет | `now()` | Timestamps |
| merged_at | TIMESTAMPTZ | да | NULL | Время merge |
| merge_commit_sha | TEXT | да | NULL | SHA merge-коммита |

`UNIQUE(repository_name, number)`.

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

## 8. Platform tables (MVP)

Все таблицы создаются через `store::migrate()` (`CREATE TABLE IF NOT EXISTS`).

### 8.1 runners

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `name` | TEXT | NOT NULL | — | UNIQUE, имя runner |
| `tags` | TEXT[] | NOT NULL | `'{}'` | Теги для фильтрации |
| `status` | TEXT | NOT NULL | `'offline'` | CHECK: `online`, `offline`, `paused` |
| `last_seen_at` | TIMESTAMPTZ | NULL | — | Последний heartbeat |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время регистрации |

### 8.2 project_secrets

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `key` | TEXT | NOT NULL | — | Имя секрета (UNIQUE per project) |
| `encrypted_value` | TEXT | NOT NULL | — | AES-256-GCM ciphertext (`v1:nonce:payload`) |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |
| `updated_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> `UNIQUE(project_id, key)`. Ключ шифрования — `CICD_SECRETS_KEY` (base64 32 bytes). Значения не возвращаются через API.

### 8.3 artifacts

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `job_id` | UUID | NOT NULL | — | FK → `jobs(id)` CASCADE |
| `name` | TEXT | NOT NULL | — | Имя файла |
| `storage_path` | TEXT | NOT NULL | — | Путь в локальной ФС |
| `content_type` | TEXT | NOT NULL | `'application/octet-stream'` | MIME |
| `size_bytes` | BIGINT | NOT NULL | — | Размер в байтах |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> Хранилище: `CICD_ARTIFACTS_DIR` (default `/var/lib/forge/artifacts`). Лимит — 50 MiB.

### 8.4 environments

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `name` | TEXT | NOT NULL | — | `UNIQUE(project_id, name)` |
| `url` | TEXT | NULL | — | URL окружения |
| `status` | TEXT | NOT NULL | `'available'` | CHECK: `available`, `stopped`, `degraded` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 8.5 deployments

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `environment_id` | UUID | NOT NULL | — | FK → `environments(id)` CASCADE |
| `pipeline_id` | UUID | NULL | — | FK → `pipelines(id)` SET NULL |
| `git_ref` | TEXT | NOT NULL | — | Деплойимый Git-реф |
| `status` | TEXT | NOT NULL | `'pending'` | CHECK: `pending`, `running`, `success`, `failed` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 8.6 schedules

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `cron` | TEXT | NOT NULL | — | 5-полей cron-выражение |
| `git_ref` | TEXT | NOT NULL | — | Git-реф для запуска |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | Включено/выключено |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 8.7 webhooks

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `url` | TEXT | NOT NULL | — | URL приёмника |
| `events` | TEXT[] | NOT NULL | `'{}'` | Подписанные события |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | — |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 8.8 notification_configs

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `channel` | TEXT | NOT NULL | — | Тип канала (slack, email, …) |
| `target` | TEXT | NOT NULL | — | Адрес назначения |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | — |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 8.9 audit_log

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | BIGSERIAL | NOT NULL | — | PK |
| `action` | TEXT | NOT NULL | — | Действие (`runner.registered`, `secret.upserted`, …) |
| `resource_type` | TEXT | NOT NULL | — | Тип ресурса |
| `resource_id` | UUID | NULL | — | ID ресурса |
| `actor` | TEXT | NULL | — | Инициатор (NULL = system) |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> Append-only. GET `/audit-log` возвращает последние 200 событий.

### 8.10 users

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `username` | TEXT | NOT NULL | — | UNIQUE |
| `role` | TEXT | NOT NULL | — | CHECK: `admin`, `maintainer`, `developer`, `viewer` |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | — |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> Пароли не хранятся — модель для будущего RBAC.

### 8.11 api_tokens

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `name` | TEXT | NOT NULL | — | Имя токена |
| `token_hash` | TEXT | NOT NULL | — | UNIQUE, SHA-256 хэш |
| `token_hint` | TEXT | NOT NULL | — | Подсказка (`cicd_xxxx...yyyy`) |
| `user_id` | UUID | NULL | — | FK → `users(id)` SET NULL |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |
| `last_used_at` | TIMESTAMPTZ | NULL | — | — |

> Полное значение возвращается только при создании. Проверка токенов при запросах — TODO.

### 8.12 Индексы

```
idx_runners_status          ON runners(status)
idx_project_secrets_project ON project_secrets(project_id)
idx_artifacts_job            ON artifacts(job_id)
idx_deployments_environment  ON deployments(environment_id)
idx_schedules_project        ON schedules(project_id)
idx_webhooks_project         ON webhooks(project_id)
idx_audit_log_created        ON audit_log(created_at DESC)
idx_pipelines_project_id     ON pipelines(project_id)
```

## 9. Планируемые таблицы (Roadmap)

| Фаза | Таблицы | Назначение |
|---|---|---|
| Phase 1 (Auth) | `sessions` | Аутентификация, JWT |
| Phase 6 (Webhooks) | `webhook_deliveries` | Доставка webhook-уведомлений |

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/API.md` — REST API спецификация.
- `backend/src/store.rs` — исходный код схемы БД.
- `backend/src/domain.rs` — доменные правила переходов статусов.
- `docs/ROADMAP.md` — план разработки.
