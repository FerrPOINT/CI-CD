# Base Production-Readiness Plan

**Goal:** Довести Base до управляемого production-ready контура: все сервисы согласованно работают, проверяются, наблюдаются, восстанавливаются и документированы.

**Architecture:** Base остаётся набором сервисов со строго определённым владением: `services-base` — общие auth/UI contracts; Admin Panel — branding и runtime catalog; Project Workflow — namespaces/workflows; Fleet Control — agent runtime и bindings; Forge CI/CD — repositories and pipelines. Каждый шаг проверяет контракт на уровне unit/integration/E2E, затем на реальном local Compose стенде.

**Tech Stack:** Rust/Axum/SeaORM/SQLx, Python/FastAPI/SQLAlchemy, Java/Spring Boot, React/Vite, PostgreSQL, Redis, Docker Compose, Prometheus/Grafana/Alertmanager, GitHub Actions.

---

## Current Baseline

- Compose configuration is valid; all runtime services are up.
- Prometheus scrapes five production service targets; all are UP; Alertmanager has no firing alerts.
- The nightly five-database backup is scheduled, includes Task Tracker uploads and the Java Agent Chromium profile, and the latest run has no dump errors.
- CI is green on the current main heads except jobs currently executing after recent changes.
- Project Workflow hosted CI was added. Its first run exposed a real strict-mypy regression in a UI smoke script; the regression test and fix are now in the next CI run.
- Fleet Control can reach Project Workflow catalog endpoints but legacy sample bindings (`dev`/`qa`, `workflow-dev`/`workflow-qa`) do not exist in the current catalog and are correctly reported stale. The contract needs an explicit unbound state and a supported rebind path, rather than false health.

## R1: Stabilize Current CI Heads

1. Inspect failed Project Workflow CI logs and add a failing regression test.
2. Apply the smallest type-safe fix, run focused tests, full suite, integration suite, coverage, ruff and mypy.
3. Rebase on remote `master`, push and confirm the GitHub Actions run for the exact head SHA.
4. Track Java Agent remote changes by SHA; only fix verified red `main` failures.

## R2: Reconcile Fleet Control and Project Workflow

1. Define three explicit local binding states: `connected`, `stale`, `unbound`.
2. Add pure unit coverage for status classification: exact ID/name match, removed remote item, partial binding, and no binding.
3. Update repository reconciliation to preserve stale data for operator action and label absent bindings as unbound.
4. Expose the live Project Workflow catalog through a versioned Fleet Control operator endpoint; validate response shape, timeout and upstream failure behavior.
5. Add a supported rebind command/API that validates selected namespace/workflow IDs against the live catalog before persisting a Fleet binding.
6. Add API and frontend tests for connected, stale, unbound and upstream-error states; update OpenAPI and contract docs.
7. Rebuild Fleet Control and prove the catalog, binding transition and agent workflow view on local Compose.

## R3: Verify Shared Platform Contracts

1. Central auth: health, JWKS, issuer/audience validation and operator/user permission failures.
2. Admin Panel: runtime branding and services catalog, ETag/conditional request behavior, consumers in every React application.
3. Java Agent: Fleet readiness adapter, sessions/chat contract and actuator metrics on its internal management port.
4. Forge: registered project/repository, runner lease lifecycle, pipeline execution, attempt logs and artifacts through the live API.
5. Task Tracker and Wiki: authenticated API, empty/error states and each application’s critical workflows.

## R4: Operational Readiness

1. Validate every Compose service’s health endpoint and frontend listener; rebuild/recreate services changed since the last image build.
2. Check Prometheus targets, Grafana datasource/dashboard queries and Alertmanager receiver delivery with a reversible smoke alert.
3. Verify backup age, five DB dumps and Task Tracker uploads archive; run a clean restore drill and remove its temporary DB.
4. Check no secrets enter source, screenshots, logs or commits.

## R5: Documentation, Release and Evidence

1. Cross-check README, API docs/OpenAPI, migrations, ADRs, environment docs and implementation plans against code and runtime contracts.
2. Correct only factual drift and add deterministic documentation regression tests where possible.
3. Run repository gates, compose smoke and relevant browser/E2E checks for touched applications.
4. Commit each coherent increment, rebase before each push, then confirm CI by exact head SHA.
5. Produce a final matrix: contracts, test evidence, deployed image status, monitoring, backup/restore and residual user-gated scope.
