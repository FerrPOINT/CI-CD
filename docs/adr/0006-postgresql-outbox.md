# ADR-0006: Надёжные асинхронные действия через PostgreSQL outbox

## Status

Accepted (target architecture; implementation pending)

## Context

Pipeline execution, Git push, schedule, deployment and administration создают побочные эффекты: dispatch runner-а, webhook, notification, realtime projection, отчёты. Непосредственный HTTP вызов после SQL commit приводит к двум ошибкам: commit может пройти, а доставка потеряться при crash; повтор endpoint-а может доставить событие дважды.

## Decision

Каждая application-команда в своей PostgreSQL-транзакции записывает изменение aggregate, append-only audit entry, `domain_events` и `outbox_messages` с immutable payload. Background workers claim due messages, доставляют идемпотентно и фиксируют attempt/result; target delivery history хранится в `outbox_deliveries`. Consumer обязан выдерживать at-least-once delivery; внешние requests получают stable idempotency key `event_id`.

Outbox является источником для webhook/notification delivery, runner dispatch projection, SSE projection, schedule processing and report projection. Он не заменяет business tables и не используется для синхронного HTTP response.

## Consequences

- Нужны versioned migrations: `domain_events`, `outbox_messages`, target `outbox_deliveries` и индексы pending events.
- Нужны worker lifecycle, backoff, dead-letter/retry policy, metrics и reconciliation.
- Появляется небольшая eventual consistency, которую UI показывает как pending/delivery state.
- До реализации старые configuration-only webhooks/schedules не считаются активной automation capability.

## Related

- `docs/FUNCTIONAL_ARCHITECTURE.md`
- `docs/AUTOMATION_ARCHITECTURE.md`
- `docs/RUNNER_ARCHITECTURE.md`
- `docs/EVENTS.md`
