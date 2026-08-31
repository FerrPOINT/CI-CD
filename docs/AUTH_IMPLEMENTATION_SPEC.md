# Спецификация реализации Auth и policy

Нормативное дополнение к `AUTHORIZATION.md`. Scope первой реализации: Phase A foundation, затем Phase B browser sessions. API tokens — отдельный Phase C toggle.

## 1. Конфигурация и feature flags

| Variable | Requirement |
|---|---|
| `CICD_AUTH_ENABLED` | default `false`; only `true` enables deny-by-default routes |
| `CICD_AUTH_JWT_ISSUER` | required when enabled, exact `https://forge.local` style issuer |
| `CICD_AUTH_JWT_AUDIENCE` | required when enabled, default `forge-api` |
| `CICD_AUTH_JWT_KEYRING` | required JSON `{active_kid,keys:{kid:base64url-secret}}`; HS256 only in first release |
| `CICD_AUTH_ACCESS_TTL_SECONDS` | 900; range 300..3600 |
| `CICD_AUTH_REFRESH_TTL_SECONDS` | 2592000; range 3600..7776000 |
| `CICD_AUTH_COOKIE_SECURE` | true in production, false allowed only development |
| `CICD_AUTH_API_TOKENS_ENABLED` | default `false` |

Startup exits non-zero if `CICD_AUTH_ENABLED=true` and issuer/audience/keyring are absent or invalid. Key rotation keeps exactly one active and zero or more verify-only keys; access tokens signed by an old key are accepted until expiry, then old key may be removed.

## 2. Auth HTTP contract

All paths are under `/api/v1/auth`.

| Operation | Request | Success | Errors |
|---|---|---|---|
| `POST /login` | `{username,password}`; 1..128/8..1024 bytes | 200 `{access_token,expires_in,user}` + refresh cookie | 401 `invalid_credential`, 429 `rate_limited` |
| `POST /refresh` | refresh cookie only | 200 same shape, rotated cookie | 401 `invalid_credential` |
| `POST /logout` | refresh cookie optional | 204; clears cookie | always 204 |
| `GET /me` | bearer access token | 200 `{id,username,tenant_id,role}` | 401/403 |

Cookie: name `forge_refresh`; `HttpOnly; SameSite=Lax; Path=/api/v1/auth; Max-Age=2592000`; `Secure` follows config; no Domain attribute. Mutating cookie-auth requests require `X-CSRF-Token` matching `forge_csrf` non-HttpOnly cookie; bearer/API token clients are exempt.

`Authorization` accepts exactly one `Bearer <JWT-or-PAT>`. A JWT has three dot-separated segments; otherwise it is a target PAT prefixed `forge_pat_`. Current MVP PATs use the legacy `cicd_` prefix until Phase C migration. A malformed/duplicate header returns 401 + `WWW-Authenticate: Bearer`.

## 3. JWT, refresh and password details

JWT HS256 claims: `{sub: UUID, tid: UUID, sid: UUID, role: string, ver: integer, typ:"access", iss, aud, iat, exp, jti, kid}`. Validate alg=HS256, issuer/audience/type, exp/iat with 30 second skew, session revoke/expiry, `token_version` and user enabled. Current MVP stores `sid` plus a role hint and refreshes the effective role from DB on protected requests. Passwords use Argon2id memory 65536 KiB, iterations 3, parallelism 4; rehash after parameter change.

Refresh tokens: random 32 bytes base64url; DB stores `HMAC-SHA256(CICD_AUTH_REFRESH_PEPPER, raw)` plus `hash_key_id`. Rotation is one transaction: `SELECT ... FOR UPDATE`, reject revoked/expired/replaced, insert child then set parent `replaced_by/revoked_at`. Reuse of an already replaced token revokes all sessions in `family_id` and increments `users.token_version`.

## 4. Migration prerequisites

`0003_auth_foundation.sql` adds a minimal `service_accounts` table before `api_tokens`; token owner is exactly one of `user_id`/`service_account_id` (CHECK). All tenant/project linking uses application validation plus composite `(tenant_id,id)` unique keys and composite FK where PostgreSQL supports it. Add partial indexes for active credential lookup and checks: expiry after creation, one active owner bootstrap, tenant equality.

Bootstrap command: `cicd-migrate bootstrap-owner --tenant <slug> --username <name> --password-stdin`; it is idempotent only for same tenant/username and refuses an already existing different owner. No implicit default admin.

## 5. Route policy inventory

| Surface | Current route family | Phase B policy |
|---|---|---|
| Health/readiness/metrics | `/api/v1/health`, `/readyz`, `/metrics` | public (readyz/metrics added with auth) |
| Auth | `/api/v1/auth/*` | public/rate-limited per contract |
| Projects/pipelines/jobs/logs | `/api/v1/projects*`, `/api/v1/pipelines*`, `/api/v1/jobs*` | resolve project; `project.read` / `pipeline.run` / `job.write` |
| Git repository/PR/refs/compare | `/api/v1/repositories*`, `/api/v1/repos/*` | resolve project; `repository.read` / `repository.write` / `pull_request.*` |
| Git Smart HTTP | `/git/{repo}/*` | project-bound `git.read` / `git.write` |
| Internal Git push | `/api/v1/internal/git-push` | internal HMAC/token only, never human JWT |
| Runners | legacy `/api/v1/runners*` | admin only; external runner routes use runner principal |
| Secrets/artifacts | `/api/v1/projects/*/secrets`, `/api/v1/jobs/*/artifacts`, `/api/v1/artifacts/*` | `secret.manage`, `artifact.read/write`; runner only leased job |
| Environments/deployments | project/environment routes | `environment.read/manage/deploy` |
| Schedules/webhooks/notifications | project routes | `automation.read/manage` |
| Reports/audit | reports/audit routes | `report.read`, `audit.read_tenant` |
| Users/tokens | `/api/v1/users*`, `/api/v1/api-tokens*` | self/token owner or tenant admin |

Unlisted route fails CI route-policy inventory test. Route layering: request ID → trace → body limit → CORS/OPTIONS → rate limit → credential extraction → authn → authz resource lookup → handler. In Axum code build layers in reverse order and test preflight bypass.

## 6. Phase acceptance

- Phase A: migrations + bootstrap command + flags default false; all legacy behaviour unchanged; real DB migration suite passes.
- Phase B: auth E2E (login/refresh/logout), disabled user, invalid/expired/old-kid JWT, refresh reuse race, cookie/CSRF matrix, 429; every current route policy is declared and unlisted route test fails.
- Phase C: PAT create/list/revoke contract has scope/expiry/project fields; raw PAT returned once; malformed/revoked/expired and cross-tenant requests rejected; CLI uses `CICD_API_TOKEN`.
