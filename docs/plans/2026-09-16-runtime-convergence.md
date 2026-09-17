# Base Runtime Convergence Implementation Plan

> **For Hermes:** Execute sequentially. Preserve user data and secrets. Do not merge Dependabot PRs without explicit authorization.

**Goal:** Bring the Base stand runtime to the current, tested application revisions and close the remaining evidence gaps without weakening security or creating business data.

**Architecture:** First establish source-to-image provenance for every umbrella service. Rebuild and recreate only services whose deployed image does not contain the checked-out, CI-green source revision; then prove readiness, telemetry, and log behavior. Dependency PRs are audited and classified only, never merged automatically.

**Tech Stack:** Docker Compose, Java/Gradle, Rust/Cargo, React/pnpm, GitHub Actions, Prometheus, Alertmanager.

---

### R1: Capture a deployment provenance baseline

**Objective:** Map each umbrella application revision to its running image and health state.

**Files:**
- Evidence: `docker-compose.local.yml`, image/container inspect metadata, repository `HEAD`

**Steps:**
1. Record clean git state and `origin` head for all eight application repositories.
2. Inspect current Compose image/container creation times and embedded revision labels when available.
3. Probe documented health/readiness endpoints, Prometheus `up`, firing alerts, disk, and runner-workspace capacity.
4. Classify each mismatch as deploy-required or source-only (for example, frontend-only documentation changes do not require a running rebuild).

**Verification:** No source changes; an evidence table names every deploy decision.

### R2: Deploy the current Java Agent source safely

**Objective:** Eliminate the confirmed Java Agent source/runtime drift while preserving PostgreSQL data and the Chromium volume.

**Files:**
- Runtime: `docker-compose.local.yml`
- Sources: `java-agent/backend/**`, `java-agent/telegram-bot/**`

**Steps:**
1. Confirm `java-agent` is clean and the latest `origin/main` CI succeeded.
2. Run the Gradle module suite against that exact revision.
3. Rebuild/recreate only `ja-agent` and `ja-telegram-bot`; do not run `down -v` and do not alter `.env`.
4. Verify Compose health, internal management readiness on `9969`, external bot health, Prometheus scrape, and absence of fresh attachment-TTL errors after one scheduler window.

**Verification:** Both containers are healthy and the deployed image has a creation time later than the source revision checkout.

### R3: Verify the Forge operational correction end-to-end

**Objective:** Prove the `finished_at` metric correction and default workspace cleanup are active in the live Forge runtime.

**Files:**
- `CI-CD/backend/src/metrics.rs`
- `docker-compose.local.yml`

**Steps:**
1. Check `/metrics` for valid Forge job state gauges and backend logs for SQL errors.
2. Confirm `CICD_RUNNER_KEEP_WORKSPACE=false` in the rendered Compose config.
3. Confirm only reserved workspace directories remain and no active runner job/container would be affected.
4. Do not manufacture business pipelines merely to create a metric sample.

**Verification:** Metrics endpoint succeeds without the former `updated_at` query error and runner workspace usage remains bounded.

### R4: Audit unmerged dependency PRs without merging

**Objective:** Produce an evidence-based keep/update/close recommendation for each open Dependabot PR.

**Files:**
- `services-base` PRs #21-#24
- `java-agent` PRs #31, #33-#39
- `admin-panel` PRs #1-#3

**Steps:**
1. Read diff, metadata, and check state for every PR.
2. Separate safe patch upgrades from major/toolchain upgrades and from PRs whose checks were intentionally skipped.
3. Check whether an equivalent dependency/version is already in `main`.
4. Leave every PR open and unmodified; publish only a recommendation table in the final report.

**Verification:** No merge, close, or dependency mutation occurs during the audit.

### R5: Release evidence and hygiene closeout

**Objective:** Confirm the converged stand is healthy and the repositories remain publishable.

**Steps:**
1. Re-run direct health/readiness probes for Forge, Task Tracker, Wiki, Fleet Control, Project Workflow, Java Agent, and Java Telegram bot.
2. Recheck Prometheus targets, firing alerts, capacity, Compose status, and service error logs.
3. Recheck clean/synced git state and hosted CI for every changed revision.
4. Commit/push this plan and any code/doc changes made by the execution, then report evidence and residual user-gated work.

**Verification:** All required services are live, monitoring is clean, no unmanaged test resources remain, and every logical change is in `origin/main`.
