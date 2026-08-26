# API v1 Specification — Forge CI/CD

## Overview

REST API первой версии Forge CI/CD. Все endpoint возвращают JSON. Контрольная плоскость: создание проектов, запуск пайплайнов, переходы статусов задач, append-only логи.

> **Source of truth:** актуальная реализация в `backend/src/api.rs`. Документация ниже — для контекста, но при расхождении приоритет у исходного кода.

## Базовая информация

- Base URL: `http://{host}:22801/api/v1`
- Content-Type: `application/json`
- Auth: не реализована в текущей версии (Phase 1 — TODO).
- Версионирование: path-based `/api/v1`.
- Сериализация: `serde_json`, `snake_case` для enum-значений статусов.
- Ошибки: `{"error": "message"}` с соответствующим HTTP статусом.

## Коды ответов

| Код | Назначение |
|---|---|
| 200 OK | Успешный GET, успешный POST с результатом |
| 400 Bad Request | Невалидный ввод, невалидный transition |
| 404 Not Found | Ресурс не найден |
| 500 Internal Server Error | Ошибка БД |
| 503 Service Unavailable | БД недоступна |

---

## Реализованные эндпоинты

### Health

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/health` | Health-check сервиса |

#### GET /api/v1/health

Проверка работоспособности API. Не требует БД.

**Response 200:**
```json
{
  "status": "ok",
  "service": "cicd"
}
```

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/health
```

---

### Projects

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects` | Список проектов |
| POST | `/projects` | Создание проекта |
| GET | `/projects/{project_id}` | Получить проект |
| PATCH | `/projects/{project_id}` | Частичное обновление проекта |
| DELETE | `/projects/{project_id}` | Удалить проект (каскадно с пайплайнами) |

#### GET /api/v1/projects

Возвращает список всех проектов, отсортированных по `created_at DESC`.

**Response 200:**
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "my-service",
    "repository_url": "git@github.com:org/my-service.git",
    "default_branch": "main",
    "created_at": "2026-08-26T10:00:00Z"
  }
]
```

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/projects
```

#### POST /api/v1/projects

Создаёт новый проект. `id` генерируется сервером (`Uuid::new_v4()`).

**Request body:**
```json
{
  "name": "my-service",
  "repository_url": "git@github.com:org/my-service.git",
  "default_branch": "main"
}
```

| Поле | Тип | Required | Описание |
|---|---|---|---|
| `name` | string | yes | Уникальное имя проекта (non-empty) |
| `repository_url` | string | yes | URL Git-репозитория (non-empty) |
| `default_branch` | string | no | Ветка по умолчанию, default `"main"` |

**Response 200:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "my-service",
  "repository_url": "git@github.com:org/my-service.git",
  "default_branch": "main",
  "created_at": "2026-08-26T10:00:00Z"
}
```

**Errors:**
- `400` — `name` или `repository_url` пустые.
- `500` — duplicate name (unique constraint violation) или ошибка БД.

**curl:**
```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"my-service","repository_url":"git@github.com:org/my-service.git"}'
```

#### GET /api/v1/projects/{project_id}

Возвращает один проект по UUID.

**Response 200:** объект проекта (см. POST).

**Errors:** `404` — проект не найден.

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID
```

#### PATCH /api/v1/projects/{project_id}

Частичное обновление: достаточно передать только изменяемые поля, остальные сохраняются (`COALESCE`).

**Request body (все поля опциональны, минимум одно):**
```json
{
  "default_branch": "release"
}
```

| Поле | Тип | Required | Описание |
|---|---|---|---|
| `name` | string | no | Новое имя (non-empty при передаче) |
| `repository_url` | string | no | Новый URL репозитория |
| `default_branch` | string | no | Новая ветка по умолчанию |

**Response 200:** обновлённый объект проекта.

**Errors:**
- `400` — тело пустое (`{}`) или переданное поле пустое.
- `404` — проект не найден.
- `500` — duplicate name.

**curl:**
```bash
curl -sS -X PATCH http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID \
  -H 'content-type: application/json' \
  -d '{"default_branch":"release"}'
```

#### DELETE /api/v1/projects/{project_id}

Удаляет проект. Все связанные пайплайны, стадии, джобы и логи удаляются каскадно (`ON DELETE CASCADE`).

**Response 200:**
```json
{"deleted": "550e8400-e29b-41d4-a716-446655440000"}
```

**Errors:** `404` — проект не найден.

**curl:**
```bash
curl -sS -X DELETE http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID
```

---

### Pipelines

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/pipelines` | Список пайплайнов проекта |
| POST | `/projects/{project_id}/pipelines` | Запуск пайплайна |
| GET | `/pipelines/{pipeline_id}` | Детали пайплайна (stages + jobs) |

#### GET /api/v1/projects/{project_id}/pipelines

Возвращает последние 50 пайплайнов проекта, отсортированные по `created_at DESC`.

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `project_id` | UUID | ID проекта |

**Response 200:**
```json
[
  {
    "id": "a1b2c3d4-...",
    "project_id": "550e8400-...",
    "git_ref": "main",
    "status": "queued",
    "created_at": "2026-08-26T10:05:00Z",
    "started_at": null,
    "finished_at": null
  }
]
```

**Errors:**
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines
```

#### POST /api/v1/projects/{project_id}/pipelines

Запускает новый пайплайн для указанного Git-рефа. Создаёт 3 стадии (build/test/deploy) с одной задачей в каждой (template pipeline). Все задачи — в статусе `queued`.

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `project_id` | UUID | ID проекта |

**Request body:**
```json
{
  "git_ref": "main"
}
```

| Поле | Тип | Required | Описание |
|---|---|---|---|
| `git_ref` | string | no | Git-реф, default `"main"` |

**Response 200** — `PipelineDetail`:
```json
{
  "pipeline": {
    "id": "a1b2c3d4-...",
    "project_id": "550e8400-...",
    "git_ref": "main",
    "status": "queued",
    "created_at": "2026-08-26T10:05:00Z",
    "started_at": null,
    "finished_at": null
  },
  "stages": [
    {
      "id": "stage-uuid-1",
      "pipeline_id": "a1b2c3d4-...",
      "name": "build",
      "position": 0,
      "status": "queued",
      "jobs": [
        {
          "id": "job-uuid-1",
          "stage_id": "stage-uuid-1",
          "name": "checkout",
          "image": "alpine/git:latest",
          "command": "git fetch --all",
          "position": 0,
          "status": "queued",
          "started_at": null,
          "finished_at": null
        }
      ]
    },
    {
      "id": "stage-uuid-2",
      "pipeline_id": "a1b2c3d4-...",
      "name": "test",
      "position": 1,
      "status": "queued",
      "jobs": [
        {
          "id": "job-uuid-2",
          "stage_id": "stage-uuid-2",
          "name": "unit-tests",
          "image": "rust:1.86",
          "command": "cargo test",
          "position": 0,
          "status": "queued",
          "started_at": null,
          "finished_at": null
        }
      ]
    },
    {
      "id": "stage-uuid-3",
      "pipeline_id": "a1b2c3d4-...",
      "name": "deploy",
      "position": 2,
      "status": "queued",
      "jobs": [
        {
          "id": "job-uuid-3",
          "stage_id": "stage-uuid-3",
          "name": "deploy",
          "image": "alpine:3.21",
          "command": "echo deploy",
          "position": 0,
          "status": "queued",
          "started_at": null,
          "finished_at": null
        }
      ]
    }
  ]
}
```

**Errors:**
- `404` — проект не найден.
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines \
  -H 'content-type: application/json' \
  -d '{"git_ref":"main"}'
```

#### GET /api/v1/pipelines/{pipeline_id}

Возвращает детали пайплайна: метаданные + массив стадий с задачами. Стадии отсортированы по `position`, jobs внутри стадии — по `position`.

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `pipeline_id` | UUID | ID пайплайна |

**Response 200** — `PipelineDetail` (структура аналогична POST above).

Stage в ответе использует `#[serde(flatten)]`: поля stage (`id`, `pipeline_id`, `name`, `position`, `status`) и `jobs` находятся на одном уровне в каждом элементе массива `stages`.

**Errors:**
- `404` — пайплайн не найден.
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/pipelines/$PIPELINE_ID
```

---

### Jobs

| Метод | Путь | Назначение |
|---|---|---|
| POST | `/jobs/{job_id}/status` | Смена статуса задачи |
| GET | `/jobs/{job_id}/logs` | Список логов задачи |
| POST | `/jobs/{job_id}/logs` | Добавление строки лога |

#### POST /api/v1/jobs/{job_id}/status

Переводит задачу в новый статус. Валидирует переход через `JobStatus::transition_to()`. После обновления статуса задачи каскадно пересчитываются статусы stage и pipeline (см. `refresh_statuses`).

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `job_id` | UUID | ID задачи |

**Request body:**
```json
{
  "status": "running"
}
```

| Поле | Тип | Required | Описание |
|---|---|---|---|
| `status` | JobStatus | yes | Новый статус: `queued` / `running` / `success` / `failed` / `canceled` |

**Response 200** — обновлённый `Job`:
```json
{
  "id": "job-uuid-1",
  "stage_id": "stage-uuid-1",
  "name": "checkout",
  "image": "alpine/git:latest",
  "command": "git fetch --all",
  "position": 0,
  "status": "running",
  "started_at": "2026-08-26T10:06:00Z",
  "finished_at": null
}
```

**Side effects:**
- При `status = "running"`: `started_at = now()` (если ещё не проставлено для stage/pipeline).
- При терминальном статусе (`success` / `failed` / `canceled`): `finished_at = now()`.
- Каскадная агрегация: `refresh_statuses()` обновляет `stages.status` и `pipelines.status` + timestamps.

**Errors:**
- `400` — невалидный статус (unknown value) или невалидный transition:
  - `{"error": "terminal status cannot change"}` — попытка изменить терминальный статус.
  - `{"error": "invalid status transition from Queued to Success"}` — переход невозможен.
- `404` — задача не найдена.
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
# Start job
curl -sS -X POST http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/status \
  -H 'content-type: application/json' \
  -d '{"status":"running"}'

# Pass job
curl -sS -X POST http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/status \
  -H 'content-type: application/json' \
  -d '{"status":"success"}'

# Fail job
curl -sS -X POST http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/status \
  -H 'content-type: application/json' \
  -d '{"status":"failed"}'
```

#### GET /api/v1/jobs/{job_id}/logs

Возвращает все логи задачи, отсортированные по `sequence`.

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `job_id` | UUID | ID задачи |

**Response 200:**
```json
[
  {
    "id": 1,
    "job_id": "job-uuid-1",
    "sequence": 1,
    "message": "Starting checkout...",
    "created_at": "2026-08-26T10:06:01Z"
  },
  {
    "id": 2,
    "job_id": "job-uuid-1",
    "sequence": 2,
    "message": "Fetching remotes",
    "created_at": "2026-08-26T10:06:02Z"
  }
]
```

**Errors:**
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs
```

#### POST /api/v1/jobs/{job_id}/logs

Добавляет строку лога. `sequence` вычисляется сервером: `COALESCE(MAX(sequence), 0) + 1`.

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `job_id` | UUID | ID задачи |

**Request body:**
```json
{
  "message": "Build completed successfully"
}
```

| Поле | Тип | Required | Описание |
|---|---|---|---|
| `message` | string | yes | Текст лога (non-empty, trim) |

**Response 200:**
```json
{
  "id": 3,
  "job_id": "job-uuid-1",
  "sequence": 3,
  "message": "Build completed successfully",
  "created_at": "2026-08-26T10:06:03Z"
}
```

**Errors:**
- `400` — `message` пустое.
- `503` — БД недоступна.
- `500` — ошибка БД (включая нарушение UNIQUE(job_id, sequence) при гонке).

**curl:**
```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs \
  -H 'content-type: application/json' \
  -d '{"message":"Build step completed"}'
```

---

## DTO Reference

### Project

| Поле | Тип | Описание |
|---|---|---|
| `id` | UUID | PK |
| `name` | string | Уникальное имя |
| `repository_url` | string | URL репозитория |
| `default_branch` | string | Ветка по умолчанию |
| `created_at` | datetime | Время создания (ISO 8601 UTC) |

### Pipeline

| Поле | Тип | Описание |
|---|---|---|
| `id` | UUID | PK |
| `project_id` | UUID | FK → projects |
| `git_ref` | string | Git-реф |
| `status` | string | `queued` / `running` / `success` / `failed` / `canceled` |
| `created_at` | datetime | Время создания |
| `started_at` | datetime\|null | Время начала |
| `finished_at` | datetime\|null | Время завершения |

### Stage

| Поле | Тип | Описание |
|---|---|---|
| `id` | UUID | PK |
| `pipeline_id` | UUID | FK → pipelines |
| `name` | string | Название стадии |
| `position` | integer | Порядок |
| `status` | string | Агрегированный статус |

### Job

| Поле | Тип | Описание |
|---|---|---|
| `id` | UUID | PK |
| `stage_id` | UUID | FK → stages |
| `name` | string | Название задачи |
| `image` | string | Docker-образ |
| `command` | string | Команда |
| `position` | integer | Порядок |
| `status` | string | Статус |
| `started_at` | datetime\|null | Время начала |
| `finished_at` | datetime\|null | Время завершения |

### PipelineDetail

| Поле | Тип | Описание |
|---|---|---|
| `pipeline` | Pipeline | Метаданные пайплайна |
| `stages` | StageDetail[] | Массив стадий с задачами |

### StageDetail

Поля `Stage` (flatten) + `jobs: Job[]`.

### JobLog

| Поле | Тип | Описание |
|---|---|---|
| `id` | integer | BIGSERIAL PK |
| `job_id` | UUID | FK → jobs |
| `sequence` | integer | Порядковый номер |
| `message` | string | Текст лога |
| `created_at` | datetime | Время записи |

---

## CLI

`cicd-cli` — отдельный бинарник, работает через публичный API.

```bash
# Project
cicd-cli project list
cicd-cli project create --name "my-service" --repository-url "git@..." --branch main

# Pipeline
cicd-cli pipeline list --project <uuid>
cicd-cli pipeline run --project <uuid> --git-ref main
cicd-cli pipeline show --id <uuid>

# Job
cicd-cli job start --id <uuid>     # POST status=running
cicd-cli job pass --id <uuid>      # POST status=success
cicd-cli job fail --id <uuid>      # POST status=failed
cicd-cli job logs --id <uuid>      # GET logs
cicd-cli job log --id <uuid> --message "..."  # POST log
```

`CICD_API_URL` env var задаёт URL API (default `http://127.0.0.1:22801`).

---

## End-to-end workflow

```bash
# 1. Create project
PROJECT=$(curl -sS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"my-service","repository_url":"git@github.com:org/my-service.git"}')
PROJECT_ID=$(printf '%s' "$PROJECT" | jq -r .id)

# 2. Trigger pipeline
PIPELINE=$(curl -sS -X POST "http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines" \
  -H 'content-type: application/json' -d '{"git_ref":"main"}')

# 3. Get first job ID
JOB_ID=$(printf '%s' "$PIPELINE" | jq -r '.stages[0].jobs[0].id')

# 4. Start job
curl -sS -X POST "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/status" \
  -H 'content-type: application/json' -d '{"status":"running"}'

# 5. Append log
curl -sS -X POST "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs" \
  -H 'content-type: application/json' -d '{"message":"Starting checkout..."}'

# 6. Complete job
curl -sS -X POST "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/status" \
  -H 'content-type: application/json' -d '{"status":"success"}'

# 7. View pipeline status (aggregated)
curl -sS "http://127.0.0.1:22801/api/v1/pipelines/$(printf '%s' "$PIPELINE" | jq -r .pipeline.id)"
```

---

## Platform endpoints (MVP)

> **Security note:** auth/RBAC is not enforced yet (Phase 1 — TODO). All endpoints below are unauthenticated in the current MVP. Use a reverse proxy or network isolation to restrict access in shared environments.

### Runners

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/runners` | Список зарегистрированных runner-ов |
| POST | `/runners` | Регистрация runner (name, tags[]) |
| DELETE | `/runners/{runner_id}` | Удалить runner |
| POST | `/runners/{runner_id}/heartbeat` | Обновить статус и last_seen_at |

### Project Secrets

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/secrets` | Метаданные секретов (без значений) |
| POST | `/projects/{project_id}/secrets` | Создать/обновить секрет (key, value) |
| DELETE | `/secrets/{secret_id}` | Удалить секрет |

> Секреты шифруются at-rest (AES-256-GCM, `CICD_SECRETS_KEY`). Значения **никогда** не возвращаются через API — только метаданные (id, key, timestamps).

### Artifacts

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/jobs/{job_id}/artifacts` | Метаданные артефактов задачи |
| POST | `/jobs/{job_id}/artifacts` | Загрузить артефакт (raw body, `X-Artifact-Name`) |
| GET | `/artifacts/{artifact_id}/download` | Скачать артефакт |

> Артефакты хранятся в локальной ФС (`CICD_ARTIFACTS_DIR`, default `/var/lib/forge/artifacts`). Лимит — 50 MiB на файл.

### Environments & Deployments

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/environments` | Список окружений проекта |
| POST | `/projects/{project_id}/environments` | Создать окружение (name, url) |
| PATCH | `/environments/{environment_id}` | Обновить окружение |
| DELETE | `/environments/{environment_id}` | Удалить окружение |
| GET | `/environments/{environment_id}/deployments` | Список деплоев |
| POST | `/environments/{environment_id}/deployments` | Создать деплой (git_ref, status) |

### Schedules

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/schedules` | Список расписаний |
| POST | `/projects/{project_id}/schedules` | Создать (cron, git_ref, enabled) |
| PATCH | `/schedules/{schedule_id}` | Обновить |
| DELETE | `/schedules/{schedule_id}` | Удалить |

> Cron — стандартное 5-полей выражение (`*/5 * * * *`). Исполнение расписаний (cron-scheduler) не реализовано в MVP — только хранение и API.

### Webhooks

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/webhooks` | Список webhook-ов |
| POST | `/projects/{project_id}/webhooks` | Создать (url, events[], enabled) |
| DELETE | `/webhooks/{webhook_id}` | Удалить |

> Отправка webhook-ов при событиях не реализована в MVP — только хранение конфигурации.

### Notifications

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/notifications` | Список каналов уведомлений |
| PUT | `/projects/{project_id}/notifications` | Заменить все каналы (array) |

### Reports

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/reports/summary` | Агрегаты: total/successful/failed, success_rate, avg_duration |

### Audit Log

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/audit-log` | Последние 200 событий аудита |

### Users & Roles

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/users` | Список пользователей |
| POST | `/users` | Создать (username, role: admin/maintainer/developer/viewer, enabled) |
| PATCH | `/users/{user_id}` | Обновить |

> Auth не реализован — пользователи хранятся как модель для будущего RBAC. Пароли не хранятся.

### API Tokens

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/api-tokens` | Список токенов (hint only) |
| POST | `/api-tokens` | Создать (name, user_id?) → возвращает `value` один раз |
| DELETE | `/api-tokens/{token_id}` | Отозвать |

> Токены хранятся как SHA-256 хэш. Полное значение возвращается только при создании. Проверка токенов при запросах не реализована в MVP.

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/DATA_MODEL.md` — схема БД.
- `backend/src/api.rs` — реализация endpoint.
- `backend/src/domain.rs` — правила переходов статусов.
- `docs/TESTING.md` — curl-проверки.
