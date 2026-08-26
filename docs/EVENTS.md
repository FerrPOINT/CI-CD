# Events — SSE-события Forge CI/CD

## 1. Overview

План Phase 6: real-time push-уведомления о смене статусов пайплайнов и задач, а также о добавлении логов в Dashboard через Server-Sent Events (SSE). SSE заменяет polling и обеспечивает мгновенное обновление UI без перезагрузки страницы.

> **Статус:** Planned (Phase 6). Не реализовано. См. `docs/ROADMAP.md`, `docs/NOTIFICATIONS.md`.

---

## 2. Архитектура

```
┌──────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  Axum API    │────▶│  Event Bus       │────▶│  SSE Stream      │
│  (producer)  │     │  (broadcast)     │     │  (consumer)      │
└──────────────┘     └──────────────────┘     └────────┬─────────┘
                                                        │
                                              ┌─────────▼──────────┐
                                              │  Dashboard (React) │
                                              │  EventSource API   │
                                              │  + TanStack Query  │
                                              └────────────────────┘
```

### Принципы

- SSE (Server-Sent Events) — однонаправленный поток от сервера к клиенту по HTTP.
- Backend публикует события в in-process event bus (`tokio::sync::broadcast`).
- Frontend подключается через `EventSource` API.
- При получении события frontend инвалидирует связанные TanStack Query keys.
- Для multi-instance deployment — Redis Pub/Sub для broadcast между инстансами (Future).

---

## 3. Endpoint

```
GET /api/v1/events/stream
Accept: text/event-stream
```

- Long-lived HTTP-соединение.
- `Content-Type: text/event-stream`.
- `Cache-Control: no-cache`.
- Keep-alive ping каждые 15 секунд (предотвращает timeout proxy).

### Реализация (Axum)

```rust
use axum::response::sse::{Event, Sse, KeepAlive};
use futures::stream::Stream;

async fn event_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_bus.subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await.ok()?;
        Some((
            Ok(Event::default()
                .event(event.type_)
                .json_data(event.payload)
                .unwrap()),
            rx,
        ))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

> См. `docs/NOTIFICATIONS.md` раздел 4.4.

---

## 4. Формат event stream

SSE-протокол: каждый_event — текстовый блок, разделённый двойным переводом строки.

```
event: pipeline.status.changed
data: {"pipelineId":"a1b2c3d4-...","projectId":"550e8400-...","status":"running","previousStatus":"queued"}

event: job.status.changed
data: {"jobId":"job-uuid-1","pipelineId":"a1b2c3d4-...","stageId":"stage-uuid-1","status":"success","previousStatus":"running"}

event: job.log.appended
data: {"jobId":"job-uuid-1","sequence":5,"message":"Build completed successfully"}

```

### Поля SSE-сообщения

| Поле | Описание |
|------|----------|
| `event:` | Тип события (event name) |
| `data:` | JSON payload события |

---

## 5. Типы событий

### 5.1. `pipeline.status.changed`

Генерируется при смене статуса pipeline (в результате агрегации статусов stages/jobs).

```json
{
  "pipelineId": "a1b2c3d4-...",
  "projectId": "550e8400-...",
  "status": "running",
  "previousStatus": "queued",
  "timestamp": "2026-08-26T10:06:00Z"
}
```

| Поле | Тип | Описание |
|------|-----|----------|
| `pipelineId` | UUID | ID пайплайна |
| `projectId` | UUID | ID проекта |
| `status` | JobStatus | Новый статус |
| `previousStatus` | JobStatus | Предыдущий статус |
| `timestamp` | ISO 8601 | Время события |

### 5.2. `job.status.changed`

Генерируется при смене статуса job (через `POST /jobs/{id}/status`).

```json
{
  "jobId": "job-uuid-1",
  "pipelineId": "a1b2c3d4-...",
  "stageId": "stage-uuid-1",
  "status": "success",
  "previousStatus": "running",
  "timestamp": "2026-08-26T10:06:30Z"
}
```

| Поле | Тип | Описание |
|------|-----|----------|
| `jobId` | UUID | ID задачи |
| `pipelineId` | UUID | ID пайплайна |
| `stageId` | UUID | ID стадии |
| `status` | JobStatus | Новый статус |
| `previousStatus` | JobStatus | Предыдущий статус |
| `timestamp` | ISO 8601 | Время события |

### 5.3. `job.log.appended`

Генерируется при добавлении строки лога (через `POST /jobs/{id}/logs`).

```json
{
  "jobId": "job-uuid-1",
  "pipelineId": "a1b2c3d4-...",
  "sequence": 5,
  "message": "Build completed successfully",
  "timestamp": "2026-08-26T10:06:31Z"
}
```

| Поле | Тип | Описание |
|------|-----|----------|
| `jobId` | UUID | ID задачи |
| `pipelineId` | UUID | ID пайплайна |
| `sequence` | integer | Порядковый номер лога |
| `message` | string | Текст лога |
| `timestamp` | ISO 8601 | Время события |

---

## 6. Инвалидация TanStack Query на фронте

### 6.1. Подключение EventSource

```typescript
// frontend/src/shared/api/events.ts
import { useQueryClient } from '@tanstack/react-query'

const KEYS = {
  projects: ['projects'] as const,
  pipelines: (projectId: string) => ['pipelines', projectId] as const,
  pipeline: (id: string) => ['pipeline', id] as const,
  logs: (jobId: string) => ['logs', jobId] as const,
}

export function connectSSE(queryClient: QueryClient) {
  const eventSource = new EventSource('/api/v1/events/stream')

  eventSource.addEventListener('pipeline.status.changed', (e) => {
    const data = JSON.parse(e.data)
    queryClient.invalidateQueries({ queryKey: KEYS.pipeline(data.pipelineId) })
    queryClient.invalidateQueries({ queryKey: KEYS.pipelines(data.projectId) })
  })

  eventSource.addEventListener('job.status.changed', (e) => {
    const data = JSON.parse(e.data)
    queryClient.invalidateQueries({ queryKey: KEYS.pipeline(data.pipelineId) })
    queryClient.invalidateQueries({ queryKey: KEYS.logs(data.jobId) })
  })

  eventSource.addEventListener('job.log.appended', (e) => {
    const data = JSON.parse(e.data)
    queryClient.invalidateQueries({ queryKey: KEYS.logs(data.jobId) })
  })

  eventSource.onerror = () => {
    // Auto-reconnect: EventSource восстанавливает соединение автоматически.
    // Браузер retry interval: 3 секунды (default).
  }

  return () => eventSource.close()
}
```

### 6.2. Маппинг событий → query keys

| Событие | Инвалидируемые query keys |
|---------|---------------------------|
| `pipeline.status.changed` | `['pipeline', pipelineId]`, `['pipelines', projectId]` |
| `job.status.changed` | `['pipeline', pipelineId]`, `['logs', jobId]` |
| `job.log.appended` | `['logs', jobId]` |

### 6.3. Дополнительно: toast-уведомления

```typescript
eventSource.addEventListener('pipeline.status.changed', (e) => {
  const data = JSON.parse(e.data)
  queryClient.invalidateQueries({ queryKey: KEYS.pipeline(data.pipelineId) })

  if (data.status === 'failed') {
    toast.error(`Pipeline failed`)
  } else if (data.status === 'success') {
    toast.success(`Pipeline succeeded`)
  }
})
```

> См. `docs/NOTIFICATIONS.md` раздел 7.2.

---

## 7. Гарантии доставки

| Характеристика | Значение |
|---|---|
| Delivery semantics | At-most-once (SSE — fire-and-forget) |
| Reconnect | EventSource auto-reconnect (3s default) |
| Ordering | В порядке генерации в рамках соединения |
| Backpressure | Нет (broadcast отбрасывает при переполнении) |
| Идемпотентность | Клиент идемпотентен через query invalidation |

> SSE не гарантирует доставку. Для критичных уведомлений (email, SMS) используются webhook-доставки с retry (см. `docs/WEBHOOKS.md`, `docs/NOTIFICATIONS.md`).

---

## 8. Multi-instance (Future)

При multi-instance deployment:

1. Backend публикует события в Redis Pub/Sub.
2. Каждый инстанс подписывается на Redis channel.
3. SSE-stream каждого инстанса транслирует события из Redis своим клиентам.

```
┌──────────┐  publish  ┌─────────┐  subscribe  ┌──────────┐  SSE  ┌──────────┐
│ Backend A │──────────▶│  Redis  │────────────▶│ Backend B │──────▶│ Client 1 │
└──────────┘           │ Pub/Sub │────────────▶│ Backend C │──────▶│ Client 2 │
                       └─────────┘            └──────────┘       └──────────┘
```

> Требует отдельный ADR для Redis (см. ADR-0004, Consequences).

---

## 9. Аутентификация SSE (Phase 1+)

- После внедрения auth (Phase 1) SSE endpoint требует JWT-токен.
- `EventSource` не поддерживает custom headers → токен передаётся через query param:
  ```
  GET /api/v1/events/stream?token=<jwt>
  ```
- Или через cookie (httpOnly refresh cookie) — если SSE и API на одном origin.

---

## 10. References

- `docs/NOTIFICATIONS.md` — система уведомлений, SSE-канал.
- `docs/WEBHOOKS.md` — webhook-уведомления (исходящие).
- `docs/CACHING.md` — инвалидация кеша при SSE-событиях.
- `docs/FRONTEND_ARCHITECTURE.md` — TanStack Query конфигурация.
- `docs/WORKFLOW.md` — статусная модель пайплайнов.
- `docs/ROADMAP.md` — Phase 6: Webhooks + Notifications.