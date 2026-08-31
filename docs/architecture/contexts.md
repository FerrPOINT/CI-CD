# Архитектурные контексты (narrative)

> **Статус:** объяснительный документ. Канонические контракты — в `docs/contracts/*`; при конфликте они правы (ADR-0009). Текущее состояние — `docs/CURRENT_STATE.md`.

## Карта контекстов

| Контекст | Responsibility | Contract owner | Narrative |
|---|---|---|---|
| Identity & Access | личности, сессии, RBAC, токены | `contracts/AUTHZ_CONTRACT.md` | `docs/AUTHORIZATION.md` |
| Project & Source | проекты, bare Git, refs, PR | `contracts/PIPELINE_DSL.md` (pipeline) + этот документ | `docs/GIT_HOSTING.md` |
| Execution | очередь, dispatch, runner protocol | `contracts/RUNNER_PROTOCOL.md` | `docs/RUNNER_ARCHITECTURE.md` |
| Automation | schedules, git events, outbox, webhooks, notifications, SSE | `contracts/EVENT_CONTRACT.md` | `docs/AUTOMATION_ARCHITECTURE.md` |
| Storage & lifecycle | Postgres, Git FS, artifacts, secrets, backup | `contracts/DATA_LIFECYCLE.md`, `contracts/MIGRATION_CONTRACT.md` | `docs/STORAGE_ARCHITECTURE.md` |
| Delivery | API, клиенты, observability | `contracts/API_CONTRACT.md`, `contracts/UI_API_CONTRACT.md` | `docs/DELIVERY_ARCHITECTURE.md` |
| Governance | audit, reports | `contracts/AUTHZ_CONTRACT.md` (audit policy) | `docs/REPORTS.md` |

## Границы между контекстами

- Identity не знает про Git; Execution не знает про HTTP-public контракт; Automation читает `domain_events`, но не пишет в агрегаты чужих контекстов.
- Каждый aggregate имеет единственного application-service автора транзакции (см. `docs/FUNCTIONAL_ARCHITECTURE.md` §4).
- Cross-context общение — только через `domain_events` / `outbox_messages` (at-least-once, идемпотентные consumer-ы).

## Текущее состояние контекстов

Соответствует `docs/CURRENT_STATE.md`: Execution — embedded runner; Automation — Git push, schedules, outgoing webhooks и `in_app`/`sse` notifications в MVP, inbound provider webhooks и внешние notification adapters остаются target/config; Identity — conditional auth/RBAC при непустом `CICD_AUTH_SECRET`; остальные контексты — current MVP с указанными ограничениями.

## Переход к target

Последовательность и strangler-порядок: `plans/architecture-rebuild-plan.md` (рабочий non-normative план) + `docs/ROADMAP.md`. Полная карта замен — `docs/architecture/transition-map.md`.
