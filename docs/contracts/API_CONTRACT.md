Статус: Accepted target contract

[ADR-0009: канонический реестр имён и приоритет источников](../adr/0009-canonical-registry.md)

# HTTP API contract

## 1. Владелец и генерация OpenAPI

| Область | Нормативный владелец | Артефакт / правило |
|---|---|---|
| Public HTTP API | `cicd-api` | Route, DTO, security, success и error responses описываются OpenAPI-аннотациями. |
| Машиночитаемый контракт | `openapi/openapi.yaml` | Единственный bundled OpenAPI 3.1 артефакт, генерируемый из `cicd-api`, закоммиченный и reviewable. |
| Генерируемые frontend-типы | `frontend/src/api/schema.d.ts` | Производятся только из закоммиченного `openapi/openapi.yaml`; правила клиента определяет `UI_API_CONTRACT.md`. |
| Narrative-документация | `docs/API.md` и связанные документы | Описывает сценарии и текущее состояние, но не заменяет OpenAPI-схемы. |

| Шаг | Команда | Обязательный результат |
|---|---|---|
| Генерация | `just openapi-generate` | Обновлён `openapi/openapi.yaml`; ручное изменение файла запрещено. |
| Валидация | `just openapi-validate` | OpenAPI 3.1 и все examples валидны. |
| Генерация frontend | `pnpm api:generate` | Обновлены только generated-файлы frontend. |
| Проверка frontend | `pnpm api:check` | Сгенерированный transport компилируется. |
| CI | generation, validation, generation client, `git diff --exit-code`, diff с default branch | Не остаётся незакоммиченного производного артефакта и нет breaking change в `v1`. |

Каждая operation обязана иметь `operationId`, tags, request/response schemas, хотя бы один валидный example, security classification и ссылку на общую error schema для каждого применимого JSON error response. Внутренние routes остаются в том же spec с `x-forge-internal: true`; frontend generator их не использует.

## 2. Общие HTTP-соглашения

| Предмет | Контракт |
|---|---|
| Base path | Public control-plane API: `/api/v1`. |
| Формат | `application/json; charset=utf-8`, кроме явно документированных upload, download и streaming operations. |
| Идентификаторы | UUID в path и JSON, OpenAPI `format: uuid`. |
| Время | RFC 3339 UTC. |
| Enum | Строковый `snake_case`; клиенты обязаны безопасно обрабатывать неизвестное значение. |
| Correlation | Сервер возвращает `X-Request-Id`; клиент может передать этот header. Error response содержит тот же идентификатор только в `error.request_id`. |
| Security | Каждая operation явно объявляет security requirement; исключения документируются в OpenAPI. |
| Сортировка | Только allowlist конкретной operation; порядок детерминирован и имеет уникальный tie-breaker. |

## 3. Версии и совместимость

`v1` остаётся активной версией. Изменение контракта начинается с OpenAPI PR; implementation, generated clients и tests изменяются в том же изменении.

| Изменение в `v1` | Допустимость | Условие |
|---|---|---|
| Новый endpoint | Допустимо | Полная OpenAPI operation и error/security declaration. |
| Новое optional поле request | Допустимо | Сервер сохраняет прежнее поведение при отсутствии поля. |
| Новое поле response | Допустимо | Клиенты обязаны игнорировать неизвестные поля. |
| Новый optional filter | Допустимо | Не меняет default results существующего вызова. |
| Новое enum value | Допустимо | Документирован fallback клиента. |
| Удаление/переименование поля или endpoint | Запрещено | Требует новую major version. |
| Optional -> required | Запрещено | Требует новую major version. |
| Изменение типа, формата, semantics, status или error code | Запрещено | Требует новую major version. |
| JSON array -> object у существующей operation | Запрещено | Допустима только additive-миграция из раздела 6. |
| Новая обязательная auth-политика | Запрещено без migration | Нужны период совместимости и deprecation plan. |

Breaking change выпускается под новым versioned path. Предыдущая версия поддерживается не менее шести месяцев; deprecated operation получает `deprecated: true`, replacement, `Sunset` и usage evidence. Проверка совместимости сравнивает bundled contract с контрактом default branch.

## 4. Error envelope

Все новые или изменённые JSON error responses используют одну OpenAPI schema:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Некоторые поля не прошли проверку.",
    "request_id": "01J8YJ2M79N01T4AZ7F9XBF0HM",
    "details": [
      {
        "field": "repository_url",
        "code": "invalid_format",
        "message": "Укажите допустимый Git URL."
      }
    ]
  }
}
```

| Поле | Тип | Правило |
|---|---|---|
| `error` | object | Обязательно. |
| `error.code` | string | Обязательно; стабильный `snake_case` code. |
| `error.message` | string | Обязательно; безопасно для показа, без инфраструктурных деталей. |
| `error.request_id` | string | Обязательно; совпадает с `X-Request-Id`. Вне `error` не размещается. |
| `error.details` | array | Опционально; для field/business validation. |
| `error.details[].field` | string | JSON field path, например `stages[0].jobs[1].image`. |
| `error.details[].code` | string | `snake_case` причина поля. |
| `error.details[].message` | string | Безопасное пояснение поля. |

| HTTP | `error.code` | Семантика / обязательный header |
|---|---|---|
| 400 | `invalid_request`, `invalid_cursor` | Невалидный JSON, path/query parameter или cursor. |
| 401 | `authentication_required`, `invalid_credential`, `credential_expired` | `WWW-Authenticate: Bearer`. |
| 403 | `permission_denied` | Аутентифицированный субъект не имеет права. |
| 404 | `not_found` | Ресурс отсутствует или скрыт policy. |
| 409 | `conflict`, `idempotency_conflict`, `lease_fenced` | Конфликт состояния, unique constraint либо key/fingerprint conflict. |
| 413 | `payload_too_large` | Body превышает лимит операции. |
| 422 | `validation_failed` | JSON корректен, но input не проходит validation. |
| 429 | `rate_limited` | Целочисленный `Retry-After`. |
| 500 | `internal_error` | Непредвиденная server error. |
| 503 | `dependency_unavailable` | Временная недоступность обязательной зависимости. |

`message` и `details` не содержат token, secret, password, cookie, SQL, stack trace, внутренний path, connection string или upstream body. Полная причина остаётся только в logs/traces. Клиенты сохраняют исходные `code` и `request_id`.

## 5. Пагинация

Новая collection operation или мигрированная collection operation использует cursor contract:

```http
GET /api/v1/<collection>?limit=50&cursor=<opaque>
```

```json
{
  "items": [{ "id": "..." }],
  "next_cursor": "..."
}
```

| Предмет | Контракт |
|---|---|
| `limit` | Integer; default `50`, maximum `200`; отсутствует или null означает default. |
| Невалидный `limit` | `422 validation_failed`. |
| `cursor` | Опциональный opaque base64url JSON, подписанный HMAC-SHA256 с `CICD_CURSOR_KEY`; TTL 24 часа. |
| Невалидный или истёкший `cursor` | `400 invalid_cursor`. |
| `items` | Массив элементов текущей выборки; всегда присутствует, включая пустой результат. |
| `next_cursor` | Следующий opaque cursor либо `null`; total/count не возвращается. |
| Порядок | Fixed per operation, stable и включает `id` или другой уникальный tie-breaker. |
| Фильтры и сортировка | Только объявленные OpenAPI parameters; SQL fragments, произвольные field names и unbounded regex запрещены. |

Для каждой paginated operation OpenAPI фиксирует parameters, default, maximum, сортировку, `items` schema и `next_cursor`. Cursor включает version и поля fixed sort; сервер не принимает cursor, созданный для другого порядка или filter scope.

Исключение для append-only логов: `/api/v1/jobs/{job_id}/logs/page` и `/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page` используют монотонный `after` checkpoint по `job_logs.sequence` и возвращают `next_after`. Это сохраняет совместимость с SSE `after` и не переносится на обычные collection endpoints.

## 6. Совместимость текущих array responses

Ни один существующий `v1` endpoint не меняет успешный JSON array на envelope молча, в том числе при добавлении `limit` или `cursor`. До отдельного OpenAPI/versioning решения сохраняются следующие current responses:

| Operation | Current 2xx shape | Current порядок / ограничение | Правило миграции |
|---|---|---|---|
| `GET /api/v1/projects` | `Project[]` | `created_at DESC`, без limit | Сохранять array; новый cursor contract вводить отдельной operation или явно объявленной additive-миграцией. |
| `GET /api/v1/projects/{project_id}/pipelines` | `Pipeline[]` | `created_at DESC`, последние 50 | Сохранять array и limit 50 до переключения всех UI/CLI consumers. |
| `GET /api/v1/jobs/{job_id}/logs` | `JobLog[]` | `sequence ASC`, без limit | Сохранять array для совместимости; bounded чтение добавлено отдельными `/logs/page` operations без подмены shape через query parameter. |

Migration array response допускается только так: OpenAPI PR вводит новый совместимый способ получения paged response, UI/CLI migration и contract tests подтверждают его, затем deprecation имеет replacement и срок. Удаление array response возможно лишь в новой API major version.

## 7. Идемпотентность mutations

| Mutation | `Idempotency-Key` | Поведение |
|---|---|---|
| Создание ресурса или запуск pipeline | Обязателен для retryable operation | Исключает duplicate create/trigger. |
| Retry/cancel и иная смена состояния | Обязателен для retryable operation | Защищает от повторного side effect. |
| Upload artifact, создание token, enqueue external delivery | Обязателен для retryable operation | Защищает необратимый или внешний side effect. |
| Mutation без риска повторной доставки | OpenAPI явно определяет policy | Клиент не предполагает идемпотентность без contract declaration. |

| Правило | Контракт |
|---|---|
| Header | `Idempotency-Key` содержит UUID. |
| Scope записи | `(principal_id, route, key)`; route -- canonical method/path operation. |
| Fingerprint | Hash method, canonical path и нормализованного body. |
| Первое выполнение | Атомарно резервирует запись до use case. Успешный или детерминированный client-error status/body сохраняется. |
| Повтор того же fingerprint | Возвращает первоначальные status/body и `Idempotency-Replayed: true`. |
| Тот же key, иной fingerprint | `409 idempotency_conflict`. |
| Временная internal error | Не кэшируется как окончательный response; reservation освобождается или помечается retryable. |
| Retention | Не менее 24 часов; уникальность обеспечивается транзакцией и unique index. |

CLI и UI создают UUID на одно пользовательское/автоматизационное намерение и сохраняют его для retry того же намерения. Новый явный запуск создаёт новый key.

## 8. Проверяемые требования

| Проверка | Требование |
|---|---|
| OpenAPI | Каждая route имеет operation, schemas, error responses и security classification. |
| Compatibility | Contract diff не содержит breaking change в `v1`. |
| Error | Integration tests проверяют HTTP status, полный envelope, `error.request_id` и `X-Request-Id`. |
| Pagination | Tests проверяют stable order, tampering/expiry cursor, empty page и max `limit`. |
| Body limits | Tests проверяют, что large artifact/Git/JUnit routes не ломаются об общий Axum default, а log append routes отсекают payload выше явного лимита. |
| Idempotency | Real PostgreSQL tests проверяют first request, replay, conflicting fingerprint и restart-safe persistence. |
