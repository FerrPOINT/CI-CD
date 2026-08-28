# Git Server and CI Parity Implementation Plan

> **For Hermes:** Execute with TDD: add a focused regression test, observe RED, implement the smallest change, run GREEN, then verify the real API/UI.

**Goal:** Close the high-value Git-server and CI/CD gaps required for a serious self-hosted Forge control plane.

**Architecture:** Keep Git reads shelling out only against validated bare repositories. Store release metadata, visibility, pipeline variables and parsed JUnit summaries in PostgreSQL migration `0006`; expose typed Axum endpoints and React-query hooks. Keep clone authorization at Smart HTTP boundaries, and pass user variables to runner jobs only under a `CICD_VAR_` namespace.

**Tech Stack:** Rust 2024, Axum, SQLx/PostgreSQL, Git Smart HTTP, React 19, TanStack Query, Vite, Playwright evidence.

---

### Task 1: Code browsing API and dashboard

**Files:** `backend/src/pulls.rs`, `backend/src/api.rs`, `frontend/src/pages/repository-browser/index.tsx`, `frontend/src/api/{hooks,types}.ts`

1. Exercise absent `GET /repos/{repo}/tree` and `/blob`; expected 404 before implementation.
2. Add validated `git ls-tree` and `git show` handlers, with a safe HEAD -> main/master fallback for bare repos without HEAD.
3. Add typed hooks and the Code tab with directory navigation, text preview, binary/large-file guards.
4. Verify by browsing repository root, subdirectory and a file with curl and dashboard.

### Task 2: Releases, tags and visibility

**Files:** `backend/migrations/0006_git_ci_details.sql`, `backend/src/{api,git_host,pulls}.rs`, repository-browser UI/hook/type files

1. Add a migration for `repositories.visibility` and release records.
2. Add tags read API plus idempotent release CRUD; render Tags and Releases, including release create/delete confirmation.
3. Enforce public read/private token Smart HTTP behavior, always requiring a token for pushes when configured.
4. Live verify public clone 200 without token, private 401, private 200 with correct token, plus releases/tags APIs.

### Task 3: Pipeline variables, badge and JUnit reports

**Files:** `backend/src/{api,runner}.rs`, `frontend/src/pages/pipeline-detail/index.tsx`, API hooks/types

1. Add `pipelines.variables` JSONB, accepting variables on manual trigger and exposing read-only inspection endpoint.
2. Pass allowed values into jobs as `CICD_VAR_<UPPER_SNAKE_KEY>`; live run a job printing two variables.
3. Add a non-sensitive SVG status badge endpoint.
4. Add JUnit summary ingestion, storage and display; test parser edge cases before code and verify a real XML upload (names, counts, decimal duration).

### Task 4: Regression gates, docs, visual evidence and delivery

**Files:** `docs/{API,DATA_MODEL,CURRENT_STATE}.md`, `CHANGELOG.md`, `openapi/openapi.yaml`, `frontend/src/api/schema.d.ts`, `docs/assets/screens/*`

1. Regenerate OpenAPI/types and test endpoint contract drift.
2. Run format, clippy, unit, integration, frontend tests/build and `verify_docs.py --all`.
3. Seed and shoot desktop/mobile evidence for Code, file preview, Tags, Releases dialog/confirmation, and JUnit report.
4. Review git diff excluding `plans/`, commit one logical unit and push `origin/main`.
