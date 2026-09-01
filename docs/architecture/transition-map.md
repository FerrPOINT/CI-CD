# Transition map: current compatibility → target

> **Статус:** нормативная карта замен. Владелец изменений — phases в `docs/ROADMAP.md`; канон имён — ADR-0009.

## HTTP routes

| Current compatibility surface | Target | Механика перехода | Gate удаления |
|---|---|---|---|
| `POST /api/v1/internal/git-push` | `POST /api/v1/internal/git-events/push` | new route + adapter, old route отвечает 308/deprecation header 1 release | usage=0 по метрикам |
| `/api/v1/*` array responses | envelope `{items,next_cursor}` | аддитивно, `?limit&cursor`; array сохраняется до миграции UI/CLI (`contracts/API_CONTRACT.md`) | все клиенты на envelope |
| `/git/*` public-read + legacy `CICD_GIT_TOKEN`/JWT/PAT project-membership checks | tenant-bound per-repo authorization + scoped Git credentials + signed events (reserved ADR-0013) | feature flag `git_auth_v2` | flag on by default |
| trusted-network fallback when `CICD_AUTH_SECRET` is unset | default-deny policy middleware (`contracts/AUTHZ_CONTRACT.md`) | flags `auth_required_*` по группам роутов + deployment gate for configured secret | Phase D complete |

## Schema

| Current compatibility surface | Target | Переход |
|---|---|---|
| runtime SQLx migrator on startup | pre-start migration job + `cicd-migrate` verify/adopt tooling | split apply/verify modes; legacy `store::migrate()` adoption is only for pre-migration installations |
| `runners` (CRUD registry) | + credentials/capacity/leases (`job_leases`) | аддитивные колонки + новые таблицы |
| `schedules`/`webhooks`/`notification_configs` (config) + current outbox delivery rows | current `domain_events`/`outbox_messages`/`outbox_delivery_attempts`; target `outbox_deliveries` leases/snapshots | add target tables/columns without rewriting immutable current history |
| `users`/`api_tokens`/`sessions` (global auth) | + `tenants`/project memberships/service_accounts | additive tenant/project scope migrations (`AUTH_IMPLEMENTATION_SPEC.md`) |

## Execution model

| Legacy | Target |
|---|---|
| embedded runner в cicd-server | внешний runner через `/api/v1/runner/*` (register/poll/ack/renew/control/`secrets:resolve`/artifacts/logs/complete) |
| jobs.status только | + `execution_attempts`, `job_queue` |
| логи только поллингом | SSE + поллинг fallback |

## Frontend

| Legacy | Target |
|---|---|
| generated DTO schema in `frontend/src/api/schema.d.ts`, hand-written transport wrappers | generated transport boundary |
| conditional login flow (`/api/v1/auth/*` when `CICD_AUTH_SECRET` is set) | default RequireAuth + tenant/project-aware session policy |
| `window.confirm` | AlertDialog |
| tables на mobile | card/list layout |

## Правила

1. Каждая замена — feature flag + метрики использования legacy + deprecation notice (`Sunset` header / CLI stderr).
2. Удаление legacy — только отдельным PR с evidence usage=0.
3. Обратная совместимость JSON не нарушается до явного versioning-решения (`contracts/API_CONTRACT.md`).
