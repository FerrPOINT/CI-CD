Статус: Accepted target contract

[ADR-0009: канонический реестр имён и приоритет источников](../adr/0009-canonical-registry.md)

# UI API contract

## 1. Границы frontend-контракта

| Область | Правило |
|---|---|
| Источник DTO | Только `openapi/openapi.yaml`; generated schema расположен в `frontend/src/api/schema.d.ts`. |
| Generated code | Не редактируется вручную; regeneration и `git diff --exit-code` обязательны в CI. |
| Handwritten transport | `frontend/src/api/client.ts` и feature-level wrappers оборачивают generated schema для headers, error decoding, binary upload/download и SSE. Generated transport остаётся target. |
| UI policy | Query keys, cache invalidation, retry, redirect, toast и presentation errors не генерируются. |
| Raw fetch | Page, widget и feature не вызывают raw `fetch`; используют entity/feature API поверх typed transport. |
| DTO duplication | Для migrated operation запрещены ручные DTO и hand-written serialization. |
| Внутренние API | Operation с `x-forge-internal: true` не доступна из browser client. |

| Transport обязан | Transport не должен |
|---|---|
| Добавлять `Accept: application/json`, correlation `X-Request-Id`, auth headers и `Idempotency-Key`, когда mutation требует его. | Делать toast, redirect, invalidate cache или принимать UI-решение. |
| Возвращать generated typed DTO и преобразовывать structured server error в `ApiError`. | Терять `code`, `request_id`, `details`, HTTP status или `Retry-After`. |
| Различать cancellation, network failure и HTTP server response. | Преобразовывать Abort/Cancellation в retryable user-facing error. |
| Использовать response entity как authoritative state. | Угадывать успешный server state из отправленного request. |

## 2. Typed `ApiError`

```ts
export type ApiError = {
  kind: 'api' | 'network' | 'cancelled'
  status?: number
  code?: string
  message: string
  request_id?: string
  details?: Array<{ field?: string; code: string; message: string }>
  retry_after_seconds?: number
}
```

| Источник | `kind` | Поля и поведение |
|---|---|---|
| JSON error envelope | `api` | Прочитать `error.code`, `error.message`, `error.request_id`, `error.details`; сохранить HTTP status и `Retry-After`. |
| Non-JSON HTTP error | `api` | Сохранить status; показать безопасное generic message; не интерпретировать body как trusted text. |
| DNS/TLS/offline/timeout | `network` | Нет server code/request ID; может быть retryable. |
| `AbortError` или отмена TanStack Query | `cancelled` | Не показывать error state, toast или retry control. |

`ApiError` не содержит request body, authorization header, cookie, token, raw response body или stack trace. UI локализует known `code`, но показывает server-safe `message` как fallback и всегда сохраняет `request_id` для support details.

## 3. Query-key conventions

Каждый entity slice экспортирует immutable factory с иерархией `all -> collection -> scoped collection -> detail -> subresource`. Query parameters входят в key как canonical serializable object; `undefined` keys удаляются, массивы и даты нормализуются до стабильного representation.

```ts
export const projectKeys = {
  all: ['projects'] as const,
  lists: () => [...projectKeys.all, 'list'] as const,
  list: (params: ProjectListParams) => [...projectKeys.lists(), params] as const,
  details: () => [...projectKeys.all, 'detail'] as const,
  detail: (projectId: string) => [...projectKeys.details(), projectId] as const,
}

export const pipelineKeys = {
  all: ['pipelines'] as const,
  lists: () => [...pipelineKeys.all, 'list'] as const,
  byProject: (projectId: string, params: PipelineListParams) =>
    [...pipelineKeys.lists(), 'project', projectId, params] as const,
  detail: (pipelineId: string) => [...pipelineKeys.all, 'detail', pipelineId] as const,
}

export const jobKeys = {
  all: ['jobs'] as const,
  detail: (jobId: string) => [...jobKeys.all, 'detail', jobId] as const,
  logs: (jobId: string, params: JobLogListParams) =>
    [...jobKeys.all, 'logs', jobId, params] as const,
}
```

| Правило | Требование |
|---|---|
| Owner | Key factory находится в entity slice ресурса, не в page и не в shared global map. |
| Сегменты | Строковые, resource-first; ID расположен после type segment; list и detail не имеют общий exact key. |
| Parameters | Включают только response-shaping parameters: filter, sort, pagination cursor/limit/page. |
| Не включать | UI-only state, callbacks, functions, request ID, idempotency key, token, transient loading state. |
| Infinite/cursor list | Key описывает scope и immutable filters; текущий cursor хранится в `pageParam`, а не размножает resource key. |
| Status enum | UI имеет fallback state для неизвестного server enum value. |

## 4. Invalidation и cache update

Mutation invalidates минимальное достаточное resource scope. Broad `queryClient.invalidateQueries()` без `queryKey` запрещён.

| Mutation | Успешное действие cache |
|---|---|
| Create project | Invalidate `projectKeys.lists()`; при наличии response можно добавить/заменить matching first-page cache. |
| Update/delete project | Invalidate `projectKeys.detail(projectId)` и `projectKeys.lists()`; delete удаляет exact detail. |
| Trigger pipeline | Invalidate `pipelineKeys.lists()` только scope соответствующего project; записать response в `pipelineKeys.detail(id)`. |
| Pipeline cancel/retry | Invalidate `pipelineKeys.detail(pipelineId)` и список project исходного pipeline; invalidate связанных job detail при известном составе. |
| Job status change | Invalidate `jobKeys.detail(jobId)`, `pipelineKeys.detail(pipelineId)` и затронутый project pipeline list. |
| Append job log | Invalidate `jobKeys.logs(jobId, ...)` или append authoritative response в matching active cache; не invalidate все jobs/pipelines. |

| Optimistic update | Контракт |
|---|---|
| Разрешён | Только для однозначно обратимой операции с корректным snapshot и известным affected cache. |
| Перед mutation | Cancel affected query, сохранить snapshot, применить reversible optimistic state. |
| Ошибка | Восстановить snapshot; показать error согласно разделу 5. |
| Успех | Заменить/синхронизировать state response entity; затем invalidate описанный scope. |
| Не разрешён | Create/trigger/retry/cancel/upload и любая операция, результат которой сервер может нормализовать, отклонить по state или выполнить асинхронно. |

Каждая retryable mutation передаёт один `Idempotency-Key` на пользовательское намерение. Повтор из UI использует тот же key; новое нажатие после завершённого flow создаёт новый key. Mutation без задекларированной server idempotency не повторяется автоматически.

## 5. Query retry semantics

| Категория | Автоматический retry query | UI-действие |
|---|---|---|
| `cancelled` | Нет | Сохранить текущий UI без error state. |
| `400`, `401`, `403`, `404`, `409`, `412`, `413`, `422` | Нет | Обработать согласно статусу; не создавать retry loop. |
| `429 rate_limited` | Не до `Retry-After`; затем максимум одна отложенная попытка, если query ещё нужен | Показать, когда доступно повторить; manual retry всегда доступен. |
| `500`, `503` | До двух повторов с exponential backoff и jitter | Данные остаются видимыми при background failure; дать manual retry. |
| `network` | До двух повторов с exponential backoff и jitter | Inline network state и manual retry. |

Default query policy: не retry mutations автоматически; не refetch on window focus для expensive lists/logs без explicit feature policy; background refetch не заменяет уже показанные данные пустым loading state. Query включается только при валидном required ID/scope (`enabled`), а route params валидируются до создания key.

## 6. Error-state contract

| Сценарий | Обязательное UI-поведение |
|---|---|
| Initial query loading | Skeleton сохраняет структуру страницы; не blank screen. |
| Background refresh | Предыдущие данные остаются видимыми, есть ненавязчивый indicator. |
| Empty 2xx result | Контекстное empty state и разрешённое action; не ошибка. |
| `401` | Единственный shared refresh flow; параллельные requests ожидают его. После success исходный request повторяется один раз. После failure очистить QueryClient и перейти на `/login` только с безопасным internal return URL. |
| `403 permission_denied` | Отдельный permission-denied state; не redirect на login и не маскировка как `404`. |
| `404 not_found` | Отдельный not-found state с безопасной навигацией. |
| `400` / `422 validation_failed` | Field messages из `details`; focus на первом invalid field. Form values сохраняются. |
| `409 conflict` / `412` | Inline conflict state, предложить reload authoritative data или явное повторное действие; не retry автоматически. |
| `429 rate_limited` | Inline rate-limit state, countdown из `Retry-After`, disabled duplicate submit до разрешённого retry. |
| `500`, `503`, `network` | Inline retryable state с кнопкой retry; request ID показывается в technical details при наличии. |
| Fatal render/application error | Error boundary с reload, переходом на dashboard и request ID, если он известен. |

Toast служит только вторичным кратким подтверждением или уведомлением. Критичная query/mutation ошибка, validation, permission и pending state обязаны иметь persistent inline representation.

## 7. Mutation pending и double-submit

| Требование | Правило |
|---|---|
| Pending | Action control disabled, label отражает выполняемое действие; form не исчезает. |
| Double submit | Один UI intent создаёт одну mutation и один idempotency key; повторное нажатие заблокировано. |
| Retry | Пользователь видит, будет ли использован прежний idempotency key; retry нельзя выполнить скрытно. |
| Success | Server response -- источник истины; redirect/cache update выполняются после response. |
| Error | Пользовательские поля и безопасные details сохраняются; технические данные ограничены `code` и `request_id`. |

## 8. Тестовые инварианты frontend

| Область | Минимальная проверка |
|---|---|
| Generated contract | `pnpm api:generate`, `pnpm api:check` и clean git diff. |
| Error mapping | Envelope, non-JSON HTTP, network и cancellation формируют разные `ApiError`. |
| Query keys | Same logical parameters дают equal key; UI/transient values в key не попадают. |
| Invalidation | Каждая mutation invalidates только утверждённые scopes; нет global invalidation. |
| Retry | Нет auto-retry для mutation и client errors; 429 соблюдает `Retry-After`; network/5xx ограничены. |
| UX | Tests покрывают loading, empty, validation, 403, 404, retryable failure, pending и double-submit. |
| Idempotency | Повтор mutation использует исходный key; новый user intent создаёт новый key. |
