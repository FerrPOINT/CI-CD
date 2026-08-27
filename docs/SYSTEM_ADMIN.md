# System Administration — Forge CI/CD

## 1. Current MVP

`/admin` — статическая справочная страница инстанса: версия, порты API/Dashboard/PostgreSQL. Она не читает runtime configuration и не является административным API.

Реально работающие административные поверхности вынесены отдельно:

| Surface | Route | API | Current behavior |
|---|---|---|---|
| Users | `/users` | `GET/POST /api/v1/users`, `PATCH /api/v1/users/{id}` | Хранение username, глобальной роли и enabled flag |
| API tokens | `/users` | `GET/POST/DELETE /api/v1/api-tokens` | SHA-256 hash, hint list, значение выдаётся один раз |
| Audit log | `/audit-log` | `GET /api/v1/audit-log` | Последние 200 событий runner/secret/artifact/token |
| Project reports | `/projects/{id}/reports` | `GET /api/v1/projects/{id}/reports/summary` | total/success/failed, success rate, average duration |

> Auth/RBAC пока не enforced: перечисленные endpoint доступны без login. Users и API tokens — модель данных/управление, а не включённая identity system. Целевая граница доступа: `docs/AUTHORIZATION.md`.

## 2. Roles

Таблица `users` принимает роли `admin`, `maintainer`, `developer`, `viewer`; сейчас они хранятся и показываются UI, но не ограничивают запросы. Целевая policy, project membership и scopes определены в `docs/AUTHORIZATION.md`.

## 3. Audit log

Audit append-only на уровне application mutation. Текущая строка содержит action/resource/actor/time без authorisation context или JSON metadata. Аудит не заменяет delivery history webhooks или execution attempts.

## 4. Planned system administration

После auth/RBAC появятся:

- защищённый system overview с runtime/readiness/versions;
- system settings с versioned/validated configuration;
- filters/pagination/audit export;
- metrics and alerts (`docs/DELIVERY_ARCHITECTURE.md`);
- project-scoped access and admin-only policy checks.

Новые `/admin/*` routes не являются текущим контрактом: до реализации использовать публичные v1 paths из `docs/API.md`.

## References

- `docs/API.md`
- `docs/AUTHORIZATION.md`
- `docs/DELIVERY_ARCHITECTURE.md`
- `docs/ROADMAP.md`
