# Changelog

Все значимые изменения в этом проекте документируются в этом файле.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
и этот проект стремится соответствовать [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Дата release-записей: формат ISO 8601 (`YYYY-MM-DD`).

## [Unreleased]

### Added

- Documentation guard: `scripts/verify_docs.py --all` now fails when `/readiness` migration-version examples drift behind committed SQLx migrations.
- Auth hardening: migration `0026_session_family_reuse` adds refresh session `family_id`/`replaced_by`/`reuse_detected_at` and `users.token_version`; refresh rotation is transactional and reuse of a replaced refresh token revokes the whole session family and invalidates already issued access JWTs.
- Auth/RBAC route-policy inventory: backend now keeps executable `ROUTE_POLICIES` for every generated OpenAPI/Git/metrics operation, cross-checks router path literals against that registry, denies unpublished API/Git routes under auth middleware and has a unit gate that fails when a new route ships without policy.
- CI reliability guard: workflow runs are concurrency-bound per ref and all jobs have `timeout-minutes`; production image build and Trivy container scan also have step-level timeouts to avoid stuck security gates.
- Frontend API transport hardening: `ApiError` now preserves server envelope fields (`code`, `message`, `request_id`, details, status and `Retry-After`) and separates API, network and cancelled failures for UI/RBAC diagnostics.
- Dashboard auth hardening: `/users` can create interactive users with optional password credentials, and `AppShell` restores a stored refresh session before redirecting to `/login`.
- Git-server parity: code browsing `GET /api/v1/repos/{repo}/tree|blob` (safe bare-repository ref fallback and binary/512 KiB preview guard), tags read API, release CRUD, repository `public`/`private` visibility and Smart HTTP read access control.
- CI parity: trigger-time pipeline variables stored in `pipelines.variables` and injected into jobs only as `CICD_VAR_<KEY>`; public SVG pipeline badge; JUnit XML report ingest/read API and dashboard summary.
- Migration `0006_git_ci_details`: `repositories.visibility`, `releases`, `pipelines.variables`, `test_reports`.
- Execution attempts (REQ-EXEC-004): migration `0007_execution_attempts`, attempt-owned job logs/artifact metadata, retry job/pipeline creates a new attempt without deleting previous logs, API/OpenAPI endpoints for attempts and attempt logs, Dashboard attempt switcher, CLI `job attempts` and `job logs --attempt`.

- P0 (GitLab/Jenkins parity): `CICD_*` CI variables in every job (`PIPELINE_ID`, `JOB_ID/NAME`, `STAGE_NAME`, `PROJECT_ID/NAME`, `COMMIT_REF/SHA`, `ARTIFACTS_DIR`), shared per-pipeline artifacts directory passed between jobs/stages, job `timeout` DSL (default 1h, kill on expiry), merge gate on protected branches (`projects.protected_branches`, 409 without a success pipeline on the PR head).
- P1: webhook HMAC-SHA256 signing (`webhooks.secret` → `X-Forge-Signature`), PAT expiry (`api_tokens.expires_at`, `expires_in_days`), SSE live logs `GET /api/v1/jobs/{id}/logs/stream`, DSL `allow_failure` / `when: manual` + `POST /api/v1/jobs/{id}/start` approval gate.
- Migration 0005_execution_gaps; outbox now emits real domain events on pipeline terminal transitions (fixes dead webhook fan-out); compose passes `CICD_AUTH_SECRET` through to the backend.


- RBAC (AUTHZ_CONTRACT §RBAC): role-политики на всех роутах (`viewer`→read, `developer`→write, `maintainer`→secrets/tokens-read/runners-read, `admin`→users/tokens/runners manage); 403 с `auth.denied` audit-событием; PAT (`cicd_…`) принимаются enforcement-слоем с ролью владельца, `last_used_at` обновляется.
- Frontend auth: страница /login (i18n ru/en), Bearer в api-клиенте, прозрачный refresh на 401, гард-редирект при включённом auth-режиме; login/logout аудит (`auth.login_success`/`auth.login_failed`).
- Error envelope по API_CONTRACT: `{error:{code,message,request_id}}` + `x-request-id` header (принимается входящий, генерится при отсутствии).
- Pagination (NFR-PERF-02): `limit`/`offset` (cap 200) на GET /projects и /projects/{id}/pipelines.
- Rate limiting (THREAT_MODEL brute-force): 30 login-попыток/мин (429 после), фиксированное окно.
- Outbox/scheduler (ADR-0006, REQ-AUTO-003): миграция 0004 (`domain_events`, `outbox_messages`, `schedules.last_fired_at`), воркер доставки webhooks с retry/backoff (15s..1h, 8 попыток, dead-letter), cron-scheduler с атомарным claim (без double-fire), счётчики метрик.
- Secret injection (REQ-SEC-002): project secrets расшифровываются и подаются в job как env-переменные; значения маскируются `***` в job-логах.
- Observability floor (SLO/METRICS): `GET /metrics` (Prometheus text: http_requests, 5xx, login attempts/failures, pipelines_created, outbox delivered/dead), request_id-пропагация.
- Outbox delivery history/requeue (REQ-AUTO-003): migration `0012_outbox_delivery_history` (`project_id`, `generation`, `replay_of_id`, `failed_at`, `outbox_delivery_attempts`), project-scoped delivery API/OpenAPI, Webhooks dashboard panel, failed delivery requeue, backend/frontend tests.
- Backup/restore helper (NFR-REL-04): `scripts/forge_backup.py` plus `backup.sh`/`restore.sh`/`verify-backup.sh` create, verify and guarded-restore local Docker Compose backups for PostgreSQL, Git and artifact volumes; CI checks syntax, self-test and dry-run.
- Artifact integrity MVP (REQ-ART-002): migration `0013_artifact_checksums` stores SHA-256 for new artifact uploads; download rejects checksum drift with `409`; Artifacts UI shows digest prefix.
- Schedule fire slots (REQ-AUTO-002): migration `0014_schedule_fire_slots` adds `next_fire_at` and `schedule_fires`; scheduler evaluates strict five-field UTC cron, materializes unique fire slots and triggers pipelines idempotently; schedules with `last_fire_error` wait for an explicit PATCH instead of blocking the due batch.
- CI: PostgreSQL service в backend-джобе — integration-тесты гоняются в CI; гейт `schema.d.ts` up-to-date; docs-джоба verify_docs.
- Supply-chain gate (NFR-SEC-06): CI job `security` запускает SQLx optional MySQL/RSA feature guard, `cargo audit --ignore RUSTSEC-2023-0071`, `pnpm audit --audit-level high`, committed-secret baseline scan и SBOM drift check; parser compatibility dependency переведена на maintained `yaml_serde` package без deprecated `serde_yaml`/`unsafe-libyaml` в resolved Cargo graph.
- Release/security hardening: backend CI теперь выполняет `cargo build --release --workspace`; security job строит production backend/frontend images и запускает pinned Trivy CLI (`aquasec/trivy@sha256:62b1e65e8869bc4b4c6aa4fa2b21595256c7c2f6018a9d9ad61caf87187c1969`) через `scripts/scan_container_images.sh` как blocking critical fixable CVE gate.
- Frontend runtime image поднят до `nginx:1.31.4-alpine`, а compose-smoke frontend check теперь ждёт nginx retry-loop вместо одиночного раннего HEAD-запроса.
- Evidence tooling: `shoot-evidence.mjs` корректно передаёт тему в browser context, frontend CI проверяет syntax evidence scripts, PR/contributor checklists синхронизированы с текущими CI gates.
- Artifact retention MVP: migration `0023_artifact_retention` добавляет `expires_at`/`purged_at`, `CICD_ARTIFACT_RETENTION_DAYS` задаёт TTL новых uploads, download блокирует expired/purged artifacts, backend retention worker удаляет expired local files и пишет `artifact.purged` audit.
- Protected delivery MVP: migration `0024_environment_approvals` добавляет `environments.protected/required_approvals`, append-only `deployment_approvals`, `deployments.rollback_of_id`, API/UI/CLI approval actions и rollback, который создаёт отдельную traceable deployment запись через pipeline.
- OpenAPI compatibility gate: `pnpm openapi:compat` сравнивает bundled contract с base/default branch и блокирует удаление существующих paths/methods/responses/parameters/schema fields, смену типов/format и новые required request параметры в active compatibility surface.
- CI actions: workflow переведён на Node 24-compatible `actions/checkout@v7`, `actions/setup-python@v7` и `pnpm/setup@v2` для pnpm v11/Node 22 runtime; target immutable digest pinning остаётся release hardening.
- Compose smoke: CI теперь собирает production Docker images, поднимает `docker compose up --build -d`, проверяет backend health/readiness и frontend nginx; frontend Dockerfile использует frozen pnpm lockfile, а secret-scan pruning не заходит в generated dirs.
- Browser E2E/a11y: добавлен Playwright Chromium gate против собранного Docker Compose stack с deterministic `seed:evidence`; critical journey проверяет Dashboard → project pipelines → pipeline plan/logs/artifacts, repository code browser и mobile drawer Escape/focus contract, а axe smoke падает на `serious`/`critical` violations representative pages. CI сохраняет Playwright report, traces, screenshots и video на failure через `actions/upload-artifact@v7`.
- I18n contract: Vitest проверяет parity ключей `ru`/`en`, непустые leaf-значения, runtime language switch и динамические переводы для стабильных API contract values, включая status/action ключи.
- Performance smoke: Playwright E2E теперь проверяет seeded API read p95 и Dashboard route ready-time против MVP regression budgets; 30-day SLO, Lighthouse и load reports остаются target.
- Documentation cleanup: удалена лишняя служебная страница из продуктовых docs; рабочие проверки теперь ссылаются на `docs/DEVELOPMENT_GUIDE.md`.
- CLI operational surface: `cicd-cli` теперь поддерживает `CICD_API_TOKEN`/`--token`, `CICD_OUTPUT`/`--output json|table`, pagination для projects/pipelines и HTTP-only команды для runners, secrets, artifacts, environments/deployments, schedules, webhooks/outbox, notifications, reports/audit, users, project members и API tokens.
- Dependency review: `git2 0.21.0` remediation проверен, но отложен из-за несовместимости с закреплённым Rust 1.86; `git2 0.20.4` warnings остаются documented accepted findings.
- SBOM: `scripts/generate_sbom.py` → docs/assets/sbom.json (CycloneDX-lite, 344 компонента; CISA Minimum Elements).
- Пустой `CICD_AUTH_SECRET` теперь трактуется как не настроенный secret, поэтому compose default корректно оставляет локальный trusted-network mode.

### Fixed

- API error envelope no longer exposes raw SQLx/internal error text to clients; internal details are logged server-side and `500` responses use a generic `internal_error` message.
- Environments Dashboard теперь явно помечает metadata/deployment-history как MVP без protected approvals/rollback, а `deployment` action переименован в запись deployment-record; user/CLI/Git docs уточняют current artifact TTL и Git proxy/direct ports.
- Embedded runner secret injection now reads `project_secrets.key` and `encrypted_value`, so saved project secrets are actually decrypted and passed to jobs as runtime env variables.
- Embedded runner now drains job stdout/stderr while the process is running and serializes log append sequence per attempt, avoiding blocked jobs on large output.
- Runner and pipeline cancel paths now finish open `execution_attempts`, avoiding stale `queued`/`running` attempts after job failure, timeout or cancel.

### Added

- Auth foundation (AUTHZ_CONTRACT Phase 1, REQ-AUTH-002 частично): миграция `0003_auth_foundation` (user_credentials/sessions), argon2id + JWT access 15m (HS256, CICD_AUTH_SECRET) + refresh-ротация 30d, `POST /auth/login|refresh`, Bearer-enforcement middleware (allowlist: health/openapi/login/refresh/git; без секрета — trusted-network режим), users create/update принимают password; e2e проверено на живой БД (401 без токена, 200 с токеном, 401 неверный пароль).
- Real-DB integration-уровень (TEST_PLAN): `backend/tests/integration_db.rs` (feature `integration`): идемпотентность миграций, project CRUD, auth сессии+ротация+disable — 3/3 зелёные на test-compose PostgreSQL.
- Strangler ADR-0005 шаг: `PipelineStatus` + `aggregate_status` перенесены в `cicd-domain` (4 unit-теста), реэкспорт через shim.

### Changed

- Cargo.lock: time 0.3.41 / simple_asn1 0.6.3 (rust 1.86 MSRV-совместимость).
- CI: backend-гейт openapi drift-diff, frontend-гейт `pnpm openapi:check`.

### Added

- ADR-0008 реализован: `backend/migrations/` (baseline `0001_bootstrap_v1.sql` verbatim из startup bootstrap, `0002_runtime_role.sql` grants forge_runtime), crate `cicd-migrate` (dry-run/apply/verify, advisory lock `FORGE`), server стартует через embedded `sqlx::migrate!`; проверено на живой test-compose БД (apply → идемпотентный повтор).
- Тестовый контур: cicd-migrate прогоняется в docker-сети test-compose.

- ADR/API_CONTRACT реализованы частично (Current): utoipa-аннотации на контроллерах core-групп (health/projects/pipelines/jobs+logs), `GET /api/v1/openapi.json`, канонический артефакт `openapi/openapi.yaml` (546 строк, OpenAPI 3.1) через `cargo run --bin openapi-dump`, CI drift-гейт `diff` против коммита; фронт: `openapi-typescript` генерация `src/api/schema.d.ts` (`pnpm openapi:generate`), гейт `pnpm openapi:check`, DTO Project/Pipeline/Job/Stage переведены на generated-типы. Platform-группа (runners/secrets/…) — следующий шаг.

### Changed

- `git_host.rs`: `unwrap()` в тестах заменены на `expect` (прод-код unwrap-free и был).

### Added

- SDLC-набор по отраслевым стандартам (ISO/IEC/IEEE 12207/15289, IEEE 829, ASVS 4.0, CISA SBOM 2026): TEST_PLAN (уровни тестирования, SEV1–SEV4, coverage-политика), TRACEABILITY/RTM (REQ-ID ↔ контракты ↔ тесты ↔ evidence, 25 capability + NFR), THREAT_MODEL (STRIDE по границам доверия, маппинг на контракты), RISK_REGISTER, DISASTER_RECOVERY (tiers/RTO/RPO/3-2-1-1/дриллы), INCIDENT_RESPONSE (SEV-матрица, постмортемы, security-инциденты), THIRD_PARTY (инвентарь, license-политика, CycloneDX SBOM target), ACCESSIBILITY (WCAG 2.2 AA программа), SLO (SLI/SLO/error budget), METRICS (DORA + runtime).
- PRODUCT_REQUIREMENTS: все capability и NFR получили REQ-ID/NFR-ID; RTM-строка обязательна в PR.
- CODEOWNERS, .well-known/security.txt.

- Канонический реестр имён и authority matrix: ADR-0009; устранены конфликты migration path, outbox-имён и runner namespace.
- Нормативные контракты `docs/contracts/` (API, AUTHZ, RUNNER_PROTOCOL, PIPELINE_DSL, EVENT, DATA_LIFECYCLE, MIGRATION, UI_API) и narrative-слой `docs/architecture/` с sequence-флоу и transition map.
- Документация по аудиториям: USER_GUIDE, DEVELOPMENT_GUIDE, OPERATIONS, PRODUCT_REQUIREMENTS; CURRENT_STATE и DOCUMENTATION_GOVERNANCE; `scripts/verify_docs.py` (ссылки/канон/статусы/дубликаты скринов).
- Public repo surface: LICENSE (FerrPOINT proprietary source-available), CONTRIBUTING, SECURITY (NOT production-safe предупреждение), SUPPORT, issue/PR-шаблоны, Dependabot.
- UI: мобильная навигация-drawer, карточные layout-ы (runners/users/tokens/environments), доступные confirm-диалоги вместо `window.confirm`, страница pull-запроса с «Посмотреть изменения», живые метрики дашборда.
- Evidence pipeline: deterministic seed (`pnpm seed:evidence`) и воспроизводимые скриншоты (`pnpm shoot:evidence`), реестр `docs/assets/screens/manifest.md` (45 скринов: 20 базовых страниц, 19 состояний действий и 6 mobile states).
- `docs/TECH_CHOICES.md` и `docs/LIBRARIES.md` восстановлены как рабочие справочники: current stack, target Rust candidates, dependency policy и список готовых CI/CD решений на Rust для reference.

### Changed

- README переписан: статус/границы доверия, capability matrix, тур по аудиториям.
- 44 legacy-дока консолидированы в гайды/контракты; оставлены redirect-stub-ы на один release-цикл.
- PostgreSQL в docker-compose публикуется только на 127.0.0.1; `CICD_RUNNER_MODE` пробрасывается в backend.
- Из текущего frontend baseline убран статичный `/admin`; системная справка и `CICD_` переменные остались в `/settings`.
- Скриншоты пересняты единым прогоном: desktop 1920×1080, mobile 375×812, реальные пайплайны/PR/артефакты.
- `RUNNER_ARCHITECTURE.md` больше не использует старую терминологию для runner-а: целевая граница названа `runner process` / `forge-runner`.
- На страницах Runners, Schedules, Webhooks и Notifications добавлены статусные capability-callout блоки, чтобы MVP/configuration-only/target возможности не выглядели завершёнными.

### Planned

- Baseline roadmap: immutable pipeline plan/DAG; diagnostic command spans/stream classification; external runner + durable queue/leases/fencing; schedule timezone/DST/misfire policies; production outbox lease/reconciliation/full dead-letter policy/metrics; external notification adapters/inbound provider webhooks; off-site/PITR backup platform with verified restore drill; artifact object storage/legal hold/quotas; tenant isolation/service accounts/scoped Git credentials/production session hardening.

## [0.1.0] — 2026-08-26

Первый публичный baseline MVP self-hosted CI/CD control plane (Rust/Axum + React).

### Added

- Проекты CRUD: `name` / `repository_url` / `default_branch`, удаление с CASCADE.
- Git-хостинг: bare-репозитории, Smart HTTP (`clone`/`fetch`/`push`), опциональная token-auth, `post-receive` → автоматический пайплайн.
- Пайплайны из `.forge-ci.yml` (stages/jobs/image/command) с fallback-шаблоном; отмена и повтор, pull requests (create/merge/close/reopen, compare).
- Embedded runner: Docker-контейнеры (`forge-job-<id>`) или host shell, стриминг stdout в `job_logs`, отмена через PID-map.
- Append-only логи джобов с sequence и поллингом; артефакты (upload/download до 50 MiB, локальная директория `CICD_ARTIFACTS_DIR`).
- Секреты проектов: AES-256-GCM at rest, значение не возвращается API.
- Environments/deployments, reports (success rate/duration), audit log (append-only, последние 200 записей).
- Users/roles и API-токены: хранение и управление (enforcement middleware — см. Planned).
- React Dashboard: 21 маршрут / 20 страниц + `/login` (заглушка без auth-запроса), i18n (ru/en).
- CLI `cicd-cli` (HTTP-only): project/pipeline/job; CI-пайплайн GitHub Actions; Docker Compose окружение.

### Known limitations (honestly)

- Нет auth/RBAC/TLS: API и Dashboard полностью открыты, CORS permissive — только доверенные сети, не для production (см. `SECURITY.md`).
- PostgreSQL в compose опубликован на все интерфейсы; `CICD_GIT_INTERNAL_TOKEN` обязательно менять для shared-деплоя.
- Schedules/webhooks/notifications — конфигурация без исполнения; login — UI-заглушка.
- Схема БД через bootstrap `store::migrate()`, versioned migrations ещё не введены.

[Unreleased]: https://github.com/FerrPOINT/CI-CD/compare/0.1.0...HEAD
[0.1.0]: https://github.com/FerrPOINT/CI-CD/releases/tag/0.1.0
