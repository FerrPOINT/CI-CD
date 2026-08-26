# Архитектура Forge CI/CD

## 1. Контекст

Self-hosted CI/CD control plane (GitLab CI / Jenkins-like). MVP покрывает проекты-репозитории, ручной запуск пайплайнов для Git-рефа, упорядоченные стадии (build / test / deploy) с задачами, переходы статусов задач с доменной валидацией, агрегацию статусов вверх (job → stage → pipeline) и append-only логи задач.

Связь frontend ↔ backend — REST API `/api/v1/*`, Vite dev proxy проксирует `/api` на `http://localhost:22801`. Frontend использует типизированные интерфейсы (`Project`, `Pipeline`, `Stage`, `Job`, `JobLog`), повторяющие DTO бэкенда.

Это control plane, а не remote-execution system: задачи переводятся вручную через UI/API/CLI. Runner-агенты, webhooks, secrets, artifacts, YAML-парсинг и RBAC отложены (см. `docs/ROADMAP.md`).

## 2. Технологический стек

### Backend

| Компонент | Библиотека | Версия |
|---|---|---|
| Язык | Rust (edition 2024) | 1.86 |
| Web framework | axum | 0.8 |
| Async runtime | tokio | 1 |
| Database | sqlx (PostgreSQL) | 0.8 |
| Database server | PostgreSQL | 17.6 |
| HTTP middleware | tower-http (cors, trace) | 0.6 |
| CLI | clap | 4 |
| HTTP client (CLI) | reqwest | 0.12 |
| Serialization | serde + serde_json | 1 |
| IDs | uuid (v4) | 1 |
| Time | chrono | 0.4 |
| Logging | tracing + tracing-subscriber | 0.1 / 0.3 |
| Errors | thiserror | 2 |
| Misc | anyhow | 1 |
| Testing | tower (test util) | 0.5 |

### Frontend

| Компонент | Библиотека | Версия |
|---|---|---|
| Framework | react + react-dom | 19 |
| Build | vite | 6 |
| Plugin | @vitejs/plugin-react | — |
| Styling | tailwindcss | 4 |
| Components | shadcn/ui | — |
| Icons | lucide-react | — |
| Unit tests | vitest + @testing-library/react | 3.x / 10.x |
| DOM testing | @testing-library/jest-dom | 6.x |
| Types | typescript | 5.x |
| Package manager | pnpm | 11 |

### Infrastructure

- PostgreSQL 17.6-alpine
- Docker + Docker Compose
- Backend порт: `22801`
- Frontend порт: `22802` (nginx в проде, Vite dev server локально)
- PostgreSQL порт: `22543`
- Env prefix: `CICD_`
- CI: GitHub Actions (`.github/workflows/ci.yml`)

## 3. Структура монорепозитория

```
CI-CD/
├── backend/
│   ├── Cargo.toml              # package + deps
│   ├── Dockerfile              # multi-stage Rust build
│   ├── src/
│   │   ├── lib.rs              # public modules: api, domain, store
│   │   ├── main.rs             # entrypoint: load env, connect DB, migrate, serve
│   │   ├── api.rs              # axum routes, DTO, handlers, AppState
│   │   ├── domain.rs           # JobStatus enum, transition rules, TransitionError
│   │   └── store.rs            # SQL schema bootstrap (migrate), helpers
│   ├── tests/
│   │   ├── api_contract.rs     # integration: health endpoint
│   │   ├── domain_transitions.rs # unit: status transition rules
│   │   └── cli_contract.rs     # integration: CLI --help groups
│   └── src/bin/
│       └── cicd-cli.rs         # CLI binary (clap, reqwest → API)
├── frontend/
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── pnpm-workspace.yaml
│   ├── tsconfig.json
│   ├── vite.config.ts          # react plugin, vitest, proxy /api → :22801
│   ├── Dockerfile              # multi-stage node → nginx
│   ├── nginx.conf
│   ├── dist/                   # build output
│   └── src/
│       ├── main.tsx            # React entrypoint
│       ├── dashboard.tsx       # main Dashboard component
│       ├── dashboard.test.tsx  # Vitest unit tests
│       ├── styles.css          # global styles (dark theme, layout)
│       ├── index.css           # Tailwind 4 @theme tokens
│       └── shared/ui/          # shadcn/ui components
│           ├── alert-dialog.tsx
│           ├── button.tsx
│           ├── card.tsx
│           ├── dialog.tsx
│           ├── dropdown-menu.tsx
│           ├── input.tsx
│           ├── label.tsx
│           ├── progress.tsx
│           ├── table.tsx
│           ├── tabs.tsx
│           ├── textarea.tsx
│           └── theme-toggle.tsx
├── docker-compose.yml          # postgres + backend + frontend
├── .env.example                # CICD_* env vars
├── .github/workflows/ci.yml    # CI: backend, frontend, containers
├── justfile                    # unified dev commands
├── plans/                      # local implementation plan
└── docs/
    ├── AGENTS.md
    ├── ARCHITECTURE.md
    ├── DATA_MODEL.md
    ├── API.md
    ├── UI_UX.md
    ├── ROADMAP.md
    ├── CODE_STYLE.md
    └── TESTING.md
```

## 4. Backend: слоистая архитектура

### 4.1 Presentation layer (`api.rs`)

HTTP-адаптер на Axum. Отвечает за:
- Роутинг: `Router` с маршрутами `/api/v1/*`.
- Извлечение path params (`Path<Uuid>`), JSON body (`Json<T>`), state (`State<Arc<AppState>>`).
- Валидацию входных данных (`name` / `repository_url` / `message` не пустые).
- Маппинг ошибок `ApiError` → HTTP статус через `IntoResponse`.
- CORS (`CorsLayer::permissive`) и tracing (`TraceLayer::new_for_http()`).

`AppState` содержит `Option<PgPool>`: `None` — режим без БД (health-check работает, остальные endpoint возвращают `503`).

### 4.2 Domain layer (`domain.rs`)

Доменные правила, не зависящие от HTTP и БД:
- `JobStatus` enum: `Queued`, `Running`, `Success`, `Failed`, `Canceled`.
- `transition_to()` — конечный автомат переходов:
  - `Queued → Running | Canceled`
  - `Running → Success | Failed | Canceled`
  - `Success | Failed | Canceled` — терминальные, переход невозможен.
- `TransitionError` — `TerminalStatus` / `InvalidTransition`.
- `TryFrom<&str>` для парсинга строкового статуса из БД.
- `serde` с `rename_all = "snake_case"` для JSON-сериализации.

### 4.3 Infrastructure layer (`store.rs`)

SQL-хранилище:
- `migrate()` — bootstrap схемы: `CREATE TABLE IF NOT EXISTS` для `projects`, `pipelines`, `stages`, `jobs`, `job_logs`.
- `next_log_sequence()` — вычисление следующего `sequence` для append-only логов: `COALESCE(MAX(sequence), 0) + 1`.
- Все запросы — parameterized SQL через `sqlx::query` / `sqlx::query_as`.

### 4.4 CLI (`src/bin/cicd-cli.rs`)

Отдельный бинарник `cicd-cli`, speaks only to the public API:
- `clap` с группами `project`, `pipeline`, `job`.
- HTTP-клиент `reqwest` с rustls.
- `CICD_API_URL` env var (default `http://127.0.0.1:22801`).
- Команды: `project list/create`, `pipeline list/run/show`, `job start/pass/fail/logs/log`.

## 5. Конфигурация

Конфигурация через env vars с префиксом `CICD_`:

| Env var | Назначение | Default |
|---|---|---|
| `CICD_DATABASE_URL` | PostgreSQL connection string | — (required) |
| `CICD_BIND` | Адрес привязки API | `0.0.0.0:22801` |
| `CICD_API_URL` | URL API для CLI | `http://127.0.0.1:22801` |
| `CICD_DATABASE_USER` | Пользователь БД | `cicd` |
| `CICD_DATABASE_PASSWORD` | Пароль БД | `cicd_local_only` |
| `CICD_DATABASE_NAME` | Имя БД | `cicd` |
| `CICD_DATABASE_PORT` | Порт PostgreSQL (host mapping) | `22543` |
| `CICD_API_PORT` | Порт API (host mapping) | `22801` |
| `CICD_WEB_PORT` | Порт Dashboard (host mapping) | `22802` |
| `RUST_LOG` | Уровень логирования | `info` |

Файл `.env.example` содержит шаблон. Для локального запуска: `cp .env.example .env`.

## 6. Middleware stack

```rust
Router::new()
    .route("/api/v1/health", get(health))
    // ... routes ...
    .layer(CorsLayer::permissive())
    .layer(TraceLayer::new_for_http())
    .with_state(Arc::new(AppState { pool }))
```

CORS — permissive (все origins) для dev-режима. Tracing — structured access log через `tracing`.

## 7. Status aggregation

При смене статуса job (`POST /jobs/{id}/status`) происходит каскадное обновление вверх:

1. **Job status** — обновляется напрямую, с проставлением `started_at` / `finished_at`.
2. **Stage status** — агрегация всех jobs в stage:
   - `failed` если любой job failed
   - `success` если все jobs success
   - `running` если любой job running
   - `canceled` если любой job canceled
   - иначе `queued`
3. **Pipeline status** — агрегация всех stages в pipeline (аналогичная логика).
4. **Pipeline timestamps** — `started_at` проставляется при переходе в `running`, `finished_at` — при переходе в терминальный статус.

Логика реализована в `refresh_statuses()`.

## 8. Frontend архитектура

Текущий frontend — single-page Dashboard (`dashboard.tsx`), рендерящийся через `main.tsx`. Целевая архитектура (по мере роста):

- **Pages** — экраны: Login, Dashboard, Projects, Pipeline list, Pipeline detail, Admin.
- **Shared** — ui-kit (shadcn/ui), i18n, theme, API helpers.
- **App** — роутер, провайдеры.

API-клиент — типизированная функция-обёртка над `fetch`:
```typescript
const api = async <T>(path: string, init?: RequestInit): Promise<T> => {
  const response = await fetch(`/api/v1${path}`, { ... })
  // ...
}
```

Vite dev proxy: `/api` → `http://localhost:22801`.

## 9. API, документация и тестирование

- REST API v1 — `docs/API.md`.
- UI/UX — `docs/UI_UX.md`.
- Дата-модель — `docs/DATA_MODEL.md`.
- Тестирование — `docs/TESTING.md`.
- Code style — `docs/CODE_STYLE.md`.

## 10. Dev workflow

Управляется через `justfile`:

```bash
just up              # docker compose up --build -d
just down            # docker compose down
just logs            # docker compose logs -f
just test-backend    # cargo test in Rust container
just test-frontend   # pnpm test
just build-frontend  # pnpm build
just health          # curl /api/v1/health
```

CI через GitHub Actions (`.github/workflows/ci.yml`):
- **backend**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`.
- **frontend**: `pnpm install --frozen-lockfile`, `pnpm test`, `pnpm build`.
- **containers**: `docker compose build` (после backend + frontend).

## 11. Deployment

Локальный запуск:

```bash
cp .env.example .env
docker compose up --build -d
curl http://127.0.0.1:22801/api/v1/health
```

Frontend доступен на `http://127.0.0.1:22802`.

Multi-stage Docker build:
- Backend: `rust:1.86-slim` → `debian:bookworm-slim` (non-root user `cicd`, uid 10001).
- Frontend: `node:22-bookworm-slim` → `nginx:1.27-alpine` (static files).

## References

- `README.md`
- `docs/AGENTS.md`
- `docs/DATA_MODEL.md`
- `docs/API.md`
- `docs/TESTING.md`
- `docs/ROADMAP.md`
