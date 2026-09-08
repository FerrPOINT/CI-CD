# Post-Wave 4 Fixes and Tests Implementation Plan

**Status 2026-09-08:** Active. Gate 0 baseline confirmed; initial audit found stale K1/K4/K5/K6 claims in canonical and narrative documentation. First implementation checkpoint is documentation reconciliation plus an explicit drift guard.

**Goal:** Turn the completed K1-K7 work into a durable, regression-resistant Forge CI/CD baseline: documentation accurately represents shipped behaviour, tests cover the new runner/CLI/observability/backup contracts, and compose/CI prove the operational paths actually used on the Base stand.

**Architecture:** Work in four independent tracks: truth/documentation, backend and runner contracts, dashboard/CLI, and operations. The committed OpenAPI document and SQL migrations remain the contracts of record; a small verification layer detects drift rather than relying on narrative statements. External product scope remains explicitly gated behind the Wave 5 L/M/N/O decisions.

**Tech Stack:** Rust 2024/Axum/SQLx/PostgreSQL 17; React 19/Vite/Vitest/Playwright; Docker Compose; Prometheus/Grafana/Alertmanager; Python 3 backup tooling; GitHub Actions.

**Baseline observed on 2026-09-08:** CI-CD `main` is clean at `73995a6`; GitHub CI succeeds on that SHA; `/api/v1/health`, `/api/v1/readiness`, `/metrics`, Prometheus, Grafana and Alertmanager are live. The live readiness contract reports migration 28. `docs/` still contains outdated assertions that K1/K4/K5/K6 work is target-only; this is the first deterministic fix.

---

## Scope Boundaries

- Do not reinterpret the user-selected Wave 5 modules L/M/N/O until the user decides them.
- Do not create a new repository, new persistence store, or a second compose stack.
- Do not relax existing tests, mutate committed SQL migrations, expose secrets, or replace a real integration test with mocks.
- Keep `sdlc` limited to technical paths/identifiers; user-facing copy and new documentation say Base/Forge.
- Every code fix follows RED -> GREEN -> REFACTOR and adds a focused regression test before implementation.
- A logical checkpoint is one commit. Push only under the established CI-CD plan/work delivery convention after reviewing the staged diff and green verification.

## Gate 0 - Reproducible Baseline and Audit Ledger

### Task 0.1: Record the starting surface

**Objective:** Capture a reproducible baseline before changing behavior or documentation.

**Files:**
- Create: `plans/evidence/2026-09-08-post-wave4-baseline.md`
- Read: `AGENTS.md`, `docs/CURRENT_STATE.md`, `docs/TRACEABILITY.md`, `docs/ROADMAP.md`, `.github/workflows/ci.yml`, `docker-compose.local.yml`

**Steps:**
1. Record `git status --short --branch`, HEAD SHA, latest CI run URL/status, compose service state, readiness JSON and Prometheus target state.
2. Run `python3 scripts/verify_docs.py --all` and record its exact pass result.
3. Run `docker compose -f ../docker-compose.local.yml config --quiet` from `/opt/dev/sdlc`.
4. Do not capture credentials, login cookies, bearer tokens, DSNs or data dumps.

**Verification:** baseline contains only command/status evidence and points to current SHA `73995a6` or the SHA current at execution time.

### Task 0.2: Build an actionable stale-claim inventory

**Objective:** Separate legitimate future target notes from stale claims about shipped K1-K7 functionality.

**Files:**
- Create: `plans/evidence/2026-09-08-capability-claim-inventory.md`
- Read: `README.md`, `docs/{ARCHITECTURE,DELIVERY_ARCHITECTURE,DEVELOPMENT_GUIDE,AUTOMATION_ARCHITECTURE,CURRENT_STATE,ROADMAP}.md`, `docs/adr/0005-*.md`, `docs/adr/0006-*.md`, `docs/adr/0007-*.md`, `docs/adr/0008-*.md`

**Steps:**
1. Search only first-party sources (exclude `target/`, `node_modules/`, generated caches) for `target`, `planned`, `не реализовано`, `остается target`.
2. Classify each hit as `current`, `future`, or `contradiction` using source and test evidence.
3. For contradictions, name the exact current source/test/endpoint proving it.
4. Keep genuine future work such as tenant isolation, external S3, multi-approver policy and approved Wave 5 modules marked honestly as future.

**Verification:** every document update in Gate 1 is traceable to this inventory; no broad search-and-replace of the word `target`.

## Gate 1 - Canonical Documentation and Contract Drift (P0)

### Task 1.1: Correct K1 architecture status

**Objective:** Make documentation describe the completed workspace split and API module split without claiming a fictional completed end-state.

**Files:**
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/CURRENT_STATE.md`
- Modify: `docs/TRACEABILITY.md`
- Test: `scripts/verify_docs.py`

**RED:** Add a documentation-verifier fixture/rule that expects the current architecture page to name `cicd-app`, `cicd-infra`, `cicd-api` and `src/api/`; it must fail against stale `src/api.rs` wording.

**GREEN:** Update the structure/status sections: K1 is complete as a strangler checkpoint; remaining transfers, if any, are listed by actual vertical rather than claiming the whole split is future.

**Verification:**
```bash
python3 scripts/verify_docs.py --all
rg -n 'src/api\.rs|cicd-app|cicd-infra|cicd-api' docs/ARCHITECTURE.md docs/CURRENT_STATE.md
```

### Task 1.2: Correct K4/K5/K6 capability claims

**Objective:** Replace stale “target” statements for shipped runner, CLI and observability capabilities while retaining future boundaries.

**Files:**
- Modify: `README.md`
- Modify: `docs/DELIVERY_ARCHITECTURE.md`
- Modify: `docs/DEVELOPMENT_GUIDE.md`
- Modify: `docs/AUTOMATION_ARCHITECTURE.md`
- Modify: `docs/RUNNER_ARCHITECTURE.md` if present
- Modify: `docs/CLI.md`
- Modify: `docs/runbooks/OBSERVABILITY.md`
- Test: `scripts/verify_docs.py`

**Required current facts:** artifact upload sessions are resumable; log pages support before-cursor windows; project dispatch limits exist; Docker jobs use seccomp/resource classes; CLI has profiles/NDJSON/stable exit codes/completions; `/metrics` has DB state gauges; OTLP is opt-in; observability profile includes Prometheus/Grafana/Alertmanager.

**Required future facts:** object storage, Kubernetes runner isolation, multi-tenant isolation, production TLS ingress, full alert delivery adapters, full log chunk protocol, tenant-aware auth and broader SLO/load/Lighthouse evidence are still future unless separately implemented.

**Verification:** docs verifier passes and the capability inventory has zero `contradiction` rows.

### Task 1.3: Make the documentation verifier detect regression-prone claims

**Objective:** Prevent the exact stale-claim class from silently returning.

**Files:**
- Modify: `scripts/verify_docs.py`
- Modify/Create: relevant Python verifier tests if the script has test coverage
- Modify: `docs/DOCUMENTATION_GOVERNANCE.md`

**RED tests:** require current-only tokens in the canonical pages, and reject known obsolete phrases only in the relevant claim context (not globally).

**GREEN:** Implement narrowly scoped checks keyed to the canonical current-state/traceability model. Avoid making every target roadmap statement illegal.

**Verification:** `python3 scripts/verify_docs.py --all` passes; temporarily substitute one obsolete claim in a temp fixture/copy and show the focused verifier fails.

**Commit:** `docs: reconcile post-wave4 capability status and drift guard`.

## Gate 2 - Runner and API Regression Contracts (P0)

### Task 2.1: Artifact-session restart and authorization matrix

**Objective:** Prove resumable artifact sessions remain safe across server lifecycle and are unavailable to the wrong runner/lease.

**Files:**
- Modify: `backend/tests/integration_db.rs`
- Read: `backend/src/runner_protocol.rs`, `backend/app/src/lib.rs`, `backend/migrations/0027_artifact_upload_sessions.sql`

**RED tests:**
1. Begin + append a chunk, rebuild application state on the same PostgreSQL data, re-begin the same `(attempt,path)` session and continue from the exact high-water mark.
2. A different runner credential and a valid credential for a different lease receive denial for begin/chunk/complete/abort.
3. Duplicate chunk retry is idempotent only when offset, checksum and payload match; mismatched replay is rejected without corrupting session state.
4. Abort prevents complete and leaves no downloadable artifact/blob row.

**GREEN:** Make the smallest protocol/storage change only if a red test reveals an actual gap.

**Verification:**
```bash
cargo test --features integration --test integration_db artifact_session -- --test-threads=1
cargo test --features integration --test integration_db runner -- --test-threads=1
```

### Task 2.2: Dispatch cap race, cancellation and lease-expiry release

**Objective:** Prove `projects.max_running_jobs` cannot be bypassed by concurrent polls and releases capacity reliably.

**Files:**
- Modify: `backend/tests/integration_db.rs`
- Read: `backend/src/runner_protocol.rs`, `backend/migrations/0028_project_dispatch_limits.sql`, `backend/src/api/projects_routes.rs`

**RED tests:**
1. Two compatible runners poll concurrently with project cap 1: exactly one lease is claimed.
2. Completing/cancelling the active lease makes the next project job claimable.
3. Expiring/reconciling a lease makes the next job claimable without changing the cap.
4. A cap on project A never delays compatible jobs from project B.
5. `PATCH max_running_jobs` validates null/unlimited and bounds at API, not only UI.

**GREEN:** Preserve the single CTE atomicity; do not add application-side check-then-claim logic.

**Verification:** focused integration group plus complete `integration_db` suite.

### Task 2.3: Docker sandbox integration proof

**Objective:** Test that the actual Docker command uses the selected limits and seccomp profile, not merely unit-built argument strings.

**Files:**
- Modify: `backend/tests/integration_db.rs` or create `backend/tests/runner_docker_smoke.rs` behind an explicit Docker integration feature
- Modify: `.github/workflows/ci.yml` only if a safe Docker-capable CI job is required
- Read: `backend/src/runner.rs`, `deploy/forge-job-seccomp.json`, `docker-compose.local.yml`

**RED test:** create a minimal shell job, execute it with `small` then `large` class in a Docker-enabled local/CI fixture, and inspect the created container’s `HostConfig`: `SecurityOpt` contains the configured profile, memory/swap/pids match the class, `no-new-privileges`, `cap-drop=ALL`, read-only rootfs, exec tmpfs, expected user and expected compose network are present.

**GREEN:** If direct inspect is not safely portable to public CI, keep the full smoke as an opt-in `just test-runner-docker` gate and run it on the Base stand; retain deterministic unit assertions in standard CI.

**Verification:** test executes a harmless image command and cleans container/workspace reliably even on failure.

### Task 2.4: Log pagination property and boundary tests

**Objective:** Lock tail-window semantics introduced in K4.2.

**Files:**
- Modify: `backend/tests/integration_db.rs`
- Read: `backend/src/api/jobs_routes.rs`

**RED tests:** empty result, `before=1`, cursor after final record, filtered no-match, page size at min/max, simultaneous forward/tail windows, and invariant that returned sequences are strictly ascending/no duplicates with correct `total`, `next_after`, `has_more_before`.

**Verification:** focused log test group + full integration suite.

### Task 2.5: Migration upgrade path to v28

**Objective:** Test migration compatibility from the last pre-K4 schema, not only clean-schema install.

**Files:**
- Create: `backend/tests/migration_upgrade.rs` or extend `backend/tests/integration_db.rs`
- Read: `backend/migrations/0026_session_family_reuse.sql`, `0027_artifact_upload_sessions.sql`, `0028_project_dispatch_limits.sql`, `backend/src/api/readiness.rs`

**RED test:** provision a disposable DB at v26 with representative project/job/attempt data, apply v27-v28 through the real migration binary/path, assert old data remains readable, new nullable cap is unlimited, artifact tables exist and readiness reports v28.

**Verification:** migration test runs against PostgreSQL 17 and is serialized; do not edit historical migrations.

**Commit:** `test(runner): cover post-wave4 protocol, fairness and migration edges`.

## Gate 3 - Metrics, Tracing, Alerting and Backup Reliability (P0/P1)

### Task 3.1: Metrics state-gauge correctness and scrape failure behavior

**Objective:** Prevent silently wrong operational decisions from `/metrics`.

**Files:**
- Modify: `backend/src/metrics.rs` tests or create `backend/tests/metrics_integration.rs`
- Read: `backend/src/api/mod.rs`, `backend/src/metrics.rs`

**RED tests:** seed each pipeline/job/runner state and assert exact gauges; verify unknown/terminal statuses are not double-counted; when the DB query fails, endpoint remains valid Prometheus text and exposes a bounded health/error metric rather than stale silent values.

**GREEN:** Add only low-cardinality labels/metrics. Never add project IDs, UUIDs, refs, repository URLs or request IDs as labels.

**Verification:** `/metrics` parsed by `promtool check metrics` (container image if host command unavailable) and focused Rust tests pass.

### Task 3.2: OTLP configuration and end-to-end export smoke

**Objective:** Prove `SDLC_OTLP_ENDPOINT` works when enabled and logging remains operational when it is absent/unreachable.

**Files:**
- Modify: `services-base/crates/sdlc-telemetry/src/lib.rs` tests or its owning test crate
- Modify: `CI-CD/backend/Cargo.toml` only if test feature wiring is needed
- Modify: `docker-compose.local.yml` observability profile only if an optional collector is added
- Modify: `docs/runbooks/OBSERVABILITY.md`

**RED tests:** invalid endpoint falls back without crashing process; enabled endpoint exports a span carrying service name but no secret/token; disabled endpoint initializes exactly once.

**Integration smoke:** use an ephemeral OTEL collector/test receiver, make a health request, assert at least one span was received. Do not run a collector permanently unless it is part of a separately approved production topology.

### Task 3.3: Alertmanager disabled-webhook behavior

**Objective:** Fix the current fallback that points Alertmanager to its own non-existent `/webhook-disabled` endpoint, which can produce retry noise while no external hook is configured.

**Files:**
- Modify: `docker-compose.local.yml`
- Modify: `CI-CD/deploy/observability/alertmanager.yml`
- Modify: `CI-CD/docs/runbooks/OBSERVABILITY.md`
- Test: compose config/smoke script

**RED test:** render compose without `FORGE_ALERT_WEBHOOK_URL`, start Alertmanager, inject a synthetic alert, and assert no outbound webhook retry/error loop occurs. Render with a local test webhook URL and assert both firing and resolved payloads are delivered.

**GREEN:** Generate two valid Alertmanager receiver configurations: no `webhook_configs` when unset; one configured receiver only when a non-empty URL is supplied. Keep secrets out of generated files and logs.

**Verification:** `docker compose ... config`, Alertmanager readiness, Prometheus rules API and a local HTTP receiver test.

### Task 3.4: Backup restore-drill automation and retention safety

**Objective:** Make the successful manual K7 drill repeatable and make retention/offsite failure explicit.

**Files:**
- Modify: `scripts/forge_backup.py`
- Create: `scripts/restore_drill.py` or `scripts/restore-drill.sh`
- Modify: `scripts/verify-backup.sh`
- Modify: `docs/OPERATIONS.md`
- Test: Python unit tests under `scripts/tests/` or project-standard location

**RED tests:**
1. `--retention N` considers only manifest-backed backup directories, does not delete an explicit backup directory and fails clearly on unreadable parent.
2. rsync destination failure returns non-zero and leaves local verified backup intact.
3. dry-run performs no deletion/copy.
4. restore drill creates required runtime role before `pg_restore`, restores to matching PostgreSQL 17, validates manifest checksum/migration head and compares a declared set of row-count checks.

**GREEN:** Implement idempotent isolated postgres-container lifecycle with cleanup traps; no credentials in source or output.

**Verification:** one real isolated Postgres 17 restore drill plus `verify-backup` runs; document expected duration/space.

**Commit:** `fix(ops): make alert delivery and restore drills verifiable`.

## Gate 4 - CLI and Frontend Regression Coverage (P1)

### Task 4.1: CLI profile/error/output contract matrix

**Objective:** Cover the K5 commands at process boundaries, including safe failure behavior.

**Files:**
- Modify: `backend/cli/tests/cli_contract.rs`
- Modify: `backend/cli/tests/cli_real_api.rs`
- Read: `backend/cli/src/main.rs`, `docs/CLI.md`

**RED tests:** missing profile; invalid TOML; profile inheritance is not implied; precedence `flag > env > selected profile`; unknown output value; NDJSON arrays and single object output; timeout/DNS/refused connection -> 3; 401/403 -> 5; 404 -> 4; validation -> 6; errors never include token or profile token value.

**GREEN:** Centralize parsing/classification only if a red test demonstrates inconsistent behavior.

**Verification:** `cargo test -p cicd-cli --test cli_contract` and real API suite with PostgreSQL.

### Task 4.2: Auth lifecycle and protected-route UI tests

**Objective:** Prove K2 is resilient to reload, redirect and authorization failure paths.

**Files:**
- Modify: `frontend/src/api/auth.test.ts`
- Modify: `frontend/src/app/router.test.tsx`
- Modify: `frontend/src/widgets/app-shell.test.tsx`
- Read: `frontend/src/app/auth-provider.tsx`, `frontend/src/app/router.tsx`, `frontend/src/shared/ui/query-state.tsx`

**RED tests:** refresh restore success; expired/invalid refresh becomes anonymous once; protected deep link returns after login; terminal 401 clears state and redirects; a 403 remains a forbidden state rather than logout; logout clears persisted refresh state and cache.

**Verification:** `pnpm test -- --run` and typecheck/build.

### Task 4.3: QueryState and responsive route matrix

**Objective:** Ensure every production route uses consistent loading/empty/error/403 rendering and works at mobile width.

**Files:**
- Modify: `frontend/e2e/critical-flows.spec.ts`
- Modify: `frontend/e2e/accessibility.spec.ts`
- Modify: `frontend/e2e/performance.spec.ts` only for stable budget coverage
- Modify: `docs/screenshots/manifest.md` and screenshot scripts only if screen evidence changes

**Steps:**
1. Generate the route list from `frontend/src/app/router.tsx` or an existing route inventory; make test inventory fail when a production route is omitted.
2. Add seeded scenarios for unauthorized, forbidden, empty and API error responses without changing the actual backend contract.
3. Test keyboard navigation/sidebar escape/focus and 375x812 layouts on the changed or uncovered routes.
4. Include the new operational UI surfaces that expose dispatch cap/runner status if they exist; if not, record them as API/CLI-only deliberately.

**Verification:** `pnpm openapi:check`, `pnpm openapi:compat --base-ref origin/main`, `pnpm test`, `pnpm build`, `pnpm seed:evidence && pnpm e2e`; capture desktop 1920x1080 and mobile 375x812 evidence only for materially changed screens.

### Task 4.4: Full generated-contract and browser recovery smoke

**Objective:** Catch an OpenAPI/client/real-stack mismatch before it reaches CI.

**Files:**
- Modify: `.github/workflows/ci.yml` only if the current jobs do not run the full matrix in one reproducible order
- Read: `frontend/package.json`, `frontend/playwright.config.ts`, `scripts/verify_docs.py`

**Steps:**
1. Confirm OpenAPI dump -> committed YAML -> frontend type generation/check order is deterministic.
2. In the built compose stack run login, project list, pipeline list/detail, jobs log before-window and an error/forbidden flow.
3. Assert browser console contains no unhandled errors and clean temporary evidence data after the run.

**Commit:** `test(ui): lock auth, query-state and post-wave4 contracts`.

## Gate 5 - CI, Security and Operational Policy (P1/P2)

### Task 5.1: Promote deterministic gates and keep expensive ones explicit

**Objective:** Give fast PR feedback while preserving a scheduled/nightly deep verification path.

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create/Modify: `.github/workflows/nightly.yml` only if repository policy permits scheduled CI
- Modify: `docs/DEVELOPMENT_GUIDE.md`

**Plan:**
- Required CI: format, clippy, workspace/unit, migration/integration, CLI real API, OpenAPI drift/compatibility, frontend test/build, compose smoke, focused E2E/accessibility, docs and security scans.
- Scheduled/explicit: Docker sandbox inspect, full restore drill, OTLP receiver, broad browser matrix, retained backup verification.
- Pin image/action versions and timeouts; preserve concurrency cancellation only for superseded runs, and record final SHA success as the merge evidence.

**Verification:** use a workflow dry-run/static YAML validation where possible; make a real CI run on a harmless documentation/test fixture change before claiming policy is active.

### Task 5.2: Security backlog triage with explicit acceptance rules

**Objective:** Convert vague hardening targets into bounded, testable work instead of claiming production security prematurely.

**Files:**
- Create: `docs/SECURITY_BACKLOG.md` or update the canonical security/roadmap document
- Modify: `docs/ROADMAP.md`
- Read: `docs/AUTHORIZATION.md`, `docs/THREAT_MODEL.md`, `.github/workflows/ci.yml`, `docker-compose.local.yml`

**Required tickets/groups:**
1. TLS/reverse proxy and secure-cookie/CORS deployment profile (Wave 5 O is user-gated).
2. Network boundary for `/metrics`, readiness and databases.
3. Secret-history/container scan and dependency policy (`cargo-deny` only after a reviewed deny policy exists).
4. Owner/runtime PostgreSQL migration-role matrix.
5. Tenant/service-account/scoped Git credential design.
6. Object storage and artifact legal-hold retention policy.

**Verification:** each group has threat, owner, non-goal, acceptance test and deployment rollback/restore impact. Do not enable a broad scanner with undefined severity/false-positive policy.

## Gate 6 - Wave 5 Decision and Delivery Preparation (User-Gated)

### Task 6.1: Refresh L/M/N/O choices from current repository facts

**Objective:** Re-ask decisions only after the deterministic fixes above have a green baseline.

**Files:**
- Modify: `docs/ROADMAP.md` after a response
- Modify: this plan or create a dedicated accepted-scope plan

**Decision inputs:**
- L Wiki Phase 3: comments/mentions, exports, object storage, Markdown import/export or page approvals.
- M Task-tracker: SSO/TOTP, email-to-issue, CSV, dashboard widgets, readonly sharing or PWA.
- N Base integration: project-workflow only, both project-workflow/java-agent, or defer.
- O TLS: Caddy + internal CA (recommended), Traefik/SAN or nginx+mkcert, or defer.

**Rule:** No product implementation starts until user selects scope. Each chosen module gets its own exact TDD plan, compose impacts, browser evidence and migration strategy.

## Final Release Gate

Run in this order after every affected checkpoint:

```bash
# Docs and compose
cd /opt/dev/sdlc/CI-CD
python3 scripts/verify_docs.py --all
cd /opt/dev/sdlc
docker compose -f docker-compose.local.yml config --quiet

# Backend in pinned image/toolchain (actual command derived from DEVELOPMENT_GUIDE)
cd /opt/dev/sdlc/CI-CD/backend
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --features integration --test integration_db -- --test-threads=1
cargo test -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
cargo build --release --workspace

# Frontend
cd /opt/dev/sdlc/CI-CD/frontend
pnpm openapi:check
pnpm openapi:compat --base-ref origin/main
pnpm test -- --run
pnpm build
pnpm seed:evidence && pnpm e2e

# Live Base stand
curl -fsS http://127.0.0.1:7711/api/v1/health
curl -fsS http://127.0.0.1:7711/api/v1/readiness
curl -fsS http://127.0.0.1:7711/metrics
curl -fsS http://127.0.0.1:7790/-/ready
curl -fsS http://127.0.0.1:7791/api/health
curl -fsS http://127.0.0.1:7792/-/ready
```

**Acceptance criteria:** no unexpected dirty files; all test gates green; clean compose config; live readiness shows no pending/checksum-mismatched migrations; Prometheus target `forge-cicd` is up; Alertmanager has the expected rules and no disabled-webhook retry loop; any UI change has browser and screenshot evidence; docs reference only current, verified claims. Commit evidence and CI result are linked from the relevant roadmap/current-state entry.
