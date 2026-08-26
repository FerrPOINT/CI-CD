# Error Handling — Forge CI/CD

## 1. Overview

Единая стратегия обработки ошибок на backend. Цель: отличать ожидаемые бизнес-ошибки от неожиданных технических, не дать утечь деталям инфраструктуры наружу, дать пользователю понятное сообщение.

## 2. Backend Error Types

### 2.1 ApiError

Основной тип ошибок HTTP-слоя. Определён в `backend/src/api.rs`:

```rust
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unavailable() -> Self { /* 503 */ }
    fn bad_request(message: impl Into<String>) -> Self { /* 400 */ }
    fn not_found() -> Self { /* 404 */ }
    fn internal(error: sqlx::Error) -> Self { /* 500 */ }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({"error": self.message})),
        ).into_response()
    }
}
```

### 2.2 TransitionError

Доменная ошибка валидации переходов статусов. Определена в `backend/src/domain.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("terminal status cannot change")]
    TerminalStatus,
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition { from: JobStatus, to: JobStatus },
}
```

### 2.3 Взаимодействие

```
HTTP Request
    │
    ▼
Api Handler (api.rs)
    │
    ├── валидация ввода ──> ApiError::bad_request()
    ├── domain logic ─────> TransitionError ──> ApiError::bad_request(error.to_string())
    ├── SQLx query ───────> sqlx::Error ──> ApiError::internal()
    └── DB unavailable ──> ApiError::unavailable()
                              │
                              ▼
                        IntoResponse
                              │
                              ▼
                        JSON: {"error": "..."}
```

## 3. HTTP Status Mapping

| Источник | ApiError | HTTP Status | Условие |
|----------|----------|-------------|---------|
| `pool()` — БД недоступна | `unavailable()` | `503 Service Unavailable` | `AppState.pool = None` |
| Пустой `name` или `repository_url` | `bad_request()` | `400 Bad Request` | `trim().is_empty()` |
| Пустой `message` для лога | `bad_request()` | `400 Bad Request` | `trim().is_empty()` |
| Неверный статус задачи | `bad_request()` | `400 Bad Request` | `JobStatus::try_from` failed |
| `TransitionError::TerminalStatus` | `bad_request()` | `400 Bad Request` | terminal → any |
| `TransitionError::InvalidTransition` | `bad_request()` | `400 Bad Request` | недопустимый переход |
| Ресурс не найден | `not_found()` | `404 Not Found` | `fetch_optional` → `None` |
| `sqlx::Error` | `internal()` | `500 Internal Server Error` | любая ошибка SQLx |
| Неверный UUID в path | Axum built-in | `400 Bad Request` | `Path<Uuid>` parse error |
| Неверный JSON body | Axum built-in | `400 Bad Request` | `Json` deserialization error |

## 4. Error Response JSON Shape

Все ошибки возвращают единый формат:

```json
{
  "error": "human-readable message"
}
```

Примеры:

```json
// 503 — БД недоступна
{ "error": "database is unavailable" }

// 400 — невалидный ввод
{ "error": "name and repository_url are required" }

// 400 — недопустимый переход
{ "error": "invalid status transition from Queued to Success" }

// 400 — terminal status
{ "error": "terminal status cannot change" }

// 404 — ресурс не найден
{ "error": "resource not found" }

// 500 — ошибка БД
{ "error": "constraint violation: duplicate key value ..." }
```

## 5. TransitionError — детали

### 5.1 Конечный автомат статусов

```
                 ┌──────────┐
                 │  queued  │
                 └────┬─────┘
                      │
           ┌──────────┼──────────┐
           ▼          │          ▼
     ┌──────────┐     │     ┌──────────┐
     │ running  │     │     │ canceled │ (terminal)
     └──┬───┬───┘     │     └──────────┘
        │   │        │
        ▼   ▼        │
  ┌────────┐ ┌─────┐ │
  │success │ │failed│ │
  │(term.) │ │(term)│ │
  └────────┘ └─────┘ │
                      │
              cancelled from queue
```

### 5.2 Варианты TransitionError

| Variant | Условие | Сообщение |
|---------|---------|-----------|
| `TerminalStatus` | переход из `success`, `failed` или `canceled` | `terminal status cannot change` |
| `InvalidTransition` | недопустимый переход (например, `queued → success`) | `invalid status transition from {from:?} to {to:?}` |

### 5.3 Допустимые переходы

| Из → В | Результат |
|--------|-----------|
| `queued → queued` | `Ok(Queued)` (no-op) |
| `queued → running` | `Ok(Running)` |
| `queued → canceled` | `Ok(Canceled)` |
| `running → running` | `Ok(Running)` (no-op) |
| `running → success` | `Ok(Success)` |
| `running → failed` | `Ok(Failed)` |
| `running → canceled` | `Ok(Canceled)` |
| `success → *` | `Err(TerminalStatus)` |
| `failed → *` | `Err(TerminalStatus)` |
| `canceled → *` | `Err(TerminalStatus)` |
| `queued → success` | `Err(InvalidTransition)` |
| `queued → failed` | `Err(InvalidTransition)` |

## 6. Backend Error Handling Patterns

### 6.1 Получение пула соединений

```rust
fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.pool.as_ref().ok_or_else(ApiError::unavailable)
}
```

Если БД недоступна — `503 Service Unavailable` для всех endpoint кроме `/health`.

### 6.2 Валидация ввода

```rust
if input.name.trim().is_empty() || input.repository_url.trim().is_empty() {
    return Err(ApiError::bad_request("name and repository_url are required"));
}
```

### 6.3 Domain transition

```rust
let current = JobStatus::try_from(job.status.as_str())
    .map_err(ApiError::bad_request)?;
current
    .transition_to(input.status)
    .map_err(|error| ApiError::bad_request(error.to_string()))?;
```

### 6.4 SQLx errors

```rust
let project = sqlx::query_as::<_, Project>(...)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
```

Все `sqlx::Error` маппятся в `500 Internal Server Error`. В текущей версии сообщение об ошибке SQLx передаётся в ответ как есть (в production — заменить на generic message + логирование).

### 6.5 Not found

```rust
let job = sqlx::query_as::<_, Job>(...)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
```

## 7. Health-check — особый случай

Endpoint `GET /api/v1/health` не возвращает ошибок. Всегда `200 OK`:

```json
{ "status": "ok", "service": "cicd" }
```

Health-check не требует подключения к БД — используется для Docker healthcheck и load balancer probes.

## 8. Frontend Error Handling

### 8.1 API Client

```ts
// frontend/src/api/client.ts
export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, { ... })
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }))
    throw new Error(body.error || response.statusText)
  }
  return response.json() as Promise<T>
}
```

### 8.2 TanStack Query

```ts
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: (failureCount, error) => {
        // retry only on 5xx
        return failureCount < 3
      },
    },
    mutations: {
      onError: (error) => {
        toast.error(error.message)
      },
    },
  },
})
```

### 8.3 Error Display

- Ошибки мутаций (job start/pass/fail, create project) — через `sonner` toast.
- Ошибки загрузки (query) — в компоненте: retry button + error message.
- Ошибки переходов статусов — показываются пользователю с пояснением правила.

## 9. Планируемые улучшения

### 9.1 Structured error response

```json
{
  "error": {
    "code": "INVALID_TRANSITION",
    "message": "invalid status transition from Queued to Success",
    "requestId": "req-uuid"
  }
}
```

### 9.2 Error codes

| Code | HTTP | Описание |
|------|------|----------|
| `VALIDATION_ERROR` | 400 | Ошибка валидации ввода |
| `UNAUTHORIZED` | 401 | Не аутентифицирован |
| `FORBIDDEN` | 403 | Нет прав |
| `NOT_FOUND` | 404 | Ресурс не найден |
| `INVALID_TRANSITION` | 400 | Недопустимый переход статуса |
| `TERMINAL_STATUS` | 400 | Переход из терминального статуса |
| `CONFLICT` | 409 | Конфликт (duplicate key) |
| `SERVICE_UNAVAILABLE` | 503 | БД недоступна |
| `INTERNAL_ERROR` | 500 | Внутренняя ошибка |

### 9.3 Middleware

- `TraceLayer` — логирует все запросы с request_id (уже подключён).
- Глобальный обработчик: `error` логируется с полным контекстом; в ответе — без stack trace.
- `CatchPanicLayer` — превращает panic в 500 (планируется).

## References

- `backend/src/api.rs` — `ApiError`, `IntoResponse`, хендлеры.
- `backend/src/domain.rs` — `TransitionError`, `JobStatus::transition_to`.
- `docs/API.md` — спецификация API, коды ответов.
- `docs/TESTING.md` — тестирование переходов статусов.