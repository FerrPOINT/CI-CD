# Sequence: runner lease (target)

```mermaid
sequenceDiagram
    participant RN as runner (external)
    participant API as /api/v1/runner/*
    participant DB as PostgreSQL
    participant SEC as secret service

    RN->>API: POST /register (registration_token, tags, capacity)
    API->>DB: runners upsert + issue runner credential
    loop heartbeat (30s)
        RN->>API: POST /heartbeat
    end
    RN->>API: POST /work:poll (tags, capacity)
    API->>DB: job_queue claim → job_leases (lease_token, fencing, deadline)
    API-->>RN: lease offer (immutable plan, plan_hmac)
    RN->>API: POST /leases/{id}/ack
    RN->>SEC: POST /leases/{id}/secrets:resolve (lease_token)
    SEC-->>RN: secret values (memory only, маскируются в логах)
    RN->>API: POST /leases/{id}/logs (batch, sequence)
    RN->>API: POST /leases/{id}/complete {outcome}
    API->>DB: execution_attempts, statuses, outbox_messages
    Note over API,DB: renew до deadline; fencing-token защищает от zombie
```

Полностью target (ADR-0007); сейчас — embedded runner без lease. Контракты: `contracts/RUNNER_PROTOCOL.md`.
