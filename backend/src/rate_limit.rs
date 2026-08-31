//! In-memory fixed-window rate limiter (THREAT_MODEL: resource exhaustion mitigation).
//!
//! Current scope: per-client limits for authentication, API, Git Smart HTTP,
//! internal Git hooks and artifact uploads. A distributed limiter remains
//! Target (see AUTHZ_CONTRACT); this bounds naive floods in the single-node
//! deployment.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Default)]
pub struct RateLimiter {
    hits: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    /// `key` is typically `ip + path class`. Returns false when the window budget is exhausted.
    pub fn allow(&self, key: &str, limit: u32, window_secs: u64) -> bool {
        let mut hits = self.hits.lock().expect("rate limiter lock");
        let now = Instant::now();
        let entry = hits.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0).as_secs() >= window_secs {
            *entry = (now, 1);
            return true;
        }
        if entry.1 >= limit {
            return false;
        }
        entry.1 += 1;
        true
    }

    /// Drop expired windows (call periodically; cheap enough inline for MVP).
    pub fn prune(&self, window_secs: u64) {
        let mut hits = self.hits.lock().expect("rate limiter lock");
        let now = Instant::now();
        hits.retain(|_, (start, _)| now.duration_since(*start).as_secs() < window_secs * 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_enforced_within_window() {
        let limiter = RateLimiter::default();
        for _ in 0..5 {
            assert!(limiter.allow("ip1", 5, 60));
        }
        assert!(!limiter.allow("ip1", 5, 60));
        // Different key unaffected.
        assert!(limiter.allow("ip2", 5, 60));
    }
}
