# ADR-0006: Надёжные асинхронные действия через PostgreSQL outbox

## Status

Accepted (current MVP partially implemented; full target pending)

## Context

Pipeline execution, Git push, schedule, deployment and administration создают побочные эффекты: dispatch runner-а, webhook, notification, realtime projection, отчёты. Непосредственный HTTP вызов после SQL commit приводит к двум ошибкам: commit может пройти, а доставка потеряться при crash; повтор endpoint-а может доставить событие дважды.

## Decision

Каждая application-команда в своей PostgreSQL-транзакции записывает изменение aggregate, append-only audit entry, `domain_events` и `outbox_messages` с immutable payload. Background workers claim due messages, доставляют идемпотентно и фиксируют attempt/result. Current MVP использует `outbox_messages` как delivery row, хранит bounded attempt history в `outbox_delivery_attempts` и requeue failed rows новой generation; target full dispatcher может добавить `outbox_deliveries` snapshots/leases. Consumer обязан выдерживать at-least-once delivery; внешние requests получают stable idempotency key `event_id`.

Outbox является источником для webhook/notification delivery, runner dispatch projection, SSE projection, schedule processing and report projection. Он не заменяет business tables и не используется для синхронного HTTP response.

## Consequences

- Versioned migrations для `domain_events`, `outbox_messages`, pending indexes и current `outbox_delivery_attempts` уже есть; target остаётся для `outbox_deliveries` snapshots/leases.
- Current worker покрывает basic retry/backoff/history/requeue; production dead-letter policy, metrics, leases/fencing и reconciliation ещё нужны.
- Появляется небольшая eventual consistency, которую UI показывает как pending/delivery state.
- До реализации внешние adapters/handlers и любые configuration-only настройки не считаются активной external automation capability.

## Related

- `docs/FUNCTIONAL_ARCHITECTURE.md`
- `docs/AUTOMATION_ARCHITECTURE.md`
- `docs/RUNNER_ARCHITECTURE.md`
- `docs/EVENTS.md`
