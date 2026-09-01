# План закрытия реализации архитектуры

Дата: 2026-08-27. Основание: независимое ревью AUTHORIZATION, RUNNER/AUTOMATION, STORAGE/DELIVERY и проверка исходников.

> **Статус 2026-09-01:** исторический bootstrap-план. Исполнимые contracts уже созданы и включены в архитектурный индекс; часть перечисленного стала current MVP. Для актуального исполнения брать `docs/CURRENT_STATE.md`, `docs/TRACEABILITY.md`, `docs/ROADMAP.md` и соответствующий contract, а не этот список.

## Вердикт

Целевые документы полны на уровне design/ADR, но до этой доработки не были достаточно детерминированными для первого production implementation PR. Отсутствовали исполнимые contracts для migrations, auth, runner wire protocol, outbox и OpenAPI/codegen.

## Фиксируемые решения

1. `docs/IMPLEMENTATION_CONTRACTS.md`: source-of-truth матрица, принципы совместимости, canonical naming, общие error/pagination/idempotency правила.
2. `docs/MIGRATION_EXECUTION_SPEC.md`: SQLx CLI/version, schema ownership, compose test DB, baseline adoption/fingerprint, runner package and CI command contracts.
3. `docs/AUTH_IMPLEMENTATION_SPEC.md`: auth DTO/headers/cookies/JWT keyring, route policy inventory, flags, migration order, acceptance suite.
4. `docs/EXECUTION_AUTOMATION_IMPLEMENTATION_SPEC.md`: external runner JSON protocol, canonical DDL/lease tables, state matrices, outbox/scheduler parameters, idempotency and minimal rollout scope.
5. Existing target docs retain rationale; these four documents become implementer-level normative contracts and are added to ARCHITECTURE_INDEX.

## Delivery order

1. Versioned migrations + test DB harness.
2. Canonical error envelope / OpenAPI source and type generation.
3. Auth foundation and route policy inventory; then human sessions; then API tokens.
4. Outbox and schedule-only worker.
5. External Docker runner protocol, no secrets/artifacts in first external runner release.
6. Secret injection/artifacts/webhook delivery only after runner lease/reconciliation is proven.

## Non-goals / remaining target после current MVP

- Kubernetes executor, S3 backend, production sandbox/resource policy, external email/Slack adapters and inbound provider webhook handlers.
- Legacy token dual acceptance beyond documented sunset window.
- Pagination migration of all existing array endpoints before additive `/v1` collection contract has an adopted compatibility path.
