# Base QA Remediation Implementation Plan

> **For Hermes:** Execute this plan sequentially with TDD and real runtime evidence. Do not delete a runner workspace unless its job is terminal and the path is an exact `forge-runner-<job-id>` child of the configured volume.

**Goal:** Close all production-relevant defects discovered by the Base QA sweep, restore truthful observability, prevent runner-disk exhaustion, and make stand deployment and test gates reproducible.

**Architecture:** Forge fixes stay in the CI-CD control plane: metrics use the terminal job timestamp, while the existing runner cleanup path handles future workspaces after the stand disables debug retention. Java runtime status is derived from Spring Actuator readiness, with Compose healthchecks evaluating the local management port. Dependency and test-runner fixes remain isolated to their owning repositories.

**Tech stack:** Rust/SQLx/PostgreSQL, Spring Boot/Gradle, Docker Compose, pnpm, Playwright, Bash, GitHub Actions.

---

### P1: Repair Forge terminal-job metrics

**Objective:** Make `forge_jobs_failed_24h` and `forge_jobs_succeeded_24h` query the real `jobs.finished_at` terminal timestamp rather than absent `updated_at`.

**Files:**
- Modify: `CI-CD/backend/src/metrics.rs`
- Modify: `CI-CD/backend/tests/integration_db.rs`

**Steps:**
1. Add a database-backed regression test that creates failed/success jobs with recent `finished_at`, calls `refresh_state_gauges`, and asserts the two exported counters are non-zero.
2. Run the new test and confirm it fails on `updated_at`.
3. Change only the two metrics subqueries to filter on `finished_at`.
4. Run the focused test, `cargo fmt --check`, `cargo clippy -- -D warnings`, and the serial real-PostgreSQL integration suite.
5. Commit and push after fetching/rebasing origin.

### P2: Stop Forge workspace growth and clean proven stale workspaces

**Objective:** Preserve automatic cleanup for future jobs and remove only existing workspaces mapped to terminal jobs.

**Files:**
- Modify: `docker-compose.local.yml`
- Modify: `CI-CD/docs/OPERATIONS.md`
- Modify: `CI-CD/docs/ENV.md`
- Modify: `CI-CD/docs/plans/2026-09-16-qa-remediation.md`

**Steps:**
1. Confirm runner code removes a per-job directory whenever `CICD_RUNNER_KEEP_WORKSPACE=false` and that the default config is false.
2. Change the umbrella setting from `true` to `false`; document that retention is a short-lived debugging override, not a stand default.
3. Query Forge DB statuses and enumerate only exact volume children whose UUID matches a terminal job (`success`, `failed`, `canceled`). Refuse all non-terminal/unknown/malformed paths.
4. Record candidate count and reclaimed bytes before deletion; delete only that explicit set. Do not use Docker volume prune or broad glob deletion.
5. Verify all current pipelines/jobs stay terminal, health/readiness remains green, and free disk increased.

### P3: Synchronize Java runtime and make service health truthful

**Objective:** Run the current Java Agent source and expose Compose health based on its local Actuator readiness endpoint.

**Files:**
- Modify: `docker-compose.local.yml`
- Modify if needed after source inspection: `java-agent/backend/src/main/resources/application.yml`
- Test: existing Java Agent integration/unit suites and Compose health check

**Steps:**
1. Fetch/rebase `java-agent` on `origin/main`; preserve remote changes.
2. Inspect management port binding and Actuator details. Treat `model=DOWN` as a real readiness signal unless the code explicitly defines model dependency as optional.
3. Add failing configuration/unit coverage if a source behavior change is necessary. Do not mask a DOWN model by replacing readiness with liveness.
4. Add `ja-agent` healthcheck against `127.0.0.1:9969/actuator/health/readiness`; add a bot healthcheck against its local health endpoint and change dependency to `service_healthy` when its endpoint is confirmed.
5. Run Gradle tests, Compose config validation, rebuild/recreate both Java services, verify current JAR contains the TTL transaction implementation, and ensure no repeated TTL failures remain.
6. Commit/push all source-repo changes; umbrella Compose is committed with CI-CD remediation documentation if that repository owns it.

### P4: Resolve the React Router advisory in Wiki and Fleet Control

**Objective:** Upgrade the affected production dependency to a patched React Router release without enabling unneeded RSC/server routing.

**Files:**
- Modify: `wiki/frontend/package.json`, `wiki/frontend/pnpm-lock.yaml`
- Modify: `fleet-control/frontend/package.json`, `fleet-control/frontend/pnpm-lock.yaml`
- Test: frontend unit/type/lint/build and production dependency audits

**Steps:**
1. Confirm the advisory path is still in each production dependency graph and RSC entrypoints are absent.
2. Update only the React Router dependency set to a compatible patched version.
3. Run targeted router tests, full frontend quality gate, and `pnpm audit --prod`.
4. Rebuild/recreate changed frontend services and execute Chromium smoke tests.
5. Commit/push and verify hosted CI for both heads.

### P5: Make Task Tracker real-DB and browser gates runnable

**Objective:** Ensure documented test commands run from a fresh supported environment and do not fail from file mode or missing approved build tooling.

**Files:**
- Modify: `task-tracker/backend/scripts/run-e2e-tests.sh` mode (`100755`)
- Modify if needed: `task-tracker/backend/Dockerfile` or CI workflow/toolchain documentation
- Modify if needed: `task-tracker/frontend/pnpm-workspace.yaml` or CI instructions
- Test: `backend/scripts/run-e2e-tests.sh` in supported Rust image; Chromium E2E

**Steps:**
1. Add the executable bit and a preflight error that explains a missing `cargo llvm-cov` rather than failing at a later command.
2. Run the script in the documented supported Rust image, including ignored Postgres tests and coverage gate.
3. Inspect pnpm’s approved build dependency policy; encode the minimal project-local `esbuild` approval compatible with frozen install rather than bypassing scripts globally.
4. Run frontend install, unit/lint/build, and Chromium smoke tests.
5. Commit/push, wait for hosted CI, and verify no test containers remain.

### P6: Final release verification

**Objective:** Verify real deployed behavior, source/head parity, disk recovery, observability, authorization, and CI.

**Steps:**
1. Confirm all repositories are clean and synchronized after each push.
2. Check all Compose services, explicit Java health, Prometheus five targets, Alertmanager firing alerts, and relevant logs.
3. Confirm Forge metrics show actual 24-hour terminal counts after a known terminal job history exists.
4. Confirm the runner volume is bounded and sufficient host disk/swap headroom remains.
5. Run final Chromium desktop/mobile smoke where the test suites provide it, retain failure artifacts only if a genuine application defect remains.
6. Publish an evidence-backed completion report with commits, CI results, runtime values, and any explicitly accepted residual risk.
