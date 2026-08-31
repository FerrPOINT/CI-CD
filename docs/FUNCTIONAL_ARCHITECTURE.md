# Функциональная архитектура Forge CI/CD

## 1. Назначение и границы продукта

Forge CI/CD — self-hosted control plane для жизненного цикла доставки исходного кода: Git-репозиторий → определение pipeline → безопасное выполнение → логи и artifacts → deployment/окружения → automation/integrations → audit/reporting.

Это **не GitLab/GitHub replacement** и не registry/IDE/issue tracker. Внешние GitHub/GitLab-репозитории могут быть зарегистрированы как проекты; встроенный bare Git hosting остаётся минимальным transport-слоем. Границы продукта фиксируют, что Forge не реализует issues, code review UI полного уровня, package registry или cluster management.

## 2. Capability map

| Контекст | Ответственность | MVP сейчас | Целевая v1 |
|---|---|---:|---:|
| Identity & Access | Личности, сессии, membership, роли, токены, policy | Частично: users/roles/tokens + conditional auth/RBAC при непустом `CICD_AUTH_SECRET` | Да |
| Project & Source | Project, repository connection, bare Git, refs, PR metadata | Да | Да |
| Pipeline Definition | `.forge-ci.yml`, validation, plan/DAG, variables | Частично: parser и линейный plan | Да |
| Execution | Queue, attempts, dispatch, runner protocol, sandbox | Частично: embedded runner | Да |
| Logs & Artifacts | Append-only logs, upload/download, integrity, retention | Частично | Да |
| Environments & Deployments | Environment state, deploy history, approvals, rollback | Частично: metadata | Да |
| Automation | Schedules, push events, webhooks, notifications, outbox | Частично: Git push, schedules MVP, outgoing webhooks MVP, bounded outbox history/requeue и `in_app`/`sse` notifications; inbound handlers и external adapters configuration/target-only | Да |
| Observability & Governance | Audit, reports, metrics, traces, operations, DR | Частично | Да |
| API & Clients | REST/OpenAPI, Dashboard, CLI, realtime | Частично | Да |

`Частично` не означает production-ready: реальный статус и ограничения каждой области описаны в тематических документах ниже.

## 3. Основной поток данных

```text
User / CLI / Git push / Schedule
             |
             v
    Identity + policy authorization
             |
             v
 Project + Pipeline Definition -----> immutable PipelinePlan
             |                                  |
             v                                  v
  Audit event / transactional outbox      Queue + dispatcher
                                                  |
                                                  v
                                             Runner lease
                                                  |
                  +-------------------------------+-----------------------------+
                  |                               |                             |
                  v                               v                             v
            execution attempt                job logs                    artifacts
                  |                               |                             |
                  +-------------------------------+-----------------------------+
                                                  |
                                                  v
                    status aggregation -> deployment -> domain events/outbox
                                                  |
                                                  v
                         webhook / notification / SSE / reports / audit
```

Команды не передают состояния напрямую между подсистемами. Application layer создаёт доменное изменение и outbox event в одной PostgreSQL-транзакции; фоновые workers доставляют побочные эффекты. Это исключает потерю webhook/notification при падении после commit.

## 4. Агрегаты и ownership

| Aggregate | Владелец | Изменение | События |
|---|---|---|---|
| Instance/User/Session | Identity | auth service | `user.*`, `session.*` |
| Project/Membership | Access + Project | project service | `project.*`, `membership.*` |
| Repository/PullRequest | Source | Git service | `repository.*`, `pull_request.*`, `git.push.received` |
| Pipeline/PipelinePlan | Pipeline Definition | planner | `pipeline.created`, `pipeline.planned` |
| Job/ExecutionAttempt | Execution | dispatcher/runner result | `job.*`, `attempt.*` |
| Runner | Execution | runner protocol | `runner.*` |
| Artifact | Storage | artifact service | `artifact.*` |
| Secret | Secrets | secret service | `secret.*` (without value) |
| Environment/Deployment | Delivery | deployment service | `deployment.*` |
| Schedule/Webhook/NotificationConfig | Automation | automation service | `schedule.*`, `webhook.*` |
| AuditEntry | Governance | append-only audit writer | derived from every mutation |

У каждого aggregate один application service — единственный автор транзакционной записи. HTTP handlers, runners и scheduler не пишут в чужие таблицы напрямую.

## 5. Нефункциональные инварианты

- **Isolation:** все чтения/записи scoped по `project_id`; policy проверяется до загрузки секретов, artifact или Git-операции.
- **Least privilege:** browser JWT, user API token, internal worker token и runner registration/lease token — разные credential classes и scopes.
- **No secret exposure:** plaintext только в памяти worker перед процессом; не попадает в API, DB logs, audit, error response или browser.
- **At-least-once async:** outbox и runner reconciliation допускают повтор доставки; handlers/idempotency keys делают результат идемпотентным.
- **Immutable evidence:** pipeline plan, execution attempts, logs, artifact metadata, deployment history и audit entries не переписываются; correction создаёт новую запись.
- **Bounded I/O:** pagination/keyset cursor, лимит request/body/log/artifact, explicit timeout/cancel, quotas и retention.
- **Recoverability:** schema migrations, verified backup of Postgres + Git + artifacts, health/readiness и reconciliation после restart.
- **Compatibility:** `/api/v1` контракты versioned; breaking changes появляются только в новой версии и документируются OpenAPI.

## 6. Target package ownership

```text
domain     entities, value objects, status/policy interfaces, port traits
app        commands, queries, use cases, transaction boundaries, domain events
infra      PostgreSQL repos, Git/artifact/secret adapters, outbox, runner client
api        HTTP DTO, OpenAPI, auth middleware, SSE projection
server     config + DI + process lifecycle + workers
cli        HTTP client; no database/filesystem/server linkage
frontend   generated API client, feature slices, query cache, UI
```

Dependency flow is one-way: `api → app → domain`, `infra → app/domain`, `server → all composition dependencies`. `domain` cannot import Axum, SQLx, Docker, Git or filesystem API.

## 7. Required architecture documents

| Document | Answers |
|---|---|
| `ARCHITECTURE.md` | Runtime, workspace boundaries, current migration state |
| `AUTHORIZATION.md` | Identity, session, membership, RBAC, token and audit policy |
| `RUNNER_ARCHITECTURE.md` | Pipeline planning, dispatch, runner protocol and execution attempts |
| `AUTOMATION_ARCHITECTURE.md` | Schedules, Git events, outbox, webhooks, notifications and SSE |
| `STORAGE_ARCHITECTURE.md` | Migrations, Postgres, Git, artifacts, secrets, backup/restore |
| `DELIVERY_ARCHITECTURE.md` | OpenAPI, Dashboard, CLI, observability and quality gates |
| `FUNCTIONAL_ARCHITECTURE.md` | Aggregate invariants and state machines |
| `DATA_MODEL.md` | Physical schema and indexes; current vs target |
| `ROADMAP.md` | Sequenced implementation milestones and definition of done |

## 8. Delivery order

1. Workspace boundaries, typed config, safe errors, versioned migrations, real-DB tests.
2. Identity/RBAC/token enforcement and project membership.
3. Pipeline plan/DAG and execution-attempt model.
4. External runner protocol and sandboxed executor; secret injection/redaction.
5. Outbox, schedules, webhook/notification delivery and realtime projection.
6. Artifact integrity/retention, environments/approvals, backup/DR, metrics and operational automation.
7. OpenAPI-generated clients, Dashboard/CLI completion, E2E/mobile/visual evidence.

Each phase must update the corresponding architecture, API, data model, ADR, tests and screenshots. A UI record or configuration form alone is never a completed capability.

## References

- `docs/ARCHITECTURE.md`
- `docs/ADR.md`
- `docs/ROADMAP.md`
- `plans/architecture-rebuild-plan.md`
