# Notifications — Уведомления Forge CI/CD

## 1. Обзор

План Phase 6: система уведомлений о смене статуса пайплайнов и задач. Каналы доставки: email, SMS, in-app alerts, SSE (Server-Sent Events) для real-time push в Dashboard.

> **Статус:** MVP реализовано хранение конфигурации каналов (`notification_configs` + `GET/PUT /projects/{id}/notifications` + UI на странице Webhooks). Не реализовано: реальная доставка уведомлений, SSE. См. `docs/ROADMAP.md` Phase 6.

---

## 2. Архитектура

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Domain     │────▶│  Event Bus       │────▶│  Notifier       │
│  (status    │     │  (broadcast      │     │  (channel       │
│   change)   │     │   channel)       │     │   dispatcher)   │
└─────────────┘     └──────────────────┘     └────────┬────────┘
                                                       │
                    ┌──────────────────────────────────┼──────────────────────┐
                    ▼                    ▼              ▼                      ▼
              ┌──────────┐       ┌───────────┐  ┌────────────┐       ┌──────────────┐
              │  Email   │       │   SMS     │  │  In-App    │       │  SSE Push    │
              │ (SMTP)   │       │  (Twilio) │  │  (DB)      │       │  (Axum)      │
              └──────────┘       └───────────┘  └────────────┘       └──────────────┘
```

### Компоненты

| Компонент | Описание |
|---|---|
| Event producer | Domain-логика генерирует событие при `transition_to()` |
| Event bus | `tokio::sync::broadcast` in-process, Redis Streams для multi-instance |
| Notifier | Async worker, читает события, диспатчит по каналам |
| Email channel | SMTP-отправка через `lettre` crate |
| SMS channel | HTTP API провайдера (Twilio / SMS.ru) |
| In-app channel | Запись в таблицу `notifications` (БД) |
| SSE channel | Long-lived HTTP-соединение, push в Dashboard |

---

## 3. События

### 3.1. Типы событий

| Событие | Когда | Payload |
|---|---|---|
| `pipeline.queued` | Пайплайн создан | `{pipeline_id, project_id, git_ref}` |
| `pipeline.running` | Пайплайн перешёл в running | `{pipeline_id, started_at}` |
| `pipeline.success` | Пайплайн завершён успешно | `{pipeline_id, finished_at, duration}` |
| `pipeline.failed` | Пайплайн завершён с ошибкой | `{pipeline_id, finished_at, failed_stage, failed_job}` |
| `pipeline.canceled` | Пайплайн отменён | `{pipeline_id, canceled_by}` |
| `job.started` | Задача запущена | `{job_id, pipeline_id, stage_name}` |
| `job.finished` | Задача завершена | `{job_id, status, duration}` |
| `job.failed` | Задача завершена с ошибкой | `{job_id, error_message}` |

### 3.2. Формат события

```json
{
  "eventId": "uuid",
  "eventType": "pipeline.failed",
  "occurredAt": "2026-08-26T12:34:56Z",
  "projectId": "uuid",
  "pipelineId": "uuid",
  "payload": {
    "finishedAt": "2026-08-26T12:35:10Z",
    "failedStage": "build",
    "failedJob": "compile"
  }
}
```

---

## 4. Каналы доставки

### 4.1. Email

**Провайдер:** SMTP (через `lettre` crate).

**Env-переменные:**

| Переменная | Описание |
|---|---|
| `CICD_SMTP_HOST` | SMTP-сервер |
| `CICD_SMTP_PORT` | Порт (обычно 587) |
| `CICD_SMTP_USER` | Имя пользователя |
| `CICD_SMTP_PASSWORD` | Пароль |
| `CICD_SMTP_FROM` | Адрес отправителя |

**Шаблон письма:**

```
Subject: [Forge CI/CD] Pipeline {git_ref} — {status}

Pipeline: {pipeline_id}
Project: {project_name}
Ref: {git_ref}
Status: {status}
Duration: {duration}
Started: {started_at}
Finished: {finished_at}

Link: http://localhost:22802/pipelines/{pipeline_id}
```

**Подписки:** пользователь выбирает события и проекты в настройках профиля.

### 4.2. SMS

**Провайдер:** Twilio (или SMS.ru для RU).

**Env-переменные:**

| Переменная | Описание |
|---|---|
| `CICD_SMS_PROVIDER` | `twilio` / `smsru` |
| `CICD_SMS_API_KEY` | API-ключ провайдера |
| `CICD_SMS_FROM` | Отправитель (номер/имя) |

**Ограничения:**
- Только критические события: `pipeline.failed`, `pipeline.canceled`.
- Не более 10 SMS на пользователя в час (rate limit).
- Текст обрезается до 160 символов.

**Шаблон SMS:**

```
Forge CI/CD: Pipeline {git_ref} FAILED in {project_name}. Stage: {failed_stage}. Details: {url}
```

### 4.3. In-app alerts

**Хранение:** таблица `notifications` в PostgreSQL.

```sql
CREATE TABLE IF NOT EXISTS notifications (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id  UUID REFERENCES projects(id) ON DELETE CASCADE,
    event_type  TEXT NOT NULL,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    read_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**API:**

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/notifications` | Список уведомлений (unread first) |
| `POST` | `/api/v1/notifications/{id}/read` | Отметить прочитанным |
| `POST` | `/api/v1/notifications/read-all` | Отметить все прочитанными |
| `DELETE` | `/api/v1/notifications/{id}` | Удалить |

**Frontend:** bell-иконка в topbar с счётчиком непрочитанных, dropdown со списком.

### 4.4. SSE (Server-Sent Events)

**Назначение:** real-time push статусов в Dashboard без polling.

**Endpoint:**

```
GET /api/v1/events/stream
Accept: text/event-stream
```

**Формат потока:**

```
event: pipeline.failed
data: {"pipelineId":"...","status":"failed","failedStage":"build"}

event: job.started
data: {"jobId":"...","stageName":"test","pipelineId":"..."}
```

**Реализация (Axum):**

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;

async fn event_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_bus.subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await.ok()?;
        Some((Ok(Event::default().event(event.type_).json_data(event.payload).unwrap()), rx))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

**Frontend:** `EventSource` API, `@tanstack/react-query` для инвалидации кэша при получении события.

---

## 5. Подписки и предпочтения

### 5.1. Модель подписок

```sql
CREATE TABLE IF NOT EXISTS notification_subscriptions (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id  UUID REFERENCES projects(id) ON DELETE CASCADE,
    channel     TEXT NOT NULL CHECK (channel IN ('email','sms','in_app','sse')),
    event_types TEXT[] NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_id, project_id, channel)
);
```

### 5.2. Логика фильтрации

1. Событие генерируется при смене статуса.
2. Notifier ищет подписки для `project_id` и `event_type`.
3. Для каждого канала формирует сообщение и отправляет.
4. Неудачные доставки ретраятся (3 попытки, exponential backoff).

### 5.3. Настройки по умолчанию

| Канал | События по умолчанию |
|---|---|
| `in_app` | Все события |
| `sse` | Все события (real-time) |
| `email` | `pipeline.failed`, `pipeline.canceled` |
| `sms` | `pipeline.failed` (только если включено пользователем) |

---

## 6. Гарантии доставки

| Характеристика | Значение |
|---|---|
| Delivery semantics | At-least-once |
| Идемпотентность | По `eventId` |
| Retry | 3 попытки, exponential backoff (1s, 4s, 16s) |
| Dead-letter | Запись в `notification_failures` таблицу после 3 неудач |
| Rate limit | Не более 100 уведомлений на пользователя в час |

---

## 7. Frontend

### 7.1. In-app notifications

- Bell-иконка (`lucide-react Bell`) в topbar с badge непрочитанных.
- Dropdown-панель со списком последних 20 уведомлений.
- Цветовая индикация: `success` — зелёный, `failed` — красный, `canceled` — жёлтый.
- Клик по уведомлению — переход на `/pipelines/{id}`.

### 7.2. SSE подключение

```typescript
const eventSource = new EventSource('/api/v1/events/stream');

eventSource.addEventListener('pipeline.failed', (e) => {
  const data = JSON.parse(e.data);
  queryClient.invalidateQueries({ queryKey: ['pipelines'] });
  toast.error(`Pipeline failed: ${data.failedStage}`);
});

eventSource.addEventListener('pipeline.success', (e) => {
  queryClient.invalidateQueries({ queryKey: ['pipelines'] });
  toast.success('Pipeline succeeded');
});
```

### 7.3. Настройки уведомлений

- Страница `/settings/notifications` с матрицей: проект × событие × канал.
- Сохранение через `PATCH /api/v1/users/me/notification-preferences`.

---

## 8. Env-переменные (план)

| Переменная | Default | Описание |
|---|---|---|
| `CICD_NOTIFICATIONS_ENABLED` | `true` | Глобальный выключатель |
| `CICD_SMTP_HOST` | — | SMTP-сервер |
| `CICD_SMTP_PORT` | `587` | SMTP-порт |
| `CICD_SMTP_USER` | — | SMTP-пользователь |
| `CICD_SMTP_PASSWORD` | — | SMTP-пароль |
| `CICD_SMTP_FROM` | `noreply@cicd.local` | Адрес отправителя |
| `CICD_SMS_PROVIDER` | — | `twilio` / `smsru` |
| `CICD_SMS_API_KEY` | — | API-ключ SMS |
| `CICD_SMS_FROM` | — | Отправитель SMS |
| `CICD_SSE_KEEPALIVE_SECS` | `15` | Интервал SSE keep-alive |

---

## 9. План реализации

- [ ] Таблицы: `notifications`, `notification_subscriptions`, `notification_failures`.
- [ ] Event bus: `tokio::sync::broadcast` канал в `AppState`.
- [ ] Интеграция с domain: вызов `event_bus.send()` в `transition_to()`.
- [ ] Email channel: `lettre` crate, SMTP-отправка, шаблоны.
- [ ] SMS channel: `reqwest` к API провайдера.
- [ ] In-app channel: запись в `notifications`, API CRUD.
- [ ] SSE endpoint: `GET /api/v1/events/stream`, keep-alive.
- [ ] Frontend: bell + dropdown, SSE-подключение, настройки.
- [ ] Тесты: unit (event bus), integration (delivery), e2e (SSE).

---

## References

- `docs/ROADMAP.md` — Phase 6: Webhooks & Notifications
- `docs/WORKFLOW.md` — переходы статусов (источник событий)
- `docs/ARCHITECTURE.md` — `AppState`, слои приложения
- `docs/API.md` — REST API спецификация