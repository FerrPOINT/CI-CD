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

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Delivery lease (seconds): a claimed row is invisible to other workers
/// until the lease expires, so crashed workers release work within this window.
pub const DELIVERY_LEASE_SECS: f64 = 300.0;

pub const MAX_ATTEMPTS: i32 = 8;
pub const NOTIFICATION_CHANNEL_IN_APP: &str = "in_app";
pub const NOTIFICATION_CHANNEL_SSE: &str = "sse";
/// Delivery channels that fan out over HTTP on terminal pipeline events
/// (docs/AUTOMATION_ARCHITECTURE.md §9, stage 4 step 2/4).
pub const NOTIFICATION_CHANNEL_SLACK_WEBHOOK: &str = "slack_webhook";
pub const NOTIFICATION_CHANNEL_GENERIC_WEBHOOK: &str = "generic_webhook";

pub const NOTIFICATION_CHANNEL_EMAIL: &str = "email";

/// Email notification body for terminal pipeline events
/// (docs/AUTOMATION_ARCHITECTURE.md §9 stage 4 step 2).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmailNotificationPayload {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Slack incoming-webhook body: a single rendered message block.
fn slack_payload(
    event_type: &str,
    project_id: Uuid,
    pipeline_id: Uuid,
    status: &str,
) -> serde_json::Value {
    serde_json::json!({
        "text": format!("Pipeline {pipeline_id} of project {project_id} finished: {status} ({event_type})"),
    })
}
const OUTCOME_DELIVERED: &str = "delivered";
const OUTCOME_RETRY_SCHEDULED: &str = "retry_scheduled";
const OUTCOME_FAILED: &str = "failed";

/// A loaded notification rule row (stage 4 item 1).
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct NotificationRuleRow {
    pub event_types: Vec<String>,
    pub statuses: Vec<String>,
    pub channels: Vec<String>,
}

impl NotificationRuleRow {
    /// Empty arrays mean "match everything".
    fn matches(&self, event_type: &str, status: &str, channel: &str) -> bool {
        let event_ok =
            self.event_types.is_empty() || self.event_types.iter().any(|e| e == event_type);
        let status_ok = self.statuses.is_empty() || self.statuses.iter().any(|s| s == status);
        let channel_ok = self.channels.is_empty()
            || self
                .channels
                .iter()
                .any(|c| c.eq_ignore_ascii_case(channel));
        event_ok && status_ok && channel_ok
    }
}

/// Render a `{{var}}` template with the standard event variables. Unknown
/// placeholders are removed (not leaked); rendering is pure so payloads stay
/// reproducible from delivery history.
pub(crate) fn render_template(
    template: &str,
    event_type: &str,
    project_id: Uuid,
    pipeline_id: Uuid,
    status: &str,
) -> String {
    let vars = [
        ("event", event_type.to_string()),
        ("project_id", project_id.to_string()),
        ("pipeline_id", pipeline_id.to_string()),
        ("status", status.to_string()),
    ];
    let mut out = template.to_string();
    for (key, value) in &vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    // Remove unknown placeholders so templates never leak moustache noise.
    while let Some(start) = out.find("{{") {
        if let Some(end_rel) = out[start..].find("}}") {
            let end = start + end_rel + 2;
            out.replace_range(start..end, "");
        } else {
            break;
        }
    }
    out
}

/// True when the (event, status, channel) triple passes the project's enabled
/// rules. With no rules configured every event still fans out (backwards
/// compatible default).
pub(crate) async fn rules_allow(
    tx: &mut sqlx::PgConnection,
    project_id: Uuid,
    event_type: &str,
    status: &str,
    channel: &str,
) -> Result<bool, sqlx::Error> {
    let rules = sqlx::query_as::<_, NotificationRuleRow>(
        "SELECT event_types, statuses, channels FROM notification_rules \
         WHERE project_id = $1 AND enabled",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    if rules.is_empty() {
        return Ok(true);
    }
    Ok(rules
        .iter()
        .any(|rule| rule.matches(event_type, status, channel)))
}

/// The most specific enabled template for (project, channel) — last updated
/// wins, so admins can iterate templates without touching delivery code.
pub(crate) async fn template_for(
    tx: &mut sqlx::PgConnection,
    project_id: Uuid,
    channel: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT subject_template, body_template FROM notification_templates \
         WHERE project_id = $1 AND enabled AND lower(channel) = lower($2) \
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(channel)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(row)
}

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

    let emails = sqlx::query_as::<_, (Uuid, String, String, i32)>(
        "SELECT id, lower(channel), target, aggregation_window_secs FROM notification_configs \
         WHERE project_id = $1 AND enabled AND lower(channel) = 'email'",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    for (config_id, _channel, target, window_secs) in emails {
        if !rules_allow(&mut tx, project_id, event_type, status, "email").await? {
            continue;
        }
        if try_aggregate(
            &mut tx,
            project_id,
            &format!("notification:{config_id}"),
            status,
            window_secs,
        )
        .await?
        {
            continue;
        }
        let (subject_tpl, body_tpl) =
            match template_for(&mut tx, project_id, "email").await? {
                Some((subject_tpl, body_tpl)) => (subject_tpl, body_tpl),
                None => (
                    "Forge pipeline {{status}}".to_string(),
                    "Pipeline {{pipeline_id}} of project {{project_id}} finished with status {{status}} ({{event}}).".to_string(),
                ),
            };
        let payload = serde_json::to_value(EmailNotificationPayload {
            to: target.clone(),
            subject: render_template(&subject_tpl, event_type, project_id, pipeline_id, status),
            body: render_template(&body_tpl, event_type, project_id, pipeline_id, status),
        })
        .unwrap_or(serde_json::Value::Null);
        sqlx::query(
            "INSERT INTO outbox_messages (id, event_id, project_id, subscription_id, channel, destination, payload) \
             VALUES ($1, $2, $3, $4, 'email', $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(event_id)
        .bind(project_id)
        .bind(format!("notification:{config_id}"))
        .bind(target)
        .bind(payload)
        .execute(&mut *tx)
        .await?;
    }

    let external = sqlx::query_as::<_, (Uuid, String, String, i32)>(
        "SELECT id, lower(channel), target, aggregation_window_secs FROM notification_configs \
         WHERE project_id = $1 AND enabled AND lower(channel) IN ('slack_webhook', 'generic_webhook')",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    for (config_id, channel, target, window_secs) in external {
        if !rules_allow(&mut tx, project_id, event_type, status, &channel).await? {
            continue;
        }
        if try_aggregate(
            &mut tx,
            project_id,
            &format!("notification:{config_id}"),
            status,
            window_secs,
        )
        .await?
        {
            continue;
        }
        // Reuse the shared HTTP delivery subsystem: the message is delivered
        // by deliver_due with the same retry/backoff/dead-letter ledger.
        let payload = if channel == NOTIFICATION_CHANNEL_SLACK_WEBHOOK {
            slack_payload(event_type, project_id, pipeline_id, status)
        } else {
            serde_json::json!({
                "event": event_type,
                "project_id": project_id,
                "pipeline_id": pipeline_id,
                "status": status,
            })
        };
        sqlx::query(
            "INSERT INTO outbox_messages (id, event_id, project_id, subscription_id, channel, destination, payload) \
             VALUES ($1, $2, $3, $4, 'webhook', $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(event_id)
        .bind(project_id)
        .bind(format!("notification:{config_id}"))
        .bind(target)
        .bind(payload)
        .execute(&mut *tx)
        .await?;
    }

    let notifications = sqlx::query_as::<_, (Uuid, String, String, i32)>(
        "SELECT id, lower(channel), target, aggregation_window_secs FROM notification_configs \
         WHERE project_id = $1 AND enabled AND lower(channel) IN ('in_app', 'sse')",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    for (config_id, channel, target, window_secs) in notifications {
        if !rules_allow(&mut tx, project_id, event_type, status, &channel).await? {
            continue;
        }
        if try_aggregate(
            &mut tx,
            project_id,
            &format!("notification:{config_id}"),
            status,
            window_secs,
        )
        .await?
        {
            continue;
        }
        let message = match template_for(&mut tx, project_id, &channel).await? {
            Some((_, body_tpl)) => {
                render_template(&body_tpl, event_type, project_id, pipeline_id, status)
            }
            None => format!(
                "Pipeline {} finished with status {status}",
                pipeline_id.simple()
            ),
        };
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

/// Opens (or re-opens with the newer error) the destination-failure alert for
/// a dead-lettered message; a later successful delivery resolves it.
async fn open_destination_alert(
    pool: &PgPool,
    message_id: Uuid,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT m.project_id, m.channel, m.destination FROM outbox_messages m WHERE m.id = $1",
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await?;
    let Some((project_id, channel, destination)) = row else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO notification_destination_alerts (id, project_id, channel, destination, last_error, state) \
         VALUES ($1, $2, $3, $4, $5, 'open') \
         ON CONFLICT DO NOTHING",
    )
    // no unique constraint: dedupe via NOT EXISTS below instead
    .execute(pool)
    .await
    .ok();
    sqlx::query(
        "UPDATE notification_destination_alerts a SET state = 'open', last_error = $3, updated_at = now() \
         WHERE a.project_id = $1 AND a.channel = $2 AND a.destination = $4 AND a.state <> 'open'",
    )
    .bind(project_id)
    .bind(&channel)
    .bind(error_message)
    .bind(&destination)
    .execute(pool)
    .await?;
    Ok(())
}

/// Resolves destination alerts once delivery succeeds again.
async fn resolve_destination_alert(
    pool: &PgPool,
    project_id: Uuid,
    channel: &str,
    destination: &str,
) {
    let _ = sqlx::query(
        "UPDATE notification_destination_alerts SET state = 'resolved', updated_at = now() \
         WHERE project_id = $1 AND channel = $2 AND destination = $3 AND state <> 'resolved'",
    )
    .bind(project_id)
    .bind(channel)
    .bind(destination)
    .execute(pool)
    .await;
}

/// True when `now` falls inside the config's quiet window. Windows may wrap
/// midnight (start > end). Unconfigured (-1) windows never quiet.
/// Aggregation (stage 4 item 3): inside the config's window a repeated
/// identical delivery increments the pending message's counter instead of
/// enqueueing a duplicate. Returns true when the event was collapsed into an
/// existing pending message.
#[allow(clippy::too_many_arguments)]
async fn try_aggregate(
    tx: &mut sqlx::PgConnection,
    project_id: Uuid,
    subscription_id: &str,
    status: &str,
    window_secs: i32,
) -> Result<bool, sqlx::Error> {
    if window_secs <= 0 {
        return Ok(false);
    }
    let collapsed = sqlx::query(
        "UPDATE outbox_messages SET payload = jsonb_set(payload, '{agg_count}', \
             ((coalesce((payload->>'agg_count')::int, 0) + 1)::text::jsonb)) \
         WHERE id IN ( \
           SELECT id FROM outbox_messages \
           WHERE delivered_at IS NULL AND failed_at IS NULL \
             AND project_id = $1 AND subscription_id = $2 \
             AND payload->>'status' = $3 \
             AND created_at > now() - make_interval(secs => $4) \
           ORDER BY created_at DESC LIMIT 1 \
         ) RETURNING id",
    )
    .bind(project_id)
    .bind(subscription_id)
    .bind(status)
    .bind(window_secs)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(collapsed.is_some())
}

/// Reference implementation of the SQL quiet-window predicate (unit-tested;
/// the delivery gate evaluates the same logic in SQL for set-based skipping).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn in_quiet_window(now_min: i64, start_min: i64, end_min: i64) -> bool {
    if start_min < 0 || end_min < 0 {
        return false;
    }
    if start_min == end_min {
        return false;
    }
    if start_min < end_min {
        (start_min..end_min).contains(&now_min)
    } else {
        now_min >= start_min || now_min < end_min
    }
}

fn next_delay(attempts: i32) -> chrono::Duration {
    // 15s, 30s, 1m, 2m, 4m, 8m, 16m, 32m, 1h cap.
    let secs = 15i64.saturating_mul(1 << attempts.min(8) as u32).min(3600);
    Duration::seconds(secs)
}

/// Send one outbox email payload over SMTP. Errors are retryable and use
/// the standard backoff/dead-letter ledger.
async fn deliver_email(
    smtp: &crate::config::SmtpConfig,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let parsed: EmailNotificationPayload = serde_json::from_value(payload.clone())
        .map_err(|e| format!("invalid email payload: {e}"))?;
    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let mut builder = if smtp.starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
            .map_err(|e| format!("smtp relay: {e}"))?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
    };
    builder = builder.port(smtp.port);
    if let (Some(user), Some(pass)) = (&smtp.username, &smtp.password) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }
    let email = Message::builder()
        .from(
            format!("Forge <{}>", smtp.from_address)
                .parse()
                .map_err(|e| format!("from address: {e}"))?,
        )
        .to(parsed.to.parse().map_err(|e| format!("to address: {e}"))?)
        .subject(&parsed.subject)
        .header(ContentType::TEXT_PLAIN)
        .body(parsed.body)
        .map_err(|e| format!("email build: {e}"))?;
    let mailer = builder.build();
    mailer
        .send(email)
        .await
        .map_err(|e| format!("smtp send: {e}"))?;
    Ok(())
}

/// One delivery pass: claim due messages and POST/SMTP them.
pub async fn deliver_due(
    pool: &PgPool,
    client: &reqwest::Client,
    smtp: &crate::config::SmtpConfig,
) -> usize {
    // Stage 5 scale-out: atomically claim due rows (lease-claim). The claim
    // bumps attempts and pushes next_attempt_at one lease window forward in a
    // single UPDATE ... WHERE next_attempt_at <= now() so a second worker
    // deployment can never claim or deliver the same message concurrently.
    let claimed = sqlx::query_as::<_, (Uuid, Uuid, String, String, serde_json::Value, i32)>(
        "UPDATE outbox_messages m SET \
           attempts = m.attempts + 1, \
           next_attempt_at = now() + make_interval(secs => $2::double precision) \
         FROM ( \
           SELECT id FROM outbox_messages m \
           WHERE m.delivered_at IS NULL AND m.failed_at IS NULL AND m.next_attempt_at <= now() \
             AND m.attempts < $1 \
             AND NOT EXISTS ( \
               SELECT 1 FROM notification_configs c \
               WHERE c.project_id = m.project_id \
                 AND c.enabled \
                 AND ('notification:' || c.id::text) = m.subscription_id \
                 AND c.quiet_action = 'drop' \
                 AND c.quiet_start_min >= 0 \
                 AND (extract(hour from now()) * 60 + extract(minute from now()))::int \
                       BETWEEN c.quiet_start_min AND c.quiet_end_min - 1 \
                 AND NOT ( \
                   m.payload->>'status' = ANY (c.quiet_bypass_statuses) \
                   OR c.quiet_bypass_statuses = '{}' \
                 ) \
             ) \
           ORDER BY m.next_attempt_at, m.id LIMIT 20 \
           FOR UPDATE SKIP LOCKED \
         ) batch \
         WHERE m.id = batch.id \
         RETURNING m.id, m.project_id, m.channel, m.destination, m.payload, m.attempts",
    )
    .bind(MAX_ATTEMPTS)
    .bind(DELIVERY_LEASE_SECS)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut delivered = 0;
    for (id, project_id, channel, url, payload, attempts) in claimed {
        // The lease-claim already bumped attempts: this IS attempt_number.
        let attempt_number = attempts;
        let started_at = Utc::now();
        let timer = std::time::Instant::now();
        if channel == "notification" {
            let local_channel = payload
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if supported_local_notification_channel(local_channel) {
                let _ = sqlx::query(
                    "UPDATE outbox_messages SET delivered_at = now(), failed_at = NULL, last_error = NULL WHERE id = $1",
                )
                .bind(id)
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
                let local_target = payload
                    .get("target")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                resolve_destination_alert(pool, project_id, local_channel, &local_target).await;
                crate::metrics::OUTBOX_DELIVERED_TOTAL
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                delivered += 1;
            } else {
                let last_error = format!("unsupported notification channel: {local_channel}");
                if attempt_number >= MAX_ATTEMPTS {
                    let _ = sqlx::query(
                        "UPDATE outbox_messages SET last_error = $2, failed_at = now() WHERE id = $1",
                    )
                    .bind(id)
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
                        "UPDATE outbox_messages SET last_error = $2, next_attempt_at = $3 WHERE id = $1",
                    )
                    .bind(id)
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

        if channel == "email" {
            // SMTP delivery (AUTOMATION_ARCHITECTURE §9 stage 4). Disabled
            // config marks the message delivered without touching the
            // network, matching the local notification behavior.
            let email_result = if !smtp.enabled {
                Ok(())
            } else {
                deliver_email(smtp, &payload).await
            };
            match email_result {
                Ok(()) => {
                    let _ = sqlx::query(
                        "UPDATE outbox_messages SET delivered_at = now(), failed_at = NULL, last_error = NULL WHERE id = $1",
                    )
                    .bind(id)
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
                    resolve_destination_alert(pool, project_id, "email", &url).await;
                    crate::metrics::OUTBOX_DELIVERED_TOTAL
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    delivered += 1;
                }
                Err(err) => {
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
            continue;
        }

        // Stage 5 egress control: outbound webhook hosts must pass the
        // configured allowlist (empty allowlist = unrestricted local mode).
        let egress = egress_config_from_env();
        if !egress.webhook_host_allowed(&url) {
            let error_message =
                format!("webhook destination host not allowed by egress allowlist: {url}");
            record_failed_delivery_attempt(
                pool,
                id,
                attempt_number,
                started_at,
                timer,
                None,
                error_message,
            )
            .await;
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
                    "UPDATE outbox_messages SET delivered_at = now(), failed_at = NULL, last_error = NULL WHERE id = $1",
                )
                .bind(id)
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
                resolve_destination_alert(pool, project_id, "webhook", &url).await;
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
            "UPDATE outbox_messages SET last_error = $2, failed_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(&error_message)
        .execute(pool)
        .await;
        // Stage 4 item 5: a destination that exhausts retries opens an alert
        // for the project so muted success never hides failed deliveries.
        let _ = open_destination_alert(pool, id, &error_message).await;
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
            "UPDATE outbox_messages SET last_error = $2, next_attempt_at = $3 WHERE id = $1",
        )
        .bind(id)
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

/// Stage 5 item 3: retention sweep for terminal outbox rows. Deletes
/// delivered messages older than the window; attempt history cascades via
/// FK. Undelivered / retrying / dead-letter rows are never touched.
pub async fn retention_sweep(pool: &PgPool, window: chrono::Duration) -> u64 {
    let cutoff = chrono::Utc::now() - window;
    let result = sqlx::query(
        "DELETE FROM outbox_messages WHERE delivered_at IS NOT NULL AND delivered_at < $1",
    )
    .bind(cutoff)
    .execute(pool)
    .await;
    match result {
        Ok(res) => res.rows_affected(),
        Err(error) => {
            tracing::warn!(%error, "outbox retention sweep failed");
            0
        }
    }
}

/// Egress allowlist from CICD_WEBHOOK_ALLOWLIST (comma-separated hosts).
fn egress_config_from_env() -> crate::config::EgressConfig {
    let raw = std::env::var("CICD_WEBHOOK_ALLOWLIST").unwrap_or_default();
    crate::config::EgressConfig {
        webhook_allowlist: raw
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect(),
        destination_max_inflight: 4,
    }
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
    let git_root = git_root_from_env_lossy();
    fire_due_schedules_with_git_root(pool, &git_root).await
}

pub async fn fire_due_schedules_with_git_root(pool: &PgPool, git_root: &Path) -> usize {
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
    process_pending_schedule_fires(pool, git_root).await
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

async fn process_pending_schedule_fires(pool: &PgPool, git_root: &Path) -> usize {
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
            git_root,
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
    supervisor_loop_with_git_root(pool, git_root_from_env_lossy()).await;
}

pub async fn supervisor_loop_with_git_root(pool: PgPool, git_root: PathBuf) {
    let mut passes: u32 = 0;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("reqwest client");
    let smtp = crate::config::RuntimeConfig::from_env()
        .map(|cfg| cfg.smtp)
        .unwrap_or_default();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let delivered = deliver_due(&pool, &client, &smtp).await;
        let fired = fire_due_schedules_with_git_root(&pool, &git_root).await;
        if delivered > 0 || fired > 0 {
            tracing::info!(delivered, fired, "outbox/scheduler pass");
            // Stage 5 item 3: retention runs hourly (720 passes at 5s cadence).
            passes += 1;
            if passes >= 720 {
                let swept = retention_sweep(&pool, chrono::Duration::days(30)).await;
                if swept > 0 {
                    tracing::info!(swept, "outbox retention sweep");
                }
                passes = 0;
            }
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn git_root_from_env_lossy() -> PathBuf {
    crate::config::RuntimeConfig::from_env_for_app()
        .map(|config| config.git.root)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "invalid scheduler git root config; using test defaults");
            crate::config::RuntimeConfig::test_default().git.root
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_substitutes_known_and_strips_unknown_vars() {
        let out = render_template(
            "CUSTOM {{event}} {{status}} {{unknown}} done",
            "pipeline.failed",
            Uuid::nil(),
            Uuid::nil(),
            "failed",
        );
        assert_eq!(out, "CUSTOM pipeline.failed failed  done");
    }

    #[test]
    fn quiet_window_detection_handles_wrap_midnight_and_disabled() {
        assert!(!in_quiet_window(700, -1, -1), "unconfigured never quiets");
        assert!(!in_quiet_window(700, 700, 700), "empty window never quiets");
        assert!(in_quiet_window(700, 600, 800));
        assert!(!in_quiet_window(500, 600, 800));
        // 22:00-06:00 wraps midnight.
        assert!(in_quiet_window(23 * 60, 22 * 60, 6 * 60));
        assert!(in_quiet_window(3 * 60, 22 * 60, 6 * 60));
        assert!(!in_quiet_window(12 * 60, 22 * 60, 6 * 60));
    }

    #[test]
    fn notification_rule_matches_empty_arrays_as_wildcards() {
        let rule = NotificationRuleRow {
            event_types: vec![],
            statuses: vec![],
            channels: vec![],
        };
        assert!(rule.matches("pipeline.failed", "failed", "email"));
    }

    #[test]
    fn notification_rule_matches_exact_entries() {
        let rule = NotificationRuleRow {
            event_types: vec!["pipeline.failed".to_string()],
            statuses: vec!["failed".to_string(), "canceled".to_string()],
            channels: vec!["in_app".to_string()],
        };
        assert!(rule.matches("pipeline.failed", "canceled", "IN_APP"));
        assert!(!rule.matches("pipeline.succeeded", "failed", "in_app"));
        assert!(!rule.matches("pipeline.failed", "success", "in_app"));
        assert!(!rule.matches("pipeline.failed", "failed", "email"));
    }

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
