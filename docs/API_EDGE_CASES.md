# API Edge Cases — Forge CI/CD

## 1. Overview

Нестандартные сценарии API и ожидаемое поведение. Описаны граничные случаи для control plane Forge CI/CD: невалидные переходы статусов, отсутствующие проекты, пустые логи, дубликаты имён и недоступность БД.

> **Source of truth:** актуальная реализация в `backend/src/api.rs` и `backend/src/domain.rs`. При расхождении приоритет у исходного кода.

## 2. Status Transition Edge Cases

Переходы статусов задач валидируются через `JobStatus::transition_to()` (см. `docs/DATA_MODEL.md`, раздел 6).

| Scenario | HTTP | Response body | Описание |
|----------|------|---------------|----------|
| `queued → running` | 200 | обновлённый `Job` | Корректный переход |
| `queued → canceled` | 200 | обновлённый `Job` | Корректный переход |
| `queued → success` | 400 | `{"error": "invalid status transition from Queued to Success"}` | Нельзя пропустить `running` |
| `queued → failed` | 400 | `{"error": "invalid status transition from Queued to Failed"}` | Нельзя пропустить `running` |
| `running → success` | 200 | обновлённый `Job` | Корректный переход |
| `running → failed` | 200 | обновлённый `Job` | Корректный переход |
| `running → canceled` | 200 | обновлённый `Job` | Корректный переход |
| `success → running` | 400 | `{"error": "terminal status cannot change"}` | Терминальный статус |
| `failed → running` | 400 | `{"error": "terminal status cannot change"}` | Терминальный статус |
| `canceled → running` | 400 | `{"error": "terminal status cannot change"}` | Терминальный статус |
| `success → failed` | 400 | `{"error": "terminal status cannot change"}` | Терминальный статус |
| Неизвестный статус `{"status": "pending"}` | 400 | `{"error": "unknown status: pending"}` | Невалидное значение enum |

### Каскадная агрегация

После смены статуса задачи вызывается `refresh_statuses()`, которая пересчитывает статусы stage и pipeline. Ошибки агрегации возвращают `500`.

| Scenario | Behavior |
|----------|----------|
| Последний job в stage → `success` | Stage → `success`, проверяется pipeline |
| Один job в stage → `failed` | Stage → `failed`, pipeline → `failed` |
| Job → `running` | Stage → `running`, pipeline → `running` (если ещё не терминальный) |
| Все jobs в stage `success` но stage уже `failed` | Stage остаётся `failed` (терминальный) |

## 3. Missing Resource Edge Cases

| Scenario | HTTP | Response body | Endpoint |
|----------|------|---------------|----------|
| Project не найден при `POST /projects/{id}/pipelines` | 404 | `{"error": "project not found"}` | Pipeline trigger |
| Pipeline не найден при `GET /pipelines/{id}` | 404 | `{"error": "pipeline not found"}` | Pipeline detail |
| Job не найден при `POST /jobs/{id}/status` | 404 | `{"error": "job not found"}` | Job status |
| Job не найден при `GET /jobs/{id}/logs` | 200 | `[]` (пустой массив) | Job logs — пустой ответ, не 404 |
| Job не найден при `POST /jobs/{id}/logs` | 404 | `{"error": "job not found"}` | Job log append |
| Невалидный UUID в path | 400 | `{"error": "invalid path parameter"}` | Любой endpoint с UUID |

> **Внимание:** `GET /jobs/{job_id}/logs` для несуществующего job возвращает пустой массив `[]`, а не `404`. Это связано с тем, что запрос выполняется как `SELECT ... WHERE job_id = $1 ORDER BY sequence`, и отсутствие строк трактуется как пустой результат. Запланировано к исправлению в Phase 2.

## 4. Validation Edge Cases

| Input | HTTP | Response body | Endpoint |
|-------|------|---------------|----------|
| `name` пустое или whitespace | 400 | `{"error": "name is required"}` | `POST /projects` |
| `repository_url` пустое | 400 | `{"error": "repository_url is required"}` | `POST /projects` |
| `name` = `"  "` (только пробелы) | 400 | `{"error": "name is required"}` | `POST /projects` (trim + empty check) |
| `message` пустое при добавлении лога | 400 | `{"error": "message is required"}` | `POST /jobs/{id}/logs` |
| `message` = `"  "` (только пробелы) | 400 | `{"error": "message is required"}` | `POST /jobs/{id}/logs` (trim + empty check) |
| `status` отсутствует в body | 400 | `{"error": "missing field: status"}` | `POST /jobs/{id}/status` |
| `git_ref` отсутствует | 200 | Pipeline с `git_ref = "main"` | `POST /projects/{id}/pipelines` (default) |
| Пустой JSON body `{}` | 400 / 200 | Зависит от endpoint | Обязательные поля проверяются |

## 5. Duplicate Project Name

| Scenario | HTTP | Response body | Описание |
|----------|------|---------------|----------|
| Создание проекта с существующим `name` | 500 | `{"error": "internal server error"}` | Unique constraint violation на `projects_name_key` |

> **Текущее поведение:** нарушение unique constraint обрабатывается как `500 Internal Server Error`, а не `400`/`409`. Запланировано улучшение в Phase 2 — перехват `sqlx::Error::Database` с проверкой кода `23505` (unique_violation) и возврат `400` с сообщением `{"error": "project name already exists"}`.

```rust
// Планируемая реализация (Phase 2):
match err {
    sqlx::Error::Database(db_err) if db_err.code() == Some("23505") => {
        ApiError::BadRequest("project name already exists".into())
    }
    _ => ApiError::Internal,
}
```

## 6. Empty Logs

| Scenario | HTTP | Response body | Описание |
|----------|------|---------------|----------|
| Job без логов | 200 | `[]` | Пустой массив, нормальное поведение |
| Job с логами | 200 | `[{...}, {...}]` | Массив отсортирован по `sequence` |
| Удалённый job (CASCADE) | 200 | `[]` | Логи удалены вместе с job |

## 7. Database Unavailable

`AppState` содержит `Option<PgPool>`. Когда пул равен `None` (БД недоступна при старте), все endpoint, кроме `/health`, возвращают `503`.

| Scenario | HTTP | Response body | Endpoint |
|----------|------|---------------|----------|
| БД недоступна при старте | 503 | `{"error": "database unavailable"}` | Все, кроме `/health` |
| `GET /health` без БД | 200 | `{"status": "ok", "service": "cicd"}` | Health не требует БД |
| БД упала во время работы | 500 | `{"error": "internal server error"}` | Ошибка выполнения запроса |
| Connection pool exhausted | 500 | `{"error": "internal server error"}` | Timeout получения соединения |

> **Планируется (Phase 2+):** разделить `500` (внутренняя ошибка) и `503` (БД временно недоступна) через проверку `sqlx::Error::PoolTimedOut` и `sqlx::Error::Io`.

## 8. Race Conditions

| Scenario | Behavior |
|----------|----------|
| Два лога одновременно для одного job | Один успешно записан, второй — `500` (UNIQUE violation на `(job_id, sequence)`) |
| Два запроса на смену статуса одного job | Первый применяет transition, второй может получить `400` (терминальный) или `200` (если переход всё ещё валиден) |
| Удаление project во время GET pipelines | Возможно получение пустого результата или `503` |

## 9. Request Format Edge Cases

| Input | HTTP | Response body | Описание |
|-------|------|---------------|----------|
| `Content-Type: text/plain` | 400 | `{"error": "invalid content type"}` | Ожидается `application/json` |
| Невалидный JSON | 400 | `{"error": "invalid JSON body"}` | Ошибка десериализации |
| JSON с лишними полями | 200 | Игнорируется | `serde` игнорирует неизвестные поля по умолчанию |
| Очень большой body (> 1 MB) | 413 / 500 | Зависит от конфигурации Axum | Default body limit в Axum — 2 MB |

## References

- `docs/API.md` — полная спецификация endpoint.
- `docs/DATA_MODEL.md` — схема БД и state machine статусов.
- `docs/API_STANDARDS.md` — стандарты REST API.
- `docs/TROUBLESHOOTING.md` — диагностика проблем.
- `backend/src/api.rs` — реализация endpoint и обработка ошибок.
- `backend/src/domain.rs` — правила переходов статусов.