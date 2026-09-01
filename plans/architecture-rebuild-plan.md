# Forge CI/CD Target Architecture Implementation Plan

> **Status 2026-09-01:** historical/non-normative working plan. Several slices from this plan are already current MVP (workspace root, `domain`, `cli`, SQLx migrations, auth/RBAC MVP, outbox/schedules MVP, runner protocol MVP, OpenAPI YAML drift gate). Do not execute this file as-is; reconcile any new slice against `docs/CURRENT_STATE.md`, `docs/ROADMAP.md`, `docs/TRACEABILITY.md` and the relevant `docs/contracts/*` owner first.
>
> **For agents:** preserve current public REST paths and `openapi/openapi.yaml`; treat remaining work as strangler migration/hardening, not a blank rebuild.

**Goal:** Turn Forge CI/CD from a single Rust crate into a testable workspace with clear API/application/domain/infrastructure boundaries, a dedicated CLI package, versioned migrations, and a safe base for authentication and distributed runners.

**Architecture:** The backend becomes a Cargo workspace. `domain` owns pure business types and port traits; `app` owns use cases and transaction boundaries; `infra` owns PostgreSQL, filesystem Git/artifact storage and runner adapters; `api` owns HTTP DTOs/routes/middleware/OpenAPI; `server` only composes dependencies; `cli` is a separate HTTP client binary. Existing endpoints remain stable through a strangler migration: thin wrappers stay in the current server until each feature is moved and real-DB tests cover it.

**Tech Stack:** Rust 2024, Axum 0.8, SQLx 0.8, PostgreSQL 17, tokio, `thiserror`, `validator`, `clap`, `reqwest`, `utoipa`, React 19, OpenAPI-generated TypeScript client, TanStack Query, Vitest, Playwright.

---

## Target repository layout

```text
backend/
├── Cargo.toml                    # workspace + unified dependency versions
├── domain/                       # entities, enums, ports; no Axum/SQLx/filesystem
├── app/                          # use cases, policies, commands, transactions
├── infra/                        # PostgreSQL repositories, migrations, Git/artifact/runner adapters
├── api/                          # HTTP routes, DTOs, auth middleware, OpenAPI generator
├── server/                       # composition root and process lifecycle
├── cli/                          # `cicd-cli` HTTP client (Clap)
├── migration/                    # SQLx versioned migrations + migration runner
├── tests/                        # cross-package black-box integration tests
└── scripts/                      # test DB, backup/restore/verify helpers
frontend/
├── src/api/                      # generated OpenAPI types/client + query hooks
├── src/entities/                 # stable client entities
├── src/features/                 # forms/actions grouped by capability
├── src/pages/                    # route screens
├── src/shared/                   # UI, async states, i18n, utilities
├── src/widgets/                  # AppShell and composite UI
└── e2e/                          # Playwright browser specs
openapi/openapi.yaml              # generated contract committed with code
```

## Dependency rules

- `domain` imports only stdlib, serde, uuid, chrono, thiserror.
- `app` may import `domain`; it defines application errors and use cases but imports no Axum/SQLx/git2/Docker.
- `infra` implements domain/application ports. It may import `domain`, `app`, SQLx, git2, AES-GCM and filesystem/process dependencies.
- `api` maps HTTP DTOs to application commands. It depends on `app`, `domain`, and `infra` only for composition-facing interfaces; no business SQL.
- `server` is the only binary that creates `PgPool`, repositories, storage adapters, scheduler and runner supervisor.
- `cli` uses `reqwest` + generated/public DTO types only; it never links server/infra.
- Frontend API types are generated from OpenAPI. Hand-written fetch types are removed only after each generated endpoint is adopted.

## Architectural decisions to record before/with migration

1. `ADR-0005`: workspace and layered ports/adapters architecture.
2. `ADR-0008`: versioned SQLx migrations; `store::migrate()` bootstrap retired after baseline migration.
3. `docs/contracts/AUTHZ_CONTRACT.md` + `docs/AUTH_IMPLEMENTATION_SPEC.md`: auth/RBAC and API-token model.
4. `ADR-0007`: runner security boundary; Docker socket is never exposed to the control-plane API container.
5. `ADR-0009`: canonical names/paths; artifact retention and backup ownership live in `docs/contracts/DATA_LIFECYCLE.md`.
6. `ADR-0006`: PostgreSQL outbox for domain events and delivery.

## Ordered delivery

### Phase A — Foundation and no-behavior-change extraction

1. Add workspace root `backend/Cargo.toml`; create packages `domain`, `app`, `infra`, `api`, `server`, `cli`, `migration` with minimal manifests and compilation tests.
2. Move `JobStatus` and transition tests to `backend/domain`; introduce domain IDs/newtypes only where they do not change serialized REST values.
3. Move current `src/bin/cicd-cli.rs` to `backend/cli/src/main.rs`; preserve executable name and all current project/pipeline/job commands.
4. Create `backend/shared` only if cross-cutting config/error/ID utilities prove necessary; do not add a dumping-ground crate.
5. Keep the current monolith behind `server` compatibility adapters until routes are migrated feature-by-feature.
6. Add CI workspace commands: fmt, clippy workspace/all-targets, test workspace, release build.

**Gate:** Old CLI help contract and existing API/domain tests remain green; `cargo test --workspace` passes.

### Phase B — Configuration, errors, migrations, integration test harness

1. Create typed config grouped into `DatabaseConfig`, `HttpConfig`, `GitConfig`, `ArtifactConfig`, `RunnerConfig`, `AuthConfig`.
2. Add central `AppError` with safe client message, code, field errors and HTTP mapping. Database errors never reach API responses.
3. Introduce SQLx migration runner and create immutable baseline migration from current schema; future schema changes are only new migration files.
4. Add isolated `docker-compose.test.yml` and a test fixture that starts clean PostgreSQL, applies migrations and truncates between tests.
5. Add black-box real-DB tests for project CRUD, status aggregation, log sequence, platform resources, artifact upload/download and Git hook auto-trigger.
6. Add error tests for 400 validation, 404, 409 uniqueness/state conflicts and 503 dependency unavailable.

**Gate:** CI migrations job applies baseline to empty Postgres; all current API contract tests become real-DB tests unless deliberately no-DB health checks.

### Phase C — Application/infra/API strangler migration

Migrate one vertical slice at a time, with use case + port + postgres adapter + HTTP route + real DB tests:

1. Projects and pipelines.
2. Job lifecycle, logs, cancellation and retry.
3. Git repositories, ref browsing, compare and PR merge.
4. Platform resources: secrets, artifacts, environments/deployments, schedules, webhooks/notifications, reports and audit.
5. Users, roles and tokens.

For every slice, remove SQL from handlers, keep endpoint paths and JSON payload compatibility, and update OpenAPI + generated frontend types.

### Phase D — Security and execution correctness

1. Current MVP already has password identity, Argon2id, JWT access/refresh rotation/logout/revoke and guarded browser routes when `CICD_AUTH_SECRET` is set.
2. Current MVP already has project memberships, route policy checks, scoped PAT enforcement and Git Smart HTTP project checks; tenant isolation, service-account tokens, scoped Git credentials and production cookie/CSRF/session-family controls remain target.
3. Current MVP already has hashed PAT lookup, scopes, expiry/revoke and last-used update.
4. Current MVP already has configurable CORS, request IDs, in-process rate limiting and bounded request bodies; distributed/proxy policy remains target.
5. Current MVP already has `/api/v1/runner/*` pull protocol, durable `job_queue`, tags/current executor capability matching, leases, ack/renew/control/logs/artifacts/secrets and reconciliation; production fairness/pools/protected tags/sandbox remain target.
6. Execution has an external `forge-runner` shell MVP, but sandboxed production runner isolation is still open. Do not mount Docker socket into the API server. Inject decrypted secrets only into the runner execution environment; redact registered values from logs.

### Phase E — Automation, operations and UX proof

1. Durable cron scheduler with timezone, next-run calculation, idempotent trigger and audit event.
2. Outbox-backed webhook/notification delivery: HMAC signature, retries/backoff, attempts and delivery history.
3. Artifact checksum, retention worker, quota and backup/restore coverage for database, Git and artifacts.
4. Prometheus metrics/readiness, alerts/runbook, restart policy and health checks.
5. Add `shared/ui/async-states.tsx`, error boundaries, retry UX and page-level loading/empty/error consistency.
6. Add Playwright flows for login/RBAC, project → pipeline → logs/artifacts, runner lifecycle, secrets visibility, mobile tables/sidebar. Capture 375, 1920 and 2560 screenshots.

## Required documentation refresh

- Rewrite `docs/ARCHITECTURE.md` to target/current split and record package boundaries.
- Update `docs/TESTING.md`, `docs/MIGRATIONS.md`, `docs/SECURITY.md`, `docs/CI_CD.md`, `docs/DEPLOYMENT.md`, `docs/RESILIENCE.md`, `docs/STORAGE.md`, `docs/GLOSSARY.md`, `docs/PROJECT_ADMIN.md`, `docs/SYSTEM_ADMIN.md`, `docs/FRONTEND_ARCHITECTURE.md`, `docs/UI_UX.md` after corresponding features exist.
- Remove claims that execution, platform automation, or distributed runners are complete until acceptance tests prove them.
- Add `openapi/README.md`, generated OpenAPI contract and CLI documentation for the dedicated package.
