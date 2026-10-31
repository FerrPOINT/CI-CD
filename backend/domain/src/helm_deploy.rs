//! Application deployment metadata. Credentials remain an operator-owned grant.
use crate::image_build::valid_relative_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KubernetesTarget {
    pub schema: String,
    pub cluster: String,
    pub namespace: String,
    pub release: String,
    pub chart: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
    pub health_url: String,
}
fn dns(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || (i > 0 && i + 1 < value.len() && b == b'-')
        })
}
fn value_key(value: &str) -> bool {
    value.len() <= 256
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        })
        && value.split('.').next() != Some("_forge")
}
impl KubernetesTarget {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "forge/kubernetes-environment/v1"
            || !dns(&self.cluster)
            || !dns(&self.namespace)
            || !dns(&self.release)
            || self.release.len() > 53
            || !valid_relative_path(&self.chart, false)
            || self.files.len() > 16
            || !self
                .files
                .iter()
                .all(|(key, path)| value_key(key) && valid_relative_path(path, false))
            || self.values.len() > 16
            || !self.values.iter().all(|v| valid_relative_path(v, false))
        {
            return Err("invalid Kubernetes environment identity or repository paths".into());
        }
        let url = url::Url::parse(&self.health_url).map_err(|_| "invalid deployment health URL")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("deployment health requires a credential-free HTTPS URL".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageBinding {
    pub job: String,
    pub artifact: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HelmDeploySpec {
    pub environment_id: Uuid,
    pub images: BTreeMap<String, ImageBinding>,
    pub receipt_artifact: String,
}
impl HelmDeploySpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.environment_id.is_nil()
            || self.images.is_empty()
            || self.images.len() > 16
            || !valid_relative_path(&self.receipt_artifact, false)
        {
            return Err(
                "helm_deploy requires an environment identity and 1..16 image bindings".into(),
            );
        }
        for (value_path, binding) in &self.images {
            if !value_key(value_path)
                || binding.job.is_empty()
                || binding.job.len() > 63
                || !binding.job.bytes().enumerate().all(|(i, b)| {
                    b.is_ascii_alphabetic()
                        || (i > 0 && (b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')))
                })
                || !valid_relative_path(&binding.artifact, false)
            {
                return Err("invalid Helm image binding".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> KubernetesTarget {
        KubernetesTarget {
            schema: "forge/kubernetes-environment/v1".into(),
            cluster: "pdlc".into(),
            namespace: "r1-dev".into(),
            release: "r1".into(),
            chart: "deploy/helm/r1".into(),
            values: vec!["deploy/dev-values.yaml".into()],
            files: BTreeMap::new(),
            health_url: "https://r1-dev.example.test/health".into(),
        }
    }
    #[test]
    fn environment_does_not_depend_on_project_display_name() {
        target().validate().unwrap();
        let mut another = target();
        another.cluster = "other-platform".into();
        another.namespace = "project-two-dev".into();
        another.release = "application".into();
        another.validate().unwrap();
    }
    #[test]
    fn target_rejects_host_paths_credentials_and_injected_helm_options() {
        for path in ["../chart", "/chart", "-chart", "C:/chart"] {
            let mut t = target();
            t.chart = path.into();
            assert!(t.validate().is_err());
        }
        for url in [
            "http://dev.test/health",
            "https://u:p@dev.test/",
            "https://dev.test/?key=x",
        ] {
            let mut t = target();
            t.health_url = url.into();
            assert!(t.validate().is_err());
        }
        let mut value = serde_json::to_value(target()).unwrap();
        value["kubeconfig"] = serde_json::json!("/operator/admin");
        assert!(serde_json::from_value::<KubernetesTarget>(value).is_err());
    }
}

/// Immutable execution input; never contains a kubeconfig or service-account token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentOffer {
    pub environment_id: Uuid,
    pub deployment_id: Uuid,
    pub pipeline_id: Uuid,
    pub job_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_id: Uuid,
    pub generation: i64,
    pub source_commit: String,
    pub pipeline_order: i64,
    pub target: KubernetesTarget,
    pub images: BTreeMap<String, crate::published_image::PublishedImage>,
    pub receipt_artifact: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<RollbackRelease>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentReceipt {
    pub schema: String,
    pub deployment_id: Uuid,
    pub pipeline_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_id: Uuid,
    pub generation: i64,
    pub source_commit: String,
    pub cluster: String,
    pub namespace: String,
    pub namespace_uid: Uuid,
    pub release: String,
    pub release_revision: i32,
    pub images: BTreeMap<String, String>,
    pub health_status: u16,
    pub health_transport: String,
}
impl DeploymentReceipt {
    pub fn validates(&self, offer: &DeploymentOffer) -> bool {
        self.schema == "forge/kubernetes-deployment/v1"
            && self.deployment_id == offer.deployment_id
            && self.pipeline_id == offer.pipeline_id
            && self.attempt_id == offer.attempt_id
            && self.lease_id == offer.lease_id
            && self.generation == offer.generation
            && self.source_commit == offer.source_commit
            && self.cluster == offer.target.cluster
            && self.namespace == offer.target.namespace
            && !self.namespace_uid.is_nil()
            && self.release == offer.target.release
            && self.release_revision > 0
            && (200..300).contains(&self.health_status)
            && matches!(
                self.health_transport.as_str(),
                "public-https" | "cluster-service-http"
            )
            && self.images
                == offer
                    .images
                    .iter()
                    .map(|(key, image)| (key.clone(), image.image_ref.clone()))
                    .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseMetadata {
    pub environment_id: Uuid,
    pub deployment_id: Uuid,
    pub pipeline_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_id: Uuid,
    pub generation: i64,
    pub source_commit: String,
    pub pipeline_order: i64,
    pub images: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollbackRelease {
    pub revision: i32,
    pub expected_current_revision: i32,
    pub metadata: ReleaseMetadata,
}
impl ReleaseMetadata {
    pub fn installed(offer: &DeploymentOffer) -> Self {
        if let Some(rollback) = &offer.rollback {
            return rollback.metadata.clone();
        }
        Self {
            environment_id: offer.environment_id,
            deployment_id: offer.deployment_id,
            pipeline_id: offer.pipeline_id,
            attempt_id: offer.attempt_id,
            lease_id: offer.lease_id,
            generation: offer.generation,
            source_commit: offer.source_commit.clone(),
            pipeline_order: offer.pipeline_order,
            images: offer
                .images
                .iter()
                .map(|(key, image)| (key.clone(), image.image_ref.clone()))
                .collect(),
        }
    }
}
impl DeploymentOffer {
    pub fn validate(&self) -> Result<(), String> {
        self.target.validate()?;
        if [
            self.environment_id,
            self.deployment_id,
            self.pipeline_id,
            self.job_id,
            self.attempt_id,
            self.lease_id,
        ]
        .iter()
        .any(Uuid::is_nil)
            || self.generation <= 0
            || self.pipeline_order <= 0
            || !crate::image_build::valid_commit(&self.source_commit)
            || !valid_relative_path(&self.receipt_artifact, false)
            || self.images.is_empty()
            || self.images.len() > 16
            || self.images.iter().any(|(key, image)| {
                !value_key(key)
                    || image.validate().is_err()
                    || image.source_commit != self.source_commit
            })
        {
            return Err("invalid immutable Helm execution input".into());
        }
        if let Some(rollback) = &self.rollback {
            if rollback.revision <= 0
                || rollback.expected_current_revision < rollback.revision
                || rollback.metadata.environment_id != self.environment_id
                || rollback.metadata.source_commit != self.source_commit
                || rollback.metadata.images
                    != self
                        .images
                        .iter()
                        .map(|(key, image)| (key.clone(), image.image_ref.clone()))
                        .collect()
            {
                return Err("invalid rollback release snapshot".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentObservation {
    pub schema: String,
    pub phase: String,
    pub deployment_id: Uuid,
    pub pipeline_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_id: Uuid,
    pub generation: i64,
    pub source_commit: String,
    pub namespace_uid: Uuid,
    pub release_revision: Option<i32>,
    pub receipt: Option<DeploymentReceipt>,
}
impl DeploymentObservation {
    pub fn validates(&self, offer: &DeploymentOffer) -> bool {
        self.schema == "forge/kubernetes-deployment-observation/v1"
            && self.deployment_id == offer.deployment_id
            && self.pipeline_id == offer.pipeline_id
            && self.attempt_id == offer.attempt_id
            && self.lease_id == offer.lease_id
            && self.generation == offer.generation
            && self.source_commit == offer.source_commit
            && !self.namespace_uid.is_nil()
            && self.release_revision.is_none_or(|v| v > 0)
            && if self.phase == "deployed" {
                self.receipt.as_ref().is_some_and(|receipt| {
                    receipt.validates(offer)
                        && receipt.namespace_uid == self.namespace_uid
                        && Some(receipt.release_revision) == self.release_revision
                })
            } else {
                matches!(self.phase.as_str(), "failed" | "no-effect" | "superseded")
                    && self.receipt.is_none()
            }
    }
}
