# Logging Standards — Forge CI/CD

> Стартовый документ. До стабилизации observability-стека (Phase 5+) часть соглашений может измениться.

## 1. Scope

Соглашения по логированию и tracing в backend и CLI Forge CI/CD. Frontend логирование — `console.log` в dev, интеграция с внешним сервисом — TODO.

## 2. Цели

- Быстро находить причину ошибок control plane.
- Сопоставлять запросы между слоями (HTTP → domain → store → DB).
- Не допускать утечки sensitive-данных в логи.
- Снижать объём логов в production без потери диагностической ценности.

## 3. Технологии

| Компонент | Библиотека | Версия | Назначение |
|-----------|-----------|--------|------------|
| Logging / tracing | `tracing` | 0.1 | Structured logs + spans |
| Subscriber | `tracing-subscriber` | 0.3 | JSON/pretty output, env filter |
| HTTP request tracing | `tower-http` (TraceLayer) | 0.6 | Automatic HTTP request logging |
| Env filter | `tracing-subscriber::EnvFilter` | — | Уровень через `RUST_LOG` |

## 4. Уровни логирования

| Level | Когда использовать | Примеры |
|-------|-------------------|---------|
| `ERROR` | Ошибка, требующая внимания | 5xx, недоступность БД, panic |
| `WARN` | Нештатная ситуация, сервис работает | Retry, deprecated endpoint, connection pool близко к лимиту |
| `INFO` | Нормальная работа | Запуск/остановка сервиса, миграции, HTTP-запросы (default) |
| `DEBUG` | Разработка и локальный дебаг | SQL-запросы, детали валидации |
| `TRACE` | Детальная отладка алгоритмов | Вход/выход из функций, промежуточные состояния |

### Конфигурация

```bash
# .env
RUST_LOG=info
```

| Env var | Default | Описание |
|---------|---------|----------|
| `RUST_LOG` | `info` | Уровень логирования, поддерживает per-module |

## 5. Инициализация

```rust
// backend/src/main.rs
tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .with_target(true)
    .json()  // JSON в production; .pretty() для dev
    .init();
```

- `EnvFilter::from_default_env()` — читает `RUST_LOG`.
- `.json()` — structured JSON output.
- `.pretty()` — human-readable для локальной разработки (заменить `.json()` на `.pretty()` в dev).
- `.with_target(true)` — включает module path в каждой записи.

## 6. Структура лога (JSON)

В production используется JSON. Обязательные поля:

```json
{
  "timestamp": "2026-08-26T10:05:00.123Z",
  "level": "INFO",
  "target": "cicd::api",
  "message": "request completed",
  "method": "POST",
  "path": "/api/v1/projects/550e8400-.../pipelines",
  "status": 200,
  "duration_ms": 42,
  "span_id": "...",
  "trace_id": "..."
}
```

| Поле | Описание |
|------|----------|
| `timestamp` | ISO-8601 UTC |
| `level` | Uppercase: `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE` |
| `target` | Rust module path (`cicd::api`, `cicd::store`, `cicd::domain`) |
| `message` | Human-readable сообщение |
| `method` | HTTP метод (для request spans) |
| `path` | HTTP путь (для request spans) |
| `status` | HTTP статус код (для request spans) |
| `duration_ms` | Время обработки запроса |
| `span_id` / `trace_id` | Идентификаторы tracing context |

## 7. HTTP Request Tracing

### 7.1 TraceLayer

HTTP-запросы логируются автоматически через `tower-http`:

```rust
// backend/src/api.rs
use tower_http::trace::TraceLayer;

Router::new()
    .route("/api/v1/health", get(health))
    // ... routes ...
    .layer(CorsLayer::permissive())
    .layer(TraceLayer::new_for_http())
    .with_state(Arc::new(AppState { pool }))
```

### 7.2 Что логируется

| Событие | Level | Содержание |
|---------|-------|------------|
| Запрос получен | INFO | method, path, headers |
| Запрос завершён | INFO | method, path, status, duration |
| Запрос с ошибкой 4xx | INFO | method, path, status, duration |
| Запрос с ошибкой 5xx | ERROR | method, path, status, duration, error |

### 7.3 Span propagation

- `TraceLayer` создаёт top-level span для каждого HTTP-запроса.
- Вложенные spans — через `#[tracing::instrument]` macro в handler функциях.
- Span context прокидывается через `tracing` subscriber автоматически.

```rust
#[tracing::instrument(skip(state))]
async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateProjectPayload>,
) -> Result<Json<Project>, ApiError> {
    // ...
}
```

## 8. Domain Logging

### 8.1 Что логировать

| Событие | Level | Контекст |
|---------|-------|----------|
| Создание проекта | INFO | project_id, name |
| Запуск пайплайна | INFO | pipeline_id, project_id, git_ref |
| Смена статуса job | INFO | job_id, old_status, new_status |
| Каскадная агрегация статусов | DEBUG | stage_id, pipeline_id, new_status |
| Ошибка transition | WARN | job_id, attempted_transition, error |
| Миграция БД | INFO | "migrations applied" |
| Запуск сервиса | INFO | bind_address, pool_status |
| Остановка сервиса | INFO | "shutting down" |

### 8.2 Запрещено логировать

- Пароли, токены, API keys.
- Полные SQL-запросы с параметрами в production (использовать `DEBUG` level для SQLx).
- Содержимое `repository_url` при наличии sensitive-данных (токены в URL).
- Полные payloads логов задач (могут содержать sensitive output).

## 9. SQLx Logging

SQLx логирует запросы на уровне `DEBUG`:

```bash
# Включить SQL-логи
RUST_LOG=cicd=debug,sqlx=debug

# SQL-запросы + параметры (trace)
RUST_LOG=cicd=debug,sqlx=trace
```

> В production: `RUST_LOG=info,sqlx=warn` — SQL-запросы не логируются, только ошибки.

## 10. Локальная разработка

```bash
# Pretty-формат для читаемости (изменить .json() на .pretty() в main.rs)
RUST_LOG=cicd=debug,sqlx=trace cargo run

# Логи пишутся в stdout
cargo run 2>&1 | jq
```

> **Рекомендация:** добавить переключение формата через env var:
> ```rust
> if std::env::var("RUST_LOG_FORMAT").as_deref() == Ok("json") {
>     tracing_subscriber::fmt().json().init();
> } else {
>     tracing_subscriber::fmt().pretty().init();
> }
> ```

## 11. Production

- Формат — JSON в stdout.
- Сборщик логов: Docker logging driver → Loki / Fluent Bit (Phase 5+).
- Retention: 30 дней hot, 90 дней cold (Phase 5+).
- Алерты на рост `ERROR` (Phase 5+).

### Пример production-конфигурации

```bash
# .env (production)
RUST_LOG=info,cicd=info,sqlx=warn,tower_http=info
```

## 12. CLI Logging

`cicd-cli` не использует `tracing` — логирует через `eprintln!` для ошибок и `println!` для вывода.

```rust
// cicd-cli — упрощённый лог
if let Err(e) = api_call().await {
    eprintln!("error: {e}");
    std::process::exit(1);
}
```

> **Планируется (Phase 2):** `tracing` в CLI с `RUST_LOG` поддержкой для debug-режима.

## 13. Sensitive Data Policy

- Все sensitive-поля заменяются на `[REDACTED]` в логах.
- `CICD_DATABASE_URL` содержит пароль — не логировать.
- `CICD_DATABASE_PASSWORD` — не логировать.
- `repository_url` может содержать токены (e.g. `https://token@gitlab.com/...`) — маскировать в логах.
- Проверка перед коммитом: отсутствие secrets в diff.

## 14. Per-module RUST_LOG

```bash
# Только ошибки приложения, SQLx warnings
RUST_LOG=cicd=error,sqlx=warn

# Debug API handlers, trace SQLx
RUST_LOG=cicd::api=debug,sqlx=trace

# Только HTTP request tracing
RUST_LOG=tower_http=debug
```

## References

- `docs/MONITORING.md` — метрики, dashboards, alerting.
- `docs/ARCHITECTURE.md` — middleware stack (TraceLayer, CorsLayer).
- `docs/TROUBLESHOOTING.md` — диагностика через логи.
- `backend/src/main.rs` — инициализация tracing subscriber.
- `backend/src/api.rs` — TraceLayer и instrumented handlers.