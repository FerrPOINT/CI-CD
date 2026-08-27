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
        "forge_outbox_dead_total",
        "Outbox messages dead-lettered",
        "counter",
        OUTBOX_DEAD_TOTAL.load(Ordering::Relaxed),
    );
    out
}
