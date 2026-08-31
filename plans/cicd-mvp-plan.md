# CI/CD MVP Implementation Plan

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

## Deliberately deferred

- Authentication/RBAC, webhooks, external runner protocol, secret storage, artifact uploads, remote execution, YAML pipeline parser, parallel worker scheduling.

## TDD execution order

1. Write backend status-transition tests; verify they fail before the domain implementation exists.
2. Add pure transition rules and verify unit tests.
3. Add API handler tests for project creation, run triggering and invalid transitions.
4. Implement PostgreSQL repository plus idempotent schema bootstrap.
5. Write frontend formatting/status tests before dashboard components.
6. Implement dashboard and API client.
7. Add Compose, container health checks, GitHub Actions and operator documentation.
8. Run backend tests, frontend tests/build, container build/up, health/API smoke and browser dashboard check.
