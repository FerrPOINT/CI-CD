//! Outbox delivery worker + scheduler (ADR-0006, EVENT_CONTRACT, REQ-AUTO-003).
//!
//! - `domain_events` / `outbox_messages` tables (migration 0004).
//! - Webhook fan-out: project webhooks are translated into outbox messages on
//!   pipeline terminal transitions; the worker delivers with retry/backoff
//!   (8 attempts, 15s..1h) and dead-letters after exhaustion.
//! - In-app notification fan-out: `in_app`/`sse` notification configs are
//!   translated into durable local outbox messages for Dashboard history/SSE.
//! - Delivery history: every attempt is written to `outbox_delivery_attempts`;
//!   failed deliveries can be explicitly requeued as a new generation.
//! - Scheduler: enabled cron schedules compute `next_fire_at`, claim a unique
//!   `schedule_fires` slot, then trigger pipelines idempotently.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

pub const MAX_ATTEMPTS: i32 = 8;
pub const NOTIFICATION_CHANNEL_IN_APP: &str = "in_app";
pub const NOTIFICATION_CHANNEL_SSE: &str = "sse";
const OUTCOME_DELIVERED: &str = "delivered";
const OUTCOME_RETRY_SCHEDULED: &str = "retry_scheduled";
const OUTCOME_FAILED: &str = "failed";

pub fn notification_destination(project_id: Uuid) -> String {
    format!("project:{project_id}")
}

pub fn supported_local_notification_channel(channel: &str) -> bool {
    matches!(
        channel.trim().to_ascii_lowercase().as_str(),
        NOTIFICATION_CHANNEL_IN_APP | NOTIFICATION_CHANNEL_SSE
    )
}

/// Record a domain event + fan out to project webhooks in one transaction.
pub async fn emit_pipeline_event(
    pool: &PgPool,
    project_id: Uuid,
    pipeline_id: Uuid,
    event_type: &str,
    status: &str,
) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let event_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO domain_events (id, event_type, aggregate_type, aggregate_id, payload) \
         VALUES ($1, $2, 'pipeline', $3, $4)",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(pipeline_id)
    .bind(serde_json::json!({ "project_id": project_id, "status": status }))
    .execute(&mut *tx)
    .await?;

    let hooks = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
        "SELECT id, url, secret FROM webhooks WHERE project_id = $1 AND enabled",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    for (hook_id, url, secret) in hooks {
        sqlx::query(
            "INSERT INTO outbox_messages (id, event_id, project_id, subscription_id, channel, destination, payload) \
             VALUES ($1, $2, $3, $4, 'webhook', $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(event_id)
        .bind(project_id)
        .bind(format!("webhook:{hook_id}"))
        .bind(url)
        .bind(serde_json::json!({ "event": event_type, "pipeline_id": pipeline_id, "status": status, "signed": secret.is_some() }))
        .execute(&mut *tx)
        .await?;
    }

    let notifications = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, lower(channel), target FROM notification_configs \
         WHERE project_id = $1 AND enabled AND lower(channel) IN ('in_app', 'sse')",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    for (config_id, channel, target) in notifications {
        let message = format!(
            "Pipeline {} finished with status {status}",
            pipeline_id.simple()
        );
        sqlx::query(
            "INSERT INTO outbox_messages (id, event_id, project_id, subscription_id, channel, destination, payload) \
             VALUES ($1, $2, $3, $4, 'notification', $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(event_id)
        .bind(project_id)
        .bind(format!("notification:{config_id}"))
        .bind(notification_destination(project_id))
        .bind(serde_json::json!({
            "event": event_type,
            "project_id": project_id,
            "pipeline_id": pipeline_id,
            "status": status,
            "channel": channel,
            "target": target,
            "message": message,
        }))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(event_id)
}

fn next_delay(attempts: i32) -> chrono::Duration {
    // 15s, 30s, 1m, 2m, 4m, 8m, 16m, 32m, 1h cap.
    let secs = 15i64.saturating_mul(1 << attempts.min(8) as u32).min(3600);
    Duration::seconds(secs)
}

/// One delivery pass: claim due messages and POST them.
pub async fn deliver_due(pool: &PgPool, client: &reqwest::Client) -> usize {
    let due = sqlx::query_as::<_, (Uuid, String, String, serde_json::Value, i32)>(
        "SELECT id, channel, destination, payload, attempts FROM outbox_messages \
         WHERE delivered_at IS NULL AND failed_at IS NULL AND next_attempt_at <= now() \
           AND attempts < $1 \
         ORDER BY next_attempt_at, id LIMIT 20",
    )
    .bind(MAX_ATTEMPTS)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut delivered = 0;
    for (id, channel, url, payload, attempts) in due {
        let attempt_number = attempts + 1;
        let started_at = Utc::now();
        let timer = std::time::Instant::now();
        if channel == "notification" {
            let local_channel = payload
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if supported_local_notification_channel(local_channel) {
                let _ = sqlx::query(
                    "UPDATE outbox_messages SET attempts = $2, delivered_at = now(), failed_at = NULL, last_error = NULL WHERE id = $1",
                )
                .bind(id)
                .bind(attempt_number)
                .execute(pool)
                .await;
                record_delivery_attempt(
                    pool,
                    DeliveryAttemptRecord {
                        message_id: id,
                        attempt_number,
                        started_at,
                        outcome: OUTCOME_DELIVERED,
                        http_status: None,
                        error_message: None,
                        duration_ms: elapsed_ms(timer),
                    },
                )
                .await;
                crate::metrics::OUTBOX_DELIVERED_TOTAL
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                delivered += 1;
            } else {
                let last_error = format!("unsupported notification channel: {local_channel}");
                if attempt_number >= MAX_ATTEMPTS {
                    let _ = sqlx::query(
                        "UPDATE outbox_messages SET attempts = $2, last_error = $3, failed_at = now() WHERE id = $1",
                    )
                    .bind(id)
                    .bind(attempt_number)
                    .bind(&last_error)
                    .execute(pool)
                    .await;
                    record_delivery_attempt(
                        pool,
                        DeliveryAttemptRecord {
                            message_id: id,
                            attempt_number,
                            started_at,
                            outcome: OUTCOME_FAILED,
                            http_status: None,
                            error_message: Some(last_error),
                            duration_ms: elapsed_ms(timer),
                        },
                    )
                    .await;
                    crate::metrics::OUTBOX_DEAD_TOTAL
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    let _ = sqlx::query(
                        "UPDATE outbox_messages SET attempts = $2, last_error = $3, next_attempt_at = $4 WHERE id = $1",
                    )
                    .bind(id)
                    .bind(attempt_number)
                    .bind(&last_error)
                    .bind(Utc::now() + next_delay(attempts))
                    .execute(pool)
                    .await;
                    record_delivery_attempt(
                        pool,
                        DeliveryAttemptRecord {
                            message_id: id,
                            attempt_number,
                            started_at,
                            outcome: OUTCOME_RETRY_SCHEDULED,
                            http_status: None,
                            error_message: Some(last_error),
                            duration_ms: elapsed_ms(timer),
                        },
                    )
                    .await;
                }
            }
            continue;
        }

        let mut request = client.post(&url).json(&payload);
        // Sign when the webhook has a secret (subscription_id = "webhook:<id>").
        if let Some(secret) = sqlx::query_scalar::<_, Option<String>>(
            "SELECT w.secret FROM webhooks w \
             WHERE w.id = (SELECT (regexp_match(m.subscription_id, 'webhook:([0-9a-f-]{36})'))[1]::uuid \
                           FROM outbox_messages m WHERE m.id = $1)",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .flatten()
        {
            let body = serde_json::to_string(&payload).unwrap_or_default();
            type HmacSha256 = hmac::Hmac<sha2::Sha256>;
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
                .expect("hmac key");
            mac.update(body.as_bytes());
            use hmac::Mac as HmacExt;
            let sig = hex_encode(&mac.finalize().into_bytes());
            request = request.header("X-Forge-Signature", format!("sha256={sig}"));
        }
        let result = request.send().await;
        match result {
            Ok(response) if response.status().is_success() => {
                let _ = sqlx::query(
                    "UPDATE outbox_messages SET attempts = $2, delivered_at = now(), failed_at = NULL, last_error = NULL WHERE id = $1",
                )
                .bind(id)
                .bind(attempt_number)
                .execute(pool)
                .await;
                record_delivery_attempt(
                    pool,
                    DeliveryAttemptRecord {
                        message_id: id,
                        attempt_number,
                        started_at,
                        outcome: OUTCOME_DELIVERED,
                        http_status: Some(i32::from(response.status().as_u16())),
                        error_message: None,
                        duration_ms: elapsed_ms(timer),
                    },
                )
                .await;
                crate::metrics::OUTBOX_DELIVERED_TOTAL
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                delivered += 1;
            }
            Ok(response) => {
                let status = i32::from(response.status().as_u16());
                let err = format!("http status {status}");
                record_failed_delivery_attempt(
                    pool,
                    id,
                    attempt_number,
                    started_at,
                    timer,
                    Some(status),
                    err,
                )
                .await;
            }
            Err(error) => {
                let err = classify_transport_error(&error);
                record_failed_delivery_attempt(
                    pool,
                    id,
                    attempt_number,
                    started_at,
                    timer,
                    None,
                    err,
                )
                .await;
            }
        }
    }
    delivered
}

async fn record_failed_delivery_attempt(
    pool: &PgPool,
    id: Uuid,
    attempt_number: i32,
    started_at: DateTime<Utc>,
    timer: std::time::Instant,
    http_status: Option<i32>,
    error_message: String,
) {
    if attempt_number >= MAX_ATTEMPTS {
        let _ = sqlx::query(
            "UPDATE outbox_messages SET attempts = $2, last_error = $3, failed_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(attempt_number)
        .bind(&error_message)
        .execute(pool)
        .await;
        record_delivery_attempt(
            pool,
            DeliveryAttemptRecord {
                message_id: id,
                attempt_number,
                started_at,
                outcome: OUTCOME_FAILED,
                http_status,
                error_message: Some(error_message),
                duration_ms: elapsed_ms(timer),
            },
        )
        .await;
        crate::metrics::OUTBOX_DEAD_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    } else {
        let _ = sqlx::query(
            "UPDATE outbox_messages SET attempts = $2, last_error = $3, next_attempt_at = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(attempt_number)
        .bind(&error_message)
        .bind(Utc::now() + next_delay(attempt_number - 1))
        .execute(pool)
        .await;
        record_delivery_attempt(
            pool,
            DeliveryAttemptRecord {
                message_id: id,
                attempt_number,
                started_at,
                outcome: OUTCOME_RETRY_SCHEDULED,
                http_status,
                error_message: Some(error_message),
                duration_ms: elapsed_ms(timer),
            },
        )
        .await;
    }
}

struct DeliveryAttemptRecord {
    message_id: Uuid,
    attempt_number: i32,
    started_at: DateTime<Utc>,
    outcome: &'static str,
    http_status: Option<i32>,
    error_message: Option<String>,
    duration_ms: i32,
}

async fn record_delivery_attempt(pool: &PgPool, attempt: DeliveryAttemptRecord) {
    let finished_at = Utc::now();
    let _ = sqlx::query(
        "INSERT INTO outbox_delivery_attempts \
            (message_id, attempt_number, started_at, finished_at, outcome, http_status, error_message, duration_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (message_id, attempt_number) DO NOTHING",
    )
    .bind(attempt.message_id)
    .bind(attempt.attempt_number)
    .bind(attempt.started_at)
    .bind(finished_at)
    .bind(attempt.outcome)
    .bind(attempt.http_status)
    .bind(attempt.error_message)
    .bind(attempt.duration_ms)
    .execute(pool)
    .await;
}

fn elapsed_ms(timer: std::time::Instant) -> i32 {
    i32::try_from(timer.elapsed().as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX)
}

fn classify_transport_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "timeout".to_owned()
    } else if error.is_connect() {
        "connect error".to_owned()
    } else if error.is_request() {
        "request error".to_owned()
    } else {
        "transport error".to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequeueDeliveryError {
    NotFound,
    NotFailed,
}

pub async fn requeue_failed_delivery(
    pool: &PgPool,
    delivery_id: Uuid,
) -> Result<Result<Uuid, RequeueDeliveryError>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Option<Uuid>,
            String,
            String,
            String,
            serde_json::Value,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
        ),
    >(
        "SELECT event_id, project_id, subscription_id, channel, destination, payload, delivered_at, failed_at \
         FROM outbox_messages WHERE id = $1 FOR UPDATE",
    )
    .bind(delivery_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((
        event_id,
        project_id,
        subscription_id,
        channel,
        destination,
        payload,
        delivered_at,
        failed_at,
    )) = row
    else {
        return Ok(Err(RequeueDeliveryError::NotFound));
    };
    if delivered_at.is_some() || failed_at.is_none() {
        return Ok(Err(RequeueDeliveryError::NotFailed));
    }

    let generation: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(generation), -1) + 1 \
         FROM outbox_messages WHERE event_id = $1 AND subscription_id = $2",
    )
    .bind(event_id)
    .bind(&subscription_id)
    .fetch_one(&mut *tx)
    .await?;
    let new_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO outbox_messages \
            (id, event_id, project_id, subscription_id, channel, destination, payload, generation, replay_of_id, next_attempt_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())",
    )
    .bind(new_id)
    .bind(event_id)
    .bind(project_id)
    .bind(subscription_id)
    .bind(channel)
    .bind(destination)
    .bind(payload)
    .bind(generation)
    .bind(delivery_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Ok(new_id))
}

/// One scheduler pass: materialize due cron slots and process pending fires.
pub async fn fire_due_schedules(pool: &PgPool) -> usize {
    let due = sqlx::query_as::<_, (Uuid, String, Option<DateTime<Utc>>)>(
        "SELECT id, cron, next_fire_at FROM schedules \
         WHERE enabled AND last_fire_error IS NULL AND (next_fire_at IS NULL OR next_fire_at <= now()) \
         ORDER BY next_fire_at ASC NULLS FIRST, created_at ASC \
         LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (schedule_id, cron, next_fire_at) in due {
        materialize_schedule_fire(pool, schedule_id, &cron, next_fire_at).await;
    }
    process_pending_schedule_fires(pool).await
}

async fn materialize_schedule_fire(
    pool: &PgPool,
    schedule_id: Uuid,
    cron: &str,
    next_fire_at: Option<DateTime<Utc>>,
) {
    let parsed = match crate::schedule::parse_cron(cron) {
        Ok(parsed) => parsed,
        Err(error) => {
            mark_schedule_error(pool, schedule_id, error).await;
            return;
        }
    };
    let Some(scheduled_for) = next_fire_at else {
        let Some(next_fire_at) = crate::schedule::next_fire_after(&parsed, Utc::now()) else {
            mark_schedule_error(
                pool,
                schedule_id,
                "cron expression has no matching fire time in the next five years".to_owned(),
            )
            .await;
            return;
        };
        let _ = sqlx::query(
            "UPDATE schedules SET next_fire_at = $2, last_fire_error = NULL \
             WHERE id = $1 AND enabled AND next_fire_at IS NULL",
        )
        .bind(schedule_id)
        .bind(next_fire_at)
        .execute(pool)
        .await;
        return;
    };

    let Some(following_fire_at) = crate::schedule::next_fire_after(&parsed, scheduled_for) else {
        mark_schedule_error(
            pool,
            schedule_id,
            "cron expression has no matching fire time after the due slot".to_owned(),
        )
        .await;
        return;
    };

    let fire_id = Uuid::new_v4();
    let _ = sqlx::query(
        "WITH claimed AS ( \
             UPDATE schedules \
             SET next_fire_at = $3, last_fire_error = NULL \
             WHERE id = $1 AND enabled AND next_fire_at = $2 \
             RETURNING id, project_id \
         ) \
         INSERT INTO schedule_fires (id, schedule_id, project_id, scheduled_for, status) \
         SELECT $4, id, project_id, $2, 'pending' FROM claimed \
         ON CONFLICT (schedule_id, scheduled_for) DO NOTHING",
    )
    .bind(schedule_id)
    .bind(scheduled_for)
    .bind(following_fire_at)
    .bind(fire_id)
    .execute(pool)
    .await;
}

async fn process_pending_schedule_fires(pool: &PgPool) -> usize {
    let fires = sqlx::query_as::<_, (Uuid, Uuid, Uuid, DateTime<Utc>, String)>(
        "SELECT f.id, f.schedule_id, f.project_id, f.scheduled_for, s.git_ref \
         FROM schedule_fires f \
         JOIN schedules s ON s.id = f.schedule_id \
         WHERE f.status = 'pending' \
         ORDER BY f.scheduled_for ASC, f.created_at ASC \
         LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut fired = 0;
    for (fire_id, schedule_id, project_id, scheduled_for, git_ref) in fires {
        let pipeline = crate::api::create_pipeline_with_vars_idempotent(
            pool,
            project_id,
            git_ref,
            serde_json::json!({
                "schedule_id": schedule_id,
                "scheduled_for": scheduled_for.to_rfc3339(),
            }),
            crate::api::PIPELINE_TRIGGER_SOURCE_SCHEDULE,
            Some(&schedule_idempotency_key(schedule_id, scheduled_for)),
        )
        .await;
        match pipeline {
            Ok(outcome) => {
                let _ = sqlx::query(
                    "UPDATE schedule_fires \
                     SET status = 'triggered', pipeline_id = $2, error = NULL \
                     WHERE id = $1 AND status = 'pending'",
                )
                .bind(fire_id)
                .bind(outcome.pipeline.id)
                .execute(pool)
                .await;
                let _ = sqlx::query(
                    "UPDATE schedules \
                     SET last_fired_at = GREATEST(COALESCE(last_fired_at, $2), $2), last_fire_error = NULL \
                     WHERE id = $1",
                )
                .bind(schedule_id)
                .bind(scheduled_for)
                .execute(pool)
                .await;
                fired += 1;
            }
            Err(error) => {
                let error = format!("{error:?}");
                let _ = sqlx::query(
                    "UPDATE schedule_fires SET status = 'failed', error = $2 WHERE id = $1",
                )
                .bind(fire_id)
                .bind(&error)
                .execute(pool)
                .await;
                mark_schedule_error(pool, schedule_id, error).await;
            }
        }
    }
    fired
}

async fn mark_schedule_error(pool: &PgPool, schedule_id: Uuid, error: String) {
    let _ =
        sqlx::query("UPDATE schedules SET next_fire_at = NULL, last_fire_error = $2 WHERE id = $1")
            .bind(schedule_id)
            .bind(error)
            .execute(pool)
            .await;
}

fn schedule_idempotency_key(schedule_id: Uuid, scheduled_for: DateTime<Utc>) -> String {
    format!("schedule:{schedule_id}:{}", scheduled_for.to_rfc3339())
}

/// Background supervisor loop: delivery + scheduler every 5 seconds.
pub async fn supervisor_loop(pool: PgPool) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("reqwest client");
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let delivered = deliver_due(&pool, &client).await;
        let fired = fire_due_schedules(&pool).await;
        if delivered > 0 || fired > 0 {
            tracing::info!(delivered, fired, "outbox/scheduler pass");
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded() {
        assert_eq!(next_delay(0).num_seconds(), 15);
        assert_eq!(next_delay(7).num_seconds(), 1920); // 15s * 2^7
        assert_eq!(next_delay(20).num_seconds(), 3600); // capped at 1h
    }

    #[test]
    fn local_notification_channel_support_is_explicit() {
        assert!(supported_local_notification_channel("in_app"));
        assert!(supported_local_notification_channel("SSE"));
        assert!(!supported_local_notification_channel("slack"));
        assert_eq!(
            notification_destination(Uuid::nil()),
            "project:00000000-0000-0000-0000-000000000000"
        );
    }
}
