# Нормативные контракты реализации

**Статус:** Accepted target implementation contract. При конфликте этот документ приоритетнее narrative-разделов в `AUTHORIZATION.md`, `RUNNER_ARCHITECTURE.md`, `AUTOMATION_ARCHITECTURE.md`, `STORAGE_ARCHITECTURE.md` и `DELIVERY_ARCHITECTURE.md`.

## 1. Source of truth и совместимость

| Артефакт | Канонический источник |
|---|---|
| HTTP public API | `openapi/openapi.yaml`, генерируемый `cicd-api` через utoipa |
| Internal/runner API | отдельные OpenAPI tags в том же spec, `x-forge-internal: true` |
| Async payload | типы Rust в `backend/domain`; JSON Schema snapshots в `contracts/events/` |
| Схема БД | `backend/migrations/*.sql`, применённые `cicd-migrate` |
| Frontend transport types | `frontend/src/api/schema.d.ts` — только generation |
| Domain lifecycle | `cicd-domain` state transition types |

Существующие `/api/v1` array responses не меняются breakingly. Pagination добавляется аддитивно с `?limit=&cursor=` и envelope **только** после OpenAPI operation/versioning PR; до этого endpoint сохраняет array response.

## 2. Канонические имена

- Runtime pipeline остаётся `pipelines`; job остаётся `jobs`.
- Каждое выполнение job — `execution_attempts`; очередь — `job_queue`; активная или завершённая выдача — `job_leases`.
- Event journal — `domain_events`; transactional outbox — `outbox_messages`; внешняя попытка доставки — `outbox_deliveries`.
- Нельзя вводить `outbox_events`, `pipeline_runs` или `job_runs` как дубликаты этих сущностей.

## 3. Error envelope

Все новые/изменённые v1 operations возвращают:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Human-readable safe message",
    "request_id": "uuid",
    "details": [{"field":"name","code":"required","message":"Required"}]
  }
}
```

`details` опционален; `request_id` обязателен. `message` не содержит SQL, secret, token, path или stack trace. `401` содержит `WWW-Authenticate: Bearer`; rate limit — `429` и integer `Retry-After`.

| HTTP | Stable code |
|---|---|
| 400 | `invalid_request`, `invalid_cursor` |
| 401 | `authentication_required`, `invalid_credential`, `credential_expired` |
| 403 | `permission_denied` |
| 404 | `not_found` |
| 409 | `conflict`, `idempotency_conflict`, `lease_fenced` |
| 422 | `validation_failed` |
| 429 | `rate_limited` |
| 500 | `internal_error` |
| 503 | `dependency_unavailable` |

## 4. Pagination

После миграции collection contract: `GET ?limit=50&cursor=<opaque>` → `{ "items": [...], "next_cursor": "..." }`.

- Default `limit=50`, `max=200`; invalid/out-of-range limit → 422, invalid/expired cursor → 400.
- Cursor is base64url JSON `{v,sort,id}` and HMAC-SHA256 signed with `CICD_CURSOR_KEY`; TTL 24h.
- Sorting is fixed per endpoint and includes unique `id` tie-breaker. No user-provided SQL sort field in first release.
- Legacy arrays are preserved until all CLI/UI clients consume the envelope; compatibility is an explicit OpenAPI versioning decision.

## 5. Idempotency

Mutation requests that may be retried accept `Idempotency-Key` UUID. Table `idempotency_keys` has `(principal_id, route, key)` unique, SHA-256 request hash, status and stored response for 24 hours. Same key + different request hash → 409 `idempotency_conflict`.

Scheduler uses `(schedule_id, scheduled_for)`; Git ingress uses `(source, delivery_id)`; original webhook fan-out uses `(subscription_id, event_id, generation=0)`; replay creates incremented `generation` and references `replay_of_delivery_id`.

## 6. Required checks before a feature flag is enabled

1. Versioned migration + downgrade/forward recovery runbook reviewed.
2. OpenAPI operation, examples, generated TS types and route-policy record committed.
3. Real PostgreSQL tests include success, auth/tenant negative, duplicate/retry and restart cases.
4. Metrics and audit events assertable in tests.
5. Feature flag defaults `false` in production; enablement evidence attached to release.
