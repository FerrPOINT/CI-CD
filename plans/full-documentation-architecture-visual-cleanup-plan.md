# Полное приведение в порядок документации, архитектуры и визуальной части

> **For Hermes:** выполнять последовательно, не смешивать current facts, approved target contracts и планы. Каждый блок завершать проверяемым evidence и отдельным commit.

**Goal:** сделать CI-CD понятным для пользователя, разработчика и оператора: один источник истины на каждую тему, честно обозначенный MVP/target status, воспроизводимые visual evidence и готовая к старту разработки архитектурная база.

**Architecture:** сначала устранить противоречия и установить ownership документации; затем собрать компактную hierarchy с contracts/current-state/operations/quality; только после этого привести UI и screenshots к реальному продукту. Target packages и функции не имитировать документацией.

**Scope:** документация, документационные contracts, onboarding/repository hygiene, UI accessibility/responsive fixes и screenshots/evidence pipeline. Не включает реализацию Auth/runner/outbox/OpenAPI, кроме foundation artifacts, строго необходимых как evidence.

---

## Gate 0 — Freeze Authority And Status Taxonomy

**Objective:** исключить противоречивые source-of-truth до любой консолидации.

**Files:**
- Create: `docs/CURRENT_STATE.md`
- Create: `docs/DOCUMENTATION_GOVERNANCE.md`
- Modify: `docs/ARCHITECTURE_INDEX.md`, `docs/IMPLEMENTATION_CONTRACTS.md`, `docs/ADR.md`, `README.md`
- Test: `scripts/verify_docs.py` (create)

**Actions:**
1. Зафиксировать четыре статуса в начале README/index: `Current verified`, `Configuration only`, `Target approved`, `Deprecated/historical`.
2. Ввести authority matrix:
   - code + committed migrations = current runtime;
   - ADR = architectural decision only;
   - `docs/contracts/*` = normative target observable contract;
   - architecture docs = explanatory narrative;
   - `plans/*` = committed working plans; non-normative schedule.
3. Сверить и принять canonical names/paths с отдельным superseding ADR: migration directory, OpenAPI output, outbox tables, runner namespace, Git ingress path, tenant/workspace/organization vocabulary.
4. Исправить действующие conflicts, а не оставлять competing choices:
   - `docs/adr/0006-postgresql-outbox.md` vs `docs/IMPLEMENTATION_CONTRACTS.md` (`outbox_events` vs `domain_events/outbox_messages`);
   - `docs/MIGRATIONS.md`, `docs/adr/0008-versioned-sqlx-migrations.md`, `docs/STORAGE_ARCHITECTURE.md`, `docs/IMPLEMENTATION_CONTRACTS.md` (migration path);
   - `docs/DELIVERY_ARCHITECTURE.md` vs `docs/IMPLEMENTATION_CONTRACTS.md` (error envelope/request_id/codes);
   - `docs/RUNNER_ARCHITECTURE.md`, `docs/AUTHORIZATION.md`, `docs/EXECUTION_AUTOMATION_IMPLEMENTATION_SPEC.md` (runner API base path);
   - `plans/architecture-rebuild-plan.md` vs `docs/ADR.md` (ADR numbers).
5. `CURRENT_STATE.md` получать из machine-generated route/schema/package inventory: commit, verified commands, current tables/routes/screens, dev-only risks, known limits.

**Verification:**
```bash
python3 scripts/verify_docs.py --authority --current-state
rg -n 'outbox_events|pipeline_runs|job_runs|backend/migration/migrations|openapi/openapi.json' docs README.md
```
Expected: every remaining occurrence is explicitly historical/deprecated or absent; no competing normative decision.

**Commit:** `docs: establish documentation authority and current-state taxonomy`

## Gate 1 — Normalize Architecture Into Contracts And Narratives

**Objective:** convert 75 flat Markdown files into a navigable, non-duplicative architecture system.

**Files:**
- Create: `docs/contracts/{API_CONTRACT,AUTHZ_CONTRACT,RUNNER_PROTOCOL,EVENT_CONTRACT,PIPELINE_DSL,DATA_LIFECYCLE,MIGRATION_CONTRACT,UI_API_CONTRACT}.md`
- Create: `docs/architecture/{contexts,runtime-topology,backend-boundaries,frontend-boundaries,transition-map}.md`
- Create: `docs/architecture/sequences/{pipeline-trigger,runner-lease,git-ingress,webhook-delivery,auth-session,migration-deploy}.md`
- Modify: `docs/ARCHITECTURE.md`, `docs/FUNCTIONAL_ARCHITECTURE.md`, `docs/ARCHITECTURE_INDEX.md`, `docs/DATA_MODEL.md`, `docs/ROADMAP.md`
- Deprecate/delete after redirects: duplicate files listed below

**Actions:**
1. Keep `ARCHITECTURE.md` as concise C4/container/runtime map (under ~250 lines), not a second target specification.
2. Move detailed observable target rules from the five 700–1200 line target docs into contracts. Narrative architecture docs link to contracts instead of restating schema/API/state tables.
3. Add package ownership matrix for actual/target crates: responsibility, public ports, allowed/forbidden dependencies, binary owner, test owner, strangler adapter and removal gate. Do not create empty crates merely to match docs.
4. Add six sequence flows with current vs target swimlanes and evidence links: Git push, pipeline trigger, runner lease, auth session, webhook delivery, migration deployment.
5. Add `transition-map.md`: each legacy endpoint/table/module → target owner, adapter, metrics, feature flag, deprecation deadline and deletion evidence.
6. Reconcile ADR registry through supersession; never silently renumber accepted ADRs.

**Verification:**
- Every target capability has exactly one contract owner, persistence owner, API/event contract, threat boundary, state machine and acceptance suite.
- Every target diagram is labelled `Target — not implemented`.
- `scripts/verify_docs.py --contracts --adr-registry` has no duplicate authority/naming/path error.

**Commit:** `docs: normalize architecture contracts and transition map`

## Gate 2 — Rebuild Documentation By Audience

**Objective:** turn flat technical prose into usable entry paths.

**Files:**
- Create: `docs/USER_GUIDE.md`, `docs/DEVELOPMENT_GUIDE.md`, `docs/OPERATIONS.md`, `docs/PRODUCT_REQUIREMENTS.md`
- Modify: `README.md`, `docs/ARCHITECTURE_INDEX.md`, `AGENTS.md`
- Merge/deprecate source docs according to map below

**Actions:**
1. Rewrite README order: purpose → current maturity/trust boundary → 5-minute local start → core capabilities/status → curated product tour → links by audience → contribution/security/license.
2. Create audience links: User/project owner, Developer, Operator, Security reviewer, Architecture implementer.
3. Create concise `USER_GUIDE.md`: create project/repository, push/trigger pipeline, inspect logs/artifacts, manage environments/schedules/webhooks/notifications, roles/tokens limitations; every procedure marks actual or configuration-only.
4. Create `DEVELOPMENT_GUIDE.md`: local setup, test matrix, CI, code style/review, dependencies, codegen/migration prerequisites.
5. Create `OPERATIONS.md`: trusted local deployment, production prerequisites, upgrade strategy, backup/restore, monitoring, incident runbooks. Separate current local commands from target production procedures.
6. Rewrite `TZ.md` as `PRODUCT_REQUIREMENTS.md`: goals/personas/non-goals/capability acceptance, no duplicate endpoint/schema details.
7. Remove flat long README documentation list; link the audience guides and architecture index.

**Consolidation map:**

| Existing documents | Canonical destination |
|---|---|
| `DOMAIN_MODEL.md`, `WORKFLOW.md` | `FUNCTIONAL_ARCHITECTURE.md` + `RUNNER_PROTOCOL.md` |
| `MIGRATIONS.md`, `DATABASE_STANDARDS.md`, `DATABASE_INDEXES.md` | `DATA_LIFECYCLE.md` + `MIGRATION_CONTRACT.md` |
| `ARTIFACTS.md`, `SECRETS_MGMT.md`, `STORAGE.md`, `BACKUP_RESTORE.md` | `DATA_LIFECYCLE.md` + `OPERATIONS.md` |
| `EVENTS.md`, `WEBHOOKS.md`, `NOTIFICATIONS.md`, cache sections | `EVENT_CONTRACT.md` + `AUTOMATION_ARCHITECTURE.md` |
| `API_STANDARDS.md`, `API_VERSIONING.md`, `API_EDGE_CASES.md`, `ERROR_HANDLING.md`, `PAGINATION.md` | `API_CONTRACT.md` |
| `FRONTEND_ARCHITECTURE.md`, `FRONTEND_STANDARDS.md`, `I18N.md`, `LOGGING_STANDARDS.md` | `frontend-boundaries.md` + `UI_API_CONTRACT.md` + `DEVELOPMENT_GUIDE.md` |
| `PULL_REQUESTS.md` | `GIT_HOSTING.md` |
| `UI_UX.md`, `ROUTING.md`, `PROJECT_ADMIN.md`, `SYSTEM_ADMIN.md`, `CLI.md`, product portions of `REPORTS.md` | `USER_GUIDE.md` |
| `LOCAL_SETUP.md`, `TESTING.md`, `CI_CD.md`, `CODE_STYLE.md`, `CODE_REVIEW.md`, `REVIEW.md`, `LIBRARIES.md` | `DEVELOPMENT_GUIDE.md` |
| `DEPLOYMENT.md`, `RUNTIME.md`, `OPS_RUNBOOK.md`, `RELEASE.md`, `MONITORING.md`, `PERFORMANCE.md`, `RESILIENCE.md` | `OPERATIONS.md` |
| `SECURITY.md` | split into AUTHZ/DATA_LIFECYCLE/API_CONTRACT/OPERATIONS; retain a short `SECURITY.md` policy/disclosure pointer |
| `GLOSSARY.md`, `TECH_CHOICES.md`, `docs/AGENTS.md` | index canonical terminology + ADRs/development guide + root `AGENTS.md` only |

8. Publish one-line redirect stubs for one release cycle; delete only after inbound link report is zero and migration notice enters changelog.

**Verification:**
```bash
python3 scripts/verify_docs.py --links --orphan-docs --status-labels
```
Expected: no broken links, no orphan canonical doc, no stale “planned” claim about an implemented MVP resource.

**Commit:** `docs: reorganize guides by audience and retire duplicate references`

## Gate 3 — Repair Public Repository And Trust Surface

**Objective:** make the repo safe to consume and contribute to without overstating maturity.

**Files:**
- Create: `LICENSE`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `SUPPORT.md`
- Create: `.github/ISSUE_TEMPLATE/{bug_report,feature_request}.md`, `.github/PULL_REQUEST_TEMPLATE.md`, `.github/dependabot.yml`
- Modify: `README.md`, `docs/OPERATIONS.md`, `.github/workflows/ci.yml`

**Actions:**
1. Add actual FerrPOINT proprietary source-available license matching Cargo metadata and README.
2. Add `CHANGELOG.md` with `Unreleased` and factual `0.1.0` baseline; do not claim a release tag before tag exists.
3. Add public contributor and vulnerability-disclosure policy (supported versions, private reporting path, response expectations).
4. Add unavoidable local/trusted-network-only warning to README/quick start until auth/RBAC, TLS, CORS restriction and non-public DB are implemented.
5. Separate local Compose from production deployment. Bind local PostgreSQL to `127.0.0.1` or make it profile/override-only; production uses versioned/digest-pinned images, not `git pull main`.
6. Either implement/test backup/restore scripts claimed by docs or remove path claims and document manual procedure.
7. Replace macOS-only `open` command in deployment documentation.
8. Add CI/release policy honestly: current gates vs target gates. Add badges only after working stable workflow/tag release exists.

**Verification:**
- Fresh clone local onboarding works as written on Linux.
- Security warning appears before any quick-start command.
- Every script claimed by docs exists and is smoke tested.
- `git tag`/version/changelog release story agrees.

**Commit:** `docs: add public repository policies and honest deployment guidance`

## Gate 4 — Make UI Truthful, Responsive And Accessible

**Objective:** fix the actual interface before using screenshots as evidence.

**Files:**
- Modify: `frontend/src/widgets/app-shell.tsx`
- Modify: `frontend/src/pages/{pipeline-detail,runners,users,audit-log,artifacts,secrets,schedules,environments,projects}/index.tsx`
- Modify: `frontend/src/shared/i18n/locales/{ru,en}.json`
- Create: `frontend/src/shared/ui/{data-state,responsive-data-list}.tsx` (or existing project-equivalent)
- Create: `frontend/e2e/*`, `frontend/playwright.config.ts`, `frontend/lighthouserc.json`
- Modify: `docs/USER_GUIDE.md`, `docs/quality/TEST_STRATEGY.md` (or canonical development guide)

**Actions:**
1. Replace current mobile sidebar behaviour with an accessible overlay drawer: focus trap, Escape/click-away close, `aria-label`, 44px controls, focus-visible.
2. Introduce a mobile data-list/card policy for runners/users/tokens/audit/artifacts/secrets/schedules/environments. Do not squeeze four-column desktop tables into 375px. Desktop tables may use horizontally scrollable local containers only with an explicit affordance.
3. Fix page title/CTA wrapping, especially `Runner-ы` and register action; use locale-consistent Russian UI copy (`online` etc.) except machine identifiers.
4. Replace `window.confirm` in projects/runners/users/tokens with accessible AlertDialog.
5. Build reusable loading/empty/error/retry states. Static settings/admin and fake login must be visibly labelled as information/placeholder until implemented.
6. Repair pipeline detail: pipeline status/progress, ref/commit/trigger/timestamp/duration/runner, visible success/failure story, log panel/error diagnostic and retry/cancel states.
7. Add missing PR flow artifacts: dedicated pull-request detail page (conversation/meta/actions with merge/close/reopen moved there), "View diff" link from every PR card to `/repositories/{repo}/compare?from=<target>&to=<source>`, pipeline status chip per PR once pipeline linkage exists, and merge-conflict detection feedback in merge action. Keep PR comments/review UI out of scope (product non-goal).
8. Add pipeline re-run affordances: Retry button on failed pipelines/jobs in pipeline detail (endpoint `POST /pipelines/{id}/retry`, `POST /jobs/{id}/retry` already exist).
9. Rework reports around useful data: range/filter, trend, duration/failure breakdown; if target-only, mark it instead of showing weak cards as production analytics.
10. Set and test color contrast, keyboard flow, 200% zoom/reflow, reduced motion, semantic status text; add axe and Lighthouse CI gates.
11. Remove old UI_UX prose referring to nonexistent `dashboard.tsx`/`styles.css`; derive UI guide from route/component inventory.

**Verification:**
```bash
cd frontend
pnpm test
pnpm build
pnpm playwright test
pnpm lighthouse
```
- Axe has no critical/serious violations on dashboard, pipeline detail, runner list, dialog and mobile drawer.
- 320/375/768/1440/1920 viewports have no page-level horizontal overflow.
- Keyboard-only navigation/drawer/dialog test passes.

**Commit:** `feat(ui): make operational screens responsive and accessible`

## Gate 5 — Rebuild Screenshot And Visual Evidence System

**Objective:** replace a 25-image gallery with reproducible, meaningful product evidence.

**Files:**
- Create: `docs/assets/screens/manifest.md`
- Create: `frontend/e2e/screenshots.spec.ts`, `frontend/scripts/seed-evidence.*`
- Modify: `README.md`, `USER_GUIDE.md`, `DEVELOPMENT_GUIDE.md`
- Replace: `docs/screenshots/*.png`

**Actions:**
1. Create deterministic seed scenario: named repositories, commits/authors, queued/running/success/failed/canceled pipelines, failed log, retry, artifact, deployment, runner states, audit event, safely redacted secret, configuration-only labels.
2. Capture one declared desktop viewport `1920x1080`, mobile `375x812`, optional 2K `2560x1440`; use a fixed locale/theme/DPR and record commit/build/date.
3. Capture only evidence screens: dashboard with populated KPIs/recent activity; pipeline list; failed pipeline with diagnostics/retry; repository browser; PR list AND PR detail with diff link; compare view with populated file diff/statistics; artifact/deployment; users/audit; mobile dashboard/pipeline/table card/drawer.
4. Place only 4–6 curated desktop screenshots and 2–3 mobile screenshots in README. Move full evidence set to `docs/assets/screens/manifest.md`.
5. Manifest columns: file, route, viewport/DPR, locale/theme, role, seed scenario, user-visible state, capability status, capture command, commit/date and alt text.
6. Screenshot alt text must describe action/state, not just page name.
7. Remove screenshots of static placeholder pages or label them clearly as target/placeholder; no “mobile works” statement while runner cards/table fail responsive checks.

**Verification:**
```bash
cd frontend
pnpm screenshots:update
python3 scripts/verify_docs.py --screenshots --manifest
```
- Dimensions match manifest; all hashes unique; every route evidence maps to current UI state; no local secret/internal URLs or unredacted token appears.
- Review at native desktop and mobile dimensions, not only README thumbnail.

**Commit:** `docs: add reproducible product evidence and curated screenshot tour`

## Gate 6 — Final Consistency And Release Evidence

**Objective:** prove the cleaned package remains true and maintainable.

**Files:**
- Create/update: `docs/quality/RELEASE_EVIDENCE.md` or final section in `DEVELOPMENT_GUIDE.md`
- Modify: `README.md`, `CHANGELOG.md`, architecture/documentation index

**Actions:**
1. Add `scripts/verify_docs.py` CI task: Markdown paths, heading/status taxonomy, route→API/OpenAPI coverage, table→DATA_MODEL coverage, env→ENV coverage, canonical-term scan, orphan docs and screenshot manifest validation.
2. Add architecture decision review checklist to PR template: ADR? contract? current-state? migration? API? security? screenshot/e2e? runbook?
3. Establish one evidence matrix per phase: command, environment, artifact path, owner, acceptance outcome.
4. Perform independent reread from each audience entry path and verify no duplicate authority remains.
5. Delete redirect stubs after release cycle and record in CHANGELOG.

**Verification gates:**
- `git diff --check`
- `docker compose config -q`
- `docker compose -f backend/docker-compose.test.yml config -q`
- backend fmt/clippy/test/release build
- frontend typecheck/Vitest/build/Playwright/axe/Lighthouse
- documentation verifier and fresh-clone onboarding smoke
- screenshot manifest review

**Commit:** `docs: certify documentation architecture and visual evidence cleanup`

## Order And Dependency Rule

Execute strictly: Gate 0 → 1 → 2 → 3 → 4 → 5 → 6. Do not rebuild visuals before the UI is responsive/accessibility-verified, and do not consolidate or delete documents before canonical contracts/ADRs are reconciled.
