use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTaskBinding {
    #[schema(min_length = 1, max_length = 256)]
    pub tracker_instance_id: String,
    pub tracker_project_id: Uuid,
    pub task_id: Uuid,
    pub root_task_id: Uuid,
    pub assignment_id: Uuid,
    pub execution_id: Uuid,
    pub routing_snapshot_id: Uuid,
    #[schema(minimum = 1, maximum = 9007199254740991i64)]
    pub requirement_revision: i64,
    #[schema(minimum = 1, maximum = 9007199254740991i64)]
    pub fencing_token: i64,
    pub assignment_hash: String,
    pub workflow_task_ref: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    Analyst,
    Architect,
    Developer,
    Reviewer,
    Tester,
    Devops,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    ReadOnly,
    ReadWrite,
}

/// A request is declared input, never admission evidence or a caller-issued receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceOperationRequest {
    pub contract_version: u32,
    pub operation_key: String,
    pub binding: WorkspaceTaskBinding,
    pub repository_id: Uuid,
    pub source_commit: String,
    pub lease_id: Uuid,
    pub attempt_id: Uuid,
    pub workspace_generation: i64,
    pub workspace_id: String,
    pub role: WorkspaceRole,
    pub access: WorkspaceAccess,
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl WorkspaceOperationRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        let b = &self.binding;
        if self.contract_version != 1 {
            return Err("unsupported contractVersion");
        }
        if self.operation_key.is_empty()
            || self.operation_key.len() > 128
            || !self
                .operation_key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        {
            return Err("invalid operationKey");
        }
        if [
            b.tracker_project_id,
            b.task_id,
            b.root_task_id,
            b.assignment_id,
            b.execution_id,
            b.routing_snapshot_id,
            self.repository_id,
            self.lease_id,
            self.attempt_id,
        ]
        .iter()
        .any(Uuid::is_nil)
        {
            return Err("nil identity is not allowed");
        }
        if b.tracker_instance_id.trim().is_empty()
            || b.tracker_instance_id.len() > 256
            || b.tracker_instance_id.trim() != b.tracker_instance_id
            || b.tracker_instance_id.chars().any(char::is_control)
        {
            return Err("invalid exact Tracker instance namespace");
        }
        if b.requirement_revision <= 0
            || b.requirement_revision > 9007199254740991
            || b.fencing_token <= 0
            || b.fencing_token > 9007199254740991
            || self.workspace_generation <= 0
        {
            return Err("revisions and fences must be positive");
        }
        if !lower_hex(&b.assignment_hash, 64) || !lower_hex(&self.source_commit, 40) {
            return Err("full lowercase assignment hash and source SHA are required");
        }
        let ordinal = b.workflow_task_ref.strip_prefix("SDLC-").unwrap_or("");
        if ordinal.is_empty()
            || ordinal.starts_with('0')
            || !ordinal.bytes().all(|b| b.is_ascii_digit())
            || ordinal.parse::<i64>().ok().filter(|n| *n > 0).is_none()
        {
            return Err("invalid workflowTaskRef");
        }
        let prefix = format!("attempt-{}-{}-", self.attempt_id, self.workspace_generation);
        let nonce = self.workspace_id.strip_prefix(&prefix).unwrap_or("");
        if !lower_hex(nonce, 32) || Uuid::parse_str(nonce).map_or(true, |id| id.is_nil()) {
            return Err("workspaceId must match attempt and generation");
        }
        if self.access == WorkspaceAccess::ReadWrite && self.role != WorkspaceRole::Developer {
            return Err("only Developer may request scoped write access");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceOperationStatus {
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceOperationBlocker {
    TrackerAdmissionUnavailable,
    TrackerWorkspaceBindingUnavailable,
    PhysicalWorkspaceObservationUnavailable,
}

/// Owner-issued operation receipt. Not base-sdlc/workspace-receipt/v1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceOperationReceipt {
    pub schema: String,
    pub operation_id: Uuid,
    pub project_id: Uuid,
    pub request_hash: String,
    pub request: WorkspaceOperationRequest,
    pub recorded_at: DateTime<Utc>,
    pub status: WorkspaceOperationStatus,
    pub blockers: Vec<WorkspaceOperationBlocker>,
    pub physical_source_observed: bool,
    pub dispatch_allowed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceOperationLookup {
    pub request_hash: String,
    pub task_id: Uuid,
    pub root_task_id: Uuid,
    pub assignment_id: Uuid,
    pub execution_id: Uuid,
    pub fencing_token: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceOperationReadback {
    pub receipt: WorkspaceOperationReceipt,
    pub lease_status: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub expired: bool,
    pub current_generation: Option<i64>,
    pub reconciliation_needed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> WorkspaceOperationRequest {
        let attempt = Uuid::new_v4();
        WorkspaceOperationRequest {
            contract_version: 1,
            operation_key: "workspace:prepare:1".into(),
            binding: WorkspaceTaskBinding {
                tracker_instance_id: "tracker-prod.sdlc1".into(),
                tracker_project_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                root_task_id: Uuid::new_v4(),
                assignment_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
                routing_snapshot_id: Uuid::new_v4(),
                requirement_revision: 1,
                fencing_token: 1,
                assignment_hash: "a".repeat(64),
                workflow_task_ref: "SDLC-7".into(),
            },
            repository_id: Uuid::new_v4(),
            source_commit: "b".repeat(40),
            lease_id: Uuid::new_v4(),
            attempt_id: attempt,
            workspace_generation: 2,
            workspace_id: format!("attempt-{attempt}-2-{}", Uuid::new_v4().simple()),
            role: WorkspaceRole::Developer,
            access: WorkspaceAccess::ReadWrite,
        }
    }
    #[test]
    fn concrete_binding_accepts_separate_tracker_and_workspace_fences() {
        assert!(request().validate().is_ok());
    }
    #[test]
    fn readonly_roles_never_request_write() {
        for role in [
            WorkspaceRole::Analyst,
            WorkspaceRole::Architect,
            WorkspaceRole::Reviewer,
            WorkspaceRole::Tester,
            WorkspaceRole::Devops,
        ] {
            let mut r = request();
            r.role = role;
            assert!(r.validate().is_err());
            r.access = WorkspaceAccess::ReadOnly;
            assert!(r.validate().is_ok());
        }
    }
    #[test]
    fn full_lowercase_pins_only() {
        let mut r = request();
        r.source_commit = "main".into();
        assert!(r.validate().is_err());
        r.source_commit = "B".repeat(40);
        assert!(r.validate().is_err());
        r.source_commit = "b".repeat(40);
        r.binding.assignment_hash = "f".repeat(63);
        assert!(r.validate().is_err());
    }
    #[test]
    fn non_nil_positive_identities_and_backend_task_ref_required() {
        let mut r = request();
        r.binding.task_id = Uuid::nil();
        assert!(r.validate().is_err());
        r.binding.task_id = Uuid::new_v4();
        r.binding.fencing_token = 0;
        assert!(r.validate().is_err());
        r.binding.fencing_token = 1;
        r.binding.fencing_token = 9007199254740992;
        assert!(r.validate().is_err());
        r.binding.fencing_token = 1;
        r.binding.tracker_instance_id = " ".into();
        assert!(r.validate().is_err());
        r.binding.tracker_instance_id = "tracker-prod.sdlc1".into();
        for invalid in [
            "SDLC-0",
            "SDLC-01",
            "SDLC-1x",
            "SDLC-+1",
            "TASK-1",
            "SDLC-9223372036854775808",
        ] {
            r.binding.workflow_task_ref = invalid.into();
            assert!(r.validate().is_err());
        }
    }
    #[test]
    fn workspace_and_key_cannot_substitute_identity_or_path() {
        let mut r = request();
        r.workspace_generation = 3;
        assert!(r.validate().is_err());
        r.workspace_generation = 2;
        r.workspace_id.push_str("/../x");
        assert!(r.validate().is_err());
        r = request();
        r.operation_key = "a/b".into();
        assert!(r.validate().is_err());
    }
}
