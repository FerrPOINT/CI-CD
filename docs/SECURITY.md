# Security — Forge CI/CD

## 1. Overview

Forge CI/CD — self-hosted CI/CD control plane. Текущая версия остаётся MVP и не является production-safe: без непустого `CICD_AUTH_SECRET` backend работает в trusted-network режиме; при включённом секрете уже применяются route roles, project memberships, session-bound access JWT и refresh rotate/logout/revoke, но tenant isolation, scoped PAT и production cookie/CSRF/session-family policy остаются target. Безопасность встроена на уровне SQL-запросов, валидации ввода, секретов at rest, условного auth middleware и audit.

## 2. Текущий статус

| Механизм | Статус | Описание |
|----------|--------|----------|
| Auth (login/JWT) | ✅ conditional | Включается при непустом `CICD_AUTH_SECRET`; access JWT привязан к `sessions.id`, без секрета API остаётся open trusted-network |
| RBAC | ✅ MVP | Глобальные роли `admin`/`maintainer`/`developer`/`viewer` + `project_memberships`; tenant scope и scoped tokens — target |
| Secrets management | ✅ MVP | AES-256-GCM at rest, `CICD_SECRETS_KEY`; env injection в embedded runner + masking stdout |
| API tokens | ✅ conditional | PAT `cicd_...` проверяется middleware при непустом `CICD_AUTH_SECRET`; legacy SHA-256 hash без target scopes/pepper |
| Users & roles | ✅ MVP | Users, argon2id credentials, sessions, enabled flag |
| Audit log | ✅ MVP | Последние 200 событий |
| Artifacts storage | ✅ MVP | Локальная ФС, 50 MiB лимит |
| SQL injection prevention | ✅ реализовано | parameterized queries через SQLx |
| Input validation | ✅ частично | проверка `trim().is_empty()` на входе |
| CORS | ⚠️ permissive | `CorsLayer::permissive()` — ограничить в production |
| Rate limiting | ✅ MVP | in-process fixed-window для auth, API, Git Smart HTTP, internal hook и artifact upload; distributed/proxy policy — target |
| HTTPS/TLS | ❌ нет | через reverse proxy (nginx/Caddy) |

## 3. Authentication

### 3.1 JWT

- Current: access token — JWT HS256, срок жизни 15 минут, подпись через `CICD_AUTH_SECRET`, привязка к `sessions.id`; middleware проверяет активную session, enabled user и текущую роль из БД на protected routes.
- Current: refresh token хранится в таблице `sessions`, возвращается клиенту, обновляется через `/api/v1/auth/refresh` и отзывается через `/api/v1/auth/logout`; logout/rotate немедленно инвалидируют session-bound access JWT; frontend MVP держит refresh token в `localStorage`.
- Current: пароли хранятся как `argon2id` hash в `user_credentials`.
- Target: httpOnly/SameSite cookie для refresh, session-family reuse detection, CSRF policy, key management и bootstrap owner procedure.

### 3.2 Эндпоинты

```
POST /api/v1/auth/login     — вход, выдача access + refresh
POST /api/v1/auth/refresh   — обновление access-токена
POST /api/v1/auth/logout    — выход, отзыв refresh session
```

### 3.3 Login lockout

- Current: in-process per-client limit 30 login attempts/minute.
- Current: login/denied события пишутся в audit.
- Target: persistent per-IP + per-user lockout, alerting и admin unlock flow.

### 3.4 MFA

- TOTP (RFC 6238) — future, интерфейс заложить в схему `users` при реализации.

## 4. Authorization

### 4.1 RBAC

Current middleware проверяет `Authorization: Bearer ...` при непустом `CICD_AUTH_SECRET`. Если секрет не задан или пустой, запросы пропускаются без principal. Глобальная роль задаёт максимум прав:

| Role | Permissions |
|------|-------------|
| `admin` | users/tokens и все mutation routes |
| `maintainer` | управление проектами, pipelines, platform resources в рамках project membership |
| `developer` | запуск/повтор jobs/pipelines и чтение ресурсов |
| `viewer` | только чтение большинства API |

- Project-owned routes дополнительно требуют `project_memberships`; `admin` имеет instance-wide bypass. Tenant boundary, scoped PAT, repository-level permissions и Git policy checks — **Target approved**.
- `/git/*` использует отдельный `CICD_GIT_TOKEN` и не опирается на JWT/PAT.

### 4.2 Middleware

```rust
// Current shape: simplified excerpt.
async fn require_auth(req: Request, next: Next) -> Result<Response, ApiError> {
    if state.auth_secret.is_none() {
        return Ok(next.run(req).await); // trusted-network mode
    }
    // verify JWT/PAT, active session, enabled user, route role policy and project membership
}
```

## 5. Secrets Management (Phase 7 — план)

### 5.1 Текущее состояние

Project secrets хранятся в `project_secrets`, encrypted at rest в формате `v1:nonce:payload`, ключ — `CICD_SECRETS_KEY` (base64 32 bytes). API/UI возвращают только metadata. Embedded runner расшифровывает секреты проекта перед выполнением job, добавляет их в env и маскирует значения в stdout/stderr logs.

### 5.2 Планируемое

- Scoped secret selection в DSL, environment/project policy, rotation и audit trail.
- Least-privilege runner lease вместо передачи всех project secrets.
- Маскирование stderr/stdout уже есть как best-effort, но target требует edge-case suite, redaction в audit/error/trace и запрет secret-like output.
- Интеграция с внешними vault (HashiCorp Vault, AWS Secrets Manager) — future.

### 5.3 Правила

- Никогда не коммитить credentials, токены, пароли.
- Runtime/config secrets — через env vars или secret manager; project secrets — через API/UI с `CICD_SECRETS_KEY`.
- `.env.example` содержит только плейсхолдеры.
- Перед push проверять, что в diff нет чувствительных данных.
- Ротация JWT/refresh секретов периодически.

## 6. Input Validation

### 6.1 Backend

- Все входящие DTO проверяются на пустоту (`trim().is_empty()`).
- `name` и `repository_url` — обязательные поля при создании проекта.
- `message` — обязательное поле при добавлении лога.
- UUID-параметры парсятся Axum (`Path<Uuid>`) — неверный формат = 400 Bad Request.
- Статусы задач проходят `serde`-десериализацию `JobStatus` enum — неверное значение = 400 Bad Request.
- Transition-правила валидируются доменно (`JobStatus::transition_to`).

### 6.2 Планируемое

- Полная DTO-валидация через `validator` crate (email format, URL format, length limits).
- Whitelist mime-types для загружаемых артефактов.
- Filename sanitization.
- Limit на размер лог-сообщения и количество строк.

### 6.3 Frontend

- Валидация форм через `zod` (планируется при реализации форм создания).
- Типобезопасные API DTO генерируются из `openapi/openapi.yaml` в `frontend/src/api/schema.d.ts`; handwritten API wrapper отвечает за headers, errors, binary upload/download и SSE.

## 7. SQL Injection Prevention

### 7.1 SQLx Parameterized Queries

Все SQL-запросы используют parameterized queries через `sqlx::query` с плейсхолдерами `$1`, `$2`, ...:

```rust
// ✅ Правильно — parameterized query
sqlx::query_as::<_, Project>(
    "INSERT INTO projects (id, name, repository_url, default_branch) \
     VALUES ($1, $2, $3, $4) \
     RETURNING id, name, repository_url, default_branch, created_at"
)
.bind(Uuid::new_v4())
.bind(input.name.trim())
.bind(input.repository_url.trim())
.bind(input.default_branch.unwrap_or_else(|| "main".into()))
.fetch_one(pool)
.await?
```

```rust
// ❌ Запрещено — string interpolation
let sql = format!("SELECT * FROM projects WHERE name = '{}'", input.name);
```

### 7.2 Правила

- Никогда не использовать `format!` для построения SQL-запросов с пользовательским вводом.
- Все значения передаются через `.bind()`.
- Имена таблиц и колонок — статические строки в коде, не из пользовательского ввода.
- `sqlx::raw_sql` используется только для статической DDL (миграции) — без пользовательского ввода.
- Compile-time проверка запросов (при включении `sqlx::macros`) — дополнительная защита.

### 7.3 Динамические запросы

При появлении динамических фильтров (search, sort) — использовать `sqlx::QueryBuilder`:

```rust
let mut query = sqlx::QueryBuilder::new("SELECT * FROM pipelines WHERE 1=1");
if let Some(status) = status_filter {
    query.push(" AND status = ").push_bind(status);
}
```

## 8. Transport

### 8.1 Текущее состояние

- HTTP без TLS (разработка).
- Reverse proxy (nginx / Caddy / Traefik) — для HTTPS в production (см. `docs/DEPLOYMENT.md`).

### 8.2 Планируемое (production)

- HTTPS/TLS everywhere.
- HSTS header.
- Secure, SameSite=Lax/Strict, httpOnly cookies (для refresh token).
- No sensitive data в URL query params.

## 9. CORS

### 9.1 Текущее состояние

```rust
// api.rs
.layer(CorsLayer::permissive())
```

Permissive CORS — для разработки. **Ограничить в production.**

### 9.2 Планируемое (production)

```rust
let cors = CorsLayer::new()
    .allow_origin("https://cicd.example.com".parse::<HeaderValue>()?)
    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
    .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
    .allow_credentials(true);
```

Конфигурация через `CICD_CORS_ALLOWED_ORIGINS` env var. No wildcard (`*`) в production.

## 10. Rate Limiting

- Current: in-process fixed-window middleware ограничивает auth, API read/write, Git Smart HTTP, internal Git hook и artifact upload; key берётся из `X-Forwarded-For`, `X-Real-IP` или `unknown`.
- Target: trusted reverse-proxy limiter, distributed counters, per-account lockout для auth endpoints, request body/time/concurrency policy и alerting.

| Endpoint | Limit |
|----------|-------------|
| Login | current 30/min per client |
| Refresh | current 120/min per client |
| Internal Git hook | current 120/min per client |
| Git Smart HTTP | current 240/min per client |
| Artifact upload | current 60/min per client |
| API general | current 1200/min read, 600/min write per client |

## 11. Dependency Security

- `cargo audit` в CI (GitHub Actions).
- Dependabot / Renovate alerts.
- Pin major версии.
- Регулярное обновление зависимостей.

## 12. Container Security

### 12.1 Backend Dockerfile

- Multi-stage build: `rust:1.86-slim` → `debian:bookworm-slim`.
- Non-root user: `uid 10001`.
- Минимальный финальный образ.

### 12.2 Frontend Dockerfile

- Multi-stage build: `node:22-bookworm-slim` → `nginx:1.27-alpine`.
- Статические файлы, без runtime Node.js.

### 12.3 PostgreSQL

- Официальный `postgres:17.6-alpine`.
- Порт не экспонировать в production (internal network only).

### 12.4 Рекомендации

- Read-only filesystem где возможно.
- Scan images with Trivy.
- No secrets в image layers.

## 13. Audit Logging

- Current: `audit_log` append-only table, API возвращает последние 200 событий.
- Current: login, denied request, runner, secret, artifact, environment, deployment, schedule, webhook, notification, user/token и часть pipeline/job mutations пишут события.
- Target: immutable authorisation context, filters/pagination/export, retention policy и alerting.

## References

- `docs/ROADMAP.md` — фазы внедрения security-функций.
- `docs/ARCHITECTURE.md` — архитектура backend.
- `docs/DEPLOYMENT.md` — production deployment с reverse proxy.
- `docs/CODE_STYLE.md` — правила работы с секретами.
