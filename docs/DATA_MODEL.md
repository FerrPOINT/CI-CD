# Дата-модель Forge CI/CD

## 0. Фактическая схема реализованных таблиц

Фактическая схема задаётся committed SQLx migrations в `backend/migrations/*.sql` и применяется backend при старте через `sqlx::migrate!("./migrations")`; тот же набор использует `cicd-migrate`. `backend/src/store.rs` остаётся историческим baseline-источником для `0001_bootstrap_v1.sql`, но новые изменения схемы должны идти только отдельными immutable migration files.

### ER-диаграмма (логическая)

```
projects (1) ──── (N) pipelines (1) ──── (N) stages (1) ──── (N) jobs (1) ──── (N) execution_attempts (1) ──── (N) job_logs
   │                    │                    │                    │
   │ UUID PK            │ UUID PK            │ UUID PK            │ UUID PK
   │ name UNIQUE        │ project_id FK      │ pipeline_id FK     │ stage_id FK
   │                    │ status CHECK       │ position UNIQUE    │ status CHECK
   │                    │                    │ status CHECK       │
   │                    │                    │                    │ BIGSERIAL PK
   └── CASCADE          └── CASCADE          └── CASCADE          └── CASCADE             └── CASCADE
   │                    │
   └── (N) pipeline_triggers ────────────────┘
        UUID PK, UNIQUE(project_id, source, idempotency_key)

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
| visibility | TEXT CHECK | нет | `private` | `private` или `public`; public разрешает Smart HTTP fetch/clone без token, push не разрешает |
| created_at | TIMESTAMPTZ | нет | `now()` | Время создания |


## 0.1a releases

Метаданные релиза поверх существующего Git tag. Удаление release не удаляет Git tag.

| Колонка | Тип | Nullable | Описание |
|---|---|---|---|
| id | UUID PK | нет | Идентификатор релиза |
| repository_name + tag_name | TEXT | нет | Уникальная пара репозитория и Git tag |
| name / description | TEXT | нет | Публичное название и release notes |
| prerelease | BOOLEAN | нет | Маркер предрелиза |
| created_by / created_at | TEXT / TIMESTAMPTZ | да / нет | Аудит автора и времени |

## 0.1b test_reports

Нормализованные агрегаты JUnit XML: один или больше suite на job. Сырые XML-артефакты остаются в artifact storage; таблица содержит только безопасные сводные счётчики.

| Колонка | Тип | Описание |
|---|---|---|
| job_id | UUID FK | Job-владелец, `ON DELETE CASCADE` |
| suite_name | TEXT | Имя `<testsuite>` |
| tests_total / tests_passed / tests_failed / tests_skipped | INTEGER | Итоги suite |
| duration_ms | INTEGER NULL | Время из JUnit `time` (секунды → миллисекунды) |


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
    TABLE "pipeline_triggers" CONSTRAINT pipeline_triggers_project_id_fkey
        FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ, генерируется `Uuid::new_v4()` |
| `name` | TEXT | NOT NULL | — | Уникальное имя проекта |
| `repository_url` | TEXT | NOT NULL | — | URL Git-репозитория |
| `default_branch` | TEXT | NOT NULL | `'main'` | Ветка по умолчанию |
| `protected_branches` | TEXT[] | NOT NULL | `'{}'` | Ветки, для которых PR merge gate требует successful pipeline на head |
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
| `commit_sha` | TEXT | NULL | — | Best-effort resolved commit SHA для merge gate и CI env |
| `status` | TEXT | NOT NULL | — | Статус: `queued` / `running` / `success` / `failed` / `canceled` |
| `variables` | JSONB | NOT NULL | `{}` | Значения ручного запуска; runner проецирует только в `CICD_VAR_<UPPER_SNAKE_KEY>` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время создания |
| `started_at` | TIMESTAMPTZ | NULL | — | Время начала выполнения (при `running`) |
| `finished_at` | TIMESTAMPTZ | NULL | — | Время завершения (терминальный статус) |

**CHECK constraint:** `status IN ('queued','running','success','failed','canceled')`.

**Индексы:**
- `pipelines_pkey` — PRIMARY KEY (id)
- `idx_pipelines_project_id` — lookup по project для `list_pipelines`.

**FK:** `project_id` → `projects(id)` ON DELETE CASCADE.

**Referenced by:** `stages.pipeline_id` → `pipelines(id)` ON DELETE CASCADE; `pipeline_triggers.pipeline_id` → `pipelines(id)` ON DELETE CASCADE.

---

## 2.1 pipeline_triggers

Таблица идемпотентности запусков pipeline. Она не является общим хранилищем всех HTTP idempotency responses; текущий MVP закрывает manual/API trigger и internal Git push replay.

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ |
| `project_id` | UUID | NOT NULL | — | FK → `projects.id`, CASCADE |
| `source` | TEXT | NOT NULL | — | Источник ключа: `api`, `git-push` |
| `idempotency_key` | TEXT | NOT NULL | — | UUID из `Idempotency-Key` для API либо stable hash для Git push event |
| `request_fingerprint` | TEXT | NOT NULL | — | SHA-256 нормализованных `git_ref` и `variables` |
| `pipeline_id` | UUID | NOT NULL | — | FK → созданный `pipelines.id`, CASCADE |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время первой обработки ключа |

**UNIQUE constraint:** `(project_id, source, idempotency_key)` — один observable pipeline на один retry key.

**Индексы:**
- `pipeline_triggers_pkey` — PRIMARY KEY (id)
- `pipeline_triggers_project_id_source_idempotency_key_key` — UNIQUE `(project_id, source, idempotency_key)`
- `idx_pipeline_triggers_pipeline` — lookup trigger record по pipeline.
- `idx_pipeline_triggers_project_created` — аудит/диагностика replay по project.

**Поведение:** повтор с тем же key и fingerprint возвращает исходный pipeline; тот же key с другим fingerprint отклоняется `409`.

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
- `idx_stages_pipeline_id` — lookup stages внутри pipeline для `pipeline_detail`.

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
 timeout_seconds | integer               |          |
 allow_failure | boolean                | not null | false
 manual       | boolean                  | not null | false
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
    TABLE "execution_attempts" CONSTRAINT execution_attempts_job_id_fkey
        FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
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
| `timeout_seconds` | INTEGER | NULL | — | Optional timeout из `.forge-ci.yml` |
| `allow_failure` | BOOLEAN | NOT NULL | `false` | Failed job не валит stage/pipeline, если true |
| `manual` | BOOLEAN | NOT NULL | `false` | Manual job ожидает `POST /jobs/{job_id}/start` |
| `started_at` | TIMESTAMPTZ | NULL | — | Время начала (при переходе в `running`) |
| `finished_at` | TIMESTAMPTZ | NULL | — | Время завершения (терминальный статус) |

**CHECK constraint:** `status IN ('queued','running','success','failed','canceled')`.

**UNIQUE constraint:** `(stage_id, position)` — позиция уникальна в рамках стадии.

**Индексы:**
- `jobs_pkey` — PRIMARY KEY (id)
- `jobs_stage_id_position_key` — UNIQUE (stage_id, position)
- `idx_jobs_stage_id` — lookup jobs внутри stage для `pipeline_detail`.

**FK:** `stage_id` → `stages(id)` ON DELETE CASCADE.

**Referenced by:** `execution_attempts.job_id` и совместимый `job_logs.job_id` → `jobs(id)` ON DELETE CASCADE.

---

## 5. execution_attempts

Неизменяемая история запусков логической job. Каждая job получает `attempt_no = 1` при создании pipeline; retry job/pipeline добавляет новую attempt и не удаляет логи предыдущих attempts.

```
                       Table "public.execution_attempts"
    Column     |           Type           | Nullable |      Default
---------------+--------------------------+----------+-------------------
 id            | uuid                     | not null |
 job_id        | uuid                     | not null |
 attempt_no    | integer                  | not null |
 status        | text                     | not null |
 trigger       | text                     | not null | 'initial'
 exit_code     | integer                  |          |
 error_tail    | text                     |          |
 created_at    | timestamp with time zone | not null | now()
 started_at    | timestamp with time zone |          |
 finished_at   | timestamp with time zone |          |
Indexes:
    "execution_attempts_pkey" PRIMARY KEY, btree (id)
    "execution_attempts_job_id_attempt_no_key" UNIQUE CONSTRAINT, btree (job_id, attempt_no)
    "idx_execution_attempts_job" btree (job_id, attempt_no DESC)
    "idx_execution_attempts_active_job" UNIQUE, btree (job_id) WHERE status IN ('queued','running')
Foreign-key constraints:
    "execution_attempts_job_id_fkey" FOREIGN KEY (job_id)
        REFERENCES jobs(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | Первичный ключ |
| `job_id` | UUID | NOT NULL | — | FK → `jobs.id`, CASCADE |
| `attempt_no` | INTEGER | NOT NULL | — | Номер попытки внутри job, начиная с 1 |
| `status` | TEXT | NOT NULL | — | `queued` / `running` / `success` / `failed` / `canceled` |
| `trigger` | TEXT | NOT NULL | `'initial'` | Источник попытки: `initial`, `job_retry`, `pipeline_retry`, `runner`, `manual_status`, `compat` |
| `exit_code` | INTEGER | NULL | — | Код завершения процесса embedded runner, если известен |
| `error_tail` | TEXT | NULL | — | Краткий terminal diagnostic для failed/timeout |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время создания attempt |
| `started_at` | TIMESTAMPTZ | NULL | — | Время перехода attempt в `running` |
| `finished_at` | TIMESTAMPTZ | NULL | — | Время terminal result |

## 6. job_logs

Append-only логи выполнения задач. Логи принадлежат `attempt_id`, а `job_id` сохранён как совместимый lookup. `GET /jobs/{job_id}/logs` возвращает текущую или последнюю attempt; bounded page/search читается через `/jobs/{job_id}/logs/page` и `/jobs/{job_id}/attempts/{attempt_id}/logs/page`; полный retry history читается через `/jobs/{job_id}/attempts/{attempt_id}/logs`.

```
                       Table "public.job_logs"
    Column    |           Type           | Nullable |      Default
--------------+--------------------------+----------+-------------------
 id           | bigint                   | not null | nextval(...)
 job_id       | uuid                     | not null |
 attempt_id   | uuid                     | not null |
 sequence     | integer                  | not null |
 message      | text                     | not null |
 created_at   | timestamp with time zone | not null | now()
Indexes:
    "job_logs_pkey" PRIMARY KEY, btree (id)
    "idx_job_logs_attempt_sequence" UNIQUE, btree (attempt_id, sequence)
    "idx_job_logs_job_id" btree (job_id)
Foreign-key constraints:
    "job_logs_job_id_fkey" FOREIGN KEY (job_id)
        REFERENCES jobs(id) ON DELETE CASCADE
    "job_logs_attempt_id_fkey" FOREIGN KEY (attempt_id)
        REFERENCES execution_attempts(id) ON DELETE CASCADE
```

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | BIGSERIAL | NOT NULL | `nextval(...)` | Автоинкрементный PK |
| `job_id` | UUID | NOT NULL | — | FK → `jobs.id`, CASCADE |
| `attempt_id` | UUID | NOT NULL | — | FK → `execution_attempts.id`, CASCADE |
| `sequence` | INTEGER | NOT NULL | — | Порядковый номер лога в рамках attempt |
| `message` | TEXT | NOT NULL | — | Текстовая строка лога |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время записи |

**UNIQUE constraint:** `(attempt_id, sequence)` — последовательность уникальна в рамках attempt. Application append сериализуется advisory lock-ом по `attempt_id`.

**Индексы:**
- `job_logs_pkey` — PRIMARY KEY (id)
- `idx_job_logs_attempt_sequence` — UNIQUE (attempt_id, sequence)
- `idx_job_logs_job_id` — lookup по job для совместимых API.

**FK:** `job_id` → `jobs(id)` ON DELETE CASCADE; `attempt_id` → `execution_attempts(id)` ON DELETE CASCADE.

---

## 7. Status state machine

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

## 8. Template pipeline

При триггере пайплайна (`POST /projects/{id}/pipelines`) backend сначала пытается прочитать `.forge-ci.yml` из локального bare repository на указанном ref. Если файл недоступен или проект указывает внешний URL, используется fallback из 3 стадий с одной задачей в каждой:

| Position | Stage | Job | Image | Command |
|---|---|---|---|---|
| 0 | `build` | `checkout` | `alpine/git:latest` | `git fetch --all` |
| 1 | `test` | `unit-tests` | `rust:1.86` | `cargo test` |
| 2 | `deploy` | `deploy` | `alpine:3.21` | `echo deploy` |

Все задачи создаются в статусе `queued`; `timeout_seconds`, `allow_failure` и `manual` берутся из YAML, если заданы.

---

## 9. Platform tables (MVP)

Platform tables создаются и расширяются через `backend/migrations/*.sql`; `0001_bootstrap_v1.sql` содержит исторический baseline, последующие файлы добавляют auth, outbox, execution gaps и project memberships.

### 9.1 runners

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `name` | TEXT | NOT NULL | — | UNIQUE, имя runner |
| `tags` | TEXT[] | NOT NULL | `'{}'` | Теги для фильтрации |
| `status` | TEXT | NOT NULL | `'offline'` | CHECK: `online`, `offline`, `paused` |
| `last_seen_at` | TIMESTAMPTZ | NULL | — | Последний heartbeat |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время регистрации |

### 9.2 project_secrets

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `key` | TEXT | NOT NULL | — | Имя секрета (UNIQUE per project) |
| `encrypted_value` | TEXT | NOT NULL | — | AES-256-GCM ciphertext (`v1:nonce:payload`) |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |
| `updated_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> `UNIQUE(project_id, key)`. Ключ шифрования — `CICD_SECRETS_KEY` (base64 32 bytes). Значения не возвращаются через API.

### 9.3 artifacts

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `job_id` | UUID | NOT NULL | — | FK → `jobs(id)` CASCADE |
| `attempt_id` | UUID | NULL | — | FK → `execution_attempts(id)` SET NULL; новые uploads привязаны к active/latest attempt |
| `name` | TEXT | NOT NULL | — | Имя файла |
| `storage_path` | TEXT | NOT NULL | — | Canonical path в локальной ФС внутри `CICD_ARTIFACTS_DIR`; download отвергает пути вне root |
| `content_type` | TEXT | NOT NULL | `'application/octet-stream'` | MIME |
| `sha256` | TEXT | NULL | — | SHA-256 hex для новых uploads; NULL допустим только для legacy metadata |
| `size_bytes` | BIGINT | NOT NULL | — | Размер в байтах |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> Хранилище: `CICD_ARTIFACTS_DIR` (default `/var/lib/forge/artifacts`). Лимит — 50 MiB. Runtime не считает `storage_path` доверенным: перед чтением путь и root canonicalize-ятся, нарушение containment возвращает `404`, а checksum drift для записей с `sha256` — `409`.

### 9.4 environments

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `name` | TEXT | NOT NULL | — | `UNIQUE(project_id, name)` |
| `url` | TEXT | NULL | — | URL окружения |
| `status` | TEXT | NOT NULL | `'available'` | CHECK: `available`, `stopped`, `degraded` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 9.5 deployments

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `environment_id` | UUID | NOT NULL | — | FK → `environments(id)` CASCADE |
| `pipeline_id` | UUID | NULL | — | FK → `pipelines(id)` SET NULL |
| `git_ref` | TEXT | NOT NULL | — | Деплойимый Git-реф |
| `status` | TEXT | NOT NULL | `'pending'` | CHECK: `pending`, `running`, `success`, `failed` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 9.6 schedules

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `cron` | TEXT | NOT NULL | — | 5-полей cron-выражение |
| `git_ref` | TEXT | NOT NULL | — | Git-реф для запуска |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | Включено/выключено |
| `last_fired_at` | TIMESTAMPTZ | NULL | — | Последний MVP fire claim |
| `last_fire_error` | TEXT | NULL | — | Последняя ошибка scheduler |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

Текущий scheduler проверяет enabled rows примерно раз в минуту. `cron` валидируется как пять полей и хранится, но full cron semantics остаётся target.

### 9.7 webhooks

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `url` | TEXT | NOT NULL | — | URL приёмника |
| `events` | TEXT[] | NOT NULL | `'{}'` | Подписанные события |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | — |
| `secret` | TEXT | NULL | — | Optional HMAC signing secret для outgoing delivery |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

Outgoing delivery создаёт `outbox_messages` на terminal pipeline events. Current MVP использует `outbox_messages` как delivery row, сохраняет попытки в `outbox_delivery_attempts` и позволяет requeue failed-доставки новой generation. Production lease/reconciliation и full dead-letter policy остаются target.

### 9.8 notification_configs

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `channel` | TEXT | NOT NULL | — | Тип канала; current delivery поддерживает `in_app` и `sse`, email/Slack остаются target adapters |
| `target` | TEXT | NOT NULL | — | Адрес назначения |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | — |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

### 9.9 audit_log

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | BIGSERIAL | NOT NULL | — | PK |
| `action` | TEXT | NOT NULL | — | Действие (`runner.registered`, `secret.upserted`, …) |
| `resource_type` | TEXT | NOT NULL | — | Тип ресурса |
| `resource_id` | UUID | NULL | — | ID ресурса |
| `actor` | TEXT | NULL | — | Инициатор (NULL = system) |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> Append-only. GET `/audit-log` возвращает последние 200 событий.

### 9.10 users

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `username` | TEXT | NOT NULL | — | UNIQUE |
| `role` | TEXT | NOT NULL | — | CHECK: `admin`, `maintainer`, `developer`, `viewer` |
| `enabled` | BOOLEAN | NOT NULL | `TRUE` | — |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |

> Пароли хранятся отдельно в `user_credentials`; роли применяются middleware только при `CICD_AUTH_SECRET`.

### 9.11 api_tokens

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID | NOT NULL | — | PK |
| `name` | TEXT | NOT NULL | — | Имя токена |
| `token_hash` | TEXT | NOT NULL | — | UNIQUE, SHA-256 хэш |
| `token_hint` | TEXT | NOT NULL | — | Подсказка (`cicd_xxxx...yyyy`) |
| `user_id` | UUID | NULL | — | FK → `users(id)` SET NULL |
| `project_id` | UUID | NULL | — | FK → `projects(id)` CASCADE; обязателен для новых PAT при `CICD_AUTH_SECRET` |
| `scopes` | TEXT[] | NOT NULL | `ARRAY['api:read','api:write','git:read','git:write']` | Разрешённые области PAT: REST read/write и Git read/write |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | — |
| `last_used_at` | TIMESTAMPTZ | NULL | — | — |
| `expires_at` | TIMESTAMPTZ | NULL | — | Optional PAT expiry (`NULL` = без срока) |
| `revoked_at` | TIMESTAMPTZ | NULL | — | Soft revoke; активные списки фильтруют `revoked_at IS NULL` |

> Полное значение возвращается только при создании. PAT проверяется как Bearer token при включённом `CICD_AUTH_SECRET`; `project_id` ограничивает project-owned API и linked Git repository, `scopes` ограничивают REST/Git операции. Старые записи с `project_id = NULL` остаются legacy global до отзыва.

### 9.12 project_memberships

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `project_id` | UUID | NOT NULL | — | FK → `projects(id)` CASCADE |
| `user_id` | UUID | NOT NULL | — | FK → `users(id)` CASCADE |
| `role` | TEXT | NOT NULL | — | CHECK: `maintainer`, `developer`, `viewer` |
| `created_at` | TIMESTAMPTZ | NOT NULL | `now()` | Время назначения |
| `updated_at` | TIMESTAMPTZ | NOT NULL | `now()` | Последнее изменение роли |

> PK: `(project_id, user_id)`. Existing user/project pairs backfill-ятся миграцией `0008_project_memberships.sql`; новые проекты получают creator membership при включённой auth.

### 9.13 user_credentials

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `user_id` | UUID PK | нет | — | FK → `users(id)` CASCADE |
| `password_hash` | TEXT | нет | — | `argon2id` hash |
| `updated_at` | TIMESTAMPTZ | нет | `now()` | Последнее изменение credential |

### 9.14 sessions

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID PK | нет | — | Session ID |
| `user_id` | UUID | нет | — | FK → `users(id)` CASCADE |
| `refresh_token_hash` | TEXT UNIQUE | нет | — | Hash refresh token |
| `created_at` | TIMESTAMPTZ | нет | `now()` | Создана |
| `expires_at` | TIMESTAMPTZ | нет | — | Истекает |
| `revoked_at` | TIMESTAMPTZ | да | — | Отозвана |

`/api/v1/auth/refresh` rotate-ит refresh session: старый hash получает `revoked_at`, новый refresh token хранится как `hash_token(raw)`. `/api/v1/auth/logout` идемпотентно выставляет `revoked_at` для переданного refresh token. Access JWT содержит `sessions.id`; protected API проверяет, что session активна, не истекла и принадлежит enabled user, поэтому rotate/logout инвалидирует связанный access JWT сразу.

### 9.15 domain_events

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID PK | нет | — | Event ID |
| `occurred_at` | TIMESTAMPTZ | нет | `now()` | Время события |
| `event_type` | TEXT | нет | — | Например `pipeline.success` |
| `aggregate_type` | TEXT | нет | — | Тип aggregate |
| `aggregate_id` | UUID | нет | — | ID aggregate |
| `payload` | JSONB | нет | `'{}'` | Immutable payload |
| `correlation_id` | UUID | да | — | Correlation |
| `causation_id` | UUID | да | — | Causation |

### 9.16 outbox_messages

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | UUID PK | нет | — | Message ID |
| `event_id` | UUID | нет | — | FK → `domain_events(id)` CASCADE |
| `project_id` | UUID | да | — | Денормализованный project scope для delivery API/RBAC; backfill берётся из payload/domain event/pipeline |
| `subscription_id` | TEXT | нет | — | Webhook/notification/SSE subscription key |
| `channel` | TEXT CHECK | нет | — | `webhook`, `notification`, `sse` |
| `destination` | TEXT | нет | — | URL/target |
| `payload` | JSONB | нет | `'{}'` | Delivery payload |
| `generation` | INTEGER | нет | `0` | Replay generation; первичная доставка = `0` |
| `replay_of_id` | UUID | да | — | FK → `outbox_messages(id)` `ON DELETE SET NULL`, ссылка на исходную failed delivery |
| `attempts` | INTEGER | нет | `0` | Attempt count |
| `next_attempt_at` | TIMESTAMPTZ | нет | `now()` | Следующая попытка |
| `delivered_at` | TIMESTAMPTZ | да | — | Успешная доставка |
| `failed_at` | TIMESTAMPTZ | да | — | Terminal failed/dead state после исчерпания попыток или permanent error |
| `last_error` | TEXT | да | — | Последняя ошибка |
| `created_at` | TIMESTAMPTZ | нет | `now()` | Создано |

Для notification MVP `outbox_messages.channel = 'notification'`, `destination = 'project:<project_id>'`, а payload содержит `event`, `project_id`, `pipeline_id`, `status`, `channel`, `target` и `message`. Worker помечает `in_app`/`sse` сообщения delivered локально; внешние adapters не запускаются. `GET /projects/{project_id}/outbox-deliveries` читает только строки с `project_id = $1`; migration `0012_outbox_delivery_history.sql` backfill-ит старые rows.

### 9.17 outbox_delivery_attempts

История попыток доставки для current outbox MVP. Таблица хранит безопасный итог попытки; secrets, произвольные request headers и полный response body не сохраняются.

| Колонка | Тип | Nullable | Default | Описание |
|---|---|---|---|---|
| `id` | BIGSERIAL PK | нет | — | Attempt row ID |
| `message_id` | UUID | нет | — | FK → `outbox_messages(id)` CASCADE |
| `attempt_number` | INTEGER | нет | — | Номер попытки внутри delivery |
| `started_at` | TIMESTAMPTZ | нет | — | Время начала попытки |
| `finished_at` | TIMESTAMPTZ | нет | — | Время фиксации результата |
| `outcome` | TEXT CHECK | нет | — | `delivered`, `retry_scheduled` или `failed` |
| `http_status` | INTEGER | да | — | HTTP status, если была HTTP-попытка |
| `error_message` | TEXT | да | — | Safe error class/message без secret/URL payload |
| `duration_ms` | INTEGER | нет | `0` | Длительность попытки |
| `created_at` | TIMESTAMPTZ | нет | `now()` | Время записи |

**UNIQUE constraint:** `(message_id, attempt_number)`.

### 9.18 Индексы

```
idx_runners_status          ON runners(status)
idx_project_secrets_project ON project_secrets(project_id)
idx_artifacts_job            ON artifacts(job_id)
idx_deployments_environment  ON deployments(environment_id)
idx_schedules_project        ON schedules(project_id)
idx_webhooks_project         ON webhooks(project_id)
idx_audit_log_created        ON audit_log(created_at DESC)
idx_pipelines_project_id     ON pipelines(project_id)
idx_stages_pipeline_id       ON stages(pipeline_id)
idx_jobs_stage_id            ON jobs(stage_id)
idx_execution_attempts_job   ON execution_attempts(job_id, attempt_no DESC)
idx_execution_attempts_active_job ON execution_attempts(job_id) WHERE status IN ('queued','running')
idx_job_logs_attempt_sequence ON job_logs(attempt_id, sequence)
idx_job_logs_job_id          ON job_logs(job_id)
idx_artifacts_attempt        ON artifacts(attempt_id)
idx_sessions_user            ON sessions(user_id)
idx_sessions_expires         ON sessions(expires_at)
idx_api_tokens_active_owner_project ON api_tokens(user_id, project_id) WHERE revoked_at IS NULL
idx_api_tokens_active_project ON api_tokens(project_id) WHERE revoked_at IS NULL
idx_project_memberships_user ON project_memberships(user_id, project_id)
idx_domain_events_aggregate  ON domain_events(aggregate_type, aggregate_id, occurred_at DESC)
idx_domain_events_type       ON domain_events(event_type, occurred_at DESC)
idx_outbox_pending           ON outbox_messages(next_attempt_at) WHERE delivered_at IS NULL
idx_outbox_notification_project_created ON outbox_messages(destination, created_at DESC, id DESC) WHERE channel = 'notification'
idx_outbox_project_created   ON outbox_messages(project_id, created_at DESC, id DESC) WHERE project_id IS NOT NULL
idx_outbox_project_dead      ON outbox_messages(project_id, created_at DESC, id DESC) WHERE project_id IS NOT NULL AND delivered_at IS NULL AND failed_at IS NOT NULL
idx_outbox_delivery_attempts_message ON outbox_delivery_attempts(message_id, attempt_number DESC)
```

## 10. Планируемые таблицы (Roadmap)

| Фаза | Таблицы | Назначение |
|---|---|---|
| Runner protocol | `job_leases` | External dispatch, fencing, retries |
| Production outbox | `outbox_deliveries` / lease state | Full dispatcher snapshots, lease/fencing, crash recovery, response preview allowlist и operator dead-letter policy поверх current bounded history |
| External notifications | delivery-specific tables | Email/Slack sender state, templates, preferences |

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/API.md` — REST API спецификация.
- `backend/migrations/*.sql` — исходный код схемы БД.
- `backend/domain/src/lib.rs` — доменные правила переходов статусов.
- `docs/ROADMAP.md` — план разработки.
