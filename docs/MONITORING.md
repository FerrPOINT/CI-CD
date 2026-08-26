# Monitoring — Forge CI/CD

## 1. Overview

Мониторинг Forge CI/CD покрывает health-checks, логирование и tracing. Полный стек метрик (Prometheus, Grafana) запланирован в Phase 5+.

> **Текущий статус:** Phase 1 — реализован health endpoint и structured logging. Метрики и alerting — TODO.

## 2. Health Endpoint

### 2.1 Реализованный endpoint

```
GET /api/v1/health
```

**Response 200:**
```json
{
  "status": "ok",
  "service": "cicd"
}
```

- Не требует подключения к БД.
- Работает даже когда `AppState.pool = None` (режим без БД).
- Используется в Docker healthcheck и docker-compose.

### 2.2 Docker healthcheck

```dockerfile
# backend/Dockerfile
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD wget --quiet --tries=1 --spider http://127.0.0.1:22801/api/v1/health || exit 1
```

> Используется `wget`, а не `curl`, т.к. `curl` отсутствует в `debian:bookworm-slim`. Подробнее в `docs/TROUBLESHOOTING.md`.

### 2.3 Планируемые endpoint (Phase 2+)

| Endpoint | Purpose | Зависимости |
|----------|---------|-------------|
| `GET /api/v1/health` | Liveness | Нет |
| `GET /api/v1/health/ready` | Readiness | DB connection check |
| `GET /api/v1/metrics` | Prometheus metrics | — |

```rust
// Планируемая реализация readiness (Phase 2):
async fn health_ready(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match &state.pool {
        Some(pool) => match sqlx::query("SELECT 1").execute(pool).await {
            Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))),
            Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"status": "not ready"}))),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"status": "no database"}))),
    }
}
```

## 3. Tracing

### 3.1 Tracing subscriber

Инициализация при старте приложения (`backend/src/main.rs`):

```rust
tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .with_target(true)
    .with_file(true)
    .with_line_number(true)
    .json()
    .init();
```

- Формат — JSON в production (настраивается через `RUST_LOG`).
- `EnvFilter` — уровень логирования через `RUST_LOG` env var.
- `with_target` — включает module path в каждой записи.
- `with_file` / `with_line_number` — включает источник в dev-режиме.

### 3.2 Request tracing

HTTP-запросы трассируются через `tower-http` `TraceLayer`:

```rust
.layer(TraceLayer::new_for_http())
```

- Логируется: метод, путь, статус, duration.
- Каждый запрос получает span с автоматически сгенерированным ID.
- Span propagation — через `tracing` context.

### 3.3 Span структура

```
HTTP request
├── handler (api.rs)
│   ├── sqlx::query (store.rs)
│   └── sqlx::query (store.rs)
└── response
```

> **Планируется (Phase 5+):** distributed tracing через OpenTelemetry OTLP exporter → Tempo/Jaeger.

## 4. RUST_LOG

### 4.1 Уровни

| Level | Использование |
|-------|---------------|
| `error` | 5xx ошибки, недоступность БД |
| `warn` | Recoverable ошибки, deprecated endpoint |
| `info` | Запуск/остановка сервиса, миграции, HTTP-запросы (default) |
| `debug` | SQL-запросы, детали обработки |
| `trace` | Детальная отладка |

### 4.2 Конфигурация

| Env var | Default | Описание |
|---------|---------|----------|
| `RUST_LOG` | `info` | Уровень логирования |

Примеры:

```bash
# Default
RUST_LOG=info

# Debug backend + SQLx queries
RUST_LOG=cicd=debug,sqlx=trace

# Production
RUST_LOG=info,cicd=warn

# Silent (только ошибки)
RUST_LOG=error
```

### 4.3 Per-module настройка

```
RUST_LOG=cicd=debug,sqlx=warn,tower_http=debug
```

- `cicd` — модули приложения (`api`, `domain`, `store`).
- `sqlx` — SQL-запросы и connection pool.
- `tower_http` — HTTP middleware (cors, trace).
- `hyper` — низкоуровневый HTTP server (обычно `warn`).

## 5. Метрики (Phase 5+)

### 5.1 Планируемые метрики (Prometheus)

| Metric | Type | Description |
|--------|------|-------------|
| `http_requests_total` | counter | Total requests by method, route, status |
| `http_request_duration_seconds` | histogram | Request latency |
| `db_pool_connections` | gauge | Active/idle DB connections |
| `db_query_duration_seconds` | histogram | Query latency |
| `pipelines_total` | counter | Pipelines created |
| `pipelines_active` | gauge | Pipelines in non-terminal status |
| `jobs_total` | counter | Jobs by status |
| `job_logs_total` | counter | Log lines appended |

### 5.2 Планируемая реализация

```rust
// Phase 5+: metrics crate + Prometheus exporter
use metrics::{counter, histogram, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;

PrometheusBuilder::new()
    .install_recorder()
    .expect("failed to install Prometheus recorder");
```

Endpoint: `GET /api/v1/metrics` → Prometheus text format.

### 5.3 Business metrics (Phase 5+)

| Metric | Description |
|--------|-------------|
| `pipelines_success_total` | Успешные пайплайны |
| `pipelines_failed_total` | Провальные пайплайны |
| `pipeline_duration_seconds` | Время выполнения пайплайна |
| `active_projects` | Проекты с активностью за 24ч |

## 6. Alerting (Phase 5+)

### 6.1 Critical

- API down > 1 мин.
- БД недоступна.
- 5xx rate > 5%.
- Disk > 85%.

### 6.2 Warning

- 4xx rate > 20%.
- P95 latency > 500 мс.
- Pipeline failure rate > 30%.
- Connection pool utilization > 80%.

## 7. Dashboard (Phase 5+)

| Dashboard | Panels |
|-----------|--------|
| API Overview | RPS, latency, status codes |
| Database | Pool stats, query time, slow queries |
| Pipelines | Active, success rate, duration |
| Jobs | Queue depth, by status, duration |
| Infrastructure | CPU, memory, disk, network |

## 8. Uptime Monitoring

- Docker healthcheck каждые 30s.
- Планируется (Phase 5+): external uptime check (`/api/v1/health`) каждые 60s.
- Планируется (Phase 5+): Blackbox exporter или UptimeRobot.

## 9. Логирование

Структурированные JSON-логи через `tracing` crate. Подробнее в `docs/LOGGING_STANDARDS.md`.

```bash
# Просмотр логов backend
docker compose logs -f backend

# Просмотр логов с фильтром
docker compose logs -f backend | jq 'select(.level == "ERROR")'
```

## 10. Конфигурация мониторинга

```bash
# .env
RUST_LOG=info
CICD_BIND=0.0.0.0:22801
```

> Планируется (Phase 5+): отдельный `docker-compose.monitoring.yml` с Prometheus + Grafana + Loki.

## References

- `docs/LOGGING_STANDARDS.md` — стандарты логирования и tracing.
- `docs/PERFORMANCE.md` — производительность и connection pooling.
- `docs/ARCHITECTURE.md` — общая архитектура и middleware stack.
- `docs/ROADMAP.md` — фазы разработки.
- `backend/src/main.rs` — инициализация tracing subscriber.