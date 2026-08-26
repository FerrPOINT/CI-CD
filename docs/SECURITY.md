# Security — Forge CI/CD

## 1. Overview

Forge CI/CD — self-hosted CI/CD control plane. В текущей версии (Phase 0) аутентификация и авторизация не реализованы — API открыт. Безопасность встроена на уровне SQL-запросов и валидации ввода. План внедрения защиты описан в этом документе и в `docs/ROADMAP.md`.

## 2. Текущий статус

| Механизм | Статус | Описание |
|----------|--------|----------|
| Auth (login/JWT) | ❌ не реализовано | Phase 1 (план) |
| RBAC | ❌ не реализовано | Phase 1 (план) |
| Secrets management | ✅ MVP | AES-256-GCM at-rest, `CICD_SECRETS_KEY`; значения не возвращаются через API |
| API tokens | ✅ MVP (storage) | SHA-256 hash; проверка токенов при запросах не реализована |
| Users & roles | ✅ MVP (storage) | Модель без auth enforcement; пароли не хранятся |
| Audit log | ✅ MVP | Последние 200 событий |
| Artifacts storage | ✅ MVP | Локальная ФС, 50 MiB лимит |
| SQL injection prevention | ✅ реализовано | parameterized queries через SQLx |
| Input validation | ✅ частично | проверка `trim().is_empty()` на входе |
| CORS | ⚠️ permissive | `CorsLayer::permissive()` — ограничить в production |
| Rate limiting | ❌ не реализовано | Phase 1+ (план) |
| HTTPS/TLS | ❌ нет | через reverse proxy (nginx/Caddy) |

## 3. Authentication (Phase 1 — план)

### 3.1 JWT

- Access token: JWT, срок жизни 15 минут.
- Refresh token: httpOnly cookie, срок жизни 7 дней, ротация при каждом обновлении.
- Алгоритм подписи: `HS256` (или `RS256` при появлении key management).
- Секреты: `CICD_JWT_SECRET`, `CICD_REFRESH_SECRET` — минимум 32 байта, через env vars.
- Хранение паролей: `argon2id`.

### 3.2 Эндпоинты (план)

```
POST /api/v1/auth/login     — вход, выдача access + refresh
POST /api/v1/auth/logout    — выход, отзыв refresh
POST /api/v1/auth/refresh   — обновление access-токена
```

### 3.3 Login lockout

- Блокировка после 5 неудачных попыток на 15 минут (per IP + per user).
- Логирование попыток входа.

### 3.4 MFA

- TOTP (RFC 6238) — future, интерфейс заложить в схему `users` при реализации.

## 4. Authorization (Phase 1 — план)

### 4.1 RBAC

Role-based access control на уровне проекта:

| Role | Permissions |
|------|-------------|
| `admin` | всё: управление проектами, пользователями, пайплайнами |
| `maintainer` | управление проектом, запуск пайплайнов, управление задачами |
| `developer` | просмотр, запуск пайплайнов, управление задачами |
| `viewer` | только просмотр |

- Проверка прав на service layer (повторно — на repository layer).
- No data returned until permission verified.

### 4.2 Middleware

```rust
// Планируемый middleware
async fn require_auth(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Result<UserId, ApiError> {
    // извлечь JWT из Authorization header
    // проверить подпись и срок
    // вернуть user_id
}
```

## 5. Secrets Management (Phase 7 — план)

### 5.1 Текущее состояние

Все секреты передаются через env vars с префиксом `CICD_`. `.env.example` содержит только плейсхолдеры. Секреты не коммитятся в git.

### 5.2 Планируемое

- Encrypted secrets для задач (build tokens, deploy keys, API keys).
- Хранение: encrypted at rest в PostgreSQL (`cicd_secrets` таблица).
- Шифрование: AES-256-GCM, мастер-ключ через `CICD_MASTER_KEY` env var.
- Доступ: по RBAC-ролям, secrets не логируются, не возвращаются в API-ответах (только маска `***`).
- Интеграция с внешними vault (HashiCorp Vault, AWS Secrets Manager) — future.

### 5.3 Правила

- Никогда не коммитить credentials, токены, пароли.
- Все secrets — через env vars.
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
- Whitelist mime-types для загружаемых артефактов (при реализации artifacts).
- Filename sanitization.
- Limit на размер лог-сообщения и количество строк.

### 6.3 Frontend

- Валидация форм через `zod` (планируется при реализации форм создания).
- Типобезопасные API-клиенты через TypeScript-интерфейсы (`frontend/src/api/types.ts`).

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

## 10. Rate Limiting (план)

- `tower_governator` per IP and per user.
- Stricter limits для auth endpoints.

| Endpoint | Limit (план) |
|----------|-------------|
| Login | 5/min |
| API general | 100/min |
| Pipeline trigger | 10/min |

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

## 13. Audit Logging (план)

- Login/logout events.
- Pipeline trigger / cancel.
- Job status transitions.
- Project create / delete.
- Admin actions.
- Хранение: `audit_log` таблица, retention 1 год.

## References

- `docs/ROADMAP.md` — фазы внедрения security-функций.
- `docs/ARCHITECTURE.md` — архитектура backend.
- `docs/DEPLOYMENT.md` — production deployment с reverse proxy.
- `docs/CODE_STYLE.md` — правила работы с секретами.