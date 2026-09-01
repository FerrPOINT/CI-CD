# Руководство по разработке Forge CI/CD

Этот документ описывает воспроизводимый локальный цикл разработки и проверки. Он не заменяет нормативные контракты: порядок источников и статус утверждений определены в [DOCUMENTATION GOVERNANCE](DOCUMENTATION_GOVERNANCE.md). Фактически работающие возможности перечислены в [CURRENT STATE](CURRENT_STATE.md); код, committed migrations и OpenAPI после их появления имеют приоритет над этим guide.

## Статусы

- **Current verified** -- команда, пакет или проверка существуют в текущем checkout и используются runtime/CI.
- **Configuration only** -- сущность или экран можно настроить, но execution/delivery не реализованы.
- **Target approved** -- требование принято ADR или контрактом, но не должно выдаваться за работающую возможность.

## Быстрый старт

### Требования

Для полного Docker-цикла достаточно Docker Engine с Compose plugin, `curl` и (опционально) `just`. Локальный режим дополнительно требует Rust 1.86+ и Node.js 22 с pnpm 11. Версии runtime и образов зафиксированы в `docker-compose.yml` и `.github/workflows/ci.yml`.

Перед первым запуском создайте только локальный файл конфигурации:

```bash
cd /opt/dev/CI-CD
cp .env.example .env
# Замените development-only значения перед общим или удалённым запуском.
docker compose config -q
just up
just health
just readiness
```

`just up` выполняет `docker compose up --build -d`. После изменения Dockerfile, образа или env пересоздавайте сервисы командой `docker compose up -d` (с `--build`, если менялся образ); не используйте `docker compose restart` для применения этих изменений.

Полезные текущие recipes из `justfile`:

```bash
just                 # список recipes
just up              # собрать и запустить весь stack
just logs            # docker compose logs -f
just health          # GET /api/v1/health
just readiness       # GET /api/v1/readiness (PostgreSQL + SQLx migrations)
just down            # остановить stack
just test-backend    # cargo test в rust:1.86-bookworm
just test-frontend   # Vitest на хосте
just build-frontend  # TypeScript check и Vite build на хосте
```

### Адреса и порты

| Сервис | Адрес по умолчанию | Источник настройки | Статус |
|---|---|---|---|
| API и Git Smart HTTP | `http://127.0.0.1:22801` | `CICD_API_PORT`, `CICD_BIND` | Current verified |
| Dashboard в Compose | `http://127.0.0.1:22802` | `CICD_WEB_PORT` | Current verified |
| Dashboard в Vite dev mode | `http://127.0.0.1:5173` | Vite | Current verified |
| PostgreSQL 17 | `127.0.0.1:22543` | `CICD_DATABASE_PORT` | Current verified |

Vite proxy направляет `/api` на `http://localhost:22801`; отдельная frontend env-переменная для API в текущем режиме не нужна. PostgreSQL Compose-профиля привязан к `127.0.0.1`, а API и Dashboard защищены только при непустом `CICD_AUTH_SECRET`: включаются JWT/PAT, session-bound access invalidation, route roles, project membership enforcement и Git Smart HTTP read/write checks по связанному проекту. Это допустимо только для изолированной локальной разработки или закрытого shared-контура; ограничения и известные риски перечислены в [CURRENT STATE](CURRENT_STATE.md).

### Переменные окружения

Используйте только переменные с префиксом `CICD_`; `.env` не коммитится. `.env.example` содержит безопасный шаблон, а полный справочник владельцев, defaults и последствий ротации ключей находится в [ENV](ENV.md).

| Группа | Основные переменные | Когда нужны |
|---|---|---|
| Compose database | `CICD_DATABASE_USER`, `CICD_DATABASE_PASSWORD`, `CICD_DATABASE_NAME`, `CICD_DATABASE_PORT` | PostgreSQL в Compose |
| Публикация сервисов | `CICD_API_PORT`, `CICD_WEB_PORT`, `CICD_BIND` | Изменение host port или bind address |
| Прямой backend запуск | `CICD_DATABASE_URL`, `RUST_LOG` | Сервер запущен вне Compose |
| Git и embedded runner | `CICD_GIT_ROOT`, `CICD_GIT_TOKEN`, `CICD_GIT_INTERNAL_TOKEN`, `CICD_EMBEDDED_RUNNER_ENABLED`, `CICD_RUNNER_MODE`, `CICD_RUNNER_KEEP_WORKSPACE` | Local Git hosting и встроенное выполнение job |
| External runner | `CICD_API_URL`, `CICD_RUNNER_REGISTRATION_TOKEN`, `CICD_RUNNER_CREDENTIAL`, `CICD_RUNNER_NAME`, `CICD_RUNNER_TAGS`, `CICD_RUNNER_TOTAL_SLOTS`, `CICD_RUNNER_POLL_INTERVAL_SECONDS`, `CICD_RUNNER_NO_CHECKOUT`, `CICD_RUNNER_WORK_DIR`, `CICD_RUNNER_KEEP_WORKSPACE` | Отдельный `forge-runner` shell process поверх runner protocol MVP |
| Secrets и artifacts | `CICD_SECRETS_KEY`, `CICD_ARTIFACTS_DIR` | Project secrets и локальные artifacts |
| CLI | `CICD_API_URL` | `cicd-cli` вне контейнера |

Не передавайте пароль БД, bearer token, `CICD_SECRETS_KEY` или production URL в команды shell history, fixture, screenshot либо PR. Для общего deployment обязательно задайте уникальный `CICD_GIT_INTERNAL_TOKEN`; пустое значение оставляйте только для isolated local development, а legacy `forge-internal-dev-token` backend отклоняет при старте. Генерация ключей и политика ротации описаны в [ENV](ENV.md).

### Локальный режим без контейнеров приложения

Поднимите только БД и запустите два процесса в отдельных terminals:

```bash
cd /opt/dev/CI-CD
docker compose up -d postgres

# Terminal 1
cd backend
CICD_DATABASE_URL='postgresql://cicd:<password>@127.0.0.1:22543/cicd' \
  CICD_BIND=127.0.0.1:22801 \
  cargo run --bin cicd-server

# Terminal 2
cd frontend
pnpm install --frozen-lockfile
pnpm dev
```

Текущий backend применяет committed SQLx migrations из `backend/migrations/` при старте. `cicd-migrate` существует как отдельный migration binary; production target остаётся строже: pre-runtime migration under owner role, verify-only startup и runtime role без DDL.

### Внешний `forge-runner` shell MVP

Embedded runner включён по умолчанию. Чтобы проверить внешний процесс, отключите embedded execution и запустите runner отдельно:

```bash
CICD_EMBEDDED_RUNNER_ENABLED=false cargo run --bin cicd-server

CICD_API_URL=http://127.0.0.1:22801 \
  CICD_RUNNER_REGISTRATION_TOKEN="$CICD_RUNNER_REGISTRATION_TOKEN" \
  cargo run --bin forge-runner -- --once
```

В Compose доступен профиль `external-runner`:

```bash
CICD_EMBEDDED_RUNNER_ENABLED=false docker compose --profile external-runner up --build
```

Current `forge-runner` регистрируется или использует `CICD_RUNNER_CREDENTIAL`, heartbeat-ит capacity/tags, получает compatible `LeaseOffer` через durable `job_queue` claim с `required_tags ⊆ runner.tags`, клонирует `workspace.checkoutUrl`, после ack получает only declared `attempt.secrets`, выполняет команды shell с secret env, загружает only declared `attempt.artifacts` files, отправляет stdout/stderr в attempt-owned `job_logs` с masking и отправляет terminal result. Resumable artifact sessions, idempotent chunked logs, Docker/Kubernetes isolation, pool/protected-tag policy, capability matching и long-poll остаются target.

## Карта workspace и пакетов

| Путь / package | Ответственность | Статус |
|---|---|---|
| `backend/Cargo.toml` | Cargo workspace root, общие Rust dependencies; members `.` / `domain` / `cli` | Current verified |
| `backend/` -- `cicd-server` | Axum control plane, HTTP routes, SQLx store, Git hosting, embedded runner и composition root | Current verified |
| `backend/src/bin/forge-runner.rs` | Отдельный shell-runner process для runner protocol MVP | Current verified MVP |
| `backend/domain/` -- `cicd-domain` | Чистые доменные типы и state machine `JobStatus` | Current verified |
| `backend/cli/` -- `cicd-cli` | HTTP-only CLI для `project`, `pipeline`, `job`; не линкует server code | Current verified |
| `backend/tests/` | no-DB API/domain contracts, `integration_db` real PostgreSQL suite; `tests/sql/init-roles.sql` для runtime role fixture | Current verified |
| `backend/cli/tests/` | CLI contract tests для public command groups/help | Current verified |
| `backend/docker-compose.test.yml` | Ephemeral PostgreSQL 17 fixture без host port | Current verified fixture |
| `frontend/` -- `cicd-dashboard` | React SPA, Vite, Tailwind, i18n, pages/widgets/entities/shared UI | Current verified |
| `frontend/src/api/` | Typed API wrapper/hooks + generated OpenAPI DTO `schema.d.ts` | Current verified |
| `frontend/src/api/schema.d.ts` | Generated OpenAPI types from `openapi/openapi.yaml` | Current verified |
| `backend/migrations/`, `backend/migration/` -- `cicd-migrate` | Immutable SQL migrations и migration tool | Current verified |
| `backend/api/` -- `cicd-api` | Routes/DTO/OpenAPI annotations как отдельная граница | Target approved |

Целевая декомпозиция `domain -> app -> infra -> api -> server` описана в [ADR-0005](adr/0005-workspace-layered-architecture.md) и [ARCHITECTURE](ARCHITECTURE.md). Не создавайте новые target package или generated outputs только потому, что они упомянуты в документации: они появляются вместе с реализацией и тестами.

## Проверки и тестовая матрица

| Уровень | Что проверяется | Текущий запуск | Статус и следующий обязательный шаг |
|---|---|---|---|
| Rust unit | `JobStatus::transition_to()` и чистые domain правила | `cargo test -p cicd-server --test domain_transitions` | Current verified |
| API contract без БД | router wiring, liveness/readiness no-DB behavior и extractor behavior через `app(None)` | `cargo test -p cicd-server --test api_contract` | Current verified; не заменяет persistence tests |
| CLI contract | `cicd-cli --help`, public command groups | `cargo test -p cicd-cli --test cli_contract` | Current verified |
| Frontend unit | Components, pages и UI behavior через Vitest/Testing Library | `pnpm test` | Current verified |
| Compose smoke | Запуск образов, `GET /api/v1/health` и `GET /api/v1/readiness` | `just up && just health && just readiness` | Current verified локально; не является current CI gate |
| API + PostgreSQL | CRUD, constraints, migrations/readiness, persisted side effects, headers/error envelope, auth/PAT и current pagination cases | `cargo test --features integration --test integration_db -- --test-threads=1` с PostgreSQL service в CI | Current verified; isolated owner/runtime matrix всё ещё target |
| CLI + real API | Automation flow, config precedence, JSON output, exit codes и redaction | CLI against real API/PostgreSQL stack | Target approved |
| Browser E2E | Critical UI journeys against built frontend и real API/PostgreSQL | Playwright Chromium | Target approved; сценарии -- [UI API CONTRACT](contracts/UI_API_CONTRACT.md) |
| Accessibility | Keyboard flow, semantic controls, colour contrast и serious/critical violations | Playwright + axe | Target approved; не установлен как current script/gate |
| Performance | Key routes, regression budgets и report artifact | Lighthouse CI | Target approved; не установлен как current script/gate |

Для нового endpoint добавьте focused test на нужном уровне и выполните `curl` against running stack. Для UI-изменения добавьте component/feature test; после внедрения E2E сохраняйте Playwright trace, screenshot, video при failure и compose logs. Скриншоты не заменяют interaction assertions.

Нормативные требования не переписываются в этом guide. Выбирайте owner-контракт по изменению:

- HTTP/OpenAPI, errors, pagination и idempotency: [API CONTRACT](contracts/API_CONTRACT.md).
- Typed frontend transport, cache, errors и mutation behavior: [UI API CONTRACT](contracts/UI_API_CONTRACT.md).
- Auth, tenant scope и RBAC: [AUTHZ CONTRACT](contracts/AUTHZ_CONTRACT.md).
- Versioned migrations, DB roles и real-DB fixture: [MIGRATION CONTRACT](contracts/MIGRATION_CONTRACT.md).
- Pipelines and YAML parser: [PIPELINE DSL](contracts/PIPELINE_DSL.md).
- Runner registration, leases и fencing: [RUNNER PROTOCOL](contracts/RUNNER_PROTOCOL.md).
- Events, outbox and deliveries: [EVENT CONTRACT](contracts/EVENT_CONTRACT.md).
- Retention, storage ownership and destructive lifecycle: [DATA LIFECYCLE](contracts/DATA_LIFECYCLE.md).

## Команды локальной проверки

### Backend в Rust Docker image

Команда ниже повторяет текущий backend CI набор в pinned image и сохраняет artifacts в `backend/target/`:

```bash
cd /opt/dev/CI-CD
docker run --rm --entrypoint /bin/bash \
  -v "$PWD/backend:/workspace" \
  -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target \
  rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo fmt --check && \
       /usr/local/cargo/bin/cargo clippy --workspace --all-targets -- -D warnings && \
       /usr/local/cargo/bin/cargo test --workspace'
```

Для real-DB проверки нужен PostgreSQL и `CICD_TEST_DATABASE_URL`; CI запускает `cargo test --features integration --test integration_db -- --test-threads=1`, потому что scheduler/runner dispatch тесты используют глобальные due/queued scans. Release build остаётся локальной/release-проверкой, но не current CI step.

### Frontend через Node Docker image

Текущий `just test-frontend` и `just build-frontend` используют host pnpm. Если host Node/pnpm отсутствуют, запускайте эквивалент в pinned Node image:

```bash
cd /opt/dev/CI-CD
docker run --rm -it \
  -v "$PWD/frontend:/workspace" \
  -w /workspace \
  node:22-bookworm-slim \
  bash -lc 'corepack enable && pnpm install --frozen-lockfile && pnpm test && pnpm build'
```

Если меняются frontend dependencies, оставьте обновлённый `pnpm-lock.yaml` в diff; CI устанавливает его только с `--frozen-lockfile`. Current GitHub Actions frontend job запускает generated-client drift gate, `pnpm lint`, `pnpm test` и `pnpm build`.

### Compose и документация

```bash
cd /opt/dev/CI-CD
docker compose config -q
docker compose build
docker compose up --build -d
docker compose ps
curl -fsS http://127.0.0.1:22801/api/v1/health
curl -fsS http://127.0.0.1:22801/api/v1/readiness
docker compose down

# Удаляет локальные volumes и все данные Compose; используйте только для disposable dev data.
docker compose down -v

python3 scripts/verify_docs.py --all
```

## CI-гейты

### Current verified

`.github/workflows/ci.yml` запускается на `push` и `pull_request` в `main` и содержит только следующие jobs:

| Job | Фактические проверки |
|---|---|
| `backend` | PostgreSQL 17 service; `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --features integration --test integration_db -- --test-threads=1`, OpenAPI drift gate |
| `frontend` | `pnpm install --frozen-lockfile`, `pnpm openapi:generate` + clean diff для `src/api/schema.d.ts`, `pnpm lint`, `pnpm test`, `pnpm build` |
| `docs` | `python3 scripts/verify_docs.py --all` |

В текущем workflow нет compose startup/health smoke, release build, OpenAPI backward compatibility diff, Playwright, axe, Lighthouse, dependency/secret/container scans или coverage ratchet. Не утверждайте, что branch protection, approval policy или checks из устаревших narrative-доков фактически включены, пока это не подтверждено настройками GitHub.

### Target approved

Планируемый PR gate дополняет, а не ослабляет current checks:

| Gate | Требуемое evidence |
|---|---|
| `migration-test` | isolated test PostgreSQL, `cicd-migrate up/verify`, owner/runtime role matrix, prior-schema upgrade evidence, unconditional cleanup |
| `openapi-contract` | current drift/codegen checks плюс backward compatibility diff с default branch |
| Backend | workspace fmt/clippy/test/release build, domain/app/API contract tests |
| CLI | help/config/output/exit-code contracts против real API |
| Frontend | frozen lockfile, lint, Vitest, generated-client typecheck и production build |
| Container smoke | `docker compose config`, build, healthy startup и API smoke |
| E2E/accessibility/performance | Playwright critical journeys, axe without serious/critical violations, Lighthouse budgets и retained reports |
| Security | dependency audit, secret scan и container scan по согласованной severity policy |

Полная целевая модель quality gates и CI artifacts приведена в [DELIVERY ARCHITECTURE](DELIVERY_ARCHITECTURE.md). Детали migration/test DB не дублируйте: они принадлежат [MIGRATION CONTRACT](contracts/MIGRATION_CONTRACT.md).

## Стиль, review и зависимости

Код и комментарии пишутся на английском; документация и пользовательские строки -- на русском. Rust форматируется `rustfmt`; production Rust не использует `unwrap()`/`expect()` и обращается к PostgreSQL только parameterized SQL. TypeScript не использует `any`; React использует function components, typed API boundary, Tailwind/shadcn UI и i18n. Полные правила именования, import order, errors, SQL и accessibility -- в [CODE STYLE](DEVELOPMENT_GUIDE.md).

Перед PR выполните self-review, добавьте tests для изменённого поведения и проверьте, что нет secrets в diff. Один PR содержит одну логическую задачу; обычный предел -- 400--500 строк без tests и generated files. Коммиты следуют Conventional Commits (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`). Процесс, роли, SLA и escalation принадлежат `docs/REVIEW.md`, а проверяемый checklist -- [CODE REVIEW](DEVELOPMENT_GUIDE.md).

PR, добавляющий или меняющий capability, NFR, публичный контракт или статус требования, не мержится без строки в [TRACEABILITY](TRACEABILITY.md): требование получает `REQ-ID` в [PRODUCT_REQUIREMENTS](PRODUCT_REQUIREMENTS.md) до реализации, а RTM-строка связывает его с контрактом, проверкой, evidence и фактическим статусом согласно [чек-листу PR](TRACEABILITY.md#4-чек-лист-pr).

Фактические версии зависимостей находятся в `backend/Cargo.toml`, `backend/Cargo.lock`, `frontend/package.json` и `frontend/pnpm-lock.yaml`. Не добавляйте crate/package без обоснования, license/security review и отдельного reviewable изменения. Выборы и разрешённые будущие crates (включая `utoipa`, auth, metrics и object storage) принадлежат `docs/TECH_CHOICES.md` и ADR: [ADR-0001](adr/0001-rust-axum-sqlx.md), [ADR-0002](adr/0002-react-vite-tailwind.md), [ADR-0004](adr/0004-postgresql-only.md), [ADR-0005](adr/0005-workspace-layered-architecture.md), [ADR-0008](adr/0008-versioned-sqlx-migrations.md) и [ADR-0009](adr/0009-canonical-registry.md). Текущая dependency policy и команды `cargo tree` / `pnpm why` описаны в `docs/LIBRARIES.md`.

## OpenAPI и generated types

OpenAPI-first artifact уже current: Rust `utoipa` annotations генерируют committed `openapi/openapi.yaml`, а frontend генерирует DTO в `frontend/src/api/schema.d.ts`. Отдельный `cicd-api` crate, compatibility diff и generated transport boundary остаются target.

Текущий порядок при изменении API:

```bash
cd /opt/dev/CI-CD
cd backend
cargo run --bin openapi-dump -- ../openapi/openapi.yaml
cd frontend
pnpm openapi:generate       # bundled spec -> src/api/schema.d.ts
cd ..
git diff --exit-code
```

`openapi/openapi.yaml` — единственный committed bundled OpenAPI artifact и генерируется только из Rust annotations; ручное изменение YAML и generated frontend file запрещено. Handwritten frontend API wrapper остаётся тонким слоем для headers, structured errors, binary upload/download и SSE. Каждый API change включает annotations, implementation, generated client и tests в одном PR; breaking change в `/api/v1` не допускается без новой версии и compatibility evidence. Нормативные правила generation, route coverage, security, errors и compatibility находятся в [API CONTRACT](contracts/API_CONTRACT.md) и [UI API CONTRACT](contracts/UI_API_CONTRACT.md).

## Связанные документы

- [ARCHITECTURE](ARCHITECTURE.md) и [CURRENT STATE](CURRENT_STATE.md) -- runtime boundaries и verified capability inventory.
- `docs/TEST_PLAN.md` ([TEST PLAN](TEST_PLAN.md)) -- нормативный план тестирования: уровни, окружения, entry/exit criteria, SEV1--SEV4, coverage-политика, traceability и CI-гейты.
- [ENV](ENV.md), `docker-compose.yml`, `.env.example` и `justfile` -- local configuration and executable commands.
- [TESTING](DEVELOPMENT_GUIDE.md) и [CI/CD](DEVELOPMENT_GUIDE.md) -- historical/detail references; current CI source is `.github/workflows/ci.yml`.
- [DOCUMENTATION GOVERNANCE](DOCUMENTATION_GOVERNANCE.md) -- authority matrix, status taxonomy and documentation checks.
