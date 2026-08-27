# Transition map: legacy → target

> **Статус:** нормативная карта замен. Владелец изменений — phases в `docs/ROADMAP.md`; канон имён — ADR-0009.

## HTTP routes

| Legacy (current) | Target | Механика перехода | Gate удаления |
|---|---|---|---|
| `POST /api/v1/internal/git-push` | `POST /api/v1/internal/git-events/push` | new route + adapter, old route отвечает 308/deprecation header 1 release | usage=0 по метрикам |
| `/api/v1/*` array responses | envelope `{items,next_cursor}` | аддитивно, `?limit&cursor`; array сохраняется до миграции UI/CLI (`contracts/API_CONTRACT.md`) | все клиенты на envelope |
| `/git/*` token-auth `CICD_GIT_TOKEN` | per-repo authorization + signed events (reserved ADR-0013) | feature flag `git_auth_v2` | flag on by default |
| unprotected platform routes | policy middleware (`contracts/AUTHZ_CONTRACT.md`) | flags `auth_required_*` по группам роутов | Phase D complete |

## Schema

| Legacy | Target | Переход |
|---|---|---|
| `store::migrate()` bootstrap | `backend/migrations/` + `cicd-migrate` | baseline adoption (fingerprint), см. `contracts/MIGRATION_CONTRACT.md` |
| `runners` (CRUD registry) | + credentials/capacity/leases (`job_leases`) | аддитивные колонки + новые таблицы |
| `schedules`/`webhooks`/`notification_channels` (config) | + `domain_events`/`outbox_messages`/`outbox_deliveries` | новые таблицы, конфиг не переносится |
| `users`/`api_tokens` (storage) | + `tenants`/sessions/service_accounts | `0003_auth_foundation.sql` последовательность (`AUTH_IMPLEMENTATION_SPEC.md`) |

## Execution model

| Legacy | Target |
|---|---|
| embedded runner в cicd-server | внешний runner через `/api/v1/runner/*` (register/poll/ack/renew/complete) |
| jobs.status только | + `execution_attempts`, `job_queue` |
| логи только поллингом | SSE + поллинг fallback |

## Frontend

| Legacy | Target |
|---|---|
| hand-written типы `src/api` | generated `src/shared/api/generated/` |
| login-заглушка | `/api/v1/auth/*` + RequireAuth |
| `window.confirm` | AlertDialog |
| tables на mobile | card/list layout |

## Правила

1. Каждая замена — feature flag + метрики использования legacy + deprecation notice (`Sunset` header / CLI stderr).
2. Удаление legacy — только отдельным PR с evidence usage=0.
3. Обратная совместимость JSON не нарушается до явного versioning-решения (`contracts/API_CONTRACT.md`).
