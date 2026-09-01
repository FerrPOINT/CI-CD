# Целевая архитектура delivery-контура Forge CI/CD

> **Статус:** объяснительный narrative. Нормативные контракты — `contracts/API_CONTRACT.md` и `contracts/UI_API_CONTRACT.md`; при конфликте прав контракт (ADR-0009). Текущее состояние — `docs/CURRENT_STATE.md`.

> Документ предназначен для размещения в `docs/DELIVERY_ARCHITECTURE.md`.
>
> **Статус:** целевой дизайн. Он отделяет фактическое состояние репозитория от целевой архитектуры и не объявляет запланированные возможности реализованными.

## 1. Цель и границы

Forge CI/CD развивается из MVP control plane в безопасную, наблюдаемую и контрактно-совместимую платформу. Delivery-контур объединяет:

- versioned HTTP API с машиночитаемым OpenAPI-контрактом;
- React Dashboard с генерируемыми типами и предсказуемым server state;
- независимый CLI-клиент для автоматизации;
- аутентификацию, RBAC и согласованный UX ошибок;
- metrics, logs, traces, liveness/readiness;
- CI quality gates и воспроизводимые E2E-доказательства.

Целевой дизайн не меняет доменную границу: `JobStatus::transition_to()` остаётся единственным источником правил перехода статусов. API, UI и CLI вызывают application use cases и не реализуют собственные правила lifecycle.

## 2. Фактическое состояние

| Область | Текущее состояние |
|---|---|
| Backend | Cargo workspace содержит server crate, `domain` и выделенный `cli`; основная HTTP/SQL-логика пока находится в `backend/src/api.rs` и смежных модулях. |
| API | REST доступен по `/api/v1`; есть health/readiness, auth, projects, pipelines, jobs, Git и platform endpoints. OpenAPI генерируется из Rust annotations в `openapi/openapi.yaml` и проверяется drift gate в CI. |
| Ошибки | Current API возвращает structured envelope `{"error":{"code","message","request_id"}}` и header `x-request-id`; compatibility/error taxonomy ещё не полностью покрыта target contract tests. |
| Версионирование | Path-versioning `/api/v1` документирован, но нет автоматической проверки breaking changes между контрактами. |
| Пагинация | `projects` и `pipelines` поддерживают `limit/offset` с cap 200; job logs имеют bounded page/search (`limit`, `after`, `q`) и compatibility array endpoint для current/latest attempt. Унифицированный response envelope/cursor model для всех списков остаётся target. |
| Идемпотентность | Current MVP: `POST /projects/{project_id}/pipelines` поддерживает `Idempotency-Key` и хранит `pipeline_triggers`; generated Git hook дедуплицируется по `repository/ref/new_rev`. General idempotency storage для всех retryable mutations остаётся target. |
| Auth/RBAC | При `CICD_AUTH_SECRET` включены login/JWT/scoped PAT, argon2id credentials, sessions, route-level global roles и project memberships; без секрета остаётся trusted-network mode. Tenant scope, service-account/scoped Git credentials и production session policy остаются target. |
| Frontend | React 19/Vite/TanStack Query; около 20 маршрутов + `/login`. DTO генерируются в `frontend/src/api/schema.d.ts`, API wrapper/hooks остаются handwritten. |
| CLI | `backend/cli` уже отдельный package и работает через HTTP; реализованы runtime/platform группы `project`, `pipeline`, `job`, `runner`, `secret`, `artifact`, `environment`, `deployment`, `schedule`, `webhook`, `notification`, `outbox`, `report`, `audit`, `user`, `member`, `token`; есть `--api-url`/`CICD_API_URL`, `--token`/`CICD_API_TOKEN`, `--timeout-seconds`/`CICD_TIMEOUT_SECONDS`, `--output json|table`/`CICD_OUTPUT` и real-API smoke в CI. Profiles, keyring, generated DTO/client, request tracing, NDJSON и full auth/RBAC CLI E2E остаются target. |
| Observability | Есть `/api/v1/health`, `/api/v1/readiness`, `/metrics`, `TraceLayer` и `tracing`. OTLP, alerting и корреляция API--CLI не реализованы. |
| Quality | GitHub Actions запускает backend fmt/clippy/workspace tests, real PostgreSQL integration, OpenAPI drift/compatibility gate, frontend generated-client check/test/build, Compose startup/health smoke, representative Playwright/axe/performance E2E на seeded Compose stack, security и docs checks. Lighthouse/load budgets и полный evidence bundle остаются target. |

## 3. Целевые принципы

1. **Contract first.** OpenAPI является публичным контрактом API; код, frontend-типы, CLI DTO и contract tests производятся или проверяются относительно него.
2. **Совместимость по умолчанию.** Additive изменения допустимы в рамках `v1`; удаления, переименования и изменение семантики требуют новой major API version.
3. **Единая ошибка, разные представления.** Backend возвращает безопасный структурированный error envelope; frontend и CLI преобразуют его в удобный формат, не теряя `code` и `request_id`.
4. **Надёжность mutating-операций.** Создание pipeline, upload artifact, webhook delivery и другие операции с побочным эффектом защищены ключом идемпотентности.
5. **Тонкие клиенты.** UI и CLI не повторяют доменные правила, не обращаются к БД и используют только публичный HTTP contract.
6. **Наблюдаемость без утечки данных.** Логи, traces и метрики коррелируются по request/trace ID, но не содержат токенов, паролей, секретов, значений secret variables или несанитизированных job logs.
7. **Доказуемый delivery.** Изменение считается готовым только после автоматически сохранённых результатов contract, integration, E2E и визуальных проверок.

## 4. Целевая схема компонентов

```text
                  +---------------------------+
                  | OpenAPI source and policy |
                  | openapi/openapi.yaml      |
                  +-------------+-------------+
                                |
              lint / bundle / breaking-change check
                                |
           +--------------------+--------------------+
           |                                         |
           v                                         v
+--------------------------+            +---------------------------+
| Rust API package         |            | Generated TypeScript      |
| DTO, validation, docs    |            | client and schema types   |
| routes, auth middleware  |            | frontend/src/shared/api/  |
+-------------+------------+            +-------------+-------------+
              |                                           |
              v                                           v
+--------------------------+              +---------------------------+
| Application layer        |              | React FSD slices          |
| use cases, RBAC policy,  |              | entities/features/widgets |
| transactions             |              | TanStack Query adapters   |
+-------------+------------+              +---------------------------+
              |
              v
+--------------------------+
| Domain and ports         |
| JobStatus, invariants    |
+-------------+------------+
              |
              v
+--------------------------+
| Infrastructure           |
| PostgreSQL, Git, runner, |
| artifact storage, OTLP   |
+--------------------------+

+--------------------------+
| cicd-cli                 |
| Clap -> config ->        |
| generated contract DTO ->|
| HTTP client              |
+--------------------------+
```

Целевая backend-структура продолжает ADR-0005:

```text
backend/
├── domain/                 # pure business types, policies, port traits
├── app/                    # use cases, authorization, transactions
├── infra/                  # PostgreSQL, Git, runner, artifacts, telemetry adapters
├── api/                    # Axum routes, DTOs, OpenAPI, middleware
├── server/                 # composition root
├── cli/                    # standalone HTTP client
├── migration/              # versioned SQLx migrations
└── tests/                  # black-box API and real-DB integration tests

openapi/
├── openapi.yaml            # canonical, bundled OpenAPI 3.1 contract
├── examples/               # valid request/response fixtures
└── README.md               # generation and compatibility workflow
```

Во время strangler migration новые вертикали создаются сразу в `domain -> app -> infra -> api`; старые handlers временно адаптируют старый путь к новым use cases. Новый SQL в Axum handlers запрещён.

## 5. OpenAPI-first API

### 5.1 Источник контракта и генерация

Целевой контракт — OpenAPI 3.1 в `openapi/openapi.yaml`, собираемый из API package или поддерживаемый декларативно в репозитории. На первом шаге допускается генерация из Rust-аннотаций (`utoipa`) при соблюдении следующих правил:

- CI генерирует bundled-файл детерминированно.
- Сгенерированный `openapi/openapi.yaml` коммитится и рассматривается как reviewable API artifact.
- Примеры request/response являются частью схемы или лежат в `openapi/examples/`.
- Ручная документация в `docs/API.md` описывает решения и сценарии, но не дублирует полную схему DTO.
- Любой route без OpenAPI operation, response schema, error response и security requirement не проходит CI.

Типизированный TypeScript client генерируется из pinned OpenAPI artifact. Предпочтительный вариант:

```text
OpenAPI 3.1 -> openapi-typescript -> generated schema/types
                            |
                            +-> тонкий typed fetch client
```

TanStack Query hooks не должны полностью генерироваться без контроля: query keys, invalidation и UX-политика остаются в FSD feature slices. Это позволяет сделать generated transport заменяемым, а кэш-инварианты — читаемыми и тестируемыми.

### 5.2 Базовые соглашения

| Правило | Целевое состояние |
|---|---|
| Base path | `/api/v1` для public control-plane API. |
| Формат | `application/json; charset=utf-8`, кроме download/upload и streaming endpoints. |
| Идентификаторы | UUID в path и JSON; `format: uuid` в OpenAPI. |
| Время | RFC 3339 UTC, например `2026-08-26T10:00:00Z`. |
| Enum | Строковые `snake_case`; клиенты обязаны иметь fallback для неизвестного значения. |
| Сортировка | Только явный allowlist per resource; порядок всегда детерминирован и включает tie-breaker `id`. |
| Correlation | Ответы содержат `X-Request-Id`; клиент может передать `X-Request-Id`, иначе сервер генерирует его. |
| Security | В OpenAPI явно указывается security scheme и requirement каждой операции. Исключения: liveness, readiness, version metadata и при необходимости metrics endpoint в отдельной network zone. |

### 5.3 Версионирование и совместимость

`/api/v1` остаётся активной версией до реально несовместимого изменения.

В рамках одной версии разрешено:

- добавить endpoint;
- добавить optional request field;
- добавить response field;
- добавить новый optional filter;
- добавить новый enum value при documented fallback-политике;
- исправить явно ошибочную реализацию без изменения documented semantics.

В рамках одной версии запрещено:

- удалить или переименовать поле;
- сделать optional field required;
- изменить тип, формат или смысл существующего поля;
- изменить ошибку/статус так, что клиент меняет ветку поведения;
- заменить error envelope;
- сделать незашищённый endpoint защищённым без периода migration/deprecation.

Для breaking changes создаётся `/api/v2`; `v1` и `v2` работают параллельно не менее шести месяцев. У deprecated endpoints:

- есть `deprecated: true` в OpenAPI;
- есть replacement в описании операции;
- добавляется `Sunset` header по RFC 8594;
- CLI печатает предупреждение в `stderr`;
- usage метрика и structured log позволяют проверить фактическое потребление.

CI сравнивает текущий bundled contract с contract из default branch через `oasdiff` или эквивалент. Breaking change допустим только при одновременном добавлении нового versioned path и migration guide.

### 5.4 Единый error envelope

Все JSON-ошибки API, включая extractor/validation errors, возвращают одну схему:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Некоторые поля не прошли проверку.",
    "details": [
      {
        "field": "repository_url",
        "code": "invalid_format",
        "message": "Укажите допустимый Git URL."
      }
    ]
  },
  "request_id": "01J8YJ2M79N01T4AZ7F9XBF0HM"
}
```

```yaml
ErrorResponse:
  type: object
  required: [error, request_id]
  properties:
    error:
      type: object
      required: [code, message]
      properties:
        code:
          type: string
          example: VALIDATION_FAILED
        message:
          type: string
        details:
          type: array
          items:
            $ref: '#/components/schemas/ErrorDetail'
    request_id:
      type: string
      description: Correlation ID for support, logs and traces.
```

| HTTP | `error.code` | Когда использовать |
|---|---|---|
| 400 | `INVALID_REQUEST`, `INVALID_CURSOR` | Невалидный JSON, query/path parameter, cursor. |
| 401 | `UNAUTHENTICATED`, `TOKEN_EXPIRED` | Нет или недействителен Bearer token/session. |
| 403 | `FORBIDDEN` | Аутентифицированный субъект не имеет права. |
| 404 | `NOT_FOUND` | Ресурс не существует либо намеренно скрыт policy. |
| 409 | `CONFLICT`, `INVALID_STATE`, `IDEMPOTENCY_CONFLICT` | Unique conflict, недопустимый transition, тот же key с другим request fingerprint. |
| 412 | `PRECONDITION_FAILED` | Опциональная optimistic concurrency через `If-Match`. |
| 413 | `PAYLOAD_TOO_LARGE` | Превышен лимит upload/body. |
| 422 | `VALIDATION_FAILED` | Корректный JSON, но поля не соответствуют бизнес-валидации. |
| 429 | `RATE_LIMITED` | Rate limit; ответ содержит `Retry-After`. |
| 500 | `INTERNAL_ERROR` | Непредвиденная ошибка; инфраструктурные детали не раскрываются. |
| 503 | `DEPENDENCY_UNAVAILABLE`, `NOT_READY` | БД, storage или обязательная зависимость недоступны. |

Требования:

- `message` безопасен для показа пользователю и локализуется на frontend/CLI при наличии known code.
- `details[].field` использует JSON field path, например `stages[0].jobs[1].image`.
- SQLx, filesystem, Docker, upstream HTTP и stack trace записываются только в server logs/traces.
- API не возвращает raw токены, хэши, секреты, локальные пути или внутренние connection strings.
- Frontend и CLI сохраняют исходные `code` и `request_id` для диагностики.

### 5.5 Пагинация, фильтрация и сортировка

Списки не возвращают неограниченный объём данных.

Для небольших административных сущностей (`projects`, `users`, `runners`, `environments`) используется page pagination:

```http
GET /api/v1/projects?page=1&page_size=20&sort=-created_at
```

```json
{
  "data": [{ "id": "..." }],
  "page": {
    "number": 1,
    "size": 20,
    "total_items": 145,
    "total_pages": 8
  }
}
```

Для frequently appended или крупных наборов (`pipelines`, `job_logs`, audit events, webhook deliveries) используется cursor/keyset pagination:

```http
GET /api/v1/jobs/{job_id}/logs?after=eyJzZXF1ZW5jZSI6MTIwfQ&page_size=200
```

```json
{
  "data": [{ "sequence": 121, "message": "..." }],
  "page": {
    "next_cursor": "eyJzZXF1ZW5jZSI6MzIwfQ",
    "previous_cursor": null,
    "has_more": true
  }
}
```

Обязательные свойства pagination:

- `page_size` имеет documented default и per-resource maximum; превышение maximum даёт `422`.
- Cursor непрозрачен для клиента, кодируется и валидируется сервером.
- Cursor включает все поля сортировки, например `(created_at, id)` или `sequence`.
- Выборка всегда содержит stable order: `created_at DESC, id DESC` либо `sequence ASC`.
- Для logs `COUNT(*)` не выполняется и `total_items` не возвращается.
- Фильтры и sort fields allowlisted; произвольные SQL field names, SQL fragments и unbounded regex запрещены.
- В OpenAPI для каждого list endpoint задаются query parameters, default, maximum и response envelope.

Миграция существующих массивов выполняется additive-путём: сначала появляются новые paged endpoints или opt-in parameter/envelope с documented compatibility period, затем UI/CLI переходят на него. Нельзя молча заменить JSON array на object в существующем `v1` endpoint.

### 5.6 Идемпотентность

Все POST/PUT/PATCH-операции, при повторе которых возможны дубликаты или необратимый side effect, поддерживают `Idempotency-Key`:

- create project;
- trigger pipeline;
- retry/cancel pipeline и job;
- upload artifact;
- создание API token;
- webhook delivery enqueue;
- любые будущие платежеподобные или внешние side effects.

Ключ передаётся заголовком:

```http
Idempotency-Key: 8ca4a8df-0e9f-48d8-a7e0-75d1c7b88d5d
```

Алгоритм:

1. API валидирует, что ключ непустой, имеет ограниченную длину и связан с authenticated principal.
2. До выполнения use case сервер атомарно резервирует запись `(principal_id, route, idempotency_key)`.
3. В записи хранится request fingerprint: method, canonical path и hash нормализованного body.
4. Первый успешный или детерминированный client-error response сохраняется вместе со status и body.
5. Повтор с тем же fingerprint возвращает исходный status/body и `Idempotency-Replayed: true`.
6. Повтор с тем же ключом, но отличающимся request fingerprint, возвращает `409 IDEMPOTENCY_CONFLICT`.
7. Временные internal errors не кэшируются как окончательный результат; reservation корректно освобождается или помечается retryable.
8. TTL записей — минимум 24 часа; очистка выполняется фоновым task без нарушения audit retention.

Реализация должна быть транзакционной и иметь уникальный индекс по `(principal_id, route, idempotency_key)`. CLI генерирует UUID автоматически для side-effect command, но позволяет задать `--idempotency-key` для безопасного retry внешним автоматизатором. UI создаёт key на одно намерение пользователя и сохраняет его, пока mutation находится в retry state.

## 6. Auth, RBAC и UX безопасности

### 6.1 Целевая модель

| Клиент | Механизм |
|---|---|
| Browser Dashboard | Короткоживущий access token в памяти и rotating refresh token в `Secure`, `HttpOnly`, `SameSite` cookie. |
| CLI/automation | Personal access token или service token через `Authorization: Bearer`. |
| Runner | Отдельный scoped runner credential с rotation, heartbeat и lease policy. |
| Internal hooks | Отдельный service credential; не использовать пользовательский PAT. |

Обязательные endpoints:

```text
POST /api/v1/auth/login
POST /api/v1/auth/refresh
POST /api/v1/auth/logout
GET  /api/v1/auth/me
POST /api/v1/auth/tokens
DELETE /api/v1/auth/tokens/{token_id}
```

Все control-plane routes по умолчанию защищены. Исключения явно ограничены: current `/api/v1/health`, `/api/v1/readiness`, `/metrics`, target `/health/live`, `/health/ready`, `/api/meta` и observability endpoints в соответствии с network policy.

RBAC выполняется в application layer, а не в React routes и не только в Axum middleware:

```text
request -> authenticate -> attach Principal
        -> endpoint -> application authorization policy
        -> use case -> infrastructure
```

Политики проверяют role, project membership, ownership и scope токена. Для каждого mutation формируется audit event с actor, action, resource, result, request ID и trace ID без secret payload.

### 6.2 UX аутентификации во frontend

- При открытии приложения `AuthProvider` вызывает `GET /auth/me`; до результата показывается shell-level loading state, а не кратковременная protected page.
- `401` от обычного запроса запускает только один refresh flow; параллельные запросы ожидают его результат.
- При успешном refresh исходный запрос повторяется один раз.
- При неуспехе refresh cache очищается, пользователь перенаправляется на `/login`, а return URL сохраняется только для внутренних безопасных путей.
- `403` не редиректит на login: показывается экран/inline state «Недостаточно прав» и не раскрывает скрытый ресурс.
- Logout очищает QueryClient, local UI state и refresh cookie через серверный endpoint.
- Token никогда не помещается в query string, localStorage или error report.
- Страница login поддерживает loading, invalid credentials, locked/disabled account, network failure и keyboard-only flow.

## 7. Целевая frontend-архитектура

### 7.1 Feature-Sliced Design

Текущие `pages`, `widgets`, `shared` сохраняются, но бизнес-срезы выделяются в `entities` и `features`.

```text
frontend/src/
├── app/
│   ├── providers/             # QueryClient, auth, i18n, error boundary
│   ├── router/
│   └── styles/
├── pages/
│   ├── projects/
│   ├── pipelines/
│   ├── pipeline-detail/
│   ├── login/
│   └── ...
├── widgets/
│   ├── app-shell/
│   ├── pipeline-overview/
│   └── mobile-navigation/
├── features/
│   ├── auth/login/
│   ├── auth/logout/
│   ├── project/create/
│   ├── project/edit/
│   ├── pipeline/trigger/
│   ├── pipeline/cancel/
│   └── job/change-status/
├── entities/
│   ├── project/
│   ├── pipeline/
│   ├── job/
│   ├── user/
│   └── runner/
├── shared/
│   ├── api/
│   │   ├── generated/         # generated; no hand edits
│   │   ├── http-client.ts
│   │   ├── api-error.ts
│   │   └── query-client.ts
│   ├── config/
│   ├── lib/
│   ├── ui/
│   └── i18n/
└── test/
    ├── handlers/
    └── factories/
```

Правила зависимостей:

- `shared` не импортирует higher layers.
- `entities` не импортируют `features`, `widgets` или `pages`.
- `features` не импортируют друг друга напрямую; общие модели переносятся в `entities`.
- `pages` композиционно собирают widgets/features, но не делают raw fetch.
- generated schema/client из `frontend/src/api/schema.d.ts` не редактируется вручную.
- Запрещены дублирующие DTO в `frontend/src/api/types.ts` после миграции соответствующего endpoint.

### 7.2 Generated client, transport и query keys

Generated client отвечает только за serialization, transport type signatures и OpenAPI DTO. Он получает настроенный transport, который:

- добавляет `Accept`, `X-Request-Id`, `Authorization` и при необходимости `Idempotency-Key`;
- преобразует `ErrorResponse` в typed `ApiError`;
- не делает UI toast, redirect или cache invalidation;
- различает cancellation, network error и structured server error.

Query keys определяются в entity slices и строятся иерархически:

```ts
export const projectKeys = {
  all: ['projects'] as const,
  lists: () => [...projectKeys.all, 'list'] as const,
  list: (params: ProjectListParams) => [...projectKeys.lists(), params] as const,
  details: () => [...projectKeys.all, 'detail'] as const,
  detail: (id: string) => [...projectKeys.details(), id] as const,
}

export const pipelineKeys = {
  all: ['pipelines'] as const,
  byProject: (projectId: string, params: PipelineListParams) =>
    [...pipelineKeys.all, 'project', projectId, params] as const,
  detail: (pipelineId: string) =>
    [...pipelineKeys.all, 'detail', pipelineId] as const,
}
```

Mutation обязан:

- invalidировать только затронутые keys;
- обновлять detail/list cache оптимистично лишь при однозначно обратимой операции;
- откатывать optimistic update при server error;
- использовать response entity как authoritative state;
- не повторять автоматически mutation без идемпотентного ключа;
- отображать server `request_id` в details технической ошибки.

### 7.3 Async states и ошибки

Для каждого data-bearing экрана обязательны отдельные состояния:

| Состояние | Требование |
|---|---|
| Initial loading | Скелетон, сохраняющий структуру страницы; не пустой белый экран. |
| Background refresh | Данные остаются видимыми, отображается ненавязчивый индикатор обновления. |
| Empty | Контекстное сообщение и разрешённое действие, например «Создать проект». |
| Permission denied | Отдельный 403-state, без misleading «не найдено». |
| Not found | Отдельный 404-state с безопасной навигацией назад. |
| Validation error | Field-level message из `details`, focus на первом неверном поле. |
| Mutation pending | Кнопка disabled, текст действия изменён, предотвращён double submit. |
| Retryable failure | Inline error + retry; не только toast. |
| Fatal application error | Error boundary с request ID, безопасным reload и переходом на dashboard. |

Toast используется как вторичное подтверждение короткого действия, но не как единственный носитель критичной ошибки или состояния загрузки.

### 7.4 Responsive и mobile

Целевой Dashboard поддерживает минимум 320 px ширины без горизонтального scrolling страницы. Исключение — технические таблицы и log viewer, где допустим локальный scroll внутри контейнера.

Обязательные правила:

- mobile-first layout и touch targets не менее 44x44 CSS px;
- sidebar на mobile превращается в focus-trapped drawer с overlay, закрывается `Escape`, выбором маршрута и кликом по overlay;
- таблицы имеют мобильное представление: cards, priority columns либо горизонтальный container с явным affordance;
- destructive actions не располагаются рядом с primary action без confirm dialog;
- forms используют подходящие `inputMode`, autocomplete и видимые labels;
- лог viewer имеет монодисплей, controlled auto-follow, кнопку «К последним строкам» и не захватывает scroll всей страницы;
- E2E проверяет 375x812, 768x1024, 1440x900 и 2560x1440.

## 8. Целевая CLI-архитектура

### 8.1 Назначение и границы

`cicd-cli` остаётся отдельным Rust workspace package. Runtime-код зависит от `clap`, HTTP transport, generated/public contract types и локальных CLI helpers, но не зависит от server, app, infra, SQLx, Git storage или runner implementation. Integration-test harness может импортировать server crate только для запуска disposable HTTP API fixture; проверяемый CLI binary при этом всё равно общается по публичному HTTP contract.

```text
backend/cli/src/
├── main.rs                 # exit boundary and startup
├── command/                # project, pipeline, job, auth, config, ...
├── config.rs               # discovery, merge, validation
├── client.rs               # authenticated HTTP transport
├── output.rs               # json, yaml, table, ndjson
├── error.rs                # typed errors and exit codes
├── pagination.rs           # cursor/page iteration
└── generated/              # generated contract DTO/client, no manual edits
```

### 8.2 Конфигурация и profiles

Приоритет конфигурации, от высшего к низшему:

1. CLI flags: `--api-url`, target `--profile`, `--token`, `--output`, `--timeout-seconds`;
2. environment: `CICD_API_URL`, `CICD_API_TOKEN`, target `CICD_PROFILE`, `CICD_OUTPUT`, `CICD_TIMEOUT_SECONDS`;
3. profile в config file;
4. default profile;
5. безопасные compile-time defaults.

Файл конфигурации:

```text
XDG_CONFIG_HOME/forge/config.toml
# fallback: ~/.config/forge/config.toml
```

Пример:

```toml
[profiles.default]
api_url = "https://forge.example.internal"
output = "table"
timeout_seconds = 30

[profiles.staging]
api_url = "https://forge-staging.example.internal"
output = "json"
```

Токен не должен сохраняться plaintext в `config.toml`. Предпочтительно хранение в OS keyring; fallback допускается только через `CICD_API_TOKEN` или явно указанный protected token file с предупреждением о permissions. CLI никогда не печатает token в `--debug`, error output или shell completion.

Команды:

```text
cicd auth login
cicd auth logout
cicd auth status
cicd config get <key>
cicd config set <key> <value>
cicd config use-profile <name>
```

### 8.3 Командная модель

Команды отражают ресурсную модель API и стабильны в пределах API major version:

```text
cicd project list|get|create|update|delete
cicd pipeline list|run|show|cancel|retry
cicd job show|logs|retry|cancel
cicd runner list|register|heartbeat|delete
cicd artifact list|download|upload
cicd token list|create|revoke
cicd auth login|logout|status
cicd config get|set|use-profile
```

Правила:

- `--help` является тестируемым public contract.
- Mutating-команды принимают `--idempotency-key`; если не задан, CLI создаёт UUID.
- Небезопасные destructive actions требуют `--yes` в non-interactive mode.
- Интерактивные confirm prompts запрещены, если stdout не TTY, включён `--output json` или задан `--yes`.
- `list` поддерживает `--page`, `--page-size`, `--all` для offset pagination и `--limit`, `--cursor`, `--follow` для cursor/streaming endpoints.
- `job logs --follow` использует SSE/stream endpoint, не polling whole log history.
- `--dry-run` допустим только когда API поддерживает explicit validation/preview semantics; CLI не должен имитировать успешное server operation локально.

### 8.4 Output, errors и exit codes

`stdout` содержит только полезный результат; diagnostics, deprecation warnings и progress пишутся в `stderr`.

| Режим | Назначение |
|---|---|
| `--output table` | Читаемый интерактивный default при TTY. |
| `--output json` | Stable JSON для automation; без цветовых кодов и prose. |
| `--output yaml` | Опциональный human-readable structured формат. |
| `--output ndjson` | Потоковые списки и logs. |
| `--quiet` | Печатает только machine-critical value, явно documented для каждой команды. |
| `--no-color` | Отключает ANSI; автоматически включается при non-TTY. |

Ошибки используют безопасный формат:

```text
error[VALIDATION_FAILED]: Поле repository_url содержит недопустимый Git URL.
request_id: 01J8YJ2M79N01T4AZ7F9XBF0HM
hint: Используйте SSH URL вида git@host:group/repository.git.
```

| Exit code | Значение |
|---|---|
| 0 | Успех. |
| 1 | Неожиданная локальная ошибка. |
| 2 | Ошибка CLI usage/argument/config validation. |
| 3 | Auth failure (`401`). |
| 4 | Authorization failure (`403`). |
| 5 | API validation/conflict/not found (`4xx`, кроме auth). |
| 6 | Transport timeout, DNS/TLS или unavailable dependency. |
| 7 | Server-side error (`5xx`). |
| 8 | Пользователь отменил интерактивное действие. |

В JSON mode error также сериализуется структурированно и содержит `code`, `message`, `request_id`, `http_status`; plaintext API body, tokens и internal diagnostics в него не попадают.

## 9. Observability

### 9.1 Correlation и контекст

Каждый HTTP request получает:

- `request_id` в response header и error envelope;
- W3C `traceparent` при наличии входящего trace context;
- authenticated subject ID, role/scope, route template и operation name в trace/log context;
- domain identifiers: `project_id`, `pipeline_id`, `job_id`, `runner_id` только там, где они уже известны.

CLI передаёт `User-Agent: cicd-cli/<version>`, `X-Request-Id` и W3C trace headers при включённом telemetry. Frontend передаёт `X-Request-Id`; browser tracing включается с sampling и без PII/secret fields.

### 9.2 Метрики

Prometheus endpoint должен быть отделён от public API contract, например `GET /metrics`, и доступен только monitoring network/ingress policy. Нельзя добавлять user, project name, UUID, repository URL, Git ref или request ID как metric label: это создаёт неограниченную cardinality.

Минимальный набор:

| Metric | Тип | Labels |
|---|---|---|
| `forge_http_requests_total` | Counter | `method`, `route`, `status_class` |
| `forge_http_request_duration_seconds` | Histogram | `method`, `route`, `status_class` |
| `forge_http_in_flight_requests` | Gauge | `route` |
| `forge_db_pool_connections` | Gauge | `state` |
| `forge_db_query_duration_seconds` | Histogram | `operation`, `outcome` |
| `forge_pipelines_created_total` | Counter | `trigger` |
| `forge_pipeline_duration_seconds` | Histogram | `outcome` |
| `forge_jobs_total` | Counter | `status` |
| `forge_job_duration_seconds` | Histogram | `outcome`, `executor` |
| `forge_runner_heartbeat_age_seconds` | Gauge | `runner_pool` |
| `forge_queue_depth` | Gauge | `queue` |
| `forge_artifact_bytes_total` | Counter | `operation` |
| `forge_auth_failures_total` | Counter | `reason` |
| `forge_idempotency_replays_total` | Counter | `route` |

SLO-oriented alerts:

- readiness unavailable более 2 минут;
- error budget: 5xx rate выше agreed threshold;
- p95 API latency выше threshold для ключевых routes;
- queue depth или oldest queued job age выше threshold;
- runner heartbeat stale;
- database pool saturation;
- disk/artifact storage near capacity;
- значимый рост auth failures или rate limiting.

### 9.3 Логи

Backend пишет structured JSON в stdout. Обязательные поля:

```json
{
  "timestamp": "2026-08-26T10:05:00.123Z",
  "level": "INFO",
  "service": "forge-api",
  "environment": "production",
  "message": "request completed",
  "request_id": "01J8YJ2M79N01T4AZ7F9XBF0HM",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "span_id": "00f067aa0ba902b7",
  "http.method": "POST",
  "http.route": "/api/v1/projects/{project_id}/pipelines",
  "http.status_code": 201,
  "duration_ms": 42
}
```

Требования к логам:

- route template вместо raw URL, чтобы не записывать query secrets и не увеличивать cardinality;
- `INFO` для lifecycle и successful mutations, `WARN` для recoverable/denied conditions, `ERROR` для server/dependency failures;
- логировать error `code`, но не raw request body;
- job logs хранятся как domain data, а не дублируются в application log;
- values of secrets, passwords, access tokens, authorization headers, cookies, database URLs и private artifact content редактируются до записи;
- audit log не заменяет observability log и имеет собственную retention/authorization policy.

### 9.4 Traces

OpenTelemetry OTLP export отправляет traces в Tempo/Jaeger-compatible backend. Trace включает:

```text
HTTP server span
  -> auth middleware
  -> application use case
      -> authorization policy
      -> SQL/repository operation
      -> Git/artifact/runner adapter
      -> webhook/notification client
```

Правила:

- W3C propagation между API, runner, webhook workers и CLI;
- error spans включают typed error code и category, но не sensitive message/payload;
- sampling: ошибки и slow requests сохраняются с приоритетом; successful high-volume traffic sampled;
- trace ID присутствует в logs и может быть показан в error details для operator-only UI;
- database statements не содержат bound secrets в production telemetry.

### 9.5 Probes

| Endpoint | Назначение | DB | Возврат |
|---|---|---|---|
| `GET /api/v1/health` | Current liveness процесса backend. | Не проверяет. | `200` пока процесс способен обслуживать probe. |
| `GET /api/v1/readiness` | Current readiness API instance. | Проверяет connection/query timeout и обязательные SQLx migrations. | `200 ready`, иначе `503 not_ready`. |
| `GET /health/live` | Процесс жив и event loop отвечает. | Не проверяет. | `200` пока процесс способен обслуживать probe. |
| `GET /health/ready` | Инстанс может принимать traffic. | Проверяет connection/query timeout, обязательные migrations и критичные adapters. | `200 ready`, иначе `503 NOT_READY`. |
| `GET /health/startup` | Инициализация завершена. | Проверяет только startup prerequisites. | `200` после migrations/config/bootstrap. |
| `GET /metrics` | Scrape metrics. | Не является probe. | Prometheus text format. |

Target `/health/*` routes остаются будущим split для production ingress. Current `/api/v1/health` используется как liveness, а `/api/v1/readiness` — как DB/migration readiness; Docker/Kubernetes liveness не должен использовать readiness, иначе временная недоступность БД приведёт к restart loop.

## 10. CI quality gates

### 10.1 Обязательные проверки pull request

| Stage | Проверка | Failure condition |
|---|---|---|
| Contract | OpenAPI validate, lint, bundle, examples validation | Неизвестная schema, отсутствующие error/security responses, invalid examples. |
| Contract compatibility | Diff against default branch | Breaking change в `v1`. |
| Backend static | `cargo fmt --all -- --check`, clippy workspace/all-targets, release build | Formatting, warning или release build failure. |
| Backend test | Domain/app unit, real PostgreSQL integration, API contract | Любой failing test или недоступная required migration. |
| CLI | Help snapshot/contract, config precedence, output and exit-code tests | Изменён public CLI behaviour без explicit update. |
| Frontend static | frozen lockfile, lint, TypeScript build | Lint/type/build failure. |
| Frontend test | Component, feature and generated-client contract tests | Failing test. |
| Security | Dependency audit, secret scan, container scan по agreed severity policy | Confirmed secret или blocking vulnerability. |
| Container | `docker compose config`, image build, smoke startup | Невалидный compose, unhealthy container. |
| E2E | Critical journeys against real compose/test stack | Непройденный сценарий или missing evidence artifact. |

Rust workspace commands должны перейти на:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

Frontend:

```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm test
pnpm build
```

Coverage вводится как baseline ratchet: сначала публикуется report и фиксируется baseline, затем новый/изменённый код не может снижать coverage. Нельзя вводить произвольный процент без измеренного исходного уровня и исключений для generated code.

### 10.2 Database integration

API integration tests запускаются против настоящего PostgreSQL, а не `app(None)`:

- отдельный ephemeral database/schema на test worker;
- migrations применяются тем же механизмом, что и production;
- каждая test fixture создаёт только необходимые данные;
- тесты проверяют status, headers, full error envelope и persisted side effect;
- отдельно проверяются unique conflict, authorization, idempotency replay/conflict, cursor validity и unavailable dependency;
- cleanup не зависит от порядка выполнения тестов.

No-DB contract tests остаются полезны для liveness, extractor behavior и router wiring, но не считаются заменой persistence coverage.

### 10.3 E2E critical journeys

Playwright запускается против собранного frontend и real API/PostgreSQL compose stack. Критические сценарии:

1. Login, refresh, logout, redirect с protected route.
2. Viewer видит разрешённые данные и получает controlled 403 на mutation.
3. Developer создаёт project, запускает pipeline, видит его detail и async states.
4. Двойной submit/повтор запроса не создаёт второй pipeline.
5. Job lifecycle и live/loaded logs работают с pagination/auto-follow.
6. API token, CLI profile и CLI `--output json` выполняют эквивалентный automation flow.
7. Artifact upload/download соблюдает permission и size limit.
8. Mobile navigation, project list и pipeline detail проверяются на 375 px.
9. Readiness становится `503` при искусственно недоступной DB, while liveness остаётся `200`.
10. Trace/request ID из UI/CLI можно найти в backend structured logs.

## 11. Evidence bundle

Каждый PR с изменением API, UI, CLI, auth, migrations или observability публикует CI artifacts:

```text
artifacts/
├── openapi/
│   ├── openapi.yaml
│   └── compatibility-report.md
├── tests/
│   ├── backend-junit.xml
│   ├── frontend-junit.xml
│   ├── coverage/
│   └── compose-smoke.log
├── e2e/
│   ├── playwright-report/
│   ├── trace.zip
│   ├── screenshots/
│   │   ├── desktop-1440.png
│   │   ├── tablet-768.png
│   │   └── mobile-375.png
│   └── video.webm            # только для failed/retried flow
└── observability/
    ├── metrics-snapshot.prom
    ├── structured-log-sample.jsonl
    └── trace-correlation.md
```

Evidence requirements:

- Скриншоты не заменяют interaction assertions.
- Failed E2E сохраняет Playwright trace, screenshot, video и relevant compose logs.
- Снимки metrics/logs не содержат tokens, cookies, secrets, private repository URLs или job output с secret values.
- PR description содержит ссылки на CI artifacts и указывает, какие зависимости были mocked. Critical auth, DB and mutation flows не могут закрываться только mock evidence.
- Для contract-changing PR прикладывается OpenAPI diff и migration/deprecation note.

## 12. Поэтапная реализация

### Phase A — Фундамент контракта

**Цель:** сделать API измеримым и безопасным для эволюции без изменения большей части продукта.

- Ввести `backend/api` или переходный OpenAPI module.
- Описать и сгенерировать полный `openapi/openapi.yaml` для существующих routes.
- Ввести единый `AppError` и новый error envelope.
- Добавить request ID middleware и безопасное логирование errors.
- Добавить OpenAPI validation и breaking-change check в CI.
- Добавить integration test PostgreSQL stack и baseline contract tests.
- Сохранить legacy error format только через явно ограниченный compatibility adapter, если это требуется текущему frontend.

**Gate:** bundled OpenAPI валиден; все v1 operations имеют schemas/errors/security declaration; real-DB tests покрывают projects/pipelines/jobs; CI contract diff green.

### Phase B — Auth, pagination и идемпотентность

**Цель:** закрыть главные риски control-plane API.

- Реализовать migrations для sessions, token scopes и idempotency records.
- Включить auth middleware и application-level RBAC.
- Реализовать `/auth/*`, `/auth/me`, PAT lifecycle и audit events.
- Ввести page/cursor envelopes без silent breaking change.
- Реализовать `Idempotency-Key` для mutating operations.
- Добавить live/ready/startup probes.
- Мигрировать CLI на token/profile/config model.

**Gate:** E2E login/refresh/logout/RBAC green; duplicate trigger защищён; pagination stable при concurrent insert; readiness/liveness contract проверен.

### Phase C — Generated clients и FSD migration

**Цель:** убрать ручное расхождение frontend contract и API.

- Сгенерировать TypeScript schemas/client из committed OpenAPI artifact.
- Перенести transport/error handling в `shared/api`.
- Выделить `entities` и `features`; мигрировать срезами, не массовым rewrite.
- Вынести query keys из монолитного hooks file в entity slices.
- Реализовать auth provider, protected routes, 401 refresh queue и 403 UX.
- Ввести unified loading/empty/error/not-found/permission states.
- Проверить 375/768/1440/2560 layouts.

**Gate:** для migrated slices нет ручных server DTO; feature tests и E2E проходят; mobile evidence прикреплён; old client code удаляется только после полного перехода соответствующего endpoint.

### Phase D — CLI production contract

**Цель:** дать автоматизации стабильный и безопасный интерфейс.

- Разделить CLI на command/config/client/output/error modules.
- Подключить generated API DTO/client или verified shared public contract package.
- Реализовать profiles, keyring/token-env policy и request tracing.
- Добавить NDJSON mode, stable errors и exit code tests.
- Добавить pagination, `--all`, `--follow`, idempotency and safe confirmations.
- Документировать shell completion и automation examples.

**Gate:** CLI contract snapshots green; `--output json` пригоден для `jq`; error/exit behavior интеграционно проверен против real API; token не попадает в stdout/stderr/log fixtures.

### Phase E — Observability и операционная готовность

**Цель:** обнаруживать и диагностировать инциденты до пользовательского обращения.

- Добавить Prometheus metrics, OTLP tracing и trace propagation.
- Настроить structured logging schema/redaction.
- Ввести dashboards, alerts, runbook и retention policy.
- Добавить metrics/traces/probe checks в compose smoke.
- Запустить external blackbox probe в non-local environment.
- Добавить runner, queue, storage и webhook delivery indicators.

**Gate:** каждый critical API flow коррелируется по request/trace ID; alert rules тестируются; metrics labels проходят cardinality review; readiness failure не вызывает liveness restart loop.

### Phase F — Self-hosted delivery trust

**Цель:** постепенно сделать Forge исполнителем собственного pipeline без self-approval риска.

- Сначала запускать Forge pipeline параллельно GitHub Actions как non-blocking.
- Сопоставлять status, duration, logs и artifact evidence двух контуров.
- Проверить failure runner, retry, duplicate delivery, token revocation и degraded DB/storage.
- Сделать Forge required check только для ограниченной ветки после operational acceptance.
- Сохранить независимый внешний verification path для изменений control plane.

**Gate:** определённый период параллельных запусков без unexplained divergence; security review и rollback plan одобрены; внешний CI остаётся независимым источником доверия.

## 13. Definition of Done

Изменение delivery-контура завершено, только если:

- OpenAPI, API implementation, generated clients и docs согласованы.
- В `v1` не внесён breaking change без новой версии/deprecation plan.
- Все новые mutating операции имеют documented idempotency semantics.
- Ошибки возвращаются через typed safe envelope с `request_id`.
- Auth/RBAC проверяются backend-слоем и подтверждены E2E.
- UI имеет loading, empty, error, validation, 403 и mobile states.
- CLI имеет documented config precedence, deterministic output и tested exit codes.
- Metrics/logs/traces не содержат sensitive data и позволяют сопоставить request.
- Current backend/frontend/docs gates green; target container smoke, real-DB role matrix и E2E gates green для capabilities, которые их вводят.
- CI artifacts содержат contract diff, test reports, screenshots и traces/logs при failure.
- Документация обновлена: `docs/API.md`, `docs/API_VERSIONING.md`, `docs/CLI.md`, `docs/MONITORING.md`, `docs/TESTING.md`, `docs/CI_CD.md` и соответствующий ADR при архитектурном решении.

## 14. Связанные документы

- `docs/ARCHITECTURE.md`
- `docs/adr/0005-workspace-layered-architecture.md`
- `docs/API.md`
- `docs/API_STANDARDS.md`
- `docs/API_VERSIONING.md`
- `docs/PAGINATION.md`
- `docs/ERROR_HANDLING.md`
- `docs/FRONTEND_ARCHITECTURE.md`
- `docs/CLI.md`
- `docs/MONITORING.md`
- `docs/TESTING.md`
- `docs/CI_CD.md`

- Подготовлен полный текст для `docs/DELIVERY_ARCHITECTURE.md`; файлы не изменялись.
- Зафиксировано текущее состояние: ручные frontend DTO/hooks, выделенный но минимальный CLI, current OpenAPI/auth/idempotency/metrics/representative Playwright MVP и target-пробелы по compatibility/Lighthouse/full E2E.
- Целевой дизайн покрывает API contract, React FSD, CLI, observability, CI/E2E evidence и поэтапные gates.
