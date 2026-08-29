# Спецификация миграций, OpenAPI и test DB

Нормативное дополнение к `STORAGE_ARCHITECTURE.md` и `DELIVERY_ARCHITECTURE.md`.

## 1. Workspace packages и инструменты

| Package | Назначение |
|---|---|
| `cicd-server` | Current MVP применяет `backend/migrations/*.sql` при старте; production target — verify-only startup без DDL |
| `cicd-migrate` (`backend/migration`) | `up`, `verify`, `inspect-legacy`, `adopt-legacy` |
| `cicd-api` (`backend/api`, target) | HTTP routes/DTO/OpenAPI generation |

Используется `sqlx-cli` версии, совпадающей с workspace major/minor (`0.8.x`), через контейнер `rust:1.86-bookworm`; host installation не требуется. `cicd-migrate` использует `sqlx::migrate!` и PostgreSQL advisory lock `forge_migration_lock`, timeout 60 seconds.

`cicd-migrate` и migrations уже находятся в workspace. Just targets для полного isolated migration lifecycle остаются target; current CI запускает real-DB suite через GitHub Actions PostgreSQL service.

## 2. Database roles и URLs

- `forge_owner`: владелец schema `forge`, единственный DDL/migration user.
- `forge_runtime`: runtime `cicd-server`; USAGE/DML only, no CREATE/ALTER/DROP.
- Test database name starts `forge_test_`; harness refuses any URL without this prefix.
- `CICD_TEST_DATABASE_URL` is mandatory for integration tests. It must use `forge_owner` only for migration setup; test runtime uses a second `forge_runtime` URL.

`backend/docker-compose.test.yml` uses `postgres:17-alpine`, no host port, tmpfs/ephemeral volume, healthcheck `pg_isready`, test roles from `backend/tests/sql/init-roles.sql`. It is a committed local fixture. Current GitHub Actions instead uses a PostgreSQL service and runs `cargo test --features integration --test integration_db`; compose lifecycle with unconditional `down -v` is still a target CI hardening step.

## 3. Legacy adoption and first migrations

Before production adoption run `cicd-migrate inspect-legacy --database-url ... --json > legacy-fingerprint.json`. The fingerprint contains canonical SQL metadata: tables, columns/types/null/defaults, PK/unique/FK/check constraints and indexes in deterministic lexical order.

`adopt-legacy` succeeds only if:

1. the fingerprint matches committed bootstrap fixture byte-for-byte;
2. backup identifier is supplied via `--backup-id`;
3. migration table is empty and no pending migration exists.

The first three migrations are fixed:

```text
0001_bootstrap_v1.sql        # Exact current public schema and indexes, no schema rename
0002_runtime_role.sql        # forge_owner/forge_runtime grants
0003_auth_foundation.sql     # password credentials and sessions
0004_outbox_delivery.sql     # domain_events/outbox_messages + schedule fire bookkeeping
0005_execution_gaps.sql      # timeout/allow_failure/manual, protected branches, PAT expiry, commit_sha, webhook secret
```

No rollback SQL is executed automatically. A failed production migration stops deployment; recovery is forward migration or restore of verified backup. `CREATE INDEX CONCURRENTLY` lives in a separate `-- no-transaction` file and is run in maintenance window with `migration_progress` rows (batch 1,000, resumable).

## 4. OpenAPI and generated frontend types

**Authoritative source:** current Rust `utoipa` annotations live in `cicd-server`; target moves them behind a `cicd-api` boundary. Generation produces committed `openapi/openapi.yaml`. Manual edits to generated YAML are forbidden.

| Command | Output | CI assertion |
|---|---|---|
| `cargo run --bin openapi-dump -- ../openapi/openapi.yaml` | `openapi/openapi.yaml` | Current backend CI generates `/tmp/openapi.yaml` and diffs |
| `pnpm openapi:generate` | `frontend/src/api/schema.d.ts` | Current frontend CI checks clean diff |
| target `just openapi-validate` | OpenAPI validation + examples | zero errors |
| target compatibility diff | backward diff against default branch | no breaking change |

Pinned frontend package `openapi-typescript` generates `schema.d.ts`; handwritten API wrapper/client remains responsible for auth headers, error decoding, binary upload/download and SSE. A future generated transport boundary may add `openapi-fetch`.

Every operation needs: `operationId`, tags, DTO schemas, success/error refs, security classification and at least one request/response example. `/git/*` and `/api/v1/internal/*` are included with `x-forge-internal: true`; UI generator ignores them, contract CI does not.

## 5. CI jobs (required names)

| Job | Sequence |
|---|---|
| current `backend` | PostgreSQL service → fmt → clippy → unit/workspace tests → real PostgreSQL tests → OpenAPI drift gate |
| current `frontend` | frozen install → generate TS from OpenAPI → clean diff → Vitest → build |
| current `docs` | `python3 scripts/verify_docs.py --all` |
| target `migration-test` | isolated test DB up → `cicd-migrate up` → `cicd-migrate verify` → owner/runtime matrix → test DB down |
| target `openapi-contract` | current drift/codegen checks + validate/examples + backward diff against `origin/main` |

## 6. First executable test matrix

- `migration_bootstrap_applies_to_empty_database`
- `legacy_adoption_refuses_unexpected_schema`
- `runtime_role_cannot_execute_ddl`
- `api_returns_safe_error_envelope_and_request_id`
- `cursor_rejects_tampering_and_expiry`
- `openapi_contains_all_registered_routes`
- `generated_client_compiles_against_spec`
