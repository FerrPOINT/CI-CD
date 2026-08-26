# Pagination — Forge CI/CD

## 1. Overview

Соглашения о пагинации для списковых endpoint Forge CI/CD. Текущая версия использует фиксированный `LIMIT 50` для списка пайплайнов. Полноценная пагинация запланирована в Phase 2+.

> **Source of truth:** актуальная реализация в `backend/src/api.rs` и `backend/src/store.rs`.

## 2. Текущее состояние

### 2.1 Поведение списковых endpoint

| Endpoint | Pagination | Sort | Max records |
|----------|------------|------|-------------|
| `GET /api/v1/projects` | Нет (возвращает все) | `created_at DESC` | Без лимита |
| `GET /api/v1/projects/{id}/pipelines` | `LIMIT 50` | `created_at DESC` | 50 |
| `GET /api/v1/pipelines/{id}` | Нет (один пайплайн + stages + jobs) | `position ASC` | — |
| `GET /api/v1/jobs/{id}/logs` | Нет (возвращает все) | `sequence ASC` | Без лимита |

### 2.2 SQL

```sql
-- list_pipelines: последние 50
SELECT id, project_id, git_ref, status, created_at, started_at, finished_at
FROM pipelines
WHERE project_id = $1
ORDER BY created_at DESC
LIMIT 50;

-- list_projects: все проекты
SELECT id, name, repository_url, default_branch, created_at
FROM projects
ORDER BY created_at DESC;

-- list_logs: все логи задачи
SELECT id, job_id, sequence, message, created_at
FROM job_logs
WHERE job_id = $1
ORDER BY sequence ASC;
```

### 2.3 Ограничения текущего подхода

- `GET /projects` — без лимита: при росте числа проектов ответ может стать большим.
- `GET /projects/{id}/pipelines` — `LIMIT 50` без offset/cursor: нельзя получить пайплайны старше 50-го.
- `GET /jobs/{id}/logs` — без лимита: лог-файл задачи может содержать тысячи строк.
- Нет metadata о общем количестве записей (`total`).

## 3. Планируемая пагинация (Phase 2+)

### 3.1 Offset-based pagination

Для проектов и пайплайнов — простая offset-based пагинация:

```
GET /api/v1/projects?page=1&size=20
GET /api/v1/projects/{id}/pipelines?page=1&size=20
```

**Response:**
```json
{
  "data": [
    {
      "id": "550e8400-...",
      "name": "my-service",
      "repository_url": "git@github.com:org/repo.git",
      "default_branch": "main",
      "created_at": "2026-08-26T10:00:00Z"
    }
  ],
  "total": 145,
  "page": 1,
  "size": 20,
  "total_pages": 8
}
```

**SQL:**
```sql
SELECT id, name, repository_url, default_branch, created_at
FROM projects
ORDER BY created_at DESC
LIMIT $1 OFFSET $2;
-- $1 = size, $2 = (page - 1) * size

SELECT COUNT(*) FROM projects;
```

### 3.2 Параметры

| Параметр | Тип | Default | Max | Описание |
|----------|-----|---------|-----|----------|
| `page` | integer | 1 | — | Номер страницы (1-based) |
| `size` | integer | 20 | 100 | Размер страницы |

### 3.3 Лимиты по ресурсам

| Resource | Default size | Max size |
|----------|-------------|----------|
| `projects` | 20 | 100 |
| `pipelines` | 20 | 100 |
| `job_logs` | 50 | 200 (cursor) |

### 3.4 Response envelope

```json
{
  "data": [...],
  "total": 145,
  "page": 1,
  "size": 20,
  "total_pages": 8
}
```

| Поле | Тип | Описание |
|------|-----|----------|
| `data` | array | Массив элементов текущей страницы |
| `total` | integer | Общее количество записей |
| `page` | integer | Текущая страница (1-based) |
| `size` | integer | Размер страницы |
| `total_pages` | integer | Общее количество страниц |

> `total` вычисляется через отдельный `SELECT COUNT(*)`. Для больших таблиц (job_logs) `COUNT(*)` может быть медленным — использовать cursor-based pagination без `total`.

## 4. Cursor-based pagination (job_logs)

Логи задач — append-only и могут расти неограниченно. Offset-based pagination неэффективна для больших списков (deep offset = slow). Cursor-based pagination решает эту проблему.

### 4.1 API

```
GET /api/v1/jobs/{job_id}/logs?cursor=eyJzZXEiOjUwfQ==&size=50
```

**Response:**
```json
{
  "data": [
    {
      "id": 51,
      "job_id": "job-uuid-1",
      "sequence": 51,
      "message": "Building...",
      "created_at": "2026-08-26T10:06:51Z"
    }
  ],
  "next_cursor": "eyJzZXEiOjEwMH0=",
  "has_more": true
}
```

### 4.2 Cursor format

- Cursor — base64-encoded JSON: `base64(JSON({ "seq": 50 }))`.
- Содержит последнее значение `sequence` + 1.
- Не поддерживает произвольный page jump (только next/prev).

**SQL:**
```sql
SELECT id, job_id, sequence, message, created_at
FROM job_logs
WHERE job_id = $1
  AND sequence > $2
ORDER BY sequence ASC
LIMIT $3;
-- $2 = cursor decoded sequence, $3 = size + 1 (для has_more)
```

### 4.3 Reverse pagination

```
GET /api/v1/jobs/{job_id}/logs?cursor=...&direction=prev&size=50
```

```sql
SELECT * FROM (
  SELECT id, job_id, sequence, message, created_at
  FROM job_logs
  WHERE job_id = $1
    AND sequence < $2
  ORDER BY sequence DESC
  LIMIT $3
) sub
ORDER BY sequence ASC;
```

## 5. Keyset Pagination (альтернатива)

Для пайплайнов — keyset pagination по `(created_at, id)`:

```
GET /api/v1/projects/{id}/pipelines?after=2026-08-26T10:05:00Z,550e8400-...&size=20
```

```sql
SELECT id, project_id, git_ref, status, created_at, started_at, finished_at
FROM pipelines
WHERE project_id = $1
  AND (created_at, id) < ($2, $3)
ORDER BY created_at DESC, id DESC
LIMIT $4;
```

Преимущества:
- Стабильна при добавлении новых записей (в отличие от offset).
- Быстрее для deep pagination.

Недостатки:
- Нет `total` / `total_pages`.
- Только next/prev навигация.

> **Решение (Phase 2):** offset-based для проектов, cursor/keyset для пайплайнов и логов.

## 6. Производительность

| Режим | Запрос | Производительность |
|-------|--------|-------------------|
| Offset (small page) | `LIMIT 20 OFFSET 0` | Быстро — индекс покрывает |
| Offset (deep) | `LIMIT 20 OFFSET 10000` | Медленно — PostgreSQL сканирует 10020 строк |
| Cursor | `WHERE sequence > $1 LIMIT 20` | Быстро — всегда использует индекс |
| Keyset | `WHERE (created_at, id) < ($1, $2) LIMIT 20` | Быстро — всегда использует индекс |

### Рекомендации

- `COUNT(*)` — только для offset-based и только на маленьких таблицах (projects, pipelines).
- Для `job_logs` — не выполнять `COUNT(*)`, использовать `has_more` вместо `total`.
- `LIMIT` всегда с `ORDER BY` — детерминированный порядок.
- Индекс на `(project_id, created_at DESC)` — для keyset pagination пайплайнов.

## 7. HTTP заголовки (Phase 2+)

| Заголовок | Описание |
|-----------|----------|
| `X-Total-Count` | Общее количество записей (для offset mode) |
| `Link: <...>; rel="next"` | URL следующей страницы (cursor mode) |

## 8. CLI

```bash
# Текущее (без пагинации)
cicd-cli project list
cicd-cli pipeline list --project <uuid>

# Планируемое (Phase 2)
cicd-cli project list --page 1 --size 20
cicd-cli pipeline list --project <uuid> --page 2 --size 20
cicd-cli job logs --id <uuid> --cursor eyJzZXEiOjUwfQ==
```

## References

- `docs/API.md` — спецификация endpoint.
- `docs/API_STANDARDS.md` — стандарты REST API и пагинации.
- `docs/PERFORMANCE.md` — оптимизация запросов.
- `docs/DATABASE_INDEXES.md` — индексы для пагинации.
- `backend/src/api.rs` — реализация endpoint.