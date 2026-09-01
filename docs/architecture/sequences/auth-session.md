# Sequence: auth session (target)

```mermaid
sequenceDiagram
    participant U as browser
    participant API as /api/v1/auth/*
    participant DB as PostgreSQL
    participant POL as policy middleware

    U->>API: POST /login {email, password}
    API->>DB: user lookup + Argon2id verify
    API->>DB: sessions (refresh hash + csrf hash)
    API-->>U: 200 + access JWT body + refresh HttpOnly cookie + csrf cookie
    U->>API: GET /projects (Authorization: Bearer access)
    API->>POL: verify JWT (HS256 keyring, TTL, skew)
    POL->>DB: membership/role → decision
    API-->>U: 200 / 401 authentication_required / 403 permission_denied
    U->>API: POST /refresh (refresh cookie + X-CSRF-Token)
    Note over API,DB: reuse detected → family revoked
    U->>API: POST /logout (refresh cookie + X-CSRF-Token) → session revoked
```

Контракты: `contracts/AUTHZ_CONTRACT.md` + `AUTH_IMPLEMENTATION_SPEC.md`. Текущий MVP уже реализует conditional `/auth/login`, `/auth/refresh`, `/auth/logout`, browser refresh cookie + CSRF, session-bound access invalidation и project membership checks при заданном `CICD_AUTH_SECRET`; tenant isolation и refresh-family reuse policy остаются целевым расширением.
