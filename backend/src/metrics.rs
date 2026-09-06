//! Minimal Prometheus text-exposition metrics (SLO/METRICS observability floor).
//!
//! Process-level + HTTP counters maintained atomically; no external dependency
//! (axum-prometheus is Target when percentiles are needed).

use std::sync::atomic::{AtomicU64, Ordering};

pub static HTTP_REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static HTTP_5XX_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static LOGIN_ATTEMPTS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static LOGIN_FAILURES_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static PIPELINES_CREATED_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static OUTBOX_DELIVERED_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static OUTBOX_DEAD_TOTAL: AtomicU64 = AtomicU64::new(0);
// K6.1: state gauges refreshed by the metrics handler from PostgreSQL.
pub static PIPELINES_RUNNING: AtomicU64 = AtomicU64::new(0);
pub static PIPELINES_QUEUED: AtomicU64 = AtomicU64::new(0);
pub static JOBS_FAILED_24H: AtomicU64 = AtomicU64::new(0);
pub static JOBS_SUCCEEDED_24H: AtomicU64 = AtomicU64::new(0);
pub static RUNNERS_ONLINE: AtomicU64 = AtomicU64::new(0);
pub static RUNNERS_DRAINING: AtomicU64 = AtomicU64::new(0);

pub fn render() -> String {
    let mut out = String::new();
    let mut m = |name: &str, help: &str, ty: &str, v: u64| {
        out.push_str(&format!(
            "# HELP {name} {help}\n# TYPE {name} {ty}\n{name} {v}\n"
        ));
    };
    m(
        "forge_http_requests_total",
        "HTTP requests handled",
        "counter",
        HTTP_REQUESTS_TOTAL.load(Ordering::Relaxed),
    );
    m(
        "forge_http_5xx_total",
        "HTTP 5xx responses",
        "counter",
        HTTP_5XX_TOTAL.load(Ordering::Relaxed),
    );
    m(
        "forge_login_attempts_total",
        "Login attempts",
        "counter",
        LOGIN_ATTEMPTS_TOTAL.load(Ordering::Relaxed),
    );
    m(
        "forge_login_failures_total",
        "Failed logins",
        "counter",
        LOGIN_FAILURES_TOTAL.load(Ordering::Relaxed),
    );
    m(
        "forge_pipelines_created_total",
        "Pipelines created",
        "counter",
        PIPELINES_CREATED_TOTAL.load(Ordering::Relaxed),
    );
    m(
        "forge_outbox_delivered_total",
        "Outbox messages delivered",
        "counter",
        OUTBOX_DELIVERED_TOTAL.load(Ordering::Relaxed),
    );
    m(
        "forge_pipelines_running",
        "Pipelines currently running",
        "gauge",
        PIPELINES_RUNNING.load(Ordering::Relaxed),
    );
    m(
        "forge_pipelines_queued",
        "Pipelines currently queued",
        "gauge",
        PIPELINES_QUEUED.load(Ordering::Relaxed),
    );
    m(
        "forge_jobs_failed_24h",
        "Jobs failed in the last 24h",
        "gauge",
        JOBS_FAILED_24H.load(Ordering::Relaxed),
    );
    m(
        "forge_jobs_succeeded_24h",
        "Jobs succeeded in the last 24h",
        "gauge",
        JOBS_SUCCEEDED_24H.load(Ordering::Relaxed),
    );
    m(
        "forge_runners_online",
        "Runners online",
        "gauge",
        RUNNERS_ONLINE.load(Ordering::Relaxed),
    );
    m(
        "forge_runners_draining",
        "Runners draining",
        "gauge",
        RUNNERS_DRAINING.load(Ordering::Relaxed),
    );
    m(
        "forge_outbox_dead_total",
        "Outbox messages dead-lettered",
        "counter",
        OUTBOX_DEAD_TOTAL.load(Ordering::Relaxed),
    );
    out
}

/// K6.1: load pipeline/job/runner state counts into the gauges. Failures are
/// tolerated (metrics must never 5xx); gauges keep their last value.
pub async fn refresh_state_gauges(pool: &sqlx::PgPool) {
    use sqlx::Row;
    let ok = |row: &sqlx::postgres::PgRow, col: &str| -> u64 {
        row.try_get::<i64, _>(col)
            .map(|v| v.max(0) as u64)
            .unwrap_or(0)
    };
    if let Ok(row) = sqlx::query(
        "SELECT \
            (SELECT count(*) FROM pipelines WHERE status = 'running') AS p_running, \
            (SELECT count(*) FROM pipelines WHERE status = 'queued') AS p_queued, \
            (SELECT count(*) FROM jobs WHERE status = 'failed' AND updated_at > now() - interval '24 hours') AS j_failed, \
            (SELECT count(*) FROM jobs WHERE status = 'success' AND updated_at > now() - interval '24 hours') AS j_ok",
    )
    .fetch_one(pool)
    .await
    {
        PIPELINES_RUNNING.store(ok(&row, "p_running"), Ordering::Relaxed);
        PIPELINES_QUEUED.store(ok(&row, "p_queued"), Ordering::Relaxed);
        JOBS_FAILED_24H.store(ok(&row, "j_failed"), Ordering::Relaxed);
        JOBS_SUCCEEDED_24H.store(ok(&row, "j_ok"), Ordering::Relaxed);
    }
    if let Ok(row) = sqlx::query(
        "SELECT \
            count(*) FILTER (WHERE status = 'online') AS online, \
            count(*) FILTER (WHERE draining) AS draining \
         FROM runners WHERE disabled_at IS NULL",
    )
    .fetch_one(pool)
    .await
    {
        RUNNERS_ONLINE.store(ok(&row, "online"), Ordering::Relaxed);
        RUNNERS_DRAINING.store(ok(&row, "draining"), Ordering::Relaxed);
    }
}
