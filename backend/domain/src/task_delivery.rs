//! Local technical delivery evidence. Never Tracker admission or SDLC dispatch authority.
use super::sdlc_workspace::{WorkspaceOperationLookup, WorkspaceOperationReceipt};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryAction {
    Deploy,
    Rollback,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryCommand {
    pub operation_key: String,
    pub workspace_operation_key: String,
    pub original: WorkspaceOperationLookup,
    pub action: DeliveryAction,
    pub artifact_id: Option<Uuid>,
    /// CAS against the published manifest; null means an empty target, never wildcard.
    pub expected_manifest_sha256: Option<String>,
}

impl DeliveryCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        for key in [&self.operation_key, &self.workspace_operation_key] {
            if key.is_empty()
                || key.len() > 128
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            {
                return Err("invalid original operation key");
            }
        }
        if !digest(&self.original.request_hash)
            || self
                .expected_manifest_sha256
                .as_ref()
                .is_some_and(|h| !digest(h))
        {
            return Err("full lowercase SHA256 is required");
        }
        if [
            self.original.task_id,
            self.original.root_task_id,
            self.original.assignment_id,
            self.original.execution_id,
        ]
        .iter()
        .any(Uuid::is_nil)
            || self.original.fencing_token <= 0
        {
            return Err("original task identity is required");
        }
        match self.action {
            DeliveryAction::Deploy if self.artifact_id.is_none_or(|id| id.is_nil()) => {
                Err("deploy requires exact artifact ID")
            }
            DeliveryAction::Rollback
                if self.artifact_id.is_some() || self.expected_manifest_sha256.is_none() =>
            {
                Err("rollback uses only the last confirmed manifest and exact current CAS")
            }
            _ => Ok(()),
        }
    }
}

pub fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Unavailable,
    Failed,
    Unknown,
    Verified,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryProbe {
    pub status: DeliveryStatus,
    pub http_status: Option<u16>,
    pub body_sha256: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryManifest {
    pub schema: String,
    pub operation_receipt: WorkspaceOperationReceipt,
    pub pipeline_id: Uuid,
    pub artifact_id: Uuid,
    pub artifact_attempt_id: Uuid,
    pub artifact_sha256: String,
    pub artifact_size_bytes: i64,
    pub config_sha256: String,
    pub plan_sha256: String,
    pub target_policy_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryReceipt {
    pub schema: String,
    pub scope: String,
    pub command: DeliveryCommand,
    pub command_sha256: String,
    pub original_operation: WorkspaceOperationReceipt,
    pub manifest_sha256: Option<String>,
    pub previous_manifest_sha256: Option<String>,
    pub status: DeliveryStatus,
    pub reason: String,
    pub version: Option<DeliveryProbe>,
    pub served_artifact: Option<DeliveryProbe>,
    pub health: Option<DeliveryProbe>,
    pub acceptance: Option<DeliveryProbe>,
    pub recorded_at: DateTime<Utc>,
    pub dispatch_allowed: bool,
    pub sdlc_acceptance_verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryReadback {
    pub receipt: DeliveryReceipt,
    pub reconciled_receipt: Option<DeliveryReceipt>,
    pub current_manifest_sha256: Option<String>,
    pub confirmed_manifest_sha256: Option<String>,
    pub reconciliation_needed: bool,
}
