# Контракт аутентификации и авторизации

Статус: Accepted target contract. Основание: [ADR-0009](../adr/0009-canonical-registry.md).

Этот контракт определяет целевое наблюдаемое поведение. Он имеет приоритет над narrative-документами; текущий MVP не считается реализующим эти требования.

## 1. Термины и границы tenancy

- **tenant** -- логически изолированный владелец проектов и связанных ресурсов. В модели и API используются только `tenant`, `tenants`, `tenant_id`; термины `organization` и `workspace` не применяются.
- **project** принадлежит ровно одному tenant. Pipeline, stage, job, log, secret, artifact, environment, deployment, schedule, webhook, notification и repository наследуют `tenant_id` через project.
- **principal** -- аутентифицированный субъект. Он всегда имеет тип, стабильный ID, состояние и ограниченный scope.
- **resource scope** -- `instance`, `tenant`, `project` или ресурс, чей canonical tenant/project выводится сервером. Клиентский `tenant_id` не является доверенным scope.
- Для поиска и списков сервер обязан применять tenant/project predicate. Обращение к чужому ресурсу по ID возвращает `404`; к своему ресурсу без permission -- `403`.

## 2. Principal и credential

| Principal | Credential | Допустимый scope | Запрещено |
|---|---|---|---|
| `user` | access JWT и refresh session | memberships tenant/project | Передавать роль как доверенный claim |
| `api_token` | PAT владельца | Явные scopes и project bindings | Расширять права владельца |
| `service_account` | SAT service account | Tenant и разрешённые projects | Интерактивный вход и browser session |
| `runner` | mTLS certificate и job lease | Только выданный job | User, admin и произвольные project API |
| `system` | internal signed credential | Явно названный internal use-case | Внешний bearer access |
| `instance_admin` | credential user с instance permission | `instance` | Неаудируемый break-glass доступ |

Principal недействителен при disable владельца, revoke/expiry credential, удалённом membership или несоответствии tenant/project scope. Аутентификация не заменяет проверку permission в application layer.

## 3. Session и JWT

- Access JWT: HS256, TTL 15 минут; допустимый clock skew -- 30 секунд. Настраиваемый диапазон TTL: 5--60 минут.
- Claims: `sub`, `sid`, `ver`, `typ="access"`, `iss`, `aud`, `iat`, `exp`, `jti`, `kid`. Claims не содержат роль, membership, permission или secret scope.
- Сервер проверяет algorithm, issuer, audience, type, expiry, `users.token_version`, состояние user и revoke session на каждом запросе.
- Refresh token: 32 криптографически случайных байта в base64url; TTL семьи не более 30 дней. Он хранится только в `Secure`, `HttpOnly`, `SameSite=Lax` cookie `forge_refresh`, path `/api/v1/auth`; `Secure=false` допускается только в development.
- Refresh rotation выполняется одной транзакцией: старый token становится replaced/revoked, выдаётся новый token той же семьи с expiry не позднее исходной expiry семьи.
- Повторное использование replaced refresh token отзывает всю семью и увеличивает `token_version` user. Logout, disable user, password reset и принудительный revoke немедленно делают sessions недействительными.
- JWT keyring содержит один active `kid` и verify-only keys. Token старого key принимается только до своего `exp`; затем key удаляется. Password credential хранится только как Argon2id hash.

## 4. API tokens

- Пользовательский token имеет формат `forge_pat_<prefix>_<secret>`, service token -- `forge_sat_<prefix>_<secret>`.
- `<secret>` содержит не менее 256 бит криптографической случайности; `<prefix>` уникален, не является секретом и используется только для lookup. Raw token показывается ровно один раз и не попадает в audit, logs или responses после создания.
- В БД хранятся `id`, owner type/id, `tenant_id`, name, prefix, `HMAC-SHA-256(raw_token, versioned_auth_pepper)`, scopes, project bindings, `expires_at`, `revoked_at`, revoke reason и usage timestamps. Plaintext и простой SHA-256 без pepper запрещены.
- `Authorization: Bearer <token>` принимает ровно один credential. PAT/SAT проверяется constant-time после lookup по prefix; проверяются format, HMAC, expiry, revoke, owner state, membership и tenant/project binding.
- Effective permission равна пересечению role permission, token scope и project binding. Token не получает permission, которой нет у его owner/service account.
- Token имеет явный `expires_at`; бессрочный token запрещён. Create, revoke, expiry, anomalous use и неуспешная проверка подлежат audit. Legacy `cicd_` tokens принимаются только в явно ограниченный migration period и затем отзываются.

## 5. RBAC

Роли фиксированы; deny-rules и пользовательские роли отсутствуют. `instance_admin` управляет instance-only permissions и не наследуется из tenant/project ролей.

| Permission | project_owner | maintainer | developer | reporter | viewer |
|---|---:|---:|---:|---:|---:|
| `project.read` | yes | yes | yes | yes | yes |
| `project.manage` | yes | yes | no | no | no |
| `pipeline.read`, `job.logs.read` | yes | yes | yes | yes | no |
| `pipeline.run`, `pipeline.cancel`, `pipeline.retry` | yes | yes | yes | no | no |
| `job.write`, `artifact.write` | yes | yes | yes | no | no |
| `artifact.read`, `report.read` | yes | yes | yes | yes | no |
| `secret.read_metadata`, `secret.manage` | yes | yes | no | no | no |
| `environment.read`, `environment.manage`, `deployment.create` | yes | yes | policy | no | no |
| `automation.read`, `automation.manage` | yes | yes | no | no | no |
| `repository.read`, `pull_request.read` | yes | yes | yes | yes | no |
| `repository.write`, `pull_request.manage` | yes | yes | yes | no | no |
| `project.member.manage`, `project.delete`, `audit.read_project` | yes | no | no | no | no |

`policy` означает approval/environment policy, заданную tenant или project. Permission `secret.read_value` для user, API token и service account не существует: расшифровка разрешена только use-case для действующего runner lease.

Tenant roles: `owner` управляет tenant, projects, memberships, service accounts и runner pools; `tenant_admin` -- тем же, кроме удаления последнего owner; `member` не получает project access автоматически. Project membership требует active tenant membership. `instance_admin` один имеет `identity.manage`, `tenant.manage`, `audit.read_instance` и instance-wide `runner.manage`.

## 6. Инвентарь route policy

Все перечисленные current routes из `docs/API.md` требуют policy. Неуказанный route не может быть опубликован: CI сверяет router с этим инвентарём. `401` означает отсутствующий/недействительный credential, `403` -- недостаточную permission, `404` -- foreign resource.

| Method | Path | Principal и обязательная policy |
|---|---|---|
| GET | `/api/v1/health` | Public; network policy only |
| GET, POST | `/api/v1/projects` | user/PAT/SAT: `project.read` filtered / `tenant.project.create` |
| GET, PATCH, DELETE | `/api/v1/projects/{project_id}` | `project.read` / `project.manage` / `project.delete` |
| GET, POST | `/api/v1/projects/{project_id}/pipelines` | `pipeline.read` / `pipeline.run` |
| GET | `/api/v1/pipelines/{pipeline_id}` | `pipeline.read` via pipeline project |
| POST | `/api/v1/pipelines/{pipeline_id}/cancel` | `pipeline.cancel` |
| POST | `/api/v1/pipelines/{pipeline_id}/retry` | `pipeline.retry` |
| POST | `/api/v1/jobs/{job_id}/status` | user/PAT/SAT: `job.write`; runner: matching lease only |
| GET, POST | `/api/v1/jobs/{job_id}/logs` | `job.logs.read` / `job.write` or matching runner lease |
| POST | `/api/v1/jobs/{job_id}/retry` | `pipeline.retry` via job project |
| GET, POST, DELETE, POST | `/api/v1/runners`, `/api/v1/runners`, `/api/v1/runners/{runner_id}`, `/api/v1/runners/{runner_id}/heartbeat` | tenant `runner.manage`; runner heartbeat only own identity |
| GET, POST | `/api/v1/projects/{project_id}/secrets` | `secret.read_metadata` / `secret.manage` |
| DELETE | `/api/v1/secrets/{secret_id}` | `secret.manage` via secret project |
| GET, POST | `/api/v1/jobs/{job_id}/artifacts` | `artifact.read` / `artifact.write` or matching runner lease |
| GET | `/api/v1/artifacts/{artifact_id}/download` | `artifact.read` via artifact job |
| GET, POST | `/api/v1/projects/{project_id}/environments` | `environment.read` / `environment.manage` |
| PATCH, DELETE | `/api/v1/environments/{environment_id}` | `environment.manage` |
| GET, POST | `/api/v1/environments/{environment_id}/deployments` | `environment.read` / `deployment.create` |
| GET, POST | `/api/v1/projects/{project_id}/schedules` | `automation.read` / `automation.manage` |
| PATCH, DELETE | `/api/v1/schedules/{schedule_id}` | `automation.manage` |
| GET, POST | `/api/v1/projects/{project_id}/webhooks` | `automation.read` / `automation.manage` |
| DELETE | `/api/v1/webhooks/{webhook_id}` | `automation.manage` |
| GET | `/api/v1/projects/{project_id}/outbox-deliveries` | `automation.read` |
| GET | `/api/v1/outbox-deliveries/{delivery_id}` | `automation.read` via delivery project |
| POST | `/api/v1/outbox-deliveries/{delivery_id}/requeue` | `automation.manage` via delivery project |
| GET, PUT | `/api/v1/projects/{project_id}/notifications` | `automation.read` / `automation.manage` |
| GET | `/api/v1/projects/{project_id}/notification-events` | `automation.read` |
| GET | `/api/v1/projects/{project_id}/notifications/stream` | `automation.read` |
| GET | `/api/v1/projects/{project_id}/reports/summary` | `report.read` |
| GET | `/api/v1/audit-log` | `audit.read_project` or tenant/instance audit scope; always filtered |
| GET, POST | `/api/v1/users` | `identity.manage`; creation assigns no implicit owner |
| PATCH | `/api/v1/users/{user_id}` | self for safe profile fields, otherwise `identity.manage` |
| GET, POST | `/api/v1/api-tokens` | token owner for own tokens; tenant owner/admin for scoped service tokens |
| DELETE | `/api/v1/api-tokens/{token_id}` | token owner or tenant owner/admin |
| GET, POST | `/api/v1/repositories` | `repository.read` / `repository.write` in tenant project binding |
| DELETE | `/api/v1/repositories/{name}` | `repository.write` and project owner |
| GET | `/api/v1/repos/{repo}/refs`, `/api/v1/repos/{repo}/commits`, `/api/v1/repos/{repo}/compare` | `repository.read` |
| GET, POST | `/api/v1/repos/{repo}/pulls` | `pull_request.read` / `pull_request.manage` |
| POST | `/api/v1/repos/{repo}/pulls/{number}/action` | `pull_request.manage` |
| GET | `/git/{repo}/info/refs` | Git credential with `repository.read` |
| POST | `/git/{repo}/git-upload-pack` | Git credential with `repository.read` |
| POST | `/git/{repo}/git-receive-pack` | Git credential with `repository.write` |
| POST | `/api/v1/internal/git-push` | `system` only: signed internal event, timestamp and one-time event ID |

`/api/v1/auth/login`, `/refresh` и `/logout` являются public only for their stated credential flow, rate-limited; `/me` requires user access JWT. Cookie-authenticated unsafe requests require CSRF proof; bearer credentials are exempt.

## 7. Audit policy

- Каждая authentication success/failure, refresh reuse, logout, token create/revoke/use anomaly, membership/role change, tenant/project mutation, pipeline/job transition, runner registration/lease, secret metadata mutation, secret injection, authorization denial, break-glass и audit export создаёт event.
- Event содержит immutable `occurred_at`, `request_id`, `tenant_id`, optional `project_id`, actor type/id/display, action, outcome (`success`, `denied`, `failure`), resource type/id, IP/user-agent hashes и allowlisted metadata.
- Metadata может содержать resource name, token prefix, role, scope, transition, reason, key identifier и safe error code. Она не содержит password, raw token, session, authorization header, decrypted secret, ciphertext, raw command output или full IP.
- Security-sensitive mutation и её audit event фиксируются в одной DB transaction; при ошибке audit mutation отклоняется. Audit events append-only, не удаляются cascade вместе с actor/project и защищаются hash chain/checkpoints.
- Read/export audit требует соответствующей scope permission, всегда tenant/project filtered. Retention -- минимум 1 год; purge возможен только по утверждённой retention procedure после проверяемого archive/export.
