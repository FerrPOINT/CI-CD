# ADR-0009: Канонический реестр имён и приоритет источников

**Статус:** Accepted (2026-08-27)
**Supersedes (частично):** противоречащие фрагменты ADR-0006 (имена таблиц), ADR-0008 (расположение SQL-файлов), `docs/RUNNER_ARCHITECTURE.md` (пути runner API), `docs/DELIVERY_ARCHITECTURE.md` §5.4 (формат error envelope), `docs/STORAGE_ARCHITECTURE.md` (vocabulary), нумерации ADR в `plans/architecture-rebuild-plan.md` и `docs/AUTHORIZATION.md` (исторические списки).

## Контекст

Независимые аудиты выявили, что одни и те же сущности/пути в разных документах называются по-разному, а планы и narrative-доки расходятся с принятыми ADR. До начала реализации требуется единственный канон.

## Решение

### 1. Канонические имена и пути

| Предмет | Канон | Запрещено/устарело |
|---|---|---|
| Каталог SQL-миграций | `backend/migrations/*.sql` | `backend/migration/migrations/` |
| Crate инструмента миграций | `backend/migration` (`cicd-migrate`) | — |
| Событийный журнал | `domain_events` | — |
| Transactional outbox | `outbox_messages` | `outbox_events` |
| Попытки доставки | `outbox_deliveries` | `outbox_attempts` |
| Выполнение job | `execution_attempts` | `pipeline_runs`, `job_runs` |
| Очередь / выдача | `job_queue`, `job_leases` | — |
| Runner API | `/api/v1/runner/*` + поле `protocolVersion` в payload | `/api/v1/runner/v1/*` (двойное версионирование пути) |
| OpenAPI артефакт | `openapi/openapi.yaml` (YAML, генерируемый) | `openapi/openapi.json` |
| Generated frontend types | `frontend/src/shared/api/generated/` | — |
| Tenancy vocabulary | `tenant` (`tenants`, `tenant_id`) | `organization`, `workspace` (в модели данных) |
| Error codes | `snake_case` (`validation_failed`) | UPPER_CASE (`VALIDATION_FAILED`) |
| `request_id` | внутри объекта `error` | вне envelope |

### 2. Приоритет источников (authority matrix)

1. **Код + закоммиченные миграции + закоммиченный OpenAPI** — текущее runtime-поведение.
2. **ADR** — принятые архитектурные решения. Номера не переиспользуются; исправление — только новый superseding ADR.
3. **`docs/contracts/*`** — нормативные целевые контракты (наблюдаемые требования: API, события, протоколы, данные).
4. **Архитектурные narrative-доки** (`AUTHORIZATION.md`, `RUNNER_ARCHITECTURE.md`, `AUTOMATION_ARCHITECTURE.md`, `STORAGE_ARCHITECTURE.md`, `DELIVERY_ARCHITECTURE.md`) — объяснительный материал; при конфликте с contracts — ошибочны.
5. **`docs/CURRENT_STATE.md`** — производный от кода снимок текущего состояния.
6. **`plans/*`** — некоммитные расписания работ; не являются источником требований; исторические нумерации ADR в них недействительны.

### 3. Правила

- Новые канонические решения фиксируются только через ADR или дополнение `docs/contracts/*`; narrative-доки не вводят новых канонических имён.
- Проверка канона автоматизирована: `python3 scripts/verify_docs.py --canonical`.
- Следующий свободный номер ADR — **0010**. Зарезервированы (будущие, не приняты): 0010 auth/tenancy/RBAC, 0011 envelope encryption/secrets, 0012 audit integrity/retention, 0013 Git authorization/signed internal events.

## Последствия

- Фрагменты, противоречащие реестру, в narrative-доках исправлены в том же коммите; ADR-0006/0008 остаются исторически точными в остальном.
- Реализация Phase A стартует от канона без повторного согласования имён.
