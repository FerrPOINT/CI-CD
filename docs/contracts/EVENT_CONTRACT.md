# Контракт событий и доставок

Статус: Accepted target contract. Основание: [ADR-0009](../adr/0009-canonical-registry.md).

Этот контракт определяет целевое наблюдаемое поведение автоматизации. Текущий MVP уже доставляет outgoing webhooks и local `in_app`/`sse` notifications в ограниченных границах, хранит bounded delivery attempts в `outbox_delivery_attempts` и поддерживает requeue failed `outbox_messages` новой generation, но не реализует полный target-контракт lease/reconciliation/dead-letter policy, внешние adapters и общую delivery-модель.

## 1. Границы и хранение

- Все межконтекстные сообщения проходят только через immutable `domain_events` и transactional `outbox_messages`; внешние попытки и их результаты хранятся в `outbox_deliveries`.
- Изменение агрегата, audit event, `domain_events` и `outbox_messages` фиксируются одной PostgreSQL-транзакцией. Внешний I/O внутри этой транзакции запрещён.
- `pg_notify` и in-process broadcast допустимы только как ускорители; после restart worker обязан читать pending записи из БД.
- Payload, event type, occurred time и причинные идентификаторы после commit не изменяются. Исправление создаёт новое событие.
- Каждый project-scoped event принадлежит одному `tenant`; worker обязан проверять tenant/project scope до fan-out.

## 2. Envelope

Каждый domain event и внешний webhook body передают один и тот же UTF-8 JSON envelope:

```json
{
  "id": "uuid",
  "type": "forge.pipeline.finished.v1",
  "schema_version": 1,
  "occurred_at": "2026-08-27T12:34:56.123Z",
  "tenant_id": "uuid",
  "project_id": "uuid",
  "aggregate": {"type": "pipeline", "id": "uuid"},
  "correlation_id": "uuid",
  "causation_id": "uuid",
  "actor": {"type": "system", "id": "runner"},
  "data": {"status": "failed", "resolved_sha": "..."}
}
```

| Поле | Требование |
|---|---|
| `id` | UUID, создаётся один раз; используется как стабильный event id. |
| `type` | Только `forge.<domain>.<action>.vN`, где `N` -- версия несовместимого payload. |
| `schema_version` | Положительное целое; соответствует схеме `data`. |
| `occurred_at` | RFC 3339 UTC времени факта, а не времени доставки. |
| `tenant_id` | UUID владельца; обязателен для каждого события. |
| `project_id` | UUID project-scoped события, иначе `null`. |
| `aggregate` | Объект `type` и nullable UUID `id`; не подменяет tenant/project scope. |
| `correlation_id` | UUID всего сценария; обязателен. |
| `causation_id` | UUID непосредственного предшествующего события либо `null` для первичного намерения. |
| `actor` | `user`, `api_token`, `service_account`, `runner` или `system` со стабильным ID. |
| `data` | Строго валидируется для `type`; не содержит secret, token, raw header, credential или неограниченный внешний body. |

JSON Schema каждого типа хранится вместе с доменным кодом. Добавление optional поля совместимо в пределах `vN`; удаление, переименование, изменение типа/семантики или обязательности требует нового event type `.v(N+1)`.

## 3. Каталог domain events

| Тип | `aggregate.type` | Обязательные поля `data` |
|---|---|---|
| `forge.git.push.received.v1` | `inbound_delivery` | `delivery_id`, `repository_id`, `old_sha`, `new_sha`, `ref`, `received_at` |
| `forge.git.push.reconciled.v1` | `repository_ref` | `repository_id`, `old_sha`, `new_sha`, `ref`, `reconciliation_run_id` |
| `forge.schedule.triggered.v1` | `schedule_fire` | `schedule_id`, `scheduled_for`, `git_ref`, `trigger_key` |
| `forge.schedule.skipped.v1` | `schedule_fire` | `schedule_id`, `scheduled_for`, `reason` |
| `forge.schedule.fire_skipped_dst.v1` | `schedule` | `schedule_id`, `local_time`, `timezone` |
| `forge.pipeline.queued.v1` | `pipeline` | `pipeline_id`, `repository_id`, `requested_ref`, `resolved_sha`, `trigger_type` |
| `forge.pipeline.started.v1` | `pipeline` | `pipeline_id`, `started_at` |
| `forge.pipeline.finished.v1` | `pipeline` | `pipeline_id`, `status`, `started_at`, `finished_at`, `duration_ms`, `resolved_sha` |
| `forge.job.started.v1` | `job` | `job_id`, `pipeline_id`, `stage_id`, `execution_attempt_id`, `started_at` |
| `forge.job.finished.v1` | `job` | `job_id`, `pipeline_id`, `stage_id`, `execution_attempt_id`, `status`, `finished_at`, `duration_ms` |
| `forge.deployment.finished.v1` | `deployment` | `deployment_id`, `pipeline_id`, `environment_id`, `status`, `finished_at` |
| `forge.webhook.delivered.v1` | `outbox_delivery` | `delivery_id`, `event_id`, `subscription_id`, `attempt_count`, `delivered_at` |
| `forge.webhook.failed.v1` | `outbox_delivery` | `delivery_id`, `event_id`, `subscription_id`, `failure_code`, `attempt_count` |
| `forge.notification.delivered.v1` | `outbox_delivery` | `delivery_id`, `event_id`, `destination_id`, `channel`, `delivered_at` |
| `forge.notification.failed.v1` | `outbox_delivery` | `delivery_id`, `event_id`, `destination_id`, `channel`, `failure_code`, `attempt_count` |
| `forge.automation.reconciliation_found.v1` | `reconciliation_run` | `run_id`, `kind`, `resource_type`, `resource_id`, `finding` |

Технические events о доставке не становятся причиной нового webhook fan-out по умолчанию; это исключает циклы. `forge.webhook.test.v1` разрешён только для явного теста subscription и не имитирует production event.

## 4. Гарантии и идемпотентность

| Граница | Гарантия | Обязательное поведение consumer-а |
|---|---|---|
| Aggregate -> event/outbox | Атомарная локальная запись | Не наблюдать committed aggregate без его события. |
| `outbox_messages` -> workers | At-least-once | Claim через lease и `FOR UPDATE SKIP LOCKED`; просроченный lease повторяется. |
| Webhook и durable notification | At-least-once | Получатель дедуплицирует delivery id и возвращает `2xx` только после durable accept. |
| SSE | At-most-once | Клиент допускает потерю/повтор и восстанавливает состояние через API. |
| Порядок | Стабильный только в пределах одного aggregate consumer-а | Между разными aggregate и deliveries глобальный порядок не гарантируется. |

Exactly-once доставка наружу не заявляется. Идемпотентные ключи обязательны:

| Операция | Ключ/уникальность |
|---|---|
| HTTP mutation | `(principal_id, route, Idempotency-Key)` согласно `docs/IMPLEMENTATION_CONTRACTS.md`. |
| Git ingress | `(source, external_delivery_id)`. |
| Schedule fire | `(schedule_id, scheduled_for)`. |
| Pipeline по trigger | `trigger_event_id` unique. |
| Первичный webhook fan-out | `(subscription_id, event_id, generation=0)`. |
| Replay delivery | Новый `delivery_id`, тот же `event_id`, incremented `generation`, ссылка на исходную delivery. |
| Notification | `(destination_id, event family, pipeline_id, terminal status)` в окне дедупликации; retry не создаёт delivery. |

## 5. Исходящий webhook

Webhook выполняется `POST` без redirect, с точным сериализованным envelope. `https` обязателен в production; URL с userinfo, private/link-local/loopback/metadata destination и неразрешённым redirect отклоняется до доставки.

```text
Content-Type: application/json
User-Agent: Forge-CI-CD-Webhooks/1
X-Forge-Event: forge.pipeline.finished.v1
X-Forge-Event-Id: <event UUID>
X-Forge-Delivery-Id: <delivery UUID>
X-Forge-Timestamp: <RFC 3339 UTC>
X-Forge-Signature-256: sha256=<hex HMAC-SHA-256>
X-Forge-Secret-Version: <integer>
```

Подписываемая строка -- `v1.<timestamp>.<raw-body>`. Secret выбирается по сохранённому в delivery snapshot `secret_version`; сравнение HMAC выполняется constant-time. Получатель обязан ограничить допустимый clock skew, проверить подпись по raw body и дедуплицировать минимум по `X-Forge-Delivery-Id`.

| Результат попытки | Действие |
|---|---|
| `2xx` | `delivered`, больше попыток нет. |
| DNS/connect/read timeout, `408`, `429`, `5xx` | Retry с exponential full-jitter: base 15 секунд, ceiling 1 час, не более 8 попыток; `Retry-After` для `429` соблюдается в разумном лимите. |
| `3xx` | Terminal `failed` с `unexpected_redirect`; redirect не выполняется. |
| Остальные `4xx`, ошибка конфигурации или сериализации | Terminal `failed` без automatic retry. |

Каждая попытка сохраняет number, timestamps, outcome, HTTP status, duration, safe error class и ограниченный sanitised response preview. Secret, `Authorization`, cookies, произвольные request headers и полный response body не сохраняются. Исчерпавшая попытки delivery становится dead-letter (`failed`); alert обязателен, оператор может создать только явный replay/requeue с audit event.

## 6. Уведомления и realtime

| Канал | Модель | Гарантия и ограничение |
|---|---|---|
| `email` | Durable notification delivery через SMTP adapter | At-least-once, idempotency key delivery. |
| `slack_webhook` | Durable delivery через channel adapter | At-least-once; секрет и URL не входят в event. |
| `generic_webhook` | Общий webhook delivery subsystem | Полностью применяются HMAC, retry и dead-letter правила этого контракта. |
| `in_app` | Durable запись для authorised recipient | Создаётся транзакционно/идемпотентно, read state не изменяет domain event. |
| `sse` | Авторизованная ephemeral projection для Dashboard | At-most-once; keep-alive не является подтверждением доставки. |

Rules и destinations project-scoped, наследуют tenant и применяют RBAC. В первом релизе template context ограничен allowlist: project name, pipeline ID/status/URL/ref/SHA, deployment environment и `event.occurred_at`. Success может быть muted или aggregated; `pipeline.failed` и `deployment.finished` с failure не подавляются quiet-hours/digest без явной policy.

Целевой общий SSE передаёт `event: <type>`, `id: <event id>` и `data: <envelope>` на `GET /api/v1/events/stream`. Current MVP даёт project-scoped notification stream на `GET /api/v1/projects/{project_id}/notifications/stream`. Сервер фильтрует поток по authorisation, не обещает replay по `Last-Event-ID`; reconnect должен выполнить API refetch и идемпотентную cache invalidation.

## 7. Проверяемые требования

- Real PostgreSQL tests проверяют атомарность aggregate/event/outbox, duplicate ingress, lease recovery и повтор после crash между HTTP-вызовом и фиксацией результата.
- Contract tests проверяют schema каждого event, отсутствие secret в payload/history, HMAC raw-body verification, retry classification и dead-letter/replay generation.
- SSE tests проверяют tenant filtering, reconnect-safe client refresh и допустимость duplicate/lost event.
- Метрики содержат queue age, delivery attempts/result, dead letters и retry lag; labels не содержат tenant/project ID, URL, event ID или delivery ID.
