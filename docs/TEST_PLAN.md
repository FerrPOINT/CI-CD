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
| API contract | Router, HTTP method/path, route-policy inventory, status, extractor и поведение без pool через `app(None)`, включая liveness `200` и readiness `503` без БД | `cargo test -p cicd-server --test api_contract`; inline `backend/src/authz.rs` OpenAPI coverage test в `cargo test --workspace` | **Current verified** |
| CLI/runner binary contract | Public command groups, help, стабильное пользовательское поведение CLI и `forge-runner` flags | `cargo test -p cicd-cli --test cli_contract`; `cargo test -p cicd-server --test runner_binary_contract` | **Current verified** |
| CLI real API smoke | Настоящий `cicd-cli` против disposable HTTP API/PostgreSQL stack: config precedence, bounded HTTP timeout, JSON output, idempotent pipeline run, attempts, protected deployment approval, JWT/PAT auth-mode, RBAC denial, project-scoped read-only PAT, token redaction на failure и non-zero API error exit | `cargo test -p cicd-cli --features integration --test cli_real_api -- --test-threads=1` с `CICD_TEST_DATABASE_URL`; CI backend job | **Current verified MVP** |
| Frontend unit | Компоненты, pure helpers, typed API transport, route smoke всех Dashboard-страниц, i18n contract и доступность UI в Vitest/Testing Library | `cd frontend && pnpm test` | **Current verified** |
| Dependency/SBOM security | Rust/Node advisories, frozen dependency graph, committed-secret baseline и drift committed SBOM | `.github/workflows/ci.yml` job `security`: SQLx optional MySQL/RSA feature guard, `cargo audit --ignore RUSTSEC-2023-0071`, `pnpm audit --audit-level high`, `scripts/scan_secrets.py`, `scripts/generate_sbom.py --check` | **Current verified MVP** |
| Compose smoke | Собранные образы, startup сервисов, `GET /api/v1/health`, `GET /api/v1/readiness` и frontend nginx | `.github/workflows/ci.yml` job `compose-smoke`: `docker compose config -q`, `docker compose up --build -d`, API/frontend smoke и `docker compose down -v --remove-orphans` | **Current verified** |
| Real-DB integration | PostgreSQL constraints, migrations/readiness, CRUD, persisted effects, immutable pipeline plan snapshots, auth/PAT и current API boundaries | GitHub Actions PostgreSQL service + `cargo test --features integration --test integration_db -- --test-threads=1`; local fixture `backend/docker-compose.test.yml` | **Current verified** |
| Playwright E2E | Критические пользовательские journeys, all-route desktop accessibility и seeded performance smoke на собранном frontend и real API/PostgreSQL stack | `.github/workflows/ci.yml` job `e2e`: Docker Compose + `pnpm seed:evidence` + `pnpm e2e`; traces/screenshots/video сохраняются на failure | **Current verified MVP** |
| Accessibility | Keyboard journey, accessible names/roles, focus и серьёзные axe-нарушения | `frontend/e2e/accessibility.spec.ts`: все 21 baseline route без `serious`/`critical`; mobile drawer Escape/focus в `critical-flows.spec.ts` | **Current verified MVP; target расширение** |
| Performance | Synthetic budgets для seeded API reads и Dashboard route ready-time; target Lighthouse/load/30-day SLO evidence | `frontend/e2e/performance.spec.ts` в CI job `e2e` | **Current verified MVP; target расширение** |

### 2.1. Фактический реестр существующих тестов

Ниже приведён реестр по текущим `backend/tests/`, `backend/cli/tests/` и `frontend/src/**/*.test.*`; он не выдаёт target-покрытие за реализованное.

| Область | Файл | Проверяемое поведение |
|---|---|---|
| Domain unit | `backend/tests/domain_transitions.rs` | `queued -> running -> success`; terminal `failed` не перезапускается; `queued -> success` отклоняется. |
| API contract без БД | `backend/tests/api_contract.rs` | Health возвращает `200`; readiness без pool возвращает `503`; project CRUD без pool возвращает `503`; пустой body PATCH отклоняется. |
| AuthZ route inventory | `backend/src/authz.rs` inline tests | Executable `ROUTE_POLICIES` покрывает все generated OpenAPI operations, не содержит duplicate method/path и не разрешает unpublished API route через user policy. |
| Frontend API transport | `frontend/src/api/client.test.ts` | `ApiError` сохраняет `status`, `code`, `request_id`, details и `Retry-After` из JSON envelope, безопасно обрабатывает non-JSON HTTP error и отличает network/cancelled failures для retry policy. |
| CLI contract | `backend/cli/tests/cli_contract.rs`; `backend/cli/tests/cli_real_api.rs` | `cicd-cli --help` содержит runtime/platform command groups, `--token`, `--timeout-seconds`, `--output`, pagination flags, job attempt history, `--idempotency-key` и key mutation subcommands. Real-API smoke проверяет публичный HTTP path, env/flag precedence, bounded timeout, protected API auth/RBAC/PAT behavior, JSON output и non-zero API error exit. |
| Runner binary contract | `backend/tests/runner_binary_contract.rs` | `forge-runner --help` содержит protocol/config flags `--api-url`, `--credential`, `--registration-token`, `--tags`, `--total-slots`, `--poll-interval-seconds`, `--work-dir`, `--once`, `--no-checkout`, `--keep-workspace`. |
| Dashboard unit | `frontend/src/pages/dashboard/dashboard.test.ts`; `frontend/src/pages/users/users.test.tsx`; `frontend/src/widgets/app-shell.test.tsx` | `statusLabel` форматирует known status и `success`; Users page отправляет optional password при создании interactive user; AppShell восстанавливает refresh session до redirect на `/login`. |
| App router smoke | `frontend/src/app/router.test.tsx` | Поднимает production `appRoutes` в memory router и проверяет первый рендер 20 рабочих Dashboard-страниц + `/login` на реалистичных mocked API DTO. |
| I18n contract | `frontend/src/shared/i18n/i18n-contract.test.ts` | Проверяет parity ключей `ru`/`en`, непустые значения, отсутствие raw-key fallback, runtime switch i18next и динамические переводы для стабильных API contract values: pipeline/change/PR/runner/delivery/notification statuses, PR actions и PAT scopes. |
| Browser E2E | `frontend/e2e/critical-flows.spec.ts` | Проверяет seeded Dashboard → project pipelines → pipeline plan/logs/artifacts, repository code browser и mobile drawer Escape/focus contract на собранном приложении. |
| Accessibility smoke | `frontend/e2e/accessibility.spec.ts` | Запускает axe на всех 21 baseline route против real API/PostgreSQL stack и блокирует `serious`/`critical` violations. |
| Performance smoke | `frontend/e2e/performance.spec.ts` | Проверяет seeded API read p95 и Dashboard route ready-time против MVP regression budgets на real Compose stack. |
| Webhooks page unit | `frontend/src/pages/webhooks/webhooks.test.tsx` | Страница показывает delivered `in_app` notification events и запрашивает bounded notification history API. |
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
| Real-DB harness | Current CI применяет migrations к disposable PostgreSQL service под `forge_owner`. Полная owner/runtime role matrix, prior-schema upgrade и parallel isolated DB/schema остаются target. | **Current verified; target расширение** |
| Evidence seed | `frontend/scripts/seed-evidence.mjs` создаёт детерминированные demo repositories, projects, pipelines, runners, secrets metadata, environments, deployments, users и tokens для disposable local evidence stack. Запуск: `cd frontend && pnpm seed:evidence`. | **Current verified** |
| Screenshot evidence | `frontend/scripts/shoot-evidence.mjs` снимает predefined маршруты на `1920x1080` и `375x812`; это visual evidence, а не E2E assertion. | **Current verified** |
| E2E Compose evidence | `CI job e2e` использует disposable Compose stack с synthetic `CICD_SECRETS_KEY`, seeded repositories/projects/pipelines/artifacts и Playwright Chromium assertions. | **Current verified MVP** |

Fixture `seed:evidence` содержит только synthetic development values. Он не применяется к shared, staging или production окружению; значения секретов, password, bearer token, `CICD_SECRETS_KEY`, production URL и персональные данные не попадают в fixture output, test name, screenshot, trace или CI artifact.

## 4. Entry и exit criteria

| Уровень | Entry criteria | Exit criteria |
|---|---|---|
| Domain unit | Изменены domain rule, status enum или transition caller; тест изолирован от I/O. | Happy path, invalid transition и terminal-state boundary покрыты; focused test и relevant Cargo suite green. |
| API contract | Изменены route, DTO, HTTP status, validation или error behavior; router можно создать с требуемой fixture. | Проверены method/path, status и body/headers для changed public behavior; любой новый endpoint дополнительно проверен `curl` на running stack. |
| CLI contract | Изменены command, option, default, config precedence, output или exit code. | Help, parsing и изменённый наблюдаемый CLI contract green; для runtime/API flow также проходит `cli_real_api` integration smoke. |
| Frontend unit | Изменены component, page, helper, mutation state или accessibility semantics. | Покрыты normal, empty/error и permission/interaction boundary в затронутой области; `pnpm test` и `pnpm build` green. |
| Compose smoke | Изменены Dockerfile, Compose, image, startup/configuration или health/readiness route. | `docker compose config -q`, build, healthy startup, `GET /api/v1/health` и `GET /api/v1/readiness` успешны; временный stack остановлен. |
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
| Logs | Append/read сохраняют attempt-owned sequence; bounded `/logs/page` проверяется на `limit`, `after`, `q`, `next_after` и 404 для чужой attempt. Старые array endpoints остаются compatibility surface. |
| Pipeline plan | Trigger pipeline создаёт ровно один `pipeline_plans` snapshot на pipeline, replay `Idempotency-Key` возвращает тот же `plan_sha256`, raw config/template и normalised plan hashes сохраняются, прямой `UPDATE` snapshot отклоняется real-DB regression test-ом, а v1 `.forge-ci.yml` из bare repo сохраняет `v1-dag` jobs/needs/dependencies/required_tags/required_secrets/artifact_paths. Policy diagnostics, `on`/retry/`artifacts.expire_in` и job-level dispatcher требуют отдельной target suite. |
| Runner queue/leases | Current trigger/retry/manual start обязаны создавать durable `job_queue` row для non-manual queued attempt с copied `required_tags`; embedded claim обязан брать только untagged work, external runner claim обязан брать work через `job_queue` + `SKIP LOCKED` + `required_tags ⊆ runner.tags` + current `shell` executor compatibility, переводить row в `leased`, создавать active `job_leases`, requeue-ить unacknowledged offer после `ackDeadline`, завершать dispatch-eligible queue timeout без compatible execution path с diagnostic, закрывать queue/lease при success/failed/canceled, cancel pipeline и expired/missing lease reconciliation. Current external runner protocol MVP обязан регистрировать runner-а через bootstrap token, хранить credential/lease token только hash-ами, heartbeat-ить capacity/tags/capabilities, выдавать `workspace.checkoutUrl` + declared `attempt.secrets`/`attempt.artifacts`, принимать ack/control/`secrets:resolve`/artifact upload/renew/logs/complete, доставлять cancel signal external runner-у, проверять fencing generation, поддерживать bounded long-poll `waitSeconds` с wakeup после committed enqueue/unblock через process-local signal и PostgreSQL `LISTEN/NOTIFY`, переводить stale online runner без unexpired active lease в `offline`; `forge-runner` обязан иметь стабильный public CLI contract для запуска отдельного shell process, scoped secret env injection, active-lease heartbeat, masking, cancel polling, declared artifact upload и отправки stdout/stderr в `job_logs`. Target дополнительно требует расширенную lost-runner/restart race suite, fairness, resumable artifact sessions, richer log chunks, advanced capability/pool/protected-tag policy и sandbox/resource limits. |
| Auth и RBAC | Current enforcement требует `401`/`403`, enabled user, JWT/scoped PAT, PAT scopes/project boundary, session-bound access invalidation, refresh rotate/logout revoke, configurable CORS allowlist и audit event при заданном `CICD_AUTH_SECRET`; tenant isolation, service-account tokens, refresh-cookie/CSRF/session-family policy и persistent lockout остаются target. |
| Secrets/artifacts | Создание/metadata допускаются только через safe contract; plaintext secret не возвращается API/UI; artifact download не читает `storage_path` вне `CICD_ARTIFACTS_DIR` и возвращает `409` при checksum drift для новых uploads. Current embedded/external runners inject-ят только declared secrets и собирают только declared artifact files; runner protocol запрещает secret/artifact data plane до ack lease. Target дополнительно проверяет retention/scope, resumable sessions и redaction во всех error/audit/trace каналах. |
| Rate/body limits | Current in-process middleware должен возвращать `429` до handler/auth для auth/API/Git/artifact route classes и не ограничивать health/readiness/openapi/metrics; explicit body-limit regressions должны доказывать, что artifact/Git/JUnit upload routes принимают payload выше Axum default там, где это заявлено, а log append routes возвращают `413` выше явного лимита. Target дополнительно требует trusted proxy/distributed counters, per-account lockout, proxy-level time/concurrency policy и нагрузочный evidence. |
| Outbox и идемпотентность | Current `domain_events`/`outbox_messages` должны доказывать atomic terminal pipeline event, basic webhook retry, local `in_app`/`sse` notification fan-out/delivery, delivery attempt history и явный requeue failed-доставки новой generation; real-DB test проверяет replay/conflict для pipeline trigger `Idempotency-Key` и failed outbox delivery history/requeue. Target: general duplicate ingress, lease recovery, crash retry, full dead-letter policy и single observed outcome для всех async effects. |
| Backup/restore | Current helper должен проходить Python syntax check, checksum/manifest self-test, dry-run backup/restore и wrapper shell syntax в CI. Target дополнительно требует disposable restore drill с PostgreSQL restore, Git fsck, artifact checksum, read-only API smoke, RTO/RPO measurement и retained DR report. |
| Public contracts | Изменение API, CLI или generated client имеет focused contract test; breaking change в active `/api/v1` запрещён без новой версии и compatibility evidence. |
| UI | Изменённый critical flow имеет component/feature test; current route smoke обязан проходить для всех 20 рабочих Dashboard-страниц + `/login`; после E2E rollout -- real-flow assertion, mobile `375 px` где применимо, и axe check. Скриншот сам по себе недостаточен. |

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

Реестр требований и доказательств — [TRACEABILITY.md](TRACEABILITY.md). Для нового нормативного поведения источник requirement остаётся в `docs/PRODUCT_REQUIREMENTS.md`, ADR и `docs/contracts/` согласно ADR-0009, а RTM-строка связывает requirement с проверкой и evidence.

Каждый test case, spec или test description, который проверяет нормативное требование, содержит REQ-ID в имени либо в непосредственном описании в формате `REQ-<area>-<number>`, например `REQ-AUTH-001 viewer_cannot_mutate_project` или `it('[REQ-UI-014] ...')`. Один REQ-ID может иметь несколько уровней доказательства; изменение requirement обязано обновить связанные тесты и запись traceability.

Минимальная запись traceability связывает: `REQ-ID`, authoritative source/section, capability status, test level, file/test name, CI job, evidence artifact и результат. Отсутствующий REQ-ID для нового нормативного поведения является review finding; legacy tests получают идентификаторы при изменении затронутого поведения либо плановой инвентаризации.

## 8. CI-гейты

### Current verified

`.github/workflows/ci.yml` запускается на `push` и `pull_request` в `main`, имеет concurrency per ref и bounded `timeout-minutes` на каждом job; production image build и Trivy scan дополнительно ограничены step-level timeouts.

| Job | Обязательная проверка |
|---|---|
| `backend` | Rust 1.86 + PostgreSQL 17 service: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --features integration --test integration_db -- --test-threads=1`, `cargo build --release --workspace`, OpenAPI drift gate. |
| `frontend` | Node 22/pnpm 11: `pnpm install --frozen-lockfile`, `pnpm openapi:generate` + clean diff для `src/api/schema.d.ts`, `pnpm openapi:compat` self-test, OpenAPI backward compatibility diff с base commit/default branch, `pnpm lint`, `pnpm test`, `pnpm build`. |
| `compose-smoke` | Docker Compose: config validation, production image build, healthy startup, backend health/readiness, frontend nginx smoke, failure logs and cleanup. |
| `e2e` | Node 22/pnpm 11 + Playwright Chromium: installs browser dependencies, validates Compose config, starts production stack, seeds deterministic evidence, runs critical UI journeys, all-route axe smoke and seeded performance smoke, uploads Playwright report/traces/screenshots/video on failure and always cleans Compose volumes. |
| `security` | Rust/Node advisory gate, committed-secret baseline, SBOM drift и container image scan: SQLx optional MySQL/RSA feature guard, `cargo audit --ignore RUSTSEC-2023-0071`, `pnpm install --frozen-lockfile`, `pnpm audit --audit-level high`, `python3 scripts/scan_secrets.py`, `python3 scripts/generate_sbom.py --check`, production backend/frontend image build и `bash scripts/scan_container_images.sh forge-cicd-backend:ci forge-cicd-frontend:ci`. |
| `docs` | Python 3.12: Python syntax checks для docs/backup scripts, backup helper self-test/dry-run, shell wrapper syntax и `python3 scripts/verify_docs.py --all`. |

Current CI не запускает isolated owner/runtime migration matrix, prior-schema upgrade, Lighthouse, OpenAPI examples validation, `cargo-deny`, history/container secret scan или coverage ratchet. Backend release build, OpenAPI backward compatibility, seeded performance smoke и critical container image scan уже включены как MVP-gates. Playwright/axe включены как MVP-gate для critical flows и всех 21 baseline route, а Vitest закрывает базовый RU/EN i18n contract; full auth/RBAC E2E, full keyboard-only/theme a11y, full locale E2E, load/30-day SLO evidence, Lighthouse budgets и coverage evidence остаются target.

### Target approved

| Gate | Required evidence |
|---|---|
| `migration-test` | Current real-DB suite плюс isolated PostgreSQL lifecycle, owner/runtime roles, prior-schema upgrade, unconditional cleanup, logs и applied version/checksum. |
| `openapi-contract` | Current generation/drift/codegen/backward compatibility checks плюс validate/examples. |
| `backend` | Workspace fmt/clippy/test/release build, domain/app/API contract tests и coverage artifact. |
| `cli-contract` | Help, config precedence, JSON output, exit codes, extended token redaction fixtures и real API/PostgreSQL automation flow. |
| `frontend` | Frozen lockfile, lint, Vitest, generated-client typecheck, production build и coverage artifact. |
| `compose-smoke-plus` | Current compose smoke плюс retained compose logs/artifacts, release tag/image matrix, read-only API scenario и restore-drill binding. |
| `e2e-a11y-performance` | Current critical-flow/all-route axe gate плюс full auth/RBAC/CLI journeys, all-route keyboard coverage, theme axe matrix, Lighthouse budgets и retained reports. |
| `security` | Current dependency audit/secret/SBOM drift gate и critical container image scan плюс target `cargo-deny`, broader image policy и deeper history/container secret scan по согласованной severity policy; confirmed secret или blocking vulnerability блокирует gate. |
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
