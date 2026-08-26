# Caching Strategy — Forge CI/CD

## 1. Overview

Кеширование в Forge CI/CD решает две задачи: снижение нагрузки на PostgreSQL и ускорение отдачи статических ресурсов Dashboard. Стратегия вводится поэтапно — от HTTP-кеша статики к условным запросам API и распределённому кешу в будущих фазах.

> **Текущий статус:** кеширование на backend не реализовано (Phase 0). Все запросы идут напрямую в PostgreSQL. Frontend использует TanStack Query для кеширования server state на клиенте.

---

## 2. Текущее состояние (Phase 0)

### 2.1. Backend

- Кеш отсутствует. Все запросы (`GET /projects`, `GET /pipelines/{id}`, `GET /jobs/{id}/logs`) выполняются напрямую в PostgreSQL через SQLx `PgPool`.
- `PgPool` управляет пулом соединений (`max_connections: 10`), что частично компенсирует отсутствие кеша.
- Нет HTTP-заголовков кеширования на API-ответах (`Cache-Control`, `ETag`, `Last-Modified`).

### 2.2. Frontend

- TanStack Query (`@tanstack/react-query` 5.74.4) — единственный уровень кеширования.
- Конфигурация (`frontend/src/app/provider.tsx`):
  - `retry: 1` — одна попытка повтора при ошибке.
  - `refetchOnWindowFocus: false` — отключён авто-рефетч.
- Ключи запросов централизованы в `frontend/src/api/hooks.ts`:
  ```ts
  const KEYS = {
    projects: ['projects'] as const,
    pipelines: (projectId: string) => ['pipelines', projectId] as const,
    pipeline: (id: string) => ['pipeline', id] as const,
    logs: (jobId: string) => ['logs', jobId] as const,
  }
  ```
- Мутации (`useCreateProject`, `useTriggerPipeline`, `useUpdateJobStatus`, `useAppendLog`) инвалидируют связанные query keys.

### 2.3. Nginx (production)

- `frontend/nginx.conf` — минимальная конфигурация, без cache-control заголовков:
  ```nginx
  server {
    listen 80;
    server_name _;
    root /usr/share/nginx/html;
    location /api/ { proxy_pass http://backend:22801; }
    location / { try_files $uri $uri/ /index.html; }
  }
  ```
- Статические assets Vite (`/assets/*.js`, `/assets/*.css`) отдаются без `Cache-Control`.

---

## 3. Плановое: HTTP cache для статики

### 3.1. Nginx cache-control для Vite assets

Vite генерирует content-hashed имена (`index-a1b2c3d4.js`), поэтому assets можно кешировать агрессивно:

```nginx
# Иммутальные assets с hash в имени — 1 год
location /assets/ {
  expires 1y;
  add_header Cache-Control "public, immutable";
  try_files $uri =404;
}

# index.html — всегда свежий (no-cache)
location = /index.html {
  add_header Cache-Control "no-cache, no-store, must-revalidate";
}

# SPA fallback
location / {
  try_files $uri $uri/ /index.html;
}
```

### 3.2. Время внедрения

- Phase 2 (Projects) — при обновлении `nginx.conf` для production-сборки.
- Не требует изменений в backend.

---

## 4. Плановое: ETag для GET пайплайнов

### 4.1. Принцип

- `GET /api/v1/pipelines/{id}` возвращает `ETag` — хеш от содержимого (pipeline + stages + jobs).
- Клиент отправляет `If-None-Match: <etag>` → если данные не изменились, сервер возвращает `304 Not Modified` без body.
- `GET /api/v1/projects` — `ETag` на основе списка проектов (хеш от `MAX(created_at)` или checksum).

### 4.2. Реализация (план)

```rust
// backend/src/api.rs
use axum::response::IntoResponse;
use std::collections::hash_map::DefaultHasher;

async fn get_pipeline(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let detail = state.store.pipeline_detail(id).await?;
    let etag = format!("\"{}\"", hash_pipeline_detail(&detail));

    if let Some(if_none_match) = headers.get("if-none-match") {
        if if_none_match.to_str().unwrap_or("") == etag {
            return StatusCode::NOT_MODIFIED.into_response();
        }
    }

    let mut response = Json(detail).into_response();
    response.headers_mut().insert("ETag", etag.parse().unwrap());
    response
}
```

### 4.3. Применимость

| Endpoint | ETag | Обоснование |
|---|---|---|
| `GET /projects` | ✅ | Список меняется редко |
| `GET /projects/{id}/pipelines` | ✅ | Новые пайплайны — изменение списка |
| `GET /pipelines/{id}` | ✅ | Детали меняются при status transitions |
| `GET /jobs/{id}/logs` | ❌ | Append-only, ETag неэффективен; использовать `?since_sequence=N` |

### 4.4. Время внедрения

- Phase 2+ — при оптимизации API после load testing.
- Не требует Redis; ETag вычисляется in-process.

---

## 5. Плановое: Redis (Phase 5+)

### 5.1. Когда потребуется Redis

- Multi-instance deployment (несколько backend-инстансов за load balancer).
- SSE pub/sub для broadcast событий между инстансами (Phase 6).
- Distributed rate limiting (Future).
- Job queue для runner-ов (если PostgreSQL `SELECT FOR UPDATE SKIP LOCKED` окажется недостаточен).

> **Важно:** Redis не вводится без ADR и измеримого обоснования (см. ADR-0004, Consequences: «отдельные queue/cache хранилища добавляются только с отдельным ADR и измеримым обоснованием»).

### 5.2. Предполагаемая архитектура (план)

```
┌─────────────┐     ┌───────────────┐     ┌─────────┐
│  Client     │────▶│  Axum API     │────▶│  Redis  │
│             │     │  (cache-aside)│     │  (L2)   │
└─────────────┘     └───────┬───────┘     └────┬────┘
                            │                  │
                     ┌──────▼──────┐    ┌──────▼──────┐
                     │  In-memory  │    │  PostgreSQL │
                     │  moka (L1)  │    │  (source)   │
                     └─────────────┘    └─────────────┘
```

### 5.3. Cache key convention (план)

```
cicd:{entity}:{id}[:{version}]
```

Примеры:

- `cicd:project:{uuid}`
- `cicd:pipeline:{uuid}`
- `cicd:pipeline:{uuid}:detail`
- `cicd:projects:list`

### 5.4. Что кешировать (план)

| Data | Cache | TTL | Invalidation |
|------|-------|-----|--------------|
| Project by id | Redis | 10 min | on update/delete |
| Pipeline detail | Redis | 5 min | on status change |
| Projects list | moka | 2 min | on create/delete |
| Job logs | ❌ | — | Append-only, не кешировать |

### 5.5. Что НЕ кешировать

- Пароли, токены, secrets.
- Данные с частыми writes и редкими reads.
- Job logs (append-only, streaming).
- Большие бинарные файлы (artifacts — в S3/FS).

---

## 6. Frontend Query Caching

### 6.1. Текущая конфигурация

```ts
// frontend/src/app/provider.tsx
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
})
```

### 6.2. Per-Entity stale time (план)

| Entity | Stale Time | Обоснование |
|--------|------------|-------------|
| Projects list | 2 min | Меняется редко |
| Pipeline detail | 30 sec | Status changes |
| Job logs | 10 sec | Append-only, streaming в Phase 4 |
| Admin system info | 5 min | Статичные данные |

### 6.3. Инвалидация при SSE-событиях (Phase 6)

При получении SSE-события frontend инвалидирует связанные query keys:

```ts
const eventSource = new EventSource('/api/v1/events/stream')

eventSource.addEventListener('pipeline.status.changed', (e) => {
  const data = JSON.parse(e.data)
  queryClient.invalidateQueries({ queryKey: ['pipeline', data.pipelineId] })
  queryClient.invalidateQueries({ queryKey: ['pipelines', data.projectId] })
})

eventSource.addEventListener('job.status.changed', (e) => {
  const data = JSON.parse(e.data)
  queryClient.invalidateQueries({ queryKey: ['pipeline', data.pipelineId] })
  queryClient.invalidateQueries({ queryKey: ['logs', data.jobId] })
})
```

> См. `docs/EVENTS.md`.

---

## 7. Anti-Patterns

- Не кешировать результаты мутаций (POST/PATCH/DELETE).
- Не кешировать без invalidation стратегии.
- Не кешировать персональные данные на shared уровне (до RBAC).
- Не делать cache TTL больше допустимого time-to-inconsistent.
- Не использовать `format!` для построения cache keys из user input (injection risk).

---

## 8. Monitoring (план)

- Hit/miss ratio по namespace (после внедрения Redis).
- Cache size / memory usage.
- Alerts при cache eviction rate > 50%.
- Метрика `cache_last_success_timestamp`.

> См. `docs/MONITORING.md`, `docs/PERFORMANCE.md`.

---

## 9. References

- `docs/ARCHITECTURE.md` — общая архитектура.
- `docs/PERFORMANCE.md` — производительность и цели.
- `docs/FRONTEND_ARCHITECTURE.md` — TanStack Query конфигурация.
- `docs/EVENTS.md` — SSE-события и инвалидация кеша.
- `docs/adr/0004-postgresql-only.md` — решение о единственной СУБД (Redis — отдельный ADR).
- `docs/ROADMAP.md` — план разработки.