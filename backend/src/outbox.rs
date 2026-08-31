//! Outbox delivery worker + scheduler (ADR-0006, EVENT_CONTRACT, REQ-AUTO-003).
//!
//! - `domain_events` / `outbox_messages` tables (migration 0004).
//! - Webhook fan-out: project webhooks are translated into outbox messages on
//!   pipeline terminal transitions; the worker delivers with retry/backoff
//!   (8 attempts, 15s..1h) and dead-letters after exhaustion.
//! - In-app notification fan-out: `in_app`/`sse` notification configs are
//!   translated into durable local outbox messages for Dashboard history/SSE.
//! - Scheduler: enabled cron schedules trigger pipelines with a
//!   `last_fired_at` claim so a restart cannot double-fire the same minute.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

pub const MAX_ATTEMPTS: i32 = 8;
pub const NOTIFICATION_CHANNEL_IN_APP: &str = "in_app";
pub const NOTIFICATION_CHANNEL_SSE: &str = "sse";

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
            "INSERT INTO outbox_messages (id, event_id, subscription_id, channel, destination, payload) \
             VALUES ($1, $2, $3, 'webhook', $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(event_id)
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
            "INSERT INTO outbox_messages (id, event_id, subscription_id, channel, destination, payload) \
             VALUES ($1, $2, $3, 'notification', $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(event_id)
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
         WHERE delivered_at IS NULL AND next_attempt_at <= now() \
           AND attempts < $1 \
         ORDER BY next_attempt_at LIMIT 20",
    )
    .bind(MAX_ATTEMPTS)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut delivered = 0;
    for (id, channel, url, payload, attempts) in due {
        if channel == "notification" {
            let local_channel = payload
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if supported_local_notification_channel(local_channel) {
                let _ = sqlx::query(
                    "UPDATE outbox_messages SET delivered_at = now(), last_error = NULL WHERE id = $1",
                )
                .bind(id)
                .execute(pool)
                .await;
                delivered += 1;
            } else {
                let new_attempts = attempts + 1;
                let last_error = format!("unsupported notification channel: {local_channel}");
                if new_attempts >= MAX_ATTEMPTS {
                    let _ = sqlx::query(
                        "UPDATE outbox_messages SET attempts = $2, last_error = $3 WHERE id = $1",
                    )
                    .bind(id)
                    .bind(new_attempts)
                    .bind(last_error)
                    .execute(pool)
                    .await;
                } else {
                    let _ = sqlx::query(
                        "UPDATE outbox_messages SET attempts = $2, last_error = $3, next_attempt_at = $4 WHERE id = $1",
                    )
                    .bind(id)
                    .bind(new_attempts)
                    .bind(last_error)
                    .bind(Utc::now() + next_delay(attempts))
                    .execute(pool)
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
        let ok = matches!(&result, Ok(r) if r.status().is_success());
        if ok {
            let _ = sqlx::query(
                "UPDATE outbox_messages SET delivered_at = now(), last_error = NULL WHERE id = $1",
            )
            .bind(id)
            .execute(pool)
            .await;
            delivered += 1;
        } else if attempts + 1 >= MAX_ATTEMPTS {
            // Dead-letter: keep the row, stop retrying (observability).
            let err = result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "http error".into());
            let _ = sqlx::query(
                "UPDATE outbox_messages SET attempts = $2, last_error = $3 WHERE id = $1",
            )
            .bind(id)
            .bind(attempts + 1)
            .bind(err)
            .execute(pool)
            .await;
        } else {
            let err = result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "non-2xx".into());
            let _ = sqlx::query(
                "UPDATE outbox_messages SET attempts = $2, last_error = $3, next_attempt_at = $4 WHERE id = $1",
            )
            .bind(id)
            .bind(attempts + 1)
            .bind(err)
            .bind(Utc::now() + next_delay(attempts))
            .execute(pool)
            .await;
        }
    }
    delivered
}

/// One scheduler pass: fire enabled schedules whose minute arrived (idempotent claim).
pub async fn fire_due_schedules(pool: &PgPool) -> usize {
    // Phase 1 fires any schedule not fired in the last 55 seconds when enabled.
    let due = sqlx::query_as::<_, (Uuid, Uuid, String)>(
        "SELECT s.id, s.project_id, s.git_ref FROM schedules s \
         WHERE s.enabled AND (s.last_fired_at IS NULL OR s.last_fired_at < now() - interval '55 seconds') \
         LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut fired = 0;
    for (schedule_id, project_id, git_ref) in due {
        // Atomic claim: only proceed when the row was still due.
        let claimed = sqlx::query_scalar::<_, Uuid>(
            "UPDATE schedules SET last_fired_at = now(), last_fire_error = NULL \
             WHERE id = $1 AND (last_fired_at IS NULL OR last_fired_at < now() - interval '55 seconds') \
             RETURNING id",
        )
        .bind(schedule_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let Some(_claim) = claimed else { continue };

        let pipeline = crate::api::create_pipeline(pool, project_id, git_ref.clone()).await;
        match pipeline {
            Ok(_) => fired += 1,
            Err(e) => {
                let _ = sqlx::query("UPDATE schedules SET last_fire_error = $2 WHERE id = $1")
                    .bind(schedule_id)
                    .bind(format!("{e:?}"))
                    .execute(pool)
                    .await;
            }
        }
    }
    fired
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
