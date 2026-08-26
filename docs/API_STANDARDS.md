# API Standards — Forge CI/CD

## 1. Общие принципы

- API — REST поверх HTTP/1.1 и HTTP/2.
- Формат обмена данными — JSON.
- Кодировка — UTF-8.
- Версионирование — URL path: `/api/v1/...`. Подробнее в `docs/API_VERSIONING.md`.
- Сериализация — `serde_json`, `snake_case` для enum-значений статусов.
- Auth: не реализована в текущей версии (Phase 1 — TODO).

> **Source of truth:** актуальная реализация в `backend/src/api.rs`. Документация может отставать — при расхождении приоритет у исходного кода.

## 2. Base URL

```
http://{host}:22801/api/v1
```

- Локальная разработка: `http://127.0.0.1:22801/api/v1`
- Docker: `http://127.0.0.1:22801/api/v1` (порт проброшен на хост)
- Vite dev proxy: `/api` → `http://localhost:22801`

## 3. Content-Type

- Request: `application/json` для всех POST-запросов с телом.
- Response: `application/json` для всех endpoint.
- Кодировка — UTF-8.

```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"my-service","repository_url":"git@github.com:org/repo.git"}'
```

> Запрос без `Content-Type: application/json` возвращает `400` — Axum требует корректный content-type для `Json<T>` extractor.

## 4. URL и ресурсы

### 4.1 Именование

- Имена ресурсов — множественное число существительных: `/projects`, `/pipelines`, `/jobs`.
- Иерархия через вложенность:
  - `/projects/{project_id}/pipelines` — пайплайны проекта.
  - `/pipelines/{pipeline_id}` — детали пайплайна (вне вложенности, доступ по ID).
  - `/jobs/{job_id}/status` — смена статуса задачи.
  - `/jobs/{job_id}/logs` — логи задачи.

### 4.2 Path параметры

- UUID ресурсов — UUIDv4, строка в формате `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
- Невалидный UUID в path возвращает `400`.

### 4.3 Текущая карта endpoint

| Метод | Путь | Назначение |
|---|---|---|
| GET | `/api/v1/health` | Health-check сервиса |
| GET | `/api/v1/projects` | Список проектов |
| POST | `/api/v1/projects` | Создание проекта |
| GET | `/api/v1/projects/{project_id}/pipelines` | Список пайплайнов проекта |
| POST | `/api/v1/projects/{project_id}/pipelines` | Запуск пайплайна |
| GET | `/api/v1/pipelines/{pipeline_id}` | Детали пайплайна |
| POST | `/api/v1/jobs/{job_id}/status` | Смена статуса задачи |
| GET | `/api/v1/jobs/{job_id}/logs` | Список логов задачи |
| POST | `/api/v1/jobs/{job_id}/logs` | Добавление строки лога |

## 5. HTTP методы

| Метод | Операция | Семантика |
|-------|----------|-----------|
| GET | Чтение | Идемпотентный, без side effects |
| POST | Создание / действие | Создание ресурса или запуск действия |

> Текущая версия использует только `GET` и `POST`. `PUT`, `PATCH`, `DELETE` запланированы в Phase 2+ для обновления проектов, удаления пайплайнов и отмены.

### Статус-коды ответов

| Код | Назначение |
|---|---|
| 200 OK | Успешный GET, успешный POST с результатом |
| 400 Bad Request | Невалидный ввод, невалидный transition |
| 404 Not Found | Ресурс не найден |
| 500 Internal Server Error | Внутренняя ошибка, ошибка БД |
| 503 Service Unavailable | БД недоступна (pool = None) |

## 6. Формат ошибок

Единый конверт ошибок — простой JSON с полем `error`:

```json
{
  "error": "project not found"
}
```

| HTTP | Пример | Описание |
|------|--------|----------|
| 400 | `{"error": "name is required"}` | Невалидный ввод |
| 400 | `{"error": "terminal status cannot change"}` | Невалидный transition |
| 404 | `{"error": "project not found"}` | Ресурс не найден |
| 500 | `{"error": "internal server error"}` | Внутренняя ошибка |
| 503 | `{"error": "database unavailable"}` | БД недоступна |

> **Планируется (Phase 2+):** расширение конверта ошибок с кодом и деталями:
> ```json
> {
>   "error": {
>     "code": "VALIDATION_ERROR",
>     "message": "name is required",
>     "field": "name"
>   }
> }
> ```

## 7. Пагинация

### Текущее состояние

Пагинация не реализована. Endpoint списков возвращают фиксированные выборки:

| Endpoint | Behavior |
|----------|----------|
| `GET /projects` | Все проекты, сортировка `created_at DESC` |
| `GET /projects/{id}/pipelines` | Последние 50, сортировка `created_at DESC` |
| `GET /jobs/{id}/logs` | Все логи, сортировка `sequence ASC` |

### Планируемая пагинация (Phase 2+)

- Offset-based: `?page=1&size=20` для проектов и пайплайнов.
- Cursor-based: `?cursor=...` для логов задач (append-only, быстрый рост).
- Метаданные пагинации в response:
  ```json
  {
    "data": [...],
    "total": 145,
    "page": 1,
    "size": 20
  }
  ```
- Подробнее в `docs/PAGINATION.md`.

## 8. Сортировка

### Текущая

| Endpoint | Сортировка |
|----------|------------|
| `GET /projects` | `created_at DESC` (фиксированная) |
| `GET /projects/{id}/pipelines` | `created_at DESC` (фиксированная) |
| `GET /pipelines/{id}` | Stages по `position`, jobs по `position` |
| `GET /jobs/{id}/logs` | `sequence ASC` (фиксированная) |

### Планируемая (Phase 2+)

- Параметр `?sort=-created_at` (префикс `-` для DESC).
- Allowlist полей сортировки per-resource.

## 9. Фильтрация

### Текущая

Фильтрация не реализована. `GET /projects/{id}/pipelines` фильтрует по `project_id` через path parameter.

### Планируемая (Phase 2+)

- `GET /projects/{id}/pipelines?status=running` — фильтр по статусу.
- `GET /projects?name=my-service` — поиск по имени.

## 10. Идемпотентность

- `GET` — идемпотентный по определению.
- `POST /projects/{id}/pipelines` — не идемпотентный (каждый вызов создаёт новый pipeline).
- `POST /jobs/{id}/status` — не идемпотентный (повторный вызов может вернуть `400` для терминального статуса).
- `POST /jobs/{id}/logs` — не идемпотентный (каждый вызов добавляет новый лог с инкрементным `sequence`).

> **Планируется (Phase 2+):** заголовок `Idempotency-Key` для `POST` запросов, чувствительных к дублированию.

## 11. CORS

- `CorsLayer::permissive()` — все origins, все методы, все заголовки.
- Допустимо для dev-режима и single-instance deployment.
- **Планируется (Phase 1 — Auth):** ограничение origins до известных frontend domain.

## 12. Rate Limiting

Не реализован в текущей версии.

> **Планируется (Phase 2+):** `tower_governor` per IP. Default: 100 req/min. Заголовки `X-RateLimit-*`.

## References

- `docs/API.md` — полная спецификация endpoint и DTO.
- `docs/API_VERSIONING.md` — политика версионирования.
- `docs/API_EDGE_CASES.md` — граничные случаи и обработка ошибок.
- `docs/PAGINATION.md` — текущее и планируемое состояние пагинации.
- `backend/src/api.rs` — реализация endpoint.