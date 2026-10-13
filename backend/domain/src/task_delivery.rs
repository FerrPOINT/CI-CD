//! Technical delivery evidence; Tracker remains the lifecycle and business-completion owner.
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
    /// Current Tracker DevOps context, not a caller-issued admission receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_fence: Option<TaskDeliveryFence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskDeliveryFence {
    pub development_result_id: Uuid,
    pub assignment_id: Uuid,
    pub execution_id: Uuid,
    pub agent_id: Uuid,
    pub lease_id: Uuid,
    pub fencing_token: i64,
    pub expected_lease_version: i64,
    pub assignment_hash: String,
}

impl DeliveryCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(fence) = &self.task_fence
            && ([
                fence.development_result_id,
                fence.assignment_id,
                fence.execution_id,
                fence.agent_id,
                fence.lease_id,
            ]
            .iter()
            .any(Uuid::is_nil)
                || !(1..=9007199254740991).contains(&fence.fencing_token)
                || !(1..=9007199254740991).contains(&fence.expected_lease_version)
                || !digest(&fence.assignment_hash))
        {
            return Err("Нужен точный положительный Tracker delivery fence");
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_command_roundtrip_does_not_add_a_task_fence() {
        let id = Uuid::new_v4();
        let value = json!({"operationKey":"deploy:1","workspaceOperationKey":"prepare:1",
            "original":{"requestHash":"a".repeat(64),"taskId":id,"rootTaskId":id,
                "assignmentId":id,"executionId":id,"fencingToken":1},
            "action":"deploy","artifactId":id,"expectedManifestSha256":null});
        let command: DeliveryCommand = serde_json::from_value(value.clone()).unwrap();
        command.validate().unwrap();
        assert!(command.task_fence.is_none());
        assert_eq!(serde_json::to_value(command).unwrap(), value);
    }

    #[test]
    fn task_fence_is_exact_positive_context_not_an_authority_flag() {
        let id = Uuid::new_v4();
        let value = json!({"developmentResultId":id,"assignmentId":id,"executionId":id,"agentId":id,"leaseId":id,
            "fencingToken":1,"expectedLeaseVersion":1,"assignmentHash":"a".repeat(64)});
        assert!(serde_json::from_value::<TaskDeliveryFence>(value.clone()).is_ok());
        for field in [
            "developmentResultId",
            "assignmentId",
            "executionId",
            "agentId",
            "leaseId",
            "fencingToken",
            "expectedLeaseVersion",
            "assignmentHash",
        ] {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<TaskDeliveryFence>(bad).is_err());
        }
        let mut injected = value;
        injected["accepted"] = json!(true);
        assert!(serde_json::from_value::<TaskDeliveryFence>(injected).is_err());
    }
}
