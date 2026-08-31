# Sequence: webhook delivery (target)

```mermaid
sequenceDiagram
    participant EV as domain event
    participant OB as outbox worker
    participant WH as webhook subscription
    participant EXT as внешний endpoint
    participant DL as dead-letter

    EV->>OB: outbox_messages (webhook.pending)
    OB->>OB: claim (FOR UPDATE SKIP LOCKED), lease on outbox_deliveries
    OB->>WH: matching subscriptions (event, repo/project, active)
    OB->>EXT: POST payload + HMAC-SHA256 signature (X-Forge-Signature)
    alt 2xx
        OB->>OB: delivery=success
    else retryable
        OB->>OB: backoff (60s..6h, jitter), attempts < max
        OB->>EXT: retry (same delivery_id idempotency key)
    else exhausted / 4xx permanent
        OB->>DL: dead-letter + notification owner
    end
    OB->>OB: metrics + audit
```

Контракты: `contracts/EVENT_CONTRACT.md`. Сейчас реализован bounded MVP: terminal pipeline events создают `domain_events`/`outbox_messages`, worker делает webhook retry/HMAC, фиксирует attempts и позволяет requeue failed delivery; diagram выше описывает target-добавки leases/reconciliation/full dead-letter.
