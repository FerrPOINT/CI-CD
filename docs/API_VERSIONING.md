# API Versioning — Forge CI/CD

## 1. Overview

API Forge CI/CD версионируется path-based: `/api/v1/...`. Версия меняется только при breaking changes. Additive-изменения и новые endpoint добавляются в текущую версию без повышения.

> **Source of truth:** актуальная реализация в `backend/src/api.rs`. Полная карта endpoint — в `docs/API.md`.

## 2. Version Policy

| Change Type | Version Impact | Example |
|-------------|----------------|---------|
| Additive (новые endpoint, новые поля response) | Same version | `GET /projects/{id}/pipelines?status=running` |
| Additive (новые optional поля request) | Same version | `default_branch` в `POST /projects` |
| Behavioral (другие defaults) | Same version | Изменение `LIMIT 50` → `LIMIT 20` |
| Breaking (удалён endpoint / поле) | New version required | Удаление `GET /jobs/{id}/logs` |
| Breaking (переименование поля) | New version required | `git_ref` → `ref` |
| Breaking (изменение семантики status code) | New version required | `500` → `409` для duplicate name |
| Security hardening (новый required header) | Same или new version | Добавление auth |

## 3. Текущая версия

- **Версия:** `v1`
- **Prefix:** `/api/v1`
- **Статус:** Active, единственная версия
- **Deprecated endpoint:** нет

## 4. Breaking Changes

Breaking change требует новой мажорной версии (`v2`). К breaking changes относятся:

- Удаление endpoint.
- Удаление или переименование обязательного поля request/response.
- Изменение семантики HTTP status code.
- Изменение формата ошибки (конверт `{"error": "..."}` → другой формат).
- Изменение auth flow (введение обязательной аутентификации).

> Введение auth в Phase 1 — не breaking change, т.к. текущая версия явно заявляет auth как нереализованную.

## 5. Deprecation Strategy

При выводе endpoint из эксплуатации:

1. Endpoint помечается как deprecated в `docs/API.md` с пометкой `> **Deprecated:** ...`.
2. В response добавляется заголовок `Sunset: <date>` (RFC 8594).
3. Минимум 6 месяцев поддержки после deprecation.
4. Логи использования deprecated endpoint помечаются `WARN`.
5. Уведомление в CLI: `cicd-cli` выводит warning при вызове deprecated команды.
6. После истечения срока — endpoint удаляется в следующей мажорной версии.

### Deprecation log

| Endpoint | Deprecated | Sunset | Replacement |
|----------|-----------|--------|-------------|
| — | — | — | — |

> Таблица пуста — нет deprecated endpoint в v1.

## 6. Backward Compatibility

### 6.1 Response JSON

- Клиент должен игнорировать неизвестные поля. `serde` на стороне сервера может добавлять новые поля в response без повышения версии.
- Нельзя удалять существующие поля без повышения версии.

### 6.2 Request JSON

- Новые optional поля в request body — backward compatible.
- `serde` с `#[serde(default)]` обеспечивает обратную совместимость для новых optional полей.
- Неизвестные поля в request игнорируются (no strict deserialization).

### 6.3 Query Parameters

- Неизвестные query-параметры игнорируются.
- Новые optional параметры — backward compatible.

### 6.4 Enum Values

- Статусы (`queued`, `running`, `success`, `failed`, `canceled`) — зафиксированы в v1.
- Клиент должен обрабатывать неизвестные значения enum через fallback (на случай добавления новых статусов в v1 — additive change).
- Добавление нового статуса — additive, не breaking.

### 6.5 Path Parameters

- UUID format — зафиксирован (UUIDv4).
- Изменение формата ID — breaking change.

## 7. Version Discovery

> **Планируется (Phase 2):** endpoint для discovery доступных версий.

```
GET /api/meta
```

Response:
```json
{
  "versions": ["v1"],
  "current": "v1",
  "deprecated": [],
  "sunset": null
}
```

## 8. Client Headers

```
Accept: application/json
```

> Дополнительные заголовки не требуются в текущей версии. `Idempotency-Key` запланирован в Phase 2+.

## 9. Migration Guide

При выпуске `v2` (когда потребуется):

1. Создать `docs/API_V2_MIGRATION.md (планируемый, будет создан при v2)` с mapping endpoint/field (появится при планировании v2).
2. Breaking changes changelog в `CHANGELOG.md`.
3. Обе версии (`v1` и `v2`) работают параллельно минимум 6 месяцев.
4. CLI `cicd-cli` переключается на `v2` через env var `CICD_API_VERSION` (default — `v1`).
5. Frontend переключается на `v2` через обновление API-клиента.

## 10. Semantic Versioning vs API Versioning

- **API version** (`v1`, `v2`) — меняется только при breaking changes в API.
- **Application version** (`0.1.0`, `0.2.0`, ...) — SemVer, меняется при каждом релизе.
- API version не связана с application version напрямую. Несколько application version могут использовать одну API version.

## References

- `docs/API.md` — полная спецификация endpoint.
- `docs/API_STANDARDS.md` — стандарты REST API.
- `docs/ROADMAP.md` — план разработки и фазы.
- `backend/src/api.rs` — реализация endpoint.