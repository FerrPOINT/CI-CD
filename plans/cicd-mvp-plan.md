# CI/CD MVP Implementation Plan

> **Status 2026-09-01:** historical seed plan. The original MVP has been exceeded: current baseline includes Git hosting, `.forge-ci.yml` parsing, execution attempts, durable queue, leases, external runner protocol MVP, secrets, artifacts, schedules, webhooks, notifications, auth/RBAC MVP, OpenAPI and 45 screenshot evidence files. Use `docs/CURRENT_STATE.md`, `docs/ROADMAP.md` and `docs/TRACEABILITY.md` for actual scope.

**Goal:** Build a self-hosted CI/CD control plane with projects, manually triggered pipeline runs, ordered stages/jobs, logs, and a dashboard.

**Architecture:** The backend is a Rust Axum API backed by PostgreSQL. It persists projects, pipelines, stages, jobs and logs, and exposes safe state transitions for queued/running/success/failed/canceled jobs. The React/Vite frontend consumes the API to show the pipeline dashboard and trigger runs. Docker Compose provides the local runtime on the workspace 22XX port range.

**Tech stack:** Rust 2024, Axum, SQLx/PostgreSQL, Tokio, Serde; React 19, TypeScript, Vite, Vitest; Docker Compose; GitHub Actions.

## Scope

- Projects: create and list repositories.
- Pipelines: trigger from project + ref, list recent runs, retrieve run detail.
- Pipeline graph: stages and jobs with explicit ordering and status.
- Job actions: start, complete success/failure, cancel; append/read logs.
- Dashboard: select project, trigger pipeline, inspect run status and logs.
- Local operations: Postgres, backend health endpoint, frontend image, unit/API tests, CI workflow.

## Deliberately deferred / still relevant target

- Production sandbox/resource policy, runner pools/protected tags/fairness, tenant isolation, service-account tokens, scoped Git credentials, production cookie/CSRF/session-family policy, external notification adapters, inbound provider webhook handlers, artifact retention/object storage, outbox/scheduler leases, Playwright/axe/Lighthouse gates and OpenAPI backward-compatibility diff.

## TDD execution order

1. Write backend status-transition tests; verify they fail before the domain implementation exists.
2. Add pure transition rules and verify unit tests.
3. Add API handler tests for project creation, run triggering and invalid transitions.
4. Implement PostgreSQL repository plus idempotent schema bootstrap.
5. Write frontend formatting/status tests before dashboard components.
6. Implement dashboard and API client.
7. Add Compose, container health checks, GitHub Actions and operator documentation.
8. Run backend tests, frontend tests/build, container build/up, health/API smoke and browser dashboard check.
