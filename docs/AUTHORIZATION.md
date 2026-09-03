# Авторизация и границы доступа Forge CI/CD

## Статус

> Нормативные контракты вынесены в `contracts/AUTHZ_CONTRACT.md`; при конфликте прав контракт. Этот документ — объяснительный narrative (ADR-0009).

**Target architecture.** Документ фиксирует целевую модель для аутентификации, авторизации, tenancy, членства в проектах, API-токенов, идентичности runner, доступа к секретам и аудита.

Текущая реализация остаётся источником фактов о работающем MVP; целевая модель не должна выдаваться за уже реализованную функциональность.

## Цели и принципы

Forge CI/CD является multi-tenant control plane: один экземпляр обслуживает несколько логически изолированных рабочих пространств. Все пользовательские, API- и runner-действия выполняются от явного субъекта и в явном контексте tenant/project.

Целевая модель должна обеспечить:

- изоляцию данных, Git-репозиториев, пайплайнов, артефактов, окружений и секретов между tenant;
- least privilege: доступ выдаётся минимальной ролью и ограниченным scope;
- отсутствие неаутентифицированных control-plane endpoint в production;
- раздельные идентичности для человека, автоматизации, системного процесса и runner;
- невозможность получить значение секрета через обычный пользовательский API;
- проверяемый append-only аудит значимых действий;
- реализацию в слоях `domain -> app -> infra -> api -> server` согласно [ADR-0005](adr/0005-workspace-layered-architecture.md).

Не являются целью первой поставки:

- внешняя федерация пользователей и SSO: OIDC/SAML добавляются после стабилизации локальной модели;
- произвольные пользовательские роли и deny-политики;
- доступ runner к Docker socket control-plane;
- хранение секретов в plaintext, возврат значений секретов в UI или экспорт аудита без ограничения доступа.

## Текущее состояние

| Область | Реализовано сейчас | Риск / дефицит |
|---|---|---|
| HTTP API | Conditional auth middleware: без непустого `CICD_AUTH_SECRET` trusted-network/open; с секретом работают JWT/PAT, scoped PAT, session-bound access invalidation, browser refresh cookie + CSRF, refresh rotate/logout/revoke, session-family reuse revocation, route-role checks и project membership enforcement для project-owned API | Нет production default-deny boundary, tenant isolation и service-account tokens |
| Users | `users(id, username, role, enabled)`, `user_credentials`, `sessions`, `project_memberships` | Tenant membership и tenant isolation отсутствуют |
| Roles | `admin`, `maintainer`, `developer`, `viewer` участвуют в route-policy decisions; project role берётся из `project_memberships` для project-owned ресурсов | Нет tenant roles и tenant-wide membership model |
| API tokens | Случайный `cicd_...`, SHA-256 hash, hint, owner, project_id, explicit scopes, expiry, last-used и soft revoke; Bearer PAT работает при `CICD_AUTH_SECRET` | Нет pepper/HMAC storage, revoke reason, service-account token и tenant boundary; legacy `project_id = NULL` tokens остаются global до отзыва |
| Projects | Глобально уникальное `projects.name`; project memberships ограничивают доступ к project-owned ресурсам при включённом auth | Нет tenant ownership и tenant-aware project scope |
| Secrets | `project_secrets`, AES-256-GCM at rest, API не возвращает value; embedded runner injects env and masks stdout/stderr best-effort | Нет scoped lease, key version/rotation и full redaction coverage |
| Runners | Registry + heartbeat; embedded supervisor выполняет jobs | Runner не обладает отдельной криптографической идентичностью; registry не является границей доступа |
| Git | Public read для public repo; private read/write через legacy `CICD_GIT_TOKEN` либо JWT/PAT + project membership + `git:*` PAT scopes при `CICD_AUTH_SECRET`; hook token | Нет tenant-bound repository model, отдельного scoped Git credential class, signed push events и deny audit для Git |
| Audit | `audit_log` с action/resource/actor text, login/denied и многие mutation events | Actor не является нормализованным principal, нет tenant/project scope и фильтров/export |
| CORS и transport | `CICD_CORS_ALLOWED_ORIGINS` включает allowlist origins; пустое значение оставляет permissive CORS только для isolated dev; cookie-backed refresh/logout требует CSRF proof; HTTP в dev | Для production необходим TLS, `CICD_AUTH_COOKIE_SECURE=true` и непустой allowlist |

До внедрения целевой модели open/trusted-network режим допустим только в изолированной локальной сети разработки. Публичный или shared deployment без непустого `CICD_AUTH_SECRET`, reverse proxy/network boundary и непустых Git/internal tokens запрещён.

## Термины

- **Principal** — аутентифицированный субъект: пользователь, API-token, runner или внутренний системный процесс.
- **User** — интерактивная учётная запись человека.
- **Service token** — API-токен автоматизации, принадлежащий пользователю либо service account.
- **Tenant / workspace** — логически изолированное рабочее пространство организации или команды.
- **Project** — CI/CD-проект, принадлежащий ровно одному tenant.
- **Membership** — связь пользователя с tenant или проектом и набор его ролей.
- **Permission** — атомарное разрешение, например `pipeline.trigger` или `secret.manage`.
- **Resource scope** — tenant, project, конкретный pipeline/job/secret/artifact либо instance.
- **Runner principal** — machine identity процесса runner-а; не является user и не получает пользовательские права.
- **Lease** — ограниченное по времени право runner выполнить конкретный job.
- **Break-glass** — временный привилегированный доступ платформенного администратора с обязательным обоснованием и аудитом.

## Целевая модель tenancy и membership

### Иерархия владения

```text
Forge instance
  └── tenant / workspace
        ├── tenant memberships
        ├── projects
        │     ├── project memberships
        │     ├── repositories, pipelines, jobs, logs
        │     ├── environments, schedules, webhooks
        │     ├── secrets, deployments, artifacts
        │     └── project audit events
        ├── runners and runner pools
        └── tenant audit events
```

Каждый resource, для которого существует или выводится `project_id`, обязан быть ограничен tenant проекта. Нельзя выполнять выборку по resource ID без проверки принадлежности project/tenant.

Исключения:

- `/api/v1/health`, `/api/v1/readiness` и `/metrics` не требуют user authentication, но должны быть ограничены сетевой политикой;
- bootstrap регистрации runner использует короткоживущий registration token вместо user session;
- internal Git hook использует подписанный internal event credential;
- platform operator actions доступны только instance-admin principal и всегда создают audit event.

### Роли

Роли не являются строкой, которой доверяет HTTP-слой. Они преобразуются в фиксированный набор permissions в domain/application policy.

| Scope | Роль | Назначение |
|---|---|---|
| Instance | `instance_admin` | Управление tenant, platform policies, runner pools, identities и emergency access |
| Tenant | `owner` | Полный контроль tenant, проектов, membership и service accounts |
| Tenant | `tenant_admin` | Управление проектами и участниками, без удаления последнего owner |
| Tenant | `member` | Базовый tenant membership без автоматического доступа ко всем проектам |
| Project | `project_owner` | Полный контроль проекта, включая membership, secrets и destructive actions |
| Project | `maintainer` | Pipeline/config/environment management, secret metadata и write-операции |
| Project | `developer` | Просмотр, trigger/retry/cancel разрешённых pipeline, доступ к логам и артефактам |
| Project | `reporter` | Просмотр pipeline, логов, deployment, reports и artifact download |
| Project | `viewer` | Только метаданные проекта и pipeline без чувствительных operational data |
| Runner | `runner` | Только runner protocol для выданного lease |

Нельзя назначить project role пользователю, который не имеет активного membership в tenant. При удалении или disable user все его memberships и активные sessions/tokens немедленно теряют эффективность.

### Базовые permissions

| Permission | Owner | Maintainer | Developer | Reporter | Viewer |
|---|---:|---:|---:|---:|---:|
| `project.read` | yes | yes | yes | yes | yes |
| `pipeline.read`, `job.logs.read` | yes | yes | yes | yes | ограниченно |
| `pipeline.trigger`, `pipeline.retry`, `pipeline.cancel` | yes | yes | yes | no | no |
| `project.update`, CI config update | yes | yes | no | no | no |
| `environment.manage`, `deployment.create` | yes | yes | policy-based | no | no |
| `artifact.upload`, `artifact.download` | yes | yes | policy-based | yes | no |
| `secret.read_metadata` | yes | yes | no | no | no |
| `secret.manage`, `secret.rotate`, `secret.delete` | yes | yes | no | no | no |
| `member.manage` | yes | no | no | no | no |
| `audit.read_project` | yes | policy-based | no | no | no |

`secret.read_value` не существует для human/API principal. Расшифровка возможна только application use-case `PrepareJobSecrets` для валидного runner lease.

Платформенные permissions (`tenant.manage`, `runner_pool.manage`, `audit.read_instance`, `identity.manage`) не наследуются из project roles.

### Правило вычисления доступа

1. Middleware аутентифицирует credential и строит `Principal`.
2. API adapter извлекает tenant/project identifier из route или целевого resource.
3. Application service загружает resource и вычисляет его canonical `tenant_id`/`project_id`.
4. `AuthorizationService` вычисляет effective permissions из active user, tenant membership, project membership, token scopes и состояния principal.
5. Use-case разрешает или отклоняет действие до доступа к данным, расшифровки секрета или изменения состояния.
6. Repository получает уже авторизованный scope и всегда фильтрует запрос по tenant/project.
7. Значимое изменение и audit event фиксируются в одной транзакции.

Списковые endpoint возвращают только ресурсы доступных tenant/project. При запросе чужого resource по ID сервер возвращает `404`, чтобы не раскрывать существование ресурса. Для собственного resource с недостаточной permission возвращается `403`.

## Целевая дата-модель

Новые таблицы создаются только versioned SQLx migrations. Исторический `store::migrate()` рассматривается только как источник legacy fingerprint для старых инсталляций.

### Identity и tenancy

```sql
CREATE TABLE tenants (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID
);

CREATE TABLE users (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT UNIQUE,
    password_hash TEXT,
    enabled BOOLEAN NOT NULL DEFAULT true,
    token_version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tenant_memberships (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'tenant_admin', 'member')),
    status TEXT NOT NULL CHECK (status IN ('active', 'invited', 'disabled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    PRIMARY KEY (tenant_id, user_id)
);

ALTER TABLE projects
    ADD COLUMN tenant_id UUID REFERENCES tenants(id),
    ADD CONSTRAINT projects_tenant_name_unique UNIQUE (tenant_id, name);
```

После backfill `projects.tenant_id` становится `NOT NULL`; глобальный `UNIQUE(name)` удаляется отдельной миграцией после проверки конфликтов.

```sql
CREATE TABLE project_memberships (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (
        role IN ('project_owner', 'maintainer', 'developer', 'reporter', 'viewer')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    PRIMARY KEY (project_id, user_id)
);
```

Для первой версии роли фиксированы в коде. Таблицы произвольных role definitions и deny rules не вводятся, пока не появится реальная потребность и отдельный ADR.

### Browser sessions и API tokens

```sql
CREATE TABLE refresh_sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE,
    token_family_id UUID NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    replaced_by UUID REFERENCES refresh_sessions(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    ip_hash BYTEA,
    user_agent_hash BYTEA
);

CREATE TABLE api_tokens (
    id UUID PRIMARY KEY,
    principal_type TEXT NOT NULL CHECK (principal_type IN ('user', 'service_account')),
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    service_account_id UUID,
    tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_prefix TEXT NOT NULL UNIQUE,
    token_hash BYTEA NOT NULL UNIQUE,
    token_hash_key_version SMALLINT NOT NULL,
    scopes TEXT[] NOT NULL,
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_by UUID REFERENCES users(id) ON DELETE SET NULL,
    revoke_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (principal_type = 'user' AND user_id IS NOT NULL AND service_account_id IS NULL)
        OR
        (principal_type = 'service_account' AND user_id IS NULL AND service_account_id IS NOT NULL)
    )
);

CREATE TABLE api_token_project_scopes (
    api_token_id UUID NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    PRIMARY KEY (api_token_id, project_id)
);
```

Новый token имеет формат `forge_pat_<prefix>_<secret>` или `forge_sat_<prefix>_<secret>`, где `<secret>` содержит не менее 256 бит криптографической случайности. Секрет отображается ровно один раз.

Для поиска используется публичный prefix; для проверки хранится `HMAC-SHA-256(auth_pepper_version, raw_token)`, а не plaintext. Сравнение выполняется constant-time. `auth_pepper` хранится вне БД, имеет key version и поддерживает controlled rotation.

Существующий `api_tokens.token_hash` мигрируется как legacy credential: до отзыва он может быть принят только в transition period, но не получает новые scopes автоматически. После migration deadline legacy tokens принудительно отзываются.

### Runner identity и lease

```sql
CREATE TABLE runner_registration_tokens (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    runner_pool_id UUID,
    token_hash BYTEA NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE runners (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    runner_pool_id UUID,
    name TEXT NOT NULL,
    tags TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL CHECK (status IN ('online', 'offline', 'paused', 'revoked')),
    credential_version INTEGER NOT NULL DEFAULT 1,
    public_key_fingerprint TEXT NOT NULL UNIQUE,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name)
);

CREATE TABLE job_leases (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL UNIQUE REFERENCES jobs(id) ON DELETE CASCADE,
    runner_id UUID NOT NULL REFERENCES runners(id) ON DELETE RESTRICT,
    lease_token_hash BYTEA NOT NULL UNIQUE,
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    acknowledged_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    renewed_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);
```

Runner получает credential только после одноразовой регистрации. Предпочтительный transport — mTLS с клиентским сертификатом, выпущенным внутренним CA; краткоживущий runner JWT допустим только как переходная реализация и должен быть привязан к certificate/public key.

Runner не может выбирать произвольный `job_id`, `project_id` или secret. Он получает job исключительно через `claim` и доказывает действующий lease во всех последующих запросах.

### Secrets

```sql
CREATE TABLE project_secrets (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    active_version INTEGER NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    UNIQUE (project_id, key)
);

CREATE TABLE project_secret_versions (
    id UUID PRIMARY KEY,
    secret_id UUID NOT NULL REFERENCES project_secrets(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    encrypted_dek BYTEA NOT NULL,
    key_id TEXT NOT NULL,
    encryption_context JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    retired_at TIMESTAMPTZ,
    created_by_principal_id UUID,
    UNIQUE (secret_id, version)
);
```

Целевая криптографическая схема:

- AES-256-GCM сохраняется для шифрования значения;
- для каждой версии секрета генерируется отдельный DEK;
- DEK шифруется master key из KMS/Vault либо versioned keyring;
- encryption context включает `tenant_id`, `project_id`, `secret_id`, `version` и назначение `forge-project-secret`;
- ciphertext, nonce, encrypted DEK и context проверяются совместно, что предотвращает незаметную подмену ciphertext между tenant/project;
- `CICD_SECRETS_KEY` остаётся только временным local-development backend до внедрения keyring;
- history значений не возвращается API и удаляется по retention policy после успешной ротации.

### Audit

```sql
CREATE TABLE audit_events (
    id BIGSERIAL PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_id UUID,
    tenant_id UUID REFERENCES tenants(id) ON DELETE RESTRICT,
    project_id UUID REFERENCES projects(id) ON DELETE RESTRICT,
    actor_type TEXT NOT NULL CHECK (
        actor_type IN ('user', 'api_token', 'service_account', 'runner', 'system')
    ),
    actor_id UUID,
    actor_display TEXT,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'denied', 'failure')),
    resource_type TEXT NOT NULL,
    resource_id UUID,
    ip_hash BYTEA,
    user_agent_hash BYTEA,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    prev_event_hash BYTEA,
    event_hash BYTEA NOT NULL
);
```

`metadata` допускает только allowlist полей: имя resource, token prefix, роль, scope, status transition, reason, key identifier и технический error code. В нём запрещены password, bearer token, session token, decrypted secret, ciphertext, raw command output и authorization header.

Audit events не удаляются каскадно вместе с project/user. Для удалённых ресурсов сохраняются immutable IDs и безопасные display metadata. Retention и архивирование определяются отдельной policy; перед удалением нужны signed export и подтверждённая процедура.

## Архитектурные границы

### Domain

`domain` содержит:

- newtype IDs: `TenantId`, `ProjectId`, `UserId`, `RunnerId`, `TokenId`, `SecretId`, `AuditEventId`;
- `PrincipalKind`, `Principal`, `Role`, `Permission`, `Scope`, `AuthContext`;
- policy rules и чистые функции permission evaluation;
- `RunnerLease`, `TokenState`, `SecretAccessPurpose`;
- port traits: `IdentityRepository`, `MembershipRepository`, `TokenRepository`, `ProjectRepository`, `RunnerRepository`, `SecretRepository`, `AuditRepository`.

Domain не импортирует Axum, SQLx, filesystem, Docker, crypto key provider или HTTP headers.

### Application

`app` реализует use-cases и границы транзакций:

- `AuthenticatePassword`, `RefreshSession`, `Logout`;
- `CreateApiToken`, `RevokeApiToken`, `AuthorizeRequest`;
- `CreateProject`, `UpdateProject`, `TriggerPipeline`, `CancelPipeline`;
- `GrantProjectMembership`, `RemoveProjectMembership`;
- `CreateSecret`, `RotateSecret`, `DeleteSecret`;
- `RegisterRunner`, `ClaimJob`, `RenewLease`, `PrepareJobSecrets`, `CompleteJob`;
- `ListAuditEvents`, `ExportAuditEvents`.

Именно application layer делает resource-level authorization. API middleware не считается достаточным контролем: внутренний вызов use-case, CLI, scheduler, Git hook и future RPC также обязаны передавать `AuthContext`.

Для security-sensitive mutation (`secret.*`, `token.*`, membership, role, runner credential, break-glass) запись состояния и `audit_events` происходят в одной DB transaction. Если audit event невозможно записать, операция завершается ошибкой и не применяется.

### Infrastructure

`infra` реализует ports:

- PostgreSQL repositories с обязательным tenant/project predicate;
- Argon2id password hasher;
- JWT/session signer и refresh token store;
- HMAC token verifier с key version;
- KMS/Vault/keyring secret envelope provider;
- runner CA/certificate issuer и lease store;
- Git credential adapter и signed internal hook verifier;
- audit hash-chain/checkpoint writer;
- artifact and log redaction adapters.

Repository никогда не принимает tenant/project ID из невалидированного client body как доверенный scope. Scope передаётся application service после ownership check.

PostgreSQL Row-Level Security можно добавить как defense-in-depth после выделения отдельных DB roles и test harness. До этого она не заменяет application authorization и query scoping.

### API и server

`api` содержит DTO, extractors, route definitions, middleware и маппинг `AppError` в безопасные HTTP responses. В handler нет SQL, прямого AES decrypt или business authorization logic.

`server` создаёт config, repositories, cryptographic providers, application services, router, background workers и runner dispatcher. Только он собирает зависимости.

### Порядок middleware

```text
TLS reverse proxy
  -> request-id / tracing / body-size limit
  -> configured CORS allowlist
  -> CSRF protection for cookie-authenticated unsafe methods
  -> rate limit
  -> authentication
  -> principal extraction
  -> route-level scope extraction
  -> authorization precheck
  -> handler -> application resource authorization
  -> audit/response telemetry
```

Правила:

- `Authorization: Bearer` принимается для API token или short-lived access token, формат определяется по issuer/type, а не эвристикой роли;
- browser refresh token хранится только в `Secure`, `HttpOnly`, `SameSite=Lax` cookie;
- unsafe browser requests требуют CSRF token или origin validation;
- access JWT содержит только subject ID, token version, session ID, issued/expiry и минимальные claims; membership/role не кэшируются в долгоживущем JWT;
- проверка `enabled`, session revocation, token revocation и membership выполняется server-side;
- CORS origin allowlist задаётся `CICD_CORS_ALLOWED_ORIGINS`; wildcard запрещён, пустое значение допустимо только для isolated dev;
- request body limits отдельно на login, JSON, logs и artifact upload.

## API boundaries

### Public browser and automation API

| Endpoint group | Auth | Минимальная permission |
|---|---|---|
| `POST /auth/login`, `/auth/refresh`, `/auth/logout` | public / refresh session | rate-limited |
| `GET /tenants`, `GET /tenants/{id}` | user/token | active tenant membership |
| `POST /tenants/{id}/members` | user/token | `tenant.member.manage` |
| `GET /projects` | user/token | filter by `project.read` |
| `POST /projects` | user/token | `tenant.project.create` |
| `GET/PATCH/DELETE /projects/{id}` | user/token | `project.read` / `project.update` / `project.delete` |
| `/projects/{id}/members` | user/token | `member.manage` |
| `/projects/{id}/pipelines` | user/token | `pipeline.read` / `pipeline.trigger` |
| `/pipelines/{id}/cancel`, `/retry` | user/token | соответствующая pipeline permission |
| `/projects/{id}/secrets` | user/token | `secret.read_metadata` / `secret.manage` |
| `/artifacts/{id}/download` | user/token | `artifact.download` по project job |
| `/api-tokens` | user/token | token owner либо tenant/project admin |
| `/audit-events` | user/token | `audit.read_project`, `audit.read_tenant` или `audit.read_instance` |

`GET /api-tokens` никогда не возвращает hash, raw value, scopes других tenant или private metadata. Token creation может быть ограничен policy: user token только для самого себя, service token — только owner/tenant admin.

### Runner API

Runner API отделяется namespace и auth scheme:

```text
POST /api/v1/runner/register
POST /api/v1/runner/heartbeat
POST /api/v1/runner/jobs/claim
POST /api/v1/runner/jobs/{job_id}/lease/renew
GET  /api/v1/runner/jobs/{job_id}/manifest
POST /api/v1/runner/jobs/{job_id}/logs
POST /api/v1/runner/jobs/{job_id}/complete
```

- `register` принимает только одноразовый registration token и public key/CSR;
- остальные endpoint требуют mTLS runner identity и действующий lease;
- job manifest содержит минимальные execution metadata, checkout reference и selected secret names, но не произвольный project configuration;
- endpoint secret bundle доступен только после `claim`, только для `job_id` из lease, только до expiry lease;
- lease не может быть перевыпущен другому runner без явного revoke/timeout и audit reconciliation;
- runner не может вызывать user/project/admin API, list users, list tokens, list secrets или audit export.

### Git и internal events

Git Smart HTTP перестаёт использовать общий instance token как основной production credential. Доступ к repository вычисляется через привязку repository -> project -> tenant и principal.

- Git client аутентифицируется user token либо ограниченным Git deploy token;
- read/write Git scopes отделены (`repository.read`, `repository.write`);
- `post-receive` отправляет подписанное событие с timestamp, event ID и HMAC/mTLS;
- internal endpoint защищён от replay через срок валидности и unique event ID;
- hook не принимает actor identity из request body;
- создание pipeline из push сохраняет initiating principal или `system:git-hook` с исходным authenticated Git principal в audit metadata.

## Ключевые потоки

### Вход пользователя и browser session

1. User отправляет login credential на rate-limited `/auth/login`.
2. Application проверяет `enabled`, Argon2id hash и login policy.
3. Server создаёт короткий access token и refresh session с rotation family.
4. Access token возвращается в response body/память SPA; refresh token — только HttpOnly cookie.
5. Login success/failure создаёт audit event без credential и без полного IP.
6. При refresh старый refresh session помечается replaced/revoked; повторное использование старого token отзывает всю token family.
7. Logout, disable user, password reset или increment `token_version` инвалидируют соответствующие sessions.

### Действие в проекте

1. Client вызывает `/projects/{project_id}/pipelines`.
2. Auth middleware строит `Principal`.
3. Application получает project с canonical `tenant_id`.
4. `AuthorizationService` проверяет effective `pipeline.trigger`.
5. Use-case создаёт pipeline в tenant scope.
6. В той же transaction создаётся `pipeline.triggered` audit event с actor, project, request ID и git ref.
7. Response не раскрывает ресурсы из другого tenant.

### Создание и использование API token

1. User или tenant admin создаёт token с явными scopes, project bindings и `expires_at`.
2. Server генерирует raw token, хранит prefix и HMAC hash; raw token показывается один раз.
3. На запросе middleware извлекает prefix, загружает active token, проверяет hash constant-time, expiry/revocation и owner state.
4. Principal получает только пересечение token scopes и текущих permissions owner/service account.
5. `last_used_at` обновляется асинхронно с bounded batching, не ослабляя проверку.
6. Revoke, expiry, owner disable, membership removal или rotation key version блокируют последующие запросы.
7. Создание, use anomaly и revoke пишутся в audit log.

### Регистрация runner и выполнение job

1. Tenant admin создаёт одноразовый registration token для определённого runner pool.
2. Runner отправляет token и CSR/public key; server проверяет expiry/one-time use.
3. Server создаёт `RunnerId`, certificate/credential и audit event `runner.registered`.
4. Runner heartbeats подтверждают identity, capabilities и clock-safe timestamp.
5. Runner claim запрашивает только совместимый job; application атомарно создаёт `job_lease`.
6. Runner получает job manifest, checkout commit SHA и selected secret references.
7. Runner получает secret bundle только по действующему lease, injects values в ephemeral process/container environment.
8. Runner отправляет redacted logs, completion и attestation о lease; сервер сверяет owner, status transition и lease.
9. Expired/offline runner приводит к lease reconciliation, безопасной повторной постановке либо failure policy, но не к повторному раскрытию секрета без нового lease.

### Создание и инжекция секрета

1. `project_owner` или `maintainer` создаёт/rotates secret после `secret.manage` check.
2. Application шифрует value envelope scheme с project-bound encryption context.
3. API возвращает только metadata.
4. При старте job `PrepareJobSecrets` определяет список секретов из approved CI config и environment policy.
5. Для каждого секрета повторно проверяются tenant/project/job/runner lease.
6. Plaintext существует только в process memory control-plane и runner в пределах короткой операции; не пишется в DB, cache, audit, tracing или error response.
7. Runner запускает job с минимальным набором env variables; workspace, container и process cleanup обязательны.
8. Log ingestion применяет masking до записи в `job_logs`.

Masking является дополнительной защитой, а не гарантией от намеренного exfiltration. Secret можно преобразовать, закодировать или отправить по сети. Для high-risk secrets дополнительно требуются isolated runner pool, egress policy, protected branches/environments и approval gate.

### Чтение аудита

1. Caller аутентифицируется как user/API token.
2. Application проверяет `audit.read_project`, `audit.read_tenant` или `audit.read_instance`.
3. Query всегда ограничивается доступными tenant/project; instance audit не виден tenant admin.
4. Pagination использует stable cursor `(occurred_at, id)`, а не unbounded `LIMIT 200`.
5. Export формируется асинхронно, шифруется для authorised recipient и фиксируется event `audit.exported`.
6. Audit endpoint не возвращает secret values, token hashes, raw IP, session IDs или protected internal metadata.

## Secrets и logs

Целевые инварианты:

- пользователь и API token могут создавать, rotate и удалять secret только при `secret.manage`;
- user/API token никогда не может расшифровать secret через HTTP;
- runner получает только secrets, явно перечисленные для leased job;
- no secret by default: отсутствие явного списка в CI configuration означает отсутствие инжекции;
- секреты не передаются в command-line arguments, URL, image labels, artifact names или Docker inspect metadata;
- stdout/stderr проходят masking до persistence; raw stream не пишется в отдельный debug sink;
- secret value длиной менее безопасного порога не маскируется как отдельная строка без дополнительной policy, чтобы избежать широкого accidental redaction;
- artifact upload/download не должен включать secret bundle, runner credential или workspace metadata;
- rotation создаёт новую secret version; новые jobs используют новую version, already leased jobs используют зафиксированную version до completion;
- master key rotation выполняется как resumable re-encryption job с audit trail, rollback checkpoint и запретом на частичную silent migration.

## Threat model

| Угроза | Контроль | Остаточный риск |
|---|---|---|
| Неавторизованный доступ к открытому API | deny-by-default router, auth middleware, network policy, TLS | Ошибка route classification; покрывается route inventory tests |
| Cross-tenant IDOR | canonical resource scope, application authorization, repository predicates, 404 for foreign resources | Ошибки новых endpoint; предотвращаются contract tests для каждой resource family |
| Захваченный browser token | короткий TTL, refresh rotation, HttpOnly cookie, CSRF, revoke/version checks | Компрометация активной browser session до expiry |
| Утечка API token | 256-bit random secret, HMAC hash, scope/project binding, expiry/revoke, one-time display | Token может быть использован до revoke; необходимы detection alerts |
| Token escalation | scopes пересекаются с membership permissions; owner disable/revoke проверяется на каждом запросе | Ошибки policy implementation |
| Brute-force login | per-IP + per-account rate limit, progressive delay, audit, optional MFA | Distributed attack и legitimate shared IP |
| Подмена runner | one-time registration, mTLS/public-key binding, runner credential version, lease validation | Компрометация host с действующим certificate |
| Runner забирает чужой job/secret | atomic lease, job-project binding, secret endpoint bound to lease | Compromised valid runner может раскрыть свои разрешённые job secrets |
| Secret leakage в logs | redaction before persistence, no plaintext tracing, secure error handling | Encoded/transformed leakage требует runner isolation и egress policy |
| Подмена ciphertext | AEAD + tenant/project/secret/version encryption context | Компрометация master key |
| Replay internal Git event | signed request, timestamp window, event ID deduplication | Compromise internal signing key |
| Audit tampering | append-only permissions, transactionally written events, hash chain/checkpoints, restricted export | DBA-level compromise требует external archival/checkpoint storage |
| Privileged abuse | break-glass workflow, reason, approval/expiry, immutable audit, alerting | Instance admin remains trusted operational role |
| DoS | body limits, rate limits, bounded pagination, queue/lease limits | Volumetric network attack outside app boundary |
| SQL injection | SQLx bind parameters, static identifiers, reviewed QueryBuilder allowlists | Logic bugs in dynamic query construction |
| CORS/CSRF abuse | origin allowlist, credentialed CORS only for known UI origin, CSRF/origin checks | Misconfigured reverse proxy/origin settings |

## Миграция из текущего MVP

1. Принять текущие `backend/migrations/*.sql` как MVP baseline; старые pre-migration базы проходят verified legacy adoption по `docs/STORAGE_ARCHITECTURE.md`.
2. Добавить `tenants`, `tenant_memberships` и target service-token tables; существующие MVP `project_memberships` расширить до tenant-aware модели без немедленного включения tenant enforcement.
3. Создать единственный bootstrap tenant для существующих данных.
4. Привязать все текущие `projects` к bootstrap tenant; проверить отсутствие конфликтов перед заменой global unique name на `(tenant_id, name)`.
5. Мигрировать существующих users в tenant membership. Для каждого tenant должен существовать хотя бы один owner; нельзя оставить tenant без owner.
6. Добавить `tenant_id` или project-derived scope к runners, audit events, repository mapping и platform resources.
7. Расширить текущие `/auth/*`, session и PAT controls до tenant/project-aware модели; сначала включить shadow authorization mode: решение логируется как `would_allow`/`would_deny`, но ещё не блокирует действие.
8. Перенести project/pipeline/job вертикаль в `app`/`infra`, включить enforcement для новых и migrated routes.
9. Перевести secrets на application use-case и target encryption model до runner secret injection.
10. Включить API token middleware, ограниченный compatibility window для legacy tokens и метрики их использования.
11. Использовать `forge-runner`/external runner boundary вместо embedded execution; production server больше не запускает arbitrary job execution.
12. Требовать непустой CORS allowlist для shared/prod, удалить global Git token fallback и legacy unauthenticated routes.

Миграция не должна автоматически делать всех существующих `maintainer` instance-admin. Bootstrap access выдаётся минимальному явно указанному owner из deployment configuration и требует первого login/password reset.

## ADR

Следующие решения должны быть зафиксированы до или вместе с соответствующей реализацией:

- [ADR-0001: Rust + Axum + SQLx](adr/0001-rust-axum-sqlx.md) — сохраняется как технологическая база.
- [ADR-0004: Только PostgreSQL для постоянных данных](adr/0004-postgresql-only.md) — применяется к membership, token state, leases и audit.
- [ADR-0005: Cargo workspace и слоистая архитектура](adr/0005-workspace-layered-architecture.md) — определяет dependency boundaries.
- `ADR-0010 (reserved): Production auth, tenancy, service accounts and session hardening` — principal types, scopes, role mapping, session/token revocation.
- `ADR-0011 (reserved): Envelope encryption and project-secret access` — key provider, encryption context, rotation и secret injection.
- `ADR-0012 (reserved): Audit authorization, integrity and retention` — transaction semantics, metadata allowlist, hash checkpoints и export policy.
- `ADR-0013 (reserved): Git repository authorization and signed internal events` — replacement общего `CICD_GIT_TOKEN` в production.
- `ADR-0014 (reserved): Production runner pools, protected tags, fairness and sandbox backend` — регистрация, certificate lifecycle, claim/reconciliation и sandbox boundary.

## Тестовая стратегия

### Domain unit tests

- effective permissions для tenant role, project role, owner/maintainer/developer/reporter/viewer;
- disabled user, expired token, revoked token, revoked session и membership removal;
- token scope intersection не расширяет права owner;
- tenant/project mismatch всегда deny;
- transition `job lease`: issue, acknowledge, renew, expire, revoke, complete;
- selection secrets: отсутствующий allowlist не injects secret;
- safe audit metadata validator rejects protected fields.

### Application tests с fake ports

- use-case вызывает authorization до mutation/decrypt;
- secret mutation и audit event атомарны;
- audit write failure отменяет secret/token/membership mutation;
- project membership нельзя назначить вне tenant;
- service token не может получить project scope другого tenant;
- runner не может получить manifest/secret после expiry/revoke lease;
- idempotency Git event и lease completion.

### PostgreSQL integration tests

- каждая query resource family ограничена tenant/project;
- запрос resource ID другого tenant возвращает absence/404 без data leakage;
- foreign key и uniqueness constraints membership/token/secret version;
- concurrent `claim` выдаёт job ровно одному runner;
- concurrent token revoke/use и refresh token reuse корректно инвалидируют credential;
- миграция текущей baseline schema создаёт bootstrap tenant и сохраняет данные;
- migration rollback/recovery проверяется на копии production-like DB;
- audit hash chain и pagination cursor проверяются после параллельных событий.

### API security contract tests

Минимальный набор для каждого защищённого endpoint:

- no credential -> `401`;
- invalid/expired/revoked credential -> `401`;
- valid principal without permission -> `403`;
- foreign tenant resource -> `404`;
- allowed principal -> ожидаемый success;
- list endpoint не содержит foreign resources;
- request body не может подменить tenant/project scope;
- error response не возвращает SQL error, hash, token, ciphertext или plaintext secret;
- OPTIONS/CORS принимает только configured origin и headers;
- unsafe cookie request без CSRF/origin proof отклоняется.

Route inventory test обязан сверять все `/api/v1/**` routes с declared access policy. Добавление route без policy должно падать в CI.

### Runner и secrets tests

- registration token одноразовый и expires;
- mTLS/certificate credential другого runner не принимает lease;
- claim не может быть повторён вторым runner;
- lease renewal после expiry не работает;
- runner получает только job-selected secret keys;
- project A secret не доступен job/runner проекта B;
- decrypted value отсутствует в API response, audit event, tracing capture и persisted log;
- exact secret occurrence маскируется до `job_logs`;
- rotation применяет новую version только к новым job lease;
- e2e runner выполняет isolated test image без Docker socket и без outbound network по default policy.

### Browser и E2E tests

- login, refresh, logout, forced logout после disable;
- tenant switch без data leakage;
- project invitation/membership removal и immediate access loss;
- token creation: one-time display, revocation, expiry;
- forbidden secrets UI не показывает даже metadata;
- maintainer создаёт secret metadata, developer получает `403`;
- project audit screen фильтрует только allowed scope;
- responsive access-control UI в viewport `375x812`, `1920x1080`, `2560x1440`.

### Security automation

В CI добавляются:

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --release --workspace`;
- real PostgreSQL migration and integration job;
- dependency vulnerability scan;
- secret scanning в diff и repository history policy;
- OpenAPI route-policy check;
- container image scan;
- negative authorization regression suite;
- periodic restore test audit/DB backup and key-rotation rehearsal.

## Поэтапная поставка

### Phase A — Foundation

- Versioned migrations, typed `AuthConfig`, `AppError`, principal/domain types.
- Baseline tenant migration и explicit bootstrap owner procedure.
- `users` расширяются password/session state, но enforcement ещё feature-flagged.
- Реальные PostgreSQL tests вместо no-DB tests для persistent authorization paths.

**Gate:** migration на чистой и заполненной baseline DB проходит; старые API contracts не меняются без versioning plan.

### Phase B — Human authentication и tenancy

- Argon2id, access/refresh session rotation, browser refresh cookie + CSRF, login rate limiting и configurable CORS allowlist.
- Tenant/project memberships и application `AuthorizationService`.
- Project, pipeline, job, artifact и repository routes переводятся на deny-by-default.
- Dashboard protected routes и tenant context.

**Gate:** cross-tenant negative suite, login/logout/refresh E2E и all protected route inventory green.

### Phase C — API tokens и Git authorization

- Scoped user/service API tokens, expiry/revoke, token middleware и CLI authentication.
- Project-bound Git authorization, deploy tokens и signed internal hook events.
- Legacy `api_tokens` compatibility monitoring и retirement.

**Gate:** raw token хранится только у клиента; revoked/expired token не проходит ни на одном route; Git read/write scopes проверены E2E.

### Phase D — Secrets и runner identity

- Envelope encryption/key version, secret audit and rotation.
- External runner registration, mTLS credential, job leases, claim/reconciliation.
- Secret injection только в leased execution, log masking и runner isolation.
- Embedded runner остаётся development-only либо отключается/удаляется из production server; current `forge-runner` shell MVP дорабатывается до sandboxed runner boundary.

**Gate:** runner cannot access arbitrary project/job/secret; integration and isolated runner E2E green; plaintext secret absence verified in DB/logs/API/audit.

### Phase E — Audit integrity и operations

- Scoped audit reader/export, immutable retention policy, hash checkpoints и external archive.
- Break-glass workflow, security alerts, key/certificate/token rotation runbooks.
- Metrics: auth failure, denied authorization, token revoke, runner lease expiry, secret access and audit write failure.

**Gate:** restore drill, key rotation drill, audit export authorization tests и incident runbook review completed.

## Definition of Done

Функциональность считается завершённой, только когда:

- все public, Git, internal и runner routes имеют явно объявленную auth/access policy;
- authorization выполняется application layer, а не только middleware;
- every tenant-owned query имеет tenant/project scoping;
- normal user/API requests не способны прочитать plaintext secret;
- токены, sessions, runner credentials и leases поддерживают expiry и revoke;
- sensitive mutation имеет transactionally persisted audit event;
- cross-tenant, expired credential, revoked credential и runner lease abuse покрыты автоматическими negative tests;
- документация `API.md`, `DATA_MODEL.md`, `SECURITY.md`, `contracts/DATA_LIFECYCLE.md`, `GIT_HOSTING.md`, `DEVELOPMENT_GUIDE.md`, `ARCHITECTURE.md` и runbooks синхронизирована с реализацией;
- production deployment использует TLS, непустой `CICD_CORS_ALLOWED_ORIGINS`, configured secrets/key provider и не экспонирует PostgreSQL или unauthenticated control-plane API.
