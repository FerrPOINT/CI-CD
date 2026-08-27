# Sequence: миграция при деплое (target)

```mermaid
sequenceDiagram
    participant OP as operator/CI
    participant MG as cicd-migrate
    participant DB as PostgreSQL (forge_owner)
    participant API as cicd-server

    OP->>MG: cicd-migrate up --dry-run
    MG->>DB: SELECT applied FROM _sqlx_migrations
    MG-->>OP: план (pending migrations)
    OP->>MG: cicd-migrate up
    MG->>DB: BEGIN; apply SQL; INSERT _sqlx_migrations; COMMIT (per file)
    alt failure
        MG->>DB: ROLLBACK файла
        MG-->>OP: non-zero exit + runbook link (MIGRATION_CONTRACT.md)
    end
    OP->>API: deploy new server (readiness check)
    API->>MG: verify at startup (checksum)
    Note over MG,DB: adopt-legacy: fingerprint bootstrap-v1 → baseline 0001
```

Правила отката/forward-recovery — `contracts/MIGRATION_CONTRACT.md`.
