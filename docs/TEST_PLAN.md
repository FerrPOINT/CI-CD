# План тестирования Forge CI/CD

**Статус:** нормативный план качества. Фактический статус каждого уровня указан отдельно: **Current verified**, **Configuration only** или **Target approved**.

**Основание:** [ADR-0009: канонический реестр имён и приоритет источников](adr/0009-canonical-registry.md). Этот документ определяет обязательные доказательства качества, но не меняет приоритет источников ADR-0009: код, committed migrations и committed OpenAPI описывают runtime; `docs/contracts/` задаёт целевое нормативное поведение; `docs/CURRENT_STATE.md` фиксирует проверенный снимок.

## 1. Назначение и границы

План применим к backend, CLI, Dashboard, Docker Compose и целевым delivery-компонентам Forge. Тест выбирается по риску и наблюдаемому контракту; успешный unit-тест не заменяет API, real-DB или пользовательское E2E-доказательство.

- **Current verified** означает, что тест или воспроизводимая проверка существуют в текущем checkout.
- **Configuration only** означает наличие настройки или fixture без обязательной исполнимой suite/gate.
- **Target approved** означает обязательный уровень после поставки описанного контракта; его нельзя заявлять как действующий CI-gate.
- Изменение публичного API, CLI, схемы, auth, secrets, status transition, событий или UI требует теста на соответствующем уровне в том же изменении.

## 2. Уровни тестирования

| Уровень | Объект и минимальная область | Текущий запуск / evidence | Статус |
|---|---|---|---|
| Domain unit | Чистые доменные правила, прежде всего `JobStatus::transition_to()` | `cargo test -p cicd-server --test domain_transitions` | **Current verified** |
| API contract | Router, HTTP method/path, status, extractor и поведение без pool через `app(None)` | `cargo test -p cicd-server --test api_contract` | **Current verified** |
| CLI contract | Public command groups, help, стабильное пользовательское поведение CLI | `cargo test -p cicd-server --test cli_contract` | **Current verified** |
| Frontend unit | Компоненты, pure helpers и доступность UI в Vitest/Testing Library | `cd frontend && pnpm test` | **Current verified** |
| Compose smoke | Собранные образы, startup сервисов и `GET /api/v1/health` | `docker compose up --build -d && just health`; после проверки `docker compose down` | **Current verified** локально; не current CI-gate |
| Real-DB integration | PostgreSQL constraints, migrations, CRUD, persisted effects, runtime-role denial, transaction boundaries | Isolated `backend/docker-compose.test.yml`, migrated DB и real-DB suite | **Target approved** |
| Playwright E2E | Критические пользовательские journeys на собранном frontend и real API/PostgreSQL stack | Playwright Chromium; trace, screenshot и video для failed/retried flow | **Target approved** |
| Accessibility | Keyboard journey, accessible names/roles, focus, contrast и серьёзные нарушения | Playwright + axe; без `serious`/`critical` violations | **Target approved** |
| Performance | Бюджеты ключевых routes и регрессии production build | Lighthouse CI с сохранённым report | **Target approved** |

### 2.1. Фактический реестр существующих тестов

Ниже приведён реестр по текущим `backend/tests/` и `frontend/src/**/*.test.*`; он не выдаёт target-покрытие за реализованное.

| Область | Файл | Проверяемое поведение |
|---|---|---|
| Domain unit | `backend/tests/domain_transitions.rs` | `queued -> running -> success`; terminal `failed` не перезапускается; `queued -> success` отклоняется. |
| API contract без БД | `backend/tests/api_contract.rs` | Health возвращает `200`; project CRUD без pool возвращает `503`; пустой body PATCH отклоняется. |
| CLI contract | `backend/tests/cli_contract.rs` | `cicd-cli --help` содержит command groups `project`, `pipeline`, `job`. |
| Dashboard unit | `frontend/src/pages/dashboard/dashboard.test.ts` | `statusLabel` форматирует known status и `success`. |
| Pull request detail unit | `frontend/src/pages/pull-request-detail/pull-request-detail.test.ts` | `buildCompareHref` формирует направление compare и URL-encoding имени repository. |
| Shared UI unit | `frontend/src/shared/ui/confirm-dialog.test.tsx` | `ConfirmDialog` скрыт до открытия, подтверждает действие и не подтверждает при cancel. |
| Shell unit | `frontend/src/widgets/app-shell.test.tsx` | Mobile drawer trigger имеет доступное имя. |

Текущий Cargo suite также содержит inline unit-тесты в `backend/domain/src/lib.rs`, `backend/src/api.rs`, `backend/src/git_host.rs`, `backend/src/platform.rs` и `backend/src/runner.rs`. Они выполняются `cargo test`, но не заменяют выделенные real-DB integration tests.

## 3. Окружения и fixtures

| Окружение / fixture | Правило использования | Статус |
|---|---|---|
| Local unit/contract | Rust 1.86 и Node 22/pnpm 11 согласно `.github/workflows/ci.yml`; тест не использует production URL, токен или DB. | **Current verified** |
| Main Compose smoke | Disposable local Compose stack; проверяются health и состояние сервисов, затем stack останавливается. Реальные shared data и secrets запрещены. | **Current verified** |
| Test Compose PostgreSQL | `backend/docker-compose.test.yml`: `postgres:17-alpine`, `forge_test_cicd`, tmpfs, без host port, healthcheck. Owner fixture -- `forge_owner`; `backend/tests/sql/init-roles.sql` создаёт runtime fixture `forge_runtime`. | **Current verified fixture** |
| Real-DB harness | Migration выполняется owner credential; runtime-проверки -- отдельно под `forge_runtime`. Harness допускает destructive setup только для DB с префиксом `forge_test_`; parallel tests получают независимый migrated database/schema. | **Target approved** |
| Evidence seed | `frontend/scripts/seed-evidence.mjs` создаёт детерминированные demo repositories, projects, pipelines, runners, secrets metadata, environments, deployments, users и tokens для disposable local evidence stack. Запуск: `cd frontend && pnpm seed:evidence`. | **Current verified** |
| Screenshot evidence | `frontend/scripts/shoot-evidence.mjs` снимает predefined маршруты на `1920x1080` и `375x812`; это visual evidence, а не E2E assertion. | **Current verified** |

Fixture `seed:evidence` содержит только synthetic development values. Он не применяется к shared, staging или production окружению; значения секретов, password, bearer token, `CICD_SECRETS_KEY`, production URL и персональные данные не попадают в fixture output, test name, screenshot, trace или CI artifact.

## 4. Entry и exit criteria

| Уровень | Entry criteria | Exit criteria |
|---|---|---|
| Domain unit | Изменены domain rule, status enum или transition caller; тест изолирован от I/O. | Happy path, invalid transition и terminal-state boundary покрыты; focused test и relevant Cargo suite green. |
| API contract | Изменены route, DTO, HTTP status, validation или error behavior; router можно создать с требуемой fixture. | Проверены method/path, status и body/headers для changed public behavior; любой новый endpoint дополнительно проверен `curl` на running stack. |
| CLI contract | Изменены command, option, default, config precedence, output или exit code. | Help, parsing и изменённый наблюдаемый CLI contract green; для real API flow действует также target real-DB/CLI integration gate. |
| Frontend unit | Изменены component, page, helper, mutation state или accessibility semantics. | Покрыты normal, empty/error и permission/interaction boundary в затронутой области; `pnpm test` и `pnpm build` green. |
| Compose smoke | Изменены Dockerfile, Compose, image, startup/configuration или health route. | `docker compose config -q`, build, healthy startup и `GET /api/v1/health` успешны; временный stack остановлен. |
| Real-DB integration | Изменены SQL, migration, store, transaction, persistence, auth enforcement или side effect. | Clean DB мигрируется; prior-schema path применим, required DML работает под `forge_runtime`, DDL под ним отклонён, committed effects и cleanup доказаны. |
| Playwright E2E | Изменён критический UI/API flow либо его auth/async state; собраны frontend и real stack. | Journey проходит с role-based locator и web-first assertion; на failure сохраняются trace/screenshot/video и compose logs. |
| axe | Изменён пользовательский UI или E2E journey. | Нет новых `serious`/`critical` violations; keyboard flow, accessible name и focus проверены для затронутого сценария. |
| Lighthouse | Изменён route, critical asset, frontend build или performance-sensitive API interaction. | Все утверждённые budgets соблюдены либо имеется одобренное исключение с owner, сроком и remediation task; report сохранён. |

Target-level exit criteria становятся обязательными только после поставки соответствующей harness/tooling, однако target implementation не принимается без одновременного включения своего required level в CI.

## 5. Политика покрытия

Coverage измеряет поведение и риск, а не только line percentage. Новая или изменённая ветка обязательна к покрытию, если она влияет на инвариант, public contract, security boundary, persistence или recovery.

| Обязательная область | Минимальное доказательство |
|---|---|
| Status transitions | Domain test для всех допустимых и недопустимых переходов, terminal states и aggregation; real-DB/API test для persisted transition, concurrent/retry boundary и согласованности pipeline/stage/job. Единственный источник правил -- `JobStatus::transition_to()`. |
| Auth и RBAC | После поставки enforcement: unauthenticated `401`, forbidden `403`, tenant/project scope, role boundary, token/session revoke и audit event. До этого capability остаётся **Target approved** и тесты не могут маркировать открытый MVP как защищённый. |
| Secrets | Создание/metadata допускаются только через safe contract; plaintext не возвращается API/UI, не попадает в error, audit, log, trace, fixture и screenshot. Target injection дополнительно проверяет least privilege и masking output. |
| Outbox и идемпотентность | После поставки `domain_events`, `outbox_messages` и `outbox_deliveries`: atomic aggregate/event/outbox commit, duplicate ingress, unique idempotency key, lease recovery, retry после crash и отсутствие недопустимого duplicate observed result. Канонические имена определяет ADR-0009. |
| Public contracts | Изменение API, CLI или generated client имеет focused contract test; breaking change в active `/api/v1` запрещён без новой версии и compatibility evidence. |
| UI | Изменённый critical flow имеет component/feature test; после E2E rollout -- real-flow assertion, mobile `375 px` где применимо, и axe check. Скриншот сам по себе недостаточен. |

Нет установленного числового coverage threshold или ratchet в текущем CI. Введение числового порога без baseline и owner считается **Target approved**; оно не отменяет обязательное risk-based покрытие из этой таблицы.

## 6. Классификация дефектов и приёмка

| Класс | Определение | Правило приёмки |
|---|---|---|
| SEV1 | Подтверждённая утечка секрета, обход auth после его поставки, потеря/повреждение evidence или данных, недоступность control plane без безопасного обхода, неконтролируемый duplicate delivery с критичным эффектом. | Release/merge блокируется. Нужны containment, regression test, проверенное исправление и evidence повторной проверки. |
| SEV2 | Нарушен P0 flow, integrity/status invariant, RBAC boundary, migration/recovery path или критический UI/CLI contract; разумного обхода нет либо он небезопасен. | Merge/release блокируется для affected scope. Исправление и regression test обязательны до приёмки. |
| SEV3 | Значимый, но ограниченный дефект с безопасным documented workaround; не нарушает security, data integrity или P0 completion. | Может быть принят только с owner, linked task, severity rationale, workaround и сроком исправления; не допускается для newly introduced regression без явного risk acceptance. |
| SEV4 | Косметический, текстовый, low-risk observability или ergonomics defect без влияния на результат, безопасность и доступность critical flow. | Может быть отложен в backlog с owner и приоритетом; не маскирует SEV1--SEV3. |

Приёмка изменения требует: все применимые current gates green; отсутствуют открытые SEV1/SEV2; выполнены entry/exit criteria; target capability не получает статус **Current verified** без назначенных target evidence и CI gate. Любое исключение документирует owner, риск, компенсирующий контроль, дату истечения и ссылку на remediation task.

## 7. Traceability

Целевой реестр требований и доказательств — TRACEABILITY-документ в `docs/` (файл TRACEABILITY.md); на момент создания этого плана он является **Target approved** и должен быть создан до включения traceability gate. До появления реестра источник requirement остаётся в `docs/PRODUCT_REQUIREMENTS.md`, ADR и `docs/contracts/` согласно ADR-0009.

Каждый test case, spec или test description, который проверяет нормативное требование, содержит REQ-ID в имени либо в непосредственном описании в формате `REQ-<area>-<number>`, например `REQ-AUTH-001 viewer_cannot_mutate_project` или `it('[REQ-UI-014] ...')`. Один REQ-ID может иметь несколько уровней доказательства; изменение requirement обязано обновить связанные тесты и запись traceability.

Минимальная запись traceability связывает: `REQ-ID`, authoritative source/section, capability status, test level, file/test name, CI job, evidence artifact и результат. Отсутствующий REQ-ID для нового нормативного поведения является review finding; legacy tests получают идентификаторы при изменении затронутого поведения либо плановой инвентаризации.

## 8. CI-гейты

### Current verified

`.github/workflows/ci.yml` запускается на `push` и `pull_request` в `main`.

| Job | Обязательная проверка |
|---|---|
| `backend` | Rust 1.86: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release` в `backend/`. |
| `frontend` | Node 22/pnpm 11: `pnpm install --frozen-lockfile`, `pnpm test`, `pnpm build` в `frontend/`. |
| `containers` | После backend/frontend: `docker compose build`. |

Current CI не запускает real-PostgreSQL integration, Compose startup/health smoke, migration verification, Playwright, axe, Lighthouse, OpenAPI compatibility, secret/dependency/container scan или coverage ratchet.

### Target approved

| Gate | Required evidence |
|---|---|
| `migration-test` | Test Compose up/wait, migrations up/verify, real-DB suite под owner/runtime roles, unconditional `down -v`, logs и applied version/checksum. |
| `openapi-contract` | Generate/validate bundled `openapi/openapi.yaml`, generated TypeScript client check, clean diff и compatibility diff с default branch. |
| `backend` | Workspace fmt/clippy/test/release build, domain/app/API contract tests и coverage artifact. |
| `cli-contract` | Help, config precedence, JSON output, exit codes, redaction и real API/PostgreSQL automation flow. |
| `frontend` | Frozen lockfile, lint, Vitest, generated-client typecheck, production build и coverage artifact. |
| `compose-smoke` | `docker compose config -q`, build, healthy startup и API smoke с cleanup. |
| `e2e-a11y-performance` | Playwright critical journeys, axe без serious/critical violations, Lighthouse budgets и retained reports. |
| `security` | Dependency, secret и container scan по согласованной severity policy; confirmed secret или blocking vulnerability блокирует gate. |
| `traceability` | `docs/TRACEABILITY.md` complete for changed requirements; REQ-ID links resolve to test/evidence and no changed normative behavior lacks mapping. |

CI сохраняет artifacts для failed и required target runs: JUnit/coverage, Compose logs, migration evidence, OpenAPI compatibility report, Playwright report/trace/screenshots/video и Lighthouse reports. Успешная сборка без этих обязательных доказательств не является приёмкой target capability.

## 9. Связанные источники

- `docs/PRODUCT_REQUIREMENTS.md` -- capability requirements и acceptance criteria.
- `docs/CURRENT_STATE.md` -- проверенный текущий функциональный срез.
- `docs/DEVELOPMENT_GUIDE.md` -- локальные команды и текущая test matrix.
- `docs/contracts/MIGRATION_CONTRACT.md` -- real-DB roles, isolated fixture и migration CI contract.
- `docs/contracts/AUTHZ_CONTRACT.md` -- target auth/RBAC requirements.
- `docs/contracts/EVENT_CONTRACT.md` -- outbox, delivery и idempotency requirements.
- `docs/contracts/UI_API_CONTRACT.md` и `docs/contracts/API_CONTRACT.md` -- UI/API contract requirements.
