use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{
    domain::JobStatus,
    git_host::{
        create_repository, delete_repository, git_info_refs, git_service_endpoint,
        internal_git_push, list_repositories,
    },
    pulls::{
        compare_refs, create_pull_request, list_commits, list_pull_requests, list_refs, pr_action,
    },
    store::next_log_sequence,
};

#[derive(Clone, Default)]
pub struct AppState {
    pub pool: Option<PgPool>,
    pub git: crate::git_host::GitConfig,
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}
impl ApiError {
    pub(crate) fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "database is unavailable".into(),
        }
    }
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    pub(crate) fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "resource not found".into(),
        }
    }
    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".into(),
        }
    }
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
    pub(crate) fn internal(error: sqlx::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({"error": self.message})),
        )
            .into_response()
    }
}

fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.pool.as_ref().ok_or_else(ApiError::unavailable)
}

pub fn app(pool: Option<PgPool>) -> Router {
    app_with_git(pool, crate::git_host::GitConfig::default())
}

pub fn app_with_git(pool: Option<PgPool>, git: crate::git_host::GitConfig) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route(
            "/api/v1/projects/{project_id}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
        .route(
            "/api/v1/projects/{project_id}/pipelines",
            get(list_pipelines).post(trigger_pipeline),
        )
        .route("/api/v1/pipelines/{pipeline_id}", get(get_pipeline))
        .route("/api/v1/jobs/{job_id}/status", post(change_job_status))
        .route(
            "/api/v1/jobs/{job_id}/logs",
            get(list_logs).post(append_log),
        )
        .route(
            "/api/v1/repositories",
            get(list_repositories).post(create_repository),
        )
        .route(
            "/api/v1/repositories/{name}",
            axum::routing::delete(delete_repository),
        )
        .route("/git/{repo}/info/refs", get(git_info_refs))
        .route("/git/{repo}/git-upload-pack", post(git_service_endpoint))
        .route("/git/{repo}/git-receive-pack", post(git_service_endpoint))
        .route("/api/v1/internal/git-push", post(internal_git_push))
        .route("/api/v1/repos/{repo}/refs", get(list_refs))
        .route("/api/v1/repos/{repo}/commits", get(list_commits))
        .route("/api/v1/repos/{repo}/compare", get(compare_refs))
        .route(
            "/api/v1/repos/{repo}/pulls",
            get(list_pull_requests).post(create_pull_request),
        )
        .route(
            "/api/v1/repos/{repo}/pulls/{number}/action",
            post(pr_action),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(AppState { pool, git }))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "cicd"}))
}

#[derive(Debug, Serialize, FromRow)]
struct Project {
    id: Uuid,
    name: String,
    repository_url: String,
    default_branch: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize)]
struct CreateProject {
    name: String,
    repository_url: String,
    default_branch: Option<String>,
}

async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateProject>,
) -> ApiResult<Project> {
    if input.name.trim().is_empty() || input.repository_url.trim().is_empty() {
        return Err(ApiError::bad_request(
            "name and repository_url are required",
        ));
    }
    let project = sqlx::query_as::<_, Project>(
        "INSERT INTO projects (id, name, repository_url, default_branch) VALUES ($1, $2, $3, $4) RETURNING id, name, repository_url, default_branch, created_at"
    ).bind(Uuid::new_v4()).bind(input.name.trim()).bind(input.repository_url.trim()).bind(input.default_branch.unwrap_or_else(|| "main".into())).fetch_one(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(project))
}

async fn list_projects(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Project>> {
    let projects = sqlx::query_as::<_, Project>("SELECT id, name, repository_url, default_branch, created_at FROM projects ORDER BY created_at DESC").fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(projects))
}

async fn get_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Project> {
    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, repository_url, default_branch, created_at FROM projects WHERE id = $1",
    )
    .bind(project_id)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(project))
}

#[derive(Debug, Deserialize, Default)]
struct UpdateProject {
    name: Option<String>,
    repository_url: Option<String>,
    default_branch: Option<String>,
}

async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<UpdateProject>,
) -> ApiResult<Project> {
    if let (None, None, None) = (&input.name, &input.repository_url, &input.default_branch) {
        return Err(ApiError::bad_request(
            "at least one of name, repository_url, default_branch is required",
        ));
    }
    for field in [&input.name, &input.repository_url, &input.default_branch]
        .into_iter()
        .flatten()
    {
        if field.trim().is_empty() {
            return Err(ApiError::bad_request("fields cannot be empty"));
        }
    }
    let project = sqlx::query_as::<_, Project>(
        "UPDATE projects SET name = COALESCE($2, name), repository_url = COALESCE($3, repository_url), default_branch = COALESCE($4, default_branch) WHERE id = $1 RETURNING id, name, repository_url, default_branch, created_at",
    )
    .bind(project_id)
    .bind(input.name.as_deref().map(str::trim))
    .bind(input.repository_url.as_deref().map(str::trim))
    .bind(input.default_branch.as_deref().map(str::trim))
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(project))
}

async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let deleted = sqlx::query_scalar::<_, Uuid>("DELETE FROM projects WHERE id = $1 RETURNING id")
        .bind(project_id)
        .fetch_optional(pool(&state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({"deleted": deleted})))
}

#[derive(Debug, Deserialize)]
struct TriggerPipeline {
    git_ref: Option<String>,
}
#[derive(Debug, Serialize, FromRow)]
pub(crate) struct Pipeline {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) git_ref: String,
    pub(crate) status: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, FromRow)]
struct Stage {
    id: Uuid,
    pipeline_id: Uuid,
    name: String,
    position: i32,
    status: String,
}
#[derive(Debug, Serialize, FromRow)]
struct Job {
    id: Uuid,
    stage_id: Uuid,
    name: String,
    image: String,
    command: String,
    position: i32,
    status: String,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize)]
struct PipelineDetail {
    pipeline: Pipeline,
    stages: Vec<StageDetail>,
}
#[derive(Debug, Serialize)]
struct StageDetail {
    #[serde(flatten)]
    stage: Stage,
    jobs: Vec<Job>,
}

async fn trigger_pipeline(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<TriggerPipeline>,
) -> ApiResult<PipelineDetail> {
    let pool = pool(&state)?;
    let project_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1)")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if !project_exists {
        return Err(ApiError::not_found());
    }
    let git_ref = input.git_ref.unwrap_or_else(|| "main".into());
    let pipeline = create_pipeline(pool, project_id, git_ref).await?;
    pipeline_detail(pool, pipeline.id).await.map(Json)
}

/// Creates a queued pipeline with the default build/test/deploy template stages.
pub(crate) async fn create_pipeline(
    pool: &PgPool,
    project_id: Uuid,
    git_ref: String,
) -> Result<Pipeline, ApiError> {
    let pipeline = sqlx::query_as::<_, Pipeline>(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, $3, 'queued') RETURNING id, project_id, git_ref, status, created_at, started_at, finished_at",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(git_ref)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    let templates = [
        ("build", "checkout", "alpine/git:latest", "git fetch --all"),
        ("test", "unit-tests", "rust:1.86", "cargo test"),
        ("deploy", "deploy", "alpine:3.21", "echo deploy"),
    ];
    for (position, (stage_name, job_name, image, command)) in templates.iter().enumerate() {
        let stage_id = Uuid::new_v4();
        sqlx::query("INSERT INTO stages (id, pipeline_id, name, position, status) VALUES ($1, $2, $3, $4, 'queued')")
            .bind(stage_id).bind(pipeline.id).bind(*stage_name).bind(position as i32).execute(pool).await.map_err(ApiError::internal)?;
        sqlx::query("INSERT INTO jobs (id, stage_id, name, image, command, position, status) VALUES ($1, $2, $3, $4, $5, 0, 'queued')")
            .bind(Uuid::new_v4()).bind(stage_id).bind(*job_name).bind(*image).bind(*command).execute(pool).await.map_err(ApiError::internal)?;
    }
    Ok(pipeline)
}

async fn list_pipelines(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Pipeline>> {
    let pipelines = sqlx::query_as::<_, Pipeline>("SELECT id, project_id, git_ref, status, created_at, started_at, finished_at FROM pipelines WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(pipelines))
}
async fn get_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<PipelineDetail> {
    pipeline_detail(pool(&state)?, pipeline_id).await.map(Json)
}

async fn pipeline_detail(pool: &PgPool, pipeline_id: Uuid) -> Result<PipelineDetail, ApiError> {
    let pipeline = sqlx::query_as::<_, Pipeline>("SELECT id, project_id, git_ref, status, created_at, started_at, finished_at FROM pipelines WHERE id = $1").bind(pipeline_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let stages = sqlx::query_as::<_, Stage>("SELECT id, pipeline_id, name, position, status FROM stages WHERE pipeline_id = $1 ORDER BY position").bind(pipeline_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut details = Vec::with_capacity(stages.len());
    for stage in stages {
        let jobs = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE stage_id = $1 ORDER BY position").bind(stage.id).fetch_all(pool).await.map_err(ApiError::internal)?;
        details.push(StageDetail { stage, jobs });
    }
    Ok(PipelineDetail {
        pipeline,
        stages: details,
    })
}

#[derive(Deserialize)]
struct ChangeStatus {
    status: JobStatus,
}
async fn change_job_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(input): Json<ChangeStatus>,
) -> ApiResult<Job> {
    let pool = pool(&state)?;
    let job = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE id = $1").bind(job_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let current = JobStatus::try_from(job.status.as_str()).map_err(ApiError::bad_request)?;
    current
        .transition_to(input.status)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = $2, started_at = CASE WHEN $2 = 'running' THEN now() ELSE started_at END, finished_at = CASE WHEN $2 IN ('success','failed','canceled') THEN now() ELSE finished_at END WHERE id = $1 RETURNING id, stage_id, name, image, command, position, status, started_at, finished_at").bind(job_id).bind(input.status.as_str()).fetch_one(pool).await.map_err(ApiError::internal)?;
    refresh_statuses(pool, updated.stage_id).await?;
    Ok(Json(updated))
}

async fn refresh_statuses(pool: &PgPool, stage_id: Uuid) -> Result<(), ApiError> {
    let stage_status: String = sqlx::query_scalar("SELECT CASE WHEN bool_or(status = 'failed') THEN 'failed' WHEN bool_and(status = 'success') THEN 'success' WHEN bool_or(status = 'running') THEN 'running' WHEN bool_or(status = 'canceled') THEN 'canceled' ELSE 'queued' END FROM jobs WHERE stage_id = $1").bind(stage_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let pipeline_id: Uuid =
        sqlx::query_scalar("UPDATE stages SET status = $2 WHERE id = $1 RETURNING pipeline_id")
            .bind(stage_id)
            .bind(stage_status)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    let pipeline_status: String = sqlx::query_scalar("SELECT CASE WHEN bool_or(status = 'failed') THEN 'failed' WHEN bool_and(status = 'success') THEN 'success' WHEN bool_or(status = 'running') THEN 'running' WHEN bool_or(status = 'canceled') THEN 'canceled' ELSE 'queued' END FROM stages WHERE pipeline_id = $1").bind(pipeline_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    sqlx::query("UPDATE pipelines SET status = $2, started_at = CASE WHEN $2 = 'running' THEN COALESCE(started_at, now()) ELSE started_at END, finished_at = CASE WHEN $2 IN ('success','failed','canceled') THEN now() ELSE finished_at END WHERE id = $1").bind(pipeline_id).bind(pipeline_status).execute(pool).await.map_err(ApiError::internal)?;
    Ok(())
}

#[derive(Debug, Serialize, FromRow)]
struct JobLog {
    id: i64,
    job_id: Uuid,
    sequence: i32,
    message: String,
    created_at: DateTime<Utc>,
}
#[derive(Deserialize)]
struct AppendLog {
    message: String,
}
async fn list_logs(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<JobLog>> {
    let logs = sqlx::query_as::<_, JobLog>("SELECT id, job_id, sequence, message, created_at FROM job_logs WHERE job_id = $1 ORDER BY sequence").bind(job_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(logs))
}
async fn append_log(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(input): Json<AppendLog>,
) -> ApiResult<JobLog> {
    if input.message.trim().is_empty() {
        return Err(ApiError::bad_request("message is required"));
    }
    let pool = pool(&state)?;
    let log = sqlx::query_as::<_, JobLog>("INSERT INTO job_logs (job_id, sequence, message) VALUES ($1, $2, $3) RETURNING id, job_id, sequence, message, created_at").bind(job_id).bind(next_log_sequence(pool, job_id).await.map_err(ApiError::internal)?).bind(input.message.trim()).fetch_one(pool).await.map_err(ApiError::internal)?;
    Ok(Json(log))
}
