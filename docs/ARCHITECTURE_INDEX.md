# Architecture Index — Forge CI/CD

## Purpose

This index is the entry point for architecture decisions. It separates **current MVP behavior** from **target v1 architecture** so configuration screens or persistence tables are not mistaken for completed delivery capabilities.

## Start here

1. `docs/FUNCTIONAL_ARCHITECTURE.md` — product boundaries, capability map, aggregate ownership and cross-cutting invariants.
2. `docs/ARCHITECTURE.md` — runtime and workspace/package layout.
3. `docs/DOMAIN_MODEL.md` — domain invariants and pipeline status model.
4. `docs/DATA_MODEL.md` — physical database representation and indexes.
5. `docs/ADR.md` — accepted structural choices.

## Bounded contexts

| Context | Target architecture | Current contract/details |
|---|---|---|
| Identity & Access | `docs/AUTHORIZATION.md` | `docs/SECURITY.md`, `docs/SYSTEM_ADMIN.md` |
| Pipeline execution & runners | `docs/RUNNER_ARCHITECTURE.md` | `docs/WORKFLOW.md`, `docs/RESILIENCE.md` |
| Automation & integrations | `docs/AUTOMATION_ARCHITECTURE.md` | `docs/EVENTS.md`, `docs/WEBHOOKS.md`, `docs/NOTIFICATIONS.md` |
| Storage & lifecycle | `docs/STORAGE_ARCHITECTURE.md` | `docs/STORAGE.md`, `docs/SECRETS_MGMT.md`, `docs/ARTIFACTS.md`, `docs/MIGRATIONS.md` |
| API, clients & observability | `docs/DELIVERY_ARCHITECTURE.md` | `docs/API*.md`, `docs/FRONTEND_ARCHITECTURE.md`, `docs/CLI.md`, `docs/MONITORING.md`, `docs/TESTING.md` |
| Git and pull requests | `docs/GIT_HOSTING.md`, `docs/PULL_REQUESTS.md` | `docs/API.md` |
| Deployment and operations | `docs/DEPLOYMENT.md`, `docs/RUNTIME.md`, `docs/OPS_RUNBOOK.md`, `docs/BACKUP_RESTORE.md` | `docs/TROUBLESHOOTING.md` |

## Current vs target notation

- **Current**: behavior verified in repository code and/or real local checks.
- **Target**: approved architecture; implementation pending and must not be presented as an active product capability.
- **Phase**: delivery order in `docs/ROADMAP.md`; a phase is complete only with migrations, contract tests, integration/E2E evidence, UI proof and updated docs.

## Mandatory change impact

| Change | Must update |
|---|---|
| New/changed REST contract | `API.md`, OpenAPI contract, generated client, `API_EDGE_CASES.md` |
| Schema change | SQLx migration, `DATA_MODEL.md`, `MIGRATIONS.md`, integration tests |
| Authorization boundary | `AUTHORIZATION.md`, `SECURITY.md`, audit event catalog, policy tests |
| Runner/execution behavior | `RUNNER_ARCHITECTURE.md`, `WORKFLOW.md`, `RESILIENCE.md`, threat model/tests |
| Async event or external delivery | `AUTOMATION_ARCHITECTURE.md`, `EVENTS.md`, `WEBHOOKS.md`, metrics/tests |
| Storage/retention/key handling | `STORAGE_ARCHITECTURE.md`, `STORAGE.md`, `BACKUP_RESTORE.md`, `SECRETS_MGMT.md` |
| User-visible change | `ROUTING.md`, `UI_UX.md`, screenshots and Playwright evidence |
| New material architectural choice | new ADR + `ADR.md` index |

## Accepted target decisions

- `ADR-0005`: Cargo workspace and ports/adapters migration.
- `ADR-0006`: PostgreSQL transactional outbox for reliable async effects.
- `ADR-0007`: execution is separated from the API control plane.
- `ADR-0008`: versioned SQLx migrations replace startup schema bootstrap.

## Delivery plan

`plans/architecture-rebuild-plan.md` is the detailed, uncommitted implementation plan. `docs/ROADMAP.md` remains the public, versioned milestone view.
