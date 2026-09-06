//! Shared project/pipeline DTOs (ADR-0012 vertical split).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Project {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) repository_url: String,
    pub(crate) default_branch: String,
    /// K4.3 dispatch cap: null = unlimited.
    pub(crate) max_running_jobs: Option<i32>,
    pub(crate) created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateProject {
    pub(crate) name: String,
    pub(crate) repository_url: String,
    pub(crate) default_branch: Option<String>,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct ProjectMembership {
    pub(crate) project_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) username: String,
    pub(crate) user_enabled: bool,
    pub(crate) role: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct ProjectMembershipInput {
    pub(crate) user_id: Uuid,
    pub(crate) role: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct TriggerPipeline {
    pub(crate) git_ref: Option<String>,
    /// Optional run variables; exposed to every job as CICD_VAR_<KEY>.
    #[serde(default)]
    pub(crate) variables: Option<std::collections::BTreeMap<String, String>>,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Pipeline {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) git_ref: String,
    pub(crate) status: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Stage {
    pub(crate) id: Uuid,
    pub(crate) pipeline_id: Uuid,
    pub(crate) name: String,
    pub(crate) position: i32,
    pub(crate) status: String,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct Job {
    pub(crate) id: Uuid,
    pub(crate) stage_id: Uuid,
    pub(crate) name: String,
    pub(crate) image: String,
    pub(crate) command: String,
    pub(crate) required_tags: Vec<String>,
    pub(crate) required_secrets: Vec<String>,
    pub(crate) artifact_paths: Vec<String>,
    pub(crate) position: i32,
    pub(crate) status: String,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct PipelinePlan {
    pub(crate) pipeline_id: Uuid,
    pub(crate) config_source: String,
    pub(crate) parser_version: String,
    pub(crate) git_ref: String,
    pub(crate) resolved_commit_sha: Option<String>,
    pub(crate) config_sha256: String,
    pub(crate) plan_sha256: String,
    pub(crate) raw_config: String,
    pub(crate) plan: serde_json::Value,
    pub(crate) created_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct PipelineDetail {
    pub(crate) pipeline: Pipeline,
    pub(crate) plan: Option<PipelinePlan>,
    pub(crate) stages: Vec<StageDetail>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct StageDetail {
    #[serde(flatten)]
    pub(crate) stage: Stage,
    pub(crate) jobs: Vec<Job>,
}
