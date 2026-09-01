# Спецификация реализации runner, scheduler и outbox

Нормативное дополнение к `RUNNER_ARCHITECTURE.md` и `AUTOMATION_ARCHITECTURE.md`.

## 1. Первая поставка и границы

Этот документ описывает target-срез external runner/production scheduler/outbox. Current MVP уже имеет embedded runner, runner protocol register/heartbeat/immediate poll/ack/renew/control/`secrets:resolve`/artifact upload/logs/complete, basic tag + current executor capability matching, `forge-runner` shell process, scheduler/outgoing webhook/local notification worker, bounded `outbox_delivery_attempts` history и explicit requeue failed delivery.

Уже реализовано в runner protocol MVP: env bootstrap registration token, heartbeat, durable `job_queue` claim, basic tag + current executor capability matching, immediate `work:poll`, `workspace.checkoutUrl`, ack/renew/control/`secrets:resolve`/artifact upload/logs/complete, attempt/lease/log writes, fencing generation и отдельный shell runner binary. В target external-runner поставку остаются Docker-only/sandboxed runner, long-poll/wakeup, credential lifecycle, advanced pool/protected-tag/capability policy, lost-runner reconciliation и production sandbox boundary.

Исключено из этого target-среза: Kubernetes, external-runner secret injection, artifact streaming, email/Slack adapters и inbound provider webhooks. Они остаются feature-flagged до отдельного contract PR.

## 2. Фиксированные параметры

| Parameter | Default | Valid range |
|---|---:|---:|
| registration token TTL | 1 hour | 5m..24h |
| runner heartbeat interval | 15s | 5s..60s |
| unhealthy/offline threshold | 45s / 120s | 3x / 8x heartbeat |
| poll wait | 0s current / 25s target | 0..30s |
| offer ACK TTL | 30s | 10..120s |
| lease TTL | 120s | 30..600s |
| renewal interval | 40s | `< lease TTL / 2` |
| execution timeout | 1h | 1m..24h |
| retry | base 15s, cap 1h, max 5 | full jitter |
| outbox claim | 50 messages / 30s | — |
| scheduler tick | 15s | 5s..60s |

All values are copied to attempt/lease snapshot at creation. Config names start `CICD_RUNNER_`, `CICD_OUTBOX_`, `CICD_SCHEDULER_`; feature flags default false in production.

## 3. Canonical data model

Required versioned migrations create these tables (UUID id unless noted):

| Table | Required fields and invariants |
|---|---|
| `runner_registration_tokens` | `token_hmac`, `key_id`, `tenant_id`, `runner_pool_id?`, `expires_at`, `max_uses`, `uses`, `revoked_at`; unique token_hmac; `uses < max_uses` checked in atomic UPDATE |
| `runner_pools` | tenant ownership, tags, enabled |
| `runner_credentials` | runner_id, hash/key id, issued/expiry/revoked; one active partial unique index |
| `runners` | pool_id, status, capabilities JSONB, last_heartbeat_at, drain_requested_at; current legacy registry migrated here |
| `execution_attempts` | job_id, number, status, queued_at/started_at/finished_at, config snapshot, retry_of; unique `(job_id,number)` |
| `job_queue` | Current: attempt_id unique, open-job uniqueness, priority, not_before, required_tags, state, lease_id, queued/leased/completed timestamps; partial index ready rows `(priority DESC, not_before, queued_at, id)` where state='queued' |
| `job_leases` | attempt_id, runner_id, generation, status, offered/ack/expires timestamps, lease_token_hmac; unique `(attempt_id,generation)` and partial unique active lease per attempt |
| `domain_events` | aggregate_type/id, event_type, version, payload JSONB, occurred_at, causation/idempotency key |
| `outbox_messages` | event_id unique, topic, payload JSONB, state, attempts, next_attempt_at, worker_generation, lease_expires_at, last_error |
| `outbox_delivery_attempts` | current bounded history: message_id, attempt_number, started/finished, outcome, http_status?, safe error class, duration; unique `(message_id,attempt_number)` |
| `outbox_deliveries` | target snapshots/leases: message_id, consumer, generation default 0, replay_of_delivery_id?, status, attempts; unique `(message_id,consumer,generation)` |
| `schedule_fires` | schedule_id, scheduled_for, pipeline_id?, status; unique `(schedule_id,scheduled_for)` |
| `idempotency_keys` | as specified by IMPLEMENTATION_CONTRACTS |

`job_leases` is historical: never `UNIQUE(job_id)`. Fencing predicate is always `WHERE id=$lease_id AND generation=$generation AND status='active' AND expires_at > now()`.

## 4. Runner protocol (all `/api/v1/runner/*`)

Runner credentials: `Authorization: Bearer <opaque>`. Registration returns raw credential once; lease token is a separate opaque secret, returned only in offer. Current MVP stores SHA-256 hashes; target hardening uses HMAC-SHA256/pepper.

| Operation | Request | Response / rules |
|---|---|---|
| `POST /register` | `{registration_token,name,tags,capabilities}` | Current MVP: 201 `{runner_id,credential,credential_expires_at}`; target: registration token atomically consumed |
| `POST /heartbeat` | `{status,capacity,running_attempt_ids}` | 204; stale credential 401 |
| `POST /work:poll` | `{capacity,tags}` | Current MVP: immediate `204` or compatible `200 LeaseOffer` with `required_tags ⊆ runner.tags` and current `shell` executor compatibility; target: long-poll with signed plan |
| `POST /leases/{id}/ack` | `{generation,lease_token}` | 200 `{expires_at,renew_after}`; expired/fenced 409 `lease_fenced` |
| `POST /leases/{id}/renew` | `{generation,lease_token}` | 200 `{expires_at,cancel_requested}` |
| `POST /attempts/{id}/logs` | `{generation,lease_token,sequence,stream,message_sha256,chunk}` | 202; unique `(attempt,sequence,sha256)` makes retry idempotent |
| `POST /attempts/{id}/complete` | `{generation,lease_token,outcome,exit_code,finished_at}` | 200 terminal attempt; cancel wins if accepted before completion transaction |
| `GET /attempts/{id}/control` | lease auth | `{cancel_requested}` |

`plan` includes immutable commit SHA, image, command, env without secret values, timeout and workspace settings. It is HMAC-SHA256 signed with `kid/signature`; runner verifies signature before execution. Server validates every mutation with runner credential + lease token + generation.

## 5. State matrices

| Entity | Transition | Preconditions | Durable writes/event |
|---|---|---|---|
| Queue | queued → offered | compatible healthy runner, atomic SKIP LOCKED claim | lease offered; `forge.lease.offered.v1` |
| Lease | offered → active | ack before deadline, token/generation valid | attempt running; `forge.lease.acked.v1` |
| Lease | active → expired | now > expires_at | attempt retry_wait; new attempt later |
| Attempt | queued → running | active lease | started_at |
| Attempt | running → success/failed/canceled | fenced complete; cancel has precedence | terminal timestamps; `forge.job.completed.v1` |
| Outbox | pending → processing → delivered/dead | claim CAS with generation | retry or delivery event |
| Schedule fire | due → triggered/skipped | `(schedule_id,scheduled_for)` insert succeeds | pipeline trigger event |

Every retry creates `execution_attempts.number + 1`; no `retry_wait → leasing` within same attempt.

## 6. Scheduler/outbox algorithms

Current MVP uses the local strict 5-field UTC parser in `backend/src/schedule.rs`; PostgreSQL `schedule_fires` is authoritative. Target IANA timezone/DST support may use a dedicated cron crate, but PostgreSQL still owns claim/dedup. UTC due slots are computed deterministically; target DST ambiguous time fires once at earliest UTC instant, nonexistent time is skipped and audited. Target missed policy is `fire_once`: one latest due slot per schedule per tick, cap 100 schedules/tick.

A Postgres advisory lock (`forge_scheduler_leader`) elects one scheduler; loss stops claims. For each due slot transaction: insert schedule_fire → create pipeline → append domain_event/outbox message. Unique fire prevents duplicate restart/multi-instance trigger.

Outbox claim uses `FOR UPDATE SKIP LOCKED`, sets `state=processing`, `worker_generation=old+1`, lease 30s. Completion uses generation CAS. Retryable: transport timeout, 408, 429, 5xx. Permanent: validation/auth most 4xx. Backoff full jitter base 15s/cap 1h/max 5; exhausted is `dead`. Current MVP requeue creates a new `outbox_messages` generation and logs attempts in `outbox_delivery_attempts`; target webhook replay may additionally use `outbox_deliveries.generation`, never violating the original unique key.

## 7. First acceptance suite

- registration token one-use/max-use/expiry/revoke races;
- duplicate poll returns one offer, stale ack/renew/control/`secrets:resolve`/artifact upload/logs/complete fenced;
- server restart expires lease and creates exactly one next attempt;
- cancel versus complete deterministic precedence;
- scheduler DST/missed/restart/multi-instance yields exactly one schedule_fire;
- outbox crash after claim and receiver timeout produces observable retry, not duplicate final state;
- all runner/scheduler mutations use real PostgreSQL and feature flags remain false by default.
