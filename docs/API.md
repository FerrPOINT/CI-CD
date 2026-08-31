# API v1 Specification — Forge CI/CD

## Overview

REST API первой версии Forge CI/CD. Контрольная плоскость использует JSON; Git Smart HTTP, artifact download, SSE logs и `/metrics` имеют собственные content types. Основные группы: проекты, запуск пайплайнов, переходы статусов задач, append-only логи, platform resources, auth и Git hosting.

> **Source of truth:** актуальная реализация и OpenAPI-аннотации в `backend/src/api.rs`, `backend/src/platform.rs`, `backend/src/git_host.rs`, `backend/src/pulls.rs`; committed contract — `openapi/openapi.yaml`.

## Базовая информация

- Base URL: `http://{host}:22801/api/v1`
- Content-Type: `application/json`
- Auth: conditional. Без непустого `CICD_AUTH_SECRET` API работает в trusted-network режиме; с секретом большинство endpoint-ов требует `Authorization: Bearer <JWT-or-PAT>`.
- Версионирование: path-based `/api/v1`.
- Сериализация: `serde_json`, `snake_case` для enum-значений статусов.
- Ошибки: `{"error":{"code":"...","message":"...","request_id":"..."}}` с соответствующим HTTP статусом и header `x-request-id`.

## Коды ответов

| Код | Назначение |
|---|---|
| 200 OK | Успешный GET, успешный POST с результатом |
| 400 Bad Request | Невалидный ввод, невалидный transition |
| 401 Unauthorized | Отсутствует или невалиден Bearer token при включённом `CICD_AUTH_SECRET` |
| 403 Forbidden | Роль principal не допускает route |
| 404 Not Found | Ресурс не найден |
| 409 Conflict | Недопустимое состояние операции |
| 429 Too Many Requests | Login rate limit |
| 500 Internal Server Error | Ошибка БД |
| 503 Service Unavailable | БД недоступна |

---

## Реализованные эндпоинты

### Health

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/health` | Health-check сервиса |
| GET | `/openapi.json` | OpenAPI JSON document |
| GET | `/metrics` | Prometheus text exposition, полный путь без `/api/v1` |

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

#### GET /api/v1/openapi.json

Возвращает текущий OpenAPI document в JSON-формате. Не требует БД.

#### GET /metrics

Возвращает Prometheus text exposition. В MVP endpoint не имеет отдельной production-grade protection; используйте общую сетевую/auth boundary.

---

### Auth

| Метод | Путь | Назначение |
|---|---|---|
| POST | `/auth/login` | Выдать access/refresh tokens |
| POST | `/auth/refresh` | Обновить пару токенов |

Auth enforcement включается только если задан непустой `CICD_AUTH_SECRET`. Access token — JWT HS256 на 15 минут; refresh token хранится hash-ом в `sessions`. PAT `cicd_...` принимается как Bearer token при включённом enforcement.

#### POST /api/v1/auth/login

**Request body:**
```json
{"username":"admin","password":"..."}
```

**Response 200:**
```json
{"access_token":"...","expires_at":1787859000,"refresh_token":"..."}
```

**Errors:** `400` — пустой username/password; `401` — credential invalid или user disabled; `429` — login rate limit.

#### POST /api/v1/auth/refresh

**Request body:**
```json
{"username":"","password":"","refresh_token":"..."}
```

**Response 200:** структура `TokenPair`, как у login.

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

Возвращает список проектов, отсортированных по `created_at DESC`. Query params: `limit` и `offset`, лимит capped до 200.

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
curl -sS 'http://127.0.0.1:22801/api/v1/projects?limit=50&offset=0'
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

Возвращает пайплайны проекта, отсортированные по `created_at DESC`. Query params: `limit` и `offset`, лимит capped до 200.

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

Запускает новый пайплайн для указанного Git-рефа. Backend пытается прочитать `.forge-ci.yml` из локального bare repository; если файл недоступен, создаёт fallback build/test/deploy template. Все задачи — в статусе `queued`.

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


### Git-код, теги и релизы

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/repos/{repo}/tree?ref=&path=` | Содержимое каталога bare-репозитория |
| GET | `/repos/{repo}/blob?ref=&path=` | Текст файла, до 512 KiB; binary возвращает флаг без content |
| GET | `/repos/{repo}/tags` | Git tags, отсортированные по дате |
| GET/POST | `/repos/{repo}/releases` | Список / создание или обновление release metadata |
| GET/DELETE | `/repos/{repo}/releases/{tag}` | Один release / удаление metadata (Git tag сохраняется) |

`ref` проходит только в Git через уже валидированный bare repository; пустой `HEAD` автоматически берёт `main`, затем `master`. Для Smart HTTP fetch public repository доступен без `CICD_GIT_TOKEN`; private repository требует token. `git-receive-pack` требует token всегда, если token сконфигурирован.

### Дополнительные CI-результаты

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/pipelines/{pipeline_id}/badge.svg` | Public read-only SVG status badge |
| GET | `/pipelines/{pipeline_id}/variables` | Сохранённые variables запуска |
| GET/POST | `/jobs/{job_id}/test-report` | JUnit suite summaries / загрузка XML JSON-строкой |

`POST /projects/{project_id}/pipelines` дополнительно принимает `{ "variables": { "deploy_env": "staging" } }`. Runner преобразует только в `CICD_VAR_DEPLOY_ENV`; исходные ключи не становятся произвольными process env. JUnit endpoint принимает JSON string с XML и сохраняет только имя suite, счётчики и длительность.


### Jobs

| Метод | Путь | Назначение |
|---|---|---|
| POST | `/jobs/{job_id}/status` | Смена статуса задачи |
| POST | `/jobs/{job_id}/start` | Старт manual job |
| GET | `/jobs/{job_id}/attempts` | История execution attempts задачи |
| GET | `/jobs/{job_id}/attempts/{attempt_id}/logs` | Логи конкретной attempt |
| GET | `/jobs/{job_id}/logs` | Логи текущей или последней attempt задачи |
| GET | `/jobs/{job_id}/logs/stream` | SSE stream логов текущей/последней attempt |
| POST | `/jobs/{job_id}/logs` | Добавление строки лога в текущую/последнюю attempt |

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
- Одновременно обновляется open `execution_attempts` запись job; terminal attempt не переиспользуется.
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

#### GET /api/v1/jobs/{job_id}/attempts

Возвращает attempts задачи в порядке `attempt_no DESC`. Каждая попытка хранит собственные timestamps, terminal result и diagnostics.

**Path params:**

| Параметр | Тип | Описание |
|---|---|---|
| `job_id` | UUID | ID задачи |

**Response 200:**
```json
[
  {
    "id": "attempt-uuid-2",
    "job_id": "job-uuid-1",
    "attempt_no": 2,
    "status": "queued",
    "trigger": "job_retry",
    "exit_code": null,
    "error_tail": null,
    "created_at": "2026-08-31T10:00:00Z",
    "started_at": null,
    "finished_at": null
  }
]
```

#### GET /api/v1/jobs/{job_id}/attempts/{attempt_id}/logs

Возвращает логи конкретной attempt, отсортированные по `sequence`. Если attempt не принадлежит job, возвращается `404`.

**Response 200:**
```json
[
  {
    "id": 1,
    "job_id": "job-uuid-1",
    "attempt_id": "attempt-uuid-1",
    "sequence": 1,
    "message": "Starting checkout...",
    "created_at": "2026-08-26T10:06:01Z"
  },
  {
    "id": 2,
    "job_id": "job-uuid-1",
    "attempt_id": "attempt-uuid-1",
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
curl -sS http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/attempts
curl -sS http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/attempts/$ATTEMPT_ID/logs
```

#### GET /api/v1/jobs/{job_id}/logs

Совместимый shortcut: возвращает логи текущей open attempt (`queued`/`running`), а если open attempt нет — последней по `attempt_no`. Для полного retry history используйте endpoint-ы attempts выше.

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs
```

#### GET /api/v1/jobs/{job_id}/logs/stream

Возвращает `text/event-stream` для текущей/последней attempt: сначала существующие строки с `sequence > after`, затем новые строки при polling backend. Query param `after` опционален; default `-1`. При terminal status job отправляется event `done`.

**curl:**
```bash
curl -N 'http://127.0.0.1:22801/api/v1/jobs/'"$JOB_ID"'/logs/stream?after=0'
```

#### POST /api/v1/jobs/{job_id}/logs

Добавляет строку лога в текущую open attempt, а если её нет — в последнюю attempt. `sequence` вычисляется сервером внутри attempt под advisory lock, поэтому конкурирующие append-запросы получают монотонные номера.

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
  "attempt_id": "attempt-uuid-2",
  "sequence": 3,
  "message": "Build completed successfully",
  "created_at": "2026-08-26T10:06:03Z"
}
```

**Errors:**
- `400` — `message` пустое.
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs \
  -H 'content-type: application/json' \
  -d '{"message":"Build step completed"}'
```

#### POST /api/v1/jobs/{job_id}/start

Стартует manual job (`manual = true`) и возвращает:

```json
{"started": true}
```

**Errors:** `404` — job не найден; `409` — job не manual или уже стартовал.

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
| `attempt_id` | UUID | FK → execution_attempts |
| `sequence` | integer | Порядковый номер внутри attempt |
| `message` | string | Текст лога |
| `created_at` | datetime | Время записи |

### JobAttempt

| Поле | Тип | Описание |
|---|---|---|
| `id` | UUID | PK |
| `job_id` | UUID | FK → jobs |
| `attempt_no` | integer | Номер попытки внутри job |
| `status` | JobStatus | Текущий или terminal status attempt |
| `trigger` | string | `initial`, `job_retry`, `pipeline_retry`, `runner`, `manual_status`, `compat` |
| `exit_code` | integer/null | Код процесса embedded runner, если известен |
| `error_tail` | string/null | Краткая terminal diagnostics |
| `created_at` / `started_at` / `finished_at` | datetime/null | Timestamps attempt |

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
cicd-cli job attempts --id <uuid>  # GET attempts
cicd-cli job logs --id <uuid> --attempt <attempt-uuid>
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

> **Security note:** auth/RBAC enforcement включается только при непустом `CICD_AUTH_SECRET`. Без него все endpoints ниже работают в trusted-network режиме; с ним применяются JWT/PAT, route roles и project memberships для project-owned ресурсов. Scoped PAT, tenant isolation и production logout/session policy ещё target.

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

### Project Memberships

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/memberships` | Список участников проекта: user, enabled, role |
| POST | `/projects/{project_id}/memberships` | Создать/обновить роль участника (`maintainer`, `developer`, `viewer`) |
| DELETE | `/projects/{project_id}/memberships/{user_id}` | Удалить участника; последнего maintainer удалить нельзя |

> При включённом `CICD_AUTH_SECRET` `admin` видит все проекты; остальные пользователи видят только проекты из `project_memberships`. Эффективный доступ ограничен и глобальной ролью, и project role. Mutation секретов и membership требуют maintainer+.

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

> Cron хранится как 5-польная строка (`*/5 * * * *`). Текущий scheduler — MVP: примерно раз в минуту запускает enabled записи, не реализуя полную cron-семантику.

### Webhooks

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/projects/{project_id}/webhooks` | Список webhook-ов |
| POST | `/projects/{project_id}/webhooks` | Создать (url, events[], enabled) |
| DELETE | `/webhooks/{webhook_id}` | Удалить |

> Outgoing webhook delivery реализована для terminal pipeline events через `domain_events`/`outbox_messages`, basic retry/backoff и optional HMAC secret. Delivery history/replay/dead letters остаются target.

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

> Users, `user_credentials` и sessions используются текущим auth middleware при `CICD_AUTH_SECRET`; глобальная роль задаёт верхнюю границу, project membership задаёт доступ к конкретному проекту.

### API Tokens

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/api-tokens` | Список токенов (hint only) |
| POST | `/api-tokens` | Создать (name, user_id?) → возвращает `value` один раз |
| DELETE | `/api-tokens/{token_id}` | Отозвать |

> Токены хранятся как SHA-256 хэш. Полное значение возвращается только при создании. Bearer PAT проверяется только при включённом `CICD_AUTH_SECRET`; scopes/pepper/rotation остаются target.

## Дополнительные реализованные endpoint-ы

### Pipeline и job actions

| Метод | Путь | Body | Результат |
|---|---|---|---|
| POST | `/pipelines/{pipeline_id}/cancel` | — | Каскадно отменяет нетерминальные stages/jobs, закрывает открытые execution attempts как `canceled` и возвращает `{"canceled":"<uuid>"}` |
| POST | `/pipelines/{pipeline_id}/retry` | — | Сбрасывает failed/canceled pipeline в queued, создаёт новые attempts для affected jobs и возвращает `{"retried":"<uuid>"}` |
| POST | `/jobs/{job_id}/retry` | — | Сбрасывает terminal job в queued и создаёт новую `execution_attempt` без удаления старых логов |
| POST | `/jobs/{job_id}/start` | — | Стартует manual job и возвращает `{"started":true}` |

История попыток хранится в `execution_attempts`. Логическая запись `jobs` остаётся текущей проекцией для pipeline/stage aggregation, а terminal evidence каждой попытки читается через `/jobs/{job_id}/attempts` и `/jobs/{job_id}/attempts/{attempt_id}/logs`.

Ошибки: `404` — ресурс не найден; `400/409` — недопустимая операция/состояние; `503` — БД недоступна.

### Runner registry

| Метод | Путь | Body / результат |
|---|---|---|
| GET | `/runners` | Список `{id,name,tags,status,last_seen_at,created_at}` |
| POST | `/runners` | `{name, tags?}` → зарегистрированный runner |
| POST | `/runners/{runner_id}/heartbeat` | Обновляет `last_seen_at`/status |
| DELETE | `/runners/{runner_id}` | Удаляет registry-запись |

> Сейчас registry/heartbeat — inventory для embedded runner. Registration token, lease и dispatch на внешний runner пока не реализованы: `docs/RUNNER_ARCHITECTURE.md`.

### Secrets и artifacts

| Метод | Путь | Body / результат |
|---|---|---|
| GET/POST | `/projects/{project_id}/secrets` | Метаданные / `{key,value}`; значение никогда не возвращается |
| DELETE | `/secrets/{secret_id}` | Удаляет секрет |
| GET/POST | `/jobs/{job_id}/artifacts` | Метаданные / raw body с `X-Artifact-Name` |
| GET | `/artifacts/{artifact_id}/download` | Файл с сохранённым content type/name |

### Git repositories и Smart HTTP

| Метод | Путь | Назначение |
|---|---|---|
| GET/POST | `/repositories` | Список / создание bare repository (`{name}`) |
| DELETE | `/repositories/{name}` | Удаление repository и bare storage |
| GET | `/repos/{repo}/refs` | Branch/tag refs с SHA |
| GET | `/repos/{repo}/commits?branch=&limit=` | Commit history; default 50, maximum 200 |
| GET | `/repos/{repo}/compare?from=&to=` | Merge-base, file stats и unified patch |
| GET/POST | `/repos/{repo}/pulls` | Список / создание pull request |
| POST | `/repos/{repo}/pulls/{number}/action` | `{action:"merge"|"close"|"reopen"}` |
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | Git Smart HTTP discovery |
| POST | `/git/{repo}/git-upload-pack` | Smart HTTP fetch/clone service |
| POST | `/git/{repo}/git-receive-pack` | Smart HTTP push service |

Git Smart HTTP проверяет `CICD_GIT_TOKEN`, если он задан. Полный lifecycle — `docs/GIT_HOSTING.md`; PR merge semantics — `docs/PULL_REQUESTS.md`.

### Internal Git hook

`POST /api/v1/internal/git-push` вызывается generated `post-receive` hook. Заголовок `X-Internal-Token` обязан совпадать с `CICD_GIT_INTERNAL_TOKEN`, когда токен сконфигурирован.

```json
{"repository":"my-service","ref_name":"refs/heads/main"}
```

Ответ: `{"triggered":true,"pipeline_id":"..."}` либо `{"triggered":false,"pipeline_id":null}`, если project с данным local Git URL не найден. Ошибочная hook-доставка не откатывает Git push.

## Полный route inventory

Этот список синхронизирован с Axum route definitions в `backend/src/api.rs`, `backend/src/platform.rs`, `backend/src/git_host.rs` и `backend/src/pulls.rs`. Он нужен как машинно проверяемая карта: `python3 scripts/verify_docs.py --api-doc-routes` падает, если backend route не упомянут в этом документе.

| Route | Группа |
|---|---|
| `/api/v1/api-tokens` | Auth / tokens |
| `/api/v1/api-tokens/{token_id}` | Auth / tokens |
| `/api/v1/artifacts/{artifact_id}/download` | Artifacts |
| `/api/v1/audit-log` | Audit |
| `/api/v1/auth/login` | Auth |
| `/api/v1/auth/refresh` | Auth |
| `/api/v1/environments/{environment_id}` | Environments |
| `/api/v1/environments/{environment_id}/deployments` | Environments |
| `/api/v1/health` | Health |
| `/api/v1/internal/git-push` | Git hooks |
| `/api/v1/jobs/{job_id}/artifacts` | Artifacts |
| `/api/v1/jobs/{job_id}/attempts` | Jobs |
| `/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs` | Jobs |
| `/api/v1/jobs/{job_id}/logs` | Jobs |
| `/api/v1/jobs/{job_id}/logs/stream` | Jobs |
| `/api/v1/jobs/{job_id}/retry` | Jobs |
| `/api/v1/jobs/{job_id}/start` | Jobs |
| `/api/v1/jobs/{job_id}/status` | Jobs |
| `/api/v1/jobs/{job_id}/test-report` | Jobs |
| `/api/v1/openapi.json` | Health |
| `/api/v1/pipelines/{pipeline_id}` | Pipelines |
| `/api/v1/pipelines/{pipeline_id}/badge.svg` | Pipelines |
| `/api/v1/pipelines/{pipeline_id}/cancel` | Pipelines |
| `/api/v1/pipelines/{pipeline_id}/retry` | Pipelines |
| `/api/v1/pipelines/{pipeline_id}/variables` | Pipelines |
| `/api/v1/projects` | Projects |
| `/api/v1/projects/{project_id}` | Projects |
| `/api/v1/projects/{project_id}/environments` | Environments |
| `/api/v1/projects/{project_id}/memberships` | Project memberships |
| `/api/v1/projects/{project_id}/memberships/{user_id}` | Project memberships |
| `/api/v1/projects/{project_id}/notifications` | Notifications |
| `/api/v1/projects/{project_id}/pipelines` | Pipelines |
| `/api/v1/projects/{project_id}/reports/summary` | Reports |
| `/api/v1/projects/{project_id}/schedules` | Schedules |
| `/api/v1/projects/{project_id}/secrets` | Secrets |
| `/api/v1/projects/{project_id}/webhooks` | Webhooks |
| `/api/v1/repos/{repo}/blob` | Git repositories |
| `/api/v1/repos/{repo}/commits` | Git repositories |
| `/api/v1/repos/{repo}/compare` | Pull requests |
| `/api/v1/repos/{repo}/pulls` | Pull requests |
| `/api/v1/repos/{repo}/pulls/{number}/action` | Pull requests |
| `/api/v1/repos/{repo}/refs` | Git repositories |
| `/api/v1/repos/{repo}/releases` | Releases |
| `/api/v1/repos/{repo}/releases/{tag}` | Releases |
| `/api/v1/repos/{repo}/tags` | Releases |
| `/api/v1/repos/{repo}/tree` | Git repositories |
| `/api/v1/repositories` | Git repositories |
| `/api/v1/repositories/{name}` | Git repositories |
| `/api/v1/runners` | Runners |
| `/api/v1/runners/{runner_id}` | Runners |
| `/api/v1/runners/{runner_id}/heartbeat` | Runners |
| `/api/v1/schedules/{schedule_id}` | Schedules |
| `/api/v1/secrets/{secret_id}` | Secrets |
| `/api/v1/users` | Users |
| `/api/v1/users/{user_id}` | Users |
| `/api/v1/webhooks/{webhook_id}` | Webhooks |
| `/git/{repo}/git-receive-pack` | Git Smart HTTP |
| `/git/{repo}/git-upload-pack` | Git Smart HTTP |
| `/git/{repo}/info/refs` | Git Smart HTTP |
| `/metrics` | Metrics |

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/DATA_MODEL.md` — схема БД.
- `docs/GIT_HOSTING.md` — Smart HTTP и hooks.
- `docs/PULL_REQUESTS.md` — compare и pull requests.
- `docs/RUNNER_ARCHITECTURE.md` — target runner protocol.
- `backend/src/api.rs`, `backend/src/platform.rs`, `backend/src/git_host.rs`, `backend/src/pulls.rs` — реализация endpoint-ов.
- `backend/domain/src/lib.rs` — правила переходов статусов.
- `docs/TESTING.md` — curl-проверки.
