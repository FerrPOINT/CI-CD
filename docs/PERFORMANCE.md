# Производительность и оптимизация — Forge CI/CD

## 1. Цели

- P95 ответа API < 200 мс при 50 RPS на типичных запросах.
- Загрузка деталей пайплайна (stages + jobs) < 100 мс.
- Запись лога задачи (append) < 50 мс.
- 99.5% uptime на single-instance deployment.

> Цели ориентировочные — load testing запланирован в Phase 2+.

## 2. База данных

### 2.1 Connection Pool

SQLx `PgPoolOptions` настраивается при старте приложения:

```rust
// backend/src/main.rs
let pool = sqlx::postgres::PgPoolOptions::new()
    .max_connections(10)
    .connect(&database_url)
    .await?;
```

| Параметр | Значение | Описание |
|----------|----------|----------|
| `max_connections` | 10 | Максимум одновременных соединений |
| `acquire_timeout` | 30s (default) | Timeout получения соединения из пула |
| `idle_timeout` | 10m (default) | Timeout закрытия idle соединения |
| `min_connections` | 0 (default) | Минимум idle соединений |

> **Тюнинг (Phase 2+):** `max_connections` должен быть `(CPU_cores * 2) + effective_io_concurrency`. Для single-instance с 2 CPU — 10 соединений достаточно.

### 2.2 Индексы

См. `docs/DATABASE_INDEXES.md` для полного перечня.

Ключевые индексы для производительности:

| Query | Индекс | Статус |
|-------|--------|--------|
| `WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50` | `(project_id, created_at DESC)` | TODO |
| `WHERE pipeline_id = $1 ORDER BY position` | `(pipeline_id, position)` UNIQUE | ✅ |
| `WHERE stage_id = $1 ORDER BY position` | `(stage_id, position)` UNIQUE | ✅ |
| `WHERE job_id = $1 ORDER BY sequence` | `(job_id, sequence)` UNIQUE | ✅ |

> UNIQUE constraints на `(pipeline_id, position)`, `(stage_id, position)`, `(job_id, sequence)` уже создают B-tree индексы, покрывающие соответствующие запросы.

### 2.3 Query optimization

| Паттерн | Оптимизация |
|---------|-------------|
| `list_pipelines` | `LIMIT 50` + `ORDER BY created_at DESC` + индекс на `(project_id, created_at DESC)` |
| `pipeline_detail` | Один запрос для pipeline + два для stages и jobs (или JOIN) |
| `list_logs` | `ORDER BY sequence ASC` — покрывается UNIQUE индексом |
| `append_log` | `COALESCE(MAX(sequence), 0) + 1` — один SELECT + один INSERT |
| `refresh_statuses` | `SELECT status FROM ... WHERE ...` — агрегация в приложении |

### 2.4 N+1 prevention

Текущий `pipeline_detail` выполняет 3 запроса:
1. `SELECT * FROM pipelines WHERE id = $1`
2. `SELECT * FROM stages WHERE pipeline_id = $1 ORDER BY position`
3. `SELECT * FROM jobs WHERE stage_id IN (...) ORDER BY position`

> **Планируется (Phase 2):** один запрос с JOIN:
> ```sql
> SELECT p.*, s.id AS stage_id, s.name AS stage_name, s.position AS stage_position,
>        s.status AS stage_status, j.id AS job_id, j.name AS job_name, ...
> FROM pipelines p
> LEFT JOIN stages s ON s.pipeline_id = p.id
> LEFT JOIN jobs j ON j.stage_id = s.id
> WHERE p.id = $1
> ORDER BY s.position, j.position;
> ```

### 2.5 Query timeout

- Текущее значение: default SQLx (нет явного timeout).
- **Планируется (Phase 2+):** 5 сек на уровне приложения через `tokio::time::timeout`.

## 3. API

### 3.1 Pagination

| Endpoint | Текущее поведение | Планируемое |
|----------|-------------------|-------------|
| `GET /projects` | Все записи | `?page=1&size=20`, max 100 |
| `GET /projects/{id}/pipelines` | `LIMIT 50` | `?page=1&size=20`, max 100 |
| `GET /jobs/{id}/logs` | Все записи | Cursor-based (append-only) |

- `LIMIT 50` на pipelines list — предотвращает full table scan при росте данных.
- См. `docs/PAGINATION.md`.

### 3.2 Response size

| Endpoint | Типичный размер | Максимум |
|----------|-----------------|----------|
| `GET /health` | ~50 B | ~50 B |
| `GET /projects` | ~1 KB (10 проектов) | ~10 KB (100 проектов) |
| `GET /pipelines/{id}` | ~3 KB (3 stages × 1 job) | ~50 KB (10 stages × 10 jobs) |
| `GET /jobs/{id}/logs` | ~1 KB (10 логов) | ~1 MB (1000 логов) |

> **Планируется (Phase 2+):** gzip/brotli compression через `tower-http::compression`.

### 3.3 Request timeout

- Текущее значение: нет явного timeout (default Axum / hyper).
- **Планируется (Phase 2+):** 30 сек на уровне Axum `TimeoutLayer`.

## 4. Frontend

### 4.1 Code splitting

Vite автоматически split code по dynamic imports:

```typescript
// Lazy load route components
const Dashboard = lazy(() => import("./dashboard"));
const PipelineDetail = lazy(() => import("./pipeline-detail"));

// Suspense wrapper
<Suspense fallback={<Loading />}>
  <Routes>
    <Route path="/" element={<Dashboard />} />
    <Route path="/pipelines/:id" element={<PipelineDetail />} />
  </Routes>
</Suspense>
```

| Часть bundle | Размер (approx) | Загрузка |
|--------------|-----------------|----------|
| React + ReactDOM | ~45 KB gzip | Initial |
| shadcn/ui components | ~20 KB gzip | Initial / lazy |
| Dashboard page | ~10 KB gzip | Initial |
| Pipeline detail | ~8 KB gzip | Lazy (при переходе) |

### 4.2 Vite build

```bash
pnpm build  # vite build → dist/
```

- Vite 6 — автоматический code splitting по dynamic imports.
- Tree shaking — неиспользуемые экспорты удаляются.
- CSS — Tailwind 4 purge неиспользуемых классов.
- Source maps — отключены в production build.

### 4.3 API-клиент

Типизированная обёртка над `fetch`:

```typescript
const api = async <T>(path: string, init?: RequestInit): Promise<T> => {
  const response = await fetch(`/api/v1${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  if (!response.ok) {
    const { error } = await response.json();
    throw new Error(error);
  }
  return response.json();
};
```

> **Планируется (Phase 2):** `@tanstack/react-query` для кеширования, background refetch, optimistic updates.

### 4.4 Frontend производительность

| Метрика | Цель | Текущее |
|---------|------|---------|
| LCP (Largest Contentful Paint) | < 1.5s | Не измерено |
| INP (Interaction to Next Paint) | < 200ms | Не измерено |
| CLS (Cumulative Layout Shift) | < 0.1 | Не измерено |
| Bundle size (gzip) | < 200 KB | ~75 KB |

> **Планируется (Phase 2):** `web-vitals` library для измерения Core Web Vitals.

## 5. Docker

### 5.1 Backend image

- Multi-stage build: `rust:1.86-slim` → `debian:bookworm-slim`.
- Release build: `cargo build --release` — оптимизированный бинарник.
- Image size: ~50 MB (статический бинарник + minimal runtime).
- Non-root user `cicd` (uid 10001).

### 5.2 Frontend image

- Multi-stage build: `node:22-bookworm-slim` → `nginx:1.27-alpine`.
- Static files served nginx — минимальная нагрузка.
- Image size: ~25 MB.
- gzip compression включён в nginx.conf.

## 6. Масштабирование

### Текущее

- Single-instance deployment.
- Stateless backend (вся сессия — в PostgreSQL).
- Connection pool: 10 соединений.

### Планируемое (Phase 5+)

- Horizontal scaling: stateless app instances за load balancer.
- PostgreSQL read replica для read-heavy queries (pipeline detail, logs).
- CDN для frontend static assets.
- Object storage для artifacts (Phase 8).

## 7. Load Testing (Phase 2+)

- Инструмент: `k6` / `oha` / `drill`.
- Сценарии:
  - Create project + trigger pipeline + get pipeline detail.
  - List pipelines (50 records).
  - Append 100 log lines + read logs.
  - Job status transitions (queued → running → success).
- Целевые параметры: 50 RPS, P95 < 200 мс.

## References

- `docs/DATABASE_INDEXES.md` — индексы и стратегия.
- `docs/DATABASE_STANDARDS.md` — connection pool, типы данных.
- `docs/PAGINATION.md` — текущее и планируемое состояние пагинации.
- `docs/ARCHITECTURE.md` — middleware stack, Docker build.
- `docs/MONITORING.md` — метрики и observability.