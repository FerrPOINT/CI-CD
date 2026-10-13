use super::files::sha;
use crate::domain::task_delivery::{DeliveryProbe, DeliveryStatus, digest};
use anyhow::ensure;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Loaded from privileged owner configuration, never an HTTP command or candidate artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryPolicy {
    pub origin: String,
    pub health_path: String,
    pub health_body_sha256: String,
    pub acceptance_path: String,
    pub acceptance_body_sha256: String,
}

impl DeliveryPolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        let url = reqwest::Url::parse(&self.origin)?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "fixed credential-free owner origin required"
        );
        for path in [&self.health_path, &self.acceptance_path] {
            ensure!(
                path.starts_with('/')
                    && !path.contains("//")
                    && !path.contains("..")
                    && path.len() <= 128
                    && path
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b)),
                "invalid fixed probe path"
            );
            ensure!(
                path != "/" && path != "/.forge/version",
                "health and acceptance require distinct application paths"
            );
        }
        ensure!(
            self.health_path != self.acceptance_path
                && digest(&self.health_body_sha256)
                && digest(&self.acceptance_body_sha256),
            "distinct owner checks with full digests required"
        );
        Ok(())
    }
    pub(super) fn sha256(&self) -> anyhow::Result<String> {
        self.validate()?;
        Ok(sha(&serde_json::to_vec(self)?))
    }
}

pub(super) async fn probe(
    policy: &DeliveryPolicy,
    path: &str,
    expected: &str,
    limit: u64,
) -> DeliveryProbe {
    let mut evidence = DeliveryProbe {
        status: DeliveryStatus::Unavailable,
        http_status: None,
        body_sha256: None,
        observed_at: Utc::now(),
    };
    let result = async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .build()?;
        let url = reqwest::Url::parse(&policy.origin)?.join(path)?;
        let mut response = client
            .get(url)
            .header("cache-control", "no-cache")
            .send()
            .await?;
        evidence.http_status = Some(response.status().as_u16());
        if response.status().as_u16() != 200 || response.content_length().is_some_and(|n| n > limit)
        {
            evidence.status = DeliveryStatus::Failed;
            return Ok::<_, anyhow::Error>(());
        }
        let mut hash = sha2::Sha256::new();
        let mut total = 0u64;
        use sha2::Digest;
        while let Some(chunk) = response.chunk().await? {
            total = total.saturating_add(chunk.len() as u64);
            if total > limit {
                evidence.status = DeliveryStatus::Failed;
                return Ok(());
            }
            hash.update(&chunk);
        }
        let actual = format!("{:x}", hash.finalize());
        evidence.status = if actual == expected {
            DeliveryStatus::Verified
        } else {
            DeliveryStatus::Failed
        };
        evidence.body_sha256 = Some(actual);
        Ok(())
    }
    .await;
    if result.is_err() {
        evidence.status = DeliveryStatus::Unavailable;
    }
    evidence
}
