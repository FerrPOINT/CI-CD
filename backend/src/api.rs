use std::sync::Arc;
use std::time::Duration;

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
    pub running_jobs: Option<crate::runner::RunningJobs>,
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// OpenAPI 3 document for the current API surface (API_CONTRACT, utoipa).
#[derive(utoipa::OpenApi)]
#[openapi(
    info(title = "Forge CI/CD API", version = "0.1.0", description = "Self-hosted CI/CD control plane. Array responses and error envelope follow docs/contracts/API_CONTRACT.md current compatibility mode."),
    paths(
        health, list_projects, create_project, get_project, update_project, delete_project,
        trigger_pipeline, list_pipelines, get_pipeline, cancel_pipeline, retry_pipeline,
        change_job_status, retry_job, list_logs, append_log,
    ),
    components(schemas(Project, CreateProject, UpdateProject, TriggerPipeline, Pipeline, Stage, Job, PipelineDetail, StageDetail, JobLog, ChangeStatus)),
    tags(
        (name = "health", description = "Liveness/readiness"),
        (name = "projects", description = "Project registry"),
        (name = "pipelines", description = "Pipeline lifecycle"),
        (name = "jobs", description = "Jobs, logs and retries"),
    )
)]
pub struct ApiDoc;

pub(crate) async fn serve_openapi_json() -> Json<serde_json::Value> {
    use utoipa::OpenApi as _;
    Json(serde_json::to_value(ApiDoc::openapi()).expect("serialize openapi"))
}

/// Canonical YAML serialization of the OpenAPI document (openapi-dump bin).
pub fn openapi_yaml() -> Result<String, serde_yaml::Error> {
    use utoipa::OpenApi as _;
    serde_yaml::to_string(&ApiDoc::openapi())
}

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

pub(crate) fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.pool.as_ref().ok_or_else(ApiError::unavailable)
}

pub fn app(pool: Option<PgPool>) -> Router {
    app_with(pool, None)
}

pub fn app_with_git(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    build_router(pool, git, running)
}

#[allow(dead_code)]
fn app_with(pool: Option<PgPool>, running: Option<crate::runner::RunningJobs>) -> Router {
    build_router(pool, crate::git_host::GitConfig::default(), running)
}

fn build_router(
    pool: Option<PgPool>,
    git: crate::git_host::GitConfig,
    running: Option<crate::runner::RunningJobs>,
) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/openapi.json", get(serve_openapi_json))
        .merge(crate::platform::routes())
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
            "/api/v1/pipelines/{pipeline_id}/cancel",
            post(cancel_pipeline),
        )
        .route(
            "/api/v1/pipelines/{pipeline_id}/retry",
            post(retry_pipeline),
        )
        .route("/api/v1/jobs/{job_id}/retry", post(retry_job))
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
        .with_state(Arc::new(AppState {
            pool,
            git,
            running_jobs: running,
        }))
}

#[utoipa::path(get, path="/api/v1/health", tag="health", responses((status=200, description="Liveness and readiness")))]
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "cicd"}))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct Project {
    id: Uuid,
    name: String,
    repository_url: String,
    default_branch: String,
    created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CreateProject {
    name: String,
    repository_url: String,
    default_branch: Option<String>,
}

#[utoipa::path(post, path="/api/v1/projects", tag="projects", request_body=CreateProject, responses((status=200, body=Project), (status=400, description="validation error")))]
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

#[utoipa::path(get, path="/api/v1/projects", tag="projects", responses((status=200, body=[Project])))]
async fn list_projects(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Project>> {
    let projects = sqlx::query_as::<_, Project>("SELECT id, name, repository_url, default_branch, created_at FROM projects ORDER BY created_at DESC").fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(projects))
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}", tag="projects", params(("project_id"=Uuid, Path)), responses((status=200, body=Project), (status=404)))]
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

#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
struct UpdateProject {
    name: Option<String>,
    repository_url: Option<String>,
    default_branch: Option<String>,
}

#[utoipa::path(patch, path="/api/v1/projects/{project_id}", tag="projects", request_body=UpdateProject, params(("project_id"=Uuid, Path)), responses((status=200, body=Project), (status=404)))]
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

#[utoipa::path(delete, path="/api/v1/projects/{project_id}", tag="projects", params(("project_id"=Uuid, Path)), responses((status=200), (status=404)))]
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct TriggerPipeline {
    git_ref: Option<String>,
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
struct Stage {
    id: Uuid,
    pipeline_id: Uuid,
    name: String,
    position: i32,
    status: String,
}
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
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
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct PipelineDetail {
    pipeline: Pipeline,
    stages: Vec<StageDetail>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct StageDetail {
    #[serde(flatten)]
    stage: Stage,
    jobs: Vec<Job>,
}

#[utoipa::path(post, path="/api/v1/projects/{project_id}/pipelines", tag="pipelines", request_body=TriggerPipeline, params(("project_id"=Uuid, Path)), responses((status=200, body=Pipeline), (status=404)))]
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

/// Creates a queued pipeline. Stage/job structure comes from `.forge-ci.yml`
/// in the project repository at the given ref when available; otherwise the
/// default build/test/deploy template is used.
pub(crate) async fn create_pipeline(
    pool: &PgPool,
    project_id: Uuid,
    git_ref: String,
) -> Result<Pipeline, ApiError> {
    let repository_url: Option<String> =
        sqlx::query_scalar("SELECT repository_url FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
            .flatten();
    // Never clone here: this path is called by post-receive and must return
    // before git-receive-pack finishes. Local config is read from the bare repo.
    let config = read_local_forge_ci_config(repository_url.as_deref(), &git_ref).await;
    let stages = parse_forge_ci(config.as_deref()).map_err(ApiError::bad_request)?;

    let pipeline = sqlx::query_as::<_, Pipeline>(
        "INSERT INTO pipelines (id, project_id, git_ref, status) VALUES ($1, $2, $3, 'queued') RETURNING id, project_id, git_ref, status, created_at, started_at, finished_at",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(&git_ref)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    for (position, stage) in stages.iter().enumerate() {
        let stage_id = Uuid::new_v4();
        sqlx::query("INSERT INTO stages (id, pipeline_id, name, position, status) VALUES ($1, $2, $3, $4, 'queued')")
            .bind(stage_id).bind(pipeline.id).bind(&stage.name).bind(position as i32).execute(pool).await.map_err(ApiError::internal)?;
        for (job_position, job) in stage.jobs.iter().enumerate() {
            sqlx::query("INSERT INTO jobs (id, stage_id, name, image, command, position, status) VALUES ($1, $2, $3, $4, $5, $6, 'queued')")
                .bind(Uuid::new_v4()).bind(stage_id).bind(&job.name).bind(&job.image).bind(&job.command).bind(job_position as i32).execute(pool).await.map_err(ApiError::internal)?;
        }
    }
    Ok(pipeline)
}

#[derive(Debug, Default)]
struct CiStage {
    name: String,
    jobs: Vec<CiJob>,
}
#[derive(Debug)]
struct CiJob {
    name: String,
    image: String,
    command: String,
}

/// Reads `.forge-ci.yml` from an already-pushed local bare repository.
/// External URLs deliberately use the template: cloning during post-receive
/// could wait on the same Smart HTTP request that is still completing.
async fn read_local_forge_ci_config(repo_url: Option<&str>, git_ref: &str) -> Option<String> {
    let name = extract_repo_name_from_url(repo_url?)?;
    let git_root = std::env::var("CICD_GIT_ROOT").unwrap_or_else(|_| "/var/lib/forge/git".into());
    let bare_path = std::path::Path::new(&git_root).join(format!("{name}.git"));
    if !bare_path.is_dir() {
        return None;
    }

    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare_path.display()))
        .args(["show", &format!("{git_ref}:.forge-ci.yml")])
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Extracts the repository name from a URL like `http://host/git/name.git`.
fn extract_repo_name_from_url(url: &str) -> Option<String> {
    let path = url.split('/').next_back()?;
    let name = path.strip_suffix(".git").unwrap_or(path);
    Some(name.to_string())
}

/// Parses `.forge-ci.yml`:
/// ```yaml
/// stages:
///   - name: build
///     jobs:
///       - name: compile
///         image: rust:1.86
///         command: cargo build --release
/// ```
fn parse_forge_ci(raw: Option<&str>) -> Result<Vec<CiStage>, String> {
    #[derive(serde::Deserialize)]
    struct YamlJob {
        name: String,
        #[serde(default = "default_image")]
        image: String,
        command: String,
    }
    #[derive(serde::Deserialize)]
    struct YamlStage {
        name: String,
        #[serde(default)]
        jobs: Vec<YamlJob>,
    }
    #[derive(serde::Deserialize)]
    struct YamlConfig {
        #[serde(default)]
        stages: Vec<YamlStage>,
    }
    fn default_image() -> String {
        "alpine:3.21".into()
    }

    let Some(raw) = raw else {
        return Ok(default_pipeline());
    };
    let parsed: YamlConfig =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid .forge-ci.yml: {error}"))?;
    let mut stages: Vec<CiStage> = parsed
        .stages
        .into_iter()
        .map(|stage| CiStage {
            name: stage.name,
            jobs: stage
                .jobs
                .into_iter()
                .map(|job| CiJob {
                    name: job.name,
                    image: job.image,
                    command: job.command,
                })
                .collect(),
        })
        .collect();
    // Drop stages without jobs (nothing to execute).
    stages.retain(|stage| !stage.jobs.is_empty());
    validate_ci_stages(&stages)?;
    Ok(stages)
}

fn default_pipeline() -> Vec<CiStage> {
    vec![
        CiStage {
            name: "build".into(),
            jobs: vec![CiJob {
                name: "checkout".into(),
                image: "alpine/git:latest".into(),
                command: "git fetch --all".into(),
            }],
        },
        CiStage {
            name: "test".into(),
            jobs: vec![CiJob {
                name: "unit-tests".into(),
                image: "rust:1.86".into(),
                command: "cargo test".into(),
            }],
        },
        CiStage {
            name: "deploy".into(),
            jobs: vec![CiJob {
                name: "deploy".into(),
                image: "alpine:3.21".into(),
                command: "echo deploy".into(),
            }],
        },
    ]
}

fn validate_ci_stages(stages: &[CiStage]) -> Result<(), String> {
    if stages.is_empty() {
        return Err(".forge-ci.yml must define at least one stage".into());
    }
    for stage in stages {
        if stage.name.trim().is_empty() || stage.jobs.is_empty() {
            return Err("every stage must have a name and at least one job".into());
        }
        for job in &stage.jobs {
            if job.name.trim().is_empty()
                || job.command.trim().is_empty()
                || !is_safe_image_reference(&job.image)
            {
                return Err("every job needs a name, command, and safe image reference".into());
            }
        }
    }
    Ok(())
}

fn is_safe_image_reference(image: &str) -> bool {
    !image.is_empty()
        && !image.starts_with('.')
        && !image.contains("..")
        && image.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | ':' | '.' | '_' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_jobs_and_normalizes_optional_values() {
        let stages = parse_forge_ci(Some(
            r#"
stages:
  - name: build
    jobs:
      - name: compile
        image: rust:1.86
        command: cargo build --release
      - name: lint
        command: cargo fmt --check
  - name: test
    jobs:
      - name: unit
        image: rust:1.86
        command: cargo test
"#,
        ))
        .expect("valid configuration");

        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].name, "build");
        assert_eq!(stages[0].jobs.len(), 2);
        assert_eq!(stages[0].jobs[1].image, "alpine:3.21");
        assert_eq!(stages[1].jobs[0].command, "cargo test");
    }

    #[test]
    fn uses_the_template_only_when_no_configuration_was_found() {
        let stages = parse_forge_ci(None).expect("missing config uses template");

        assert_eq!(
            stages
                .iter()
                .map(|stage| stage.name.as_str())
                .collect::<Vec<_>>(),
            ["build", "test", "deploy"]
        );
    }

    #[test]
    fn rejects_invalid_or_empty_configuration_instead_of_deploying_the_template() {
        for source in [
            "stages: []",
            "stages:\n  - name: build\n    jobs: []",
            "stages:\n  - name: build\n    jobs:\n      - name: compile\n        command: ''",
            "stages:\n  - name: build\n    jobs:\n      - name: compile\n        image: ../unsafe\n        command: echo nope",
        ] {
            assert!(parse_forge_ci(Some(source)).is_err(), "{source}");
        }
    }
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/pipelines", tag="pipelines", params(("project_id"=Uuid, Path)), responses((status=200, body=[Pipeline])))]
async fn list_pipelines(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<Pipeline>> {
    let pipelines = sqlx::query_as::<_, Pipeline>("SELECT id, project_id, git_ref, status, created_at, started_at, finished_at FROM pipelines WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50").bind(project_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(pipelines))
}
#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=PipelineDetail), (status=404)))]
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

#[derive(Deserialize, utoipa::ToSchema)]
struct ChangeStatus {
    status: JobStatus,
}
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/status", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=Job), (status=404)))]
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

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/cancel", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=Pipeline), (status=404)))]
async fn cancel_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let pool = pool(&state)?;
    let pipeline = sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id = $1")
        .bind(pipeline_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if pipeline != "queued" && pipeline != "running" {
        return Err(ApiError::conflict("pipeline is not active"));
    }
    if let Some(running) = state.running_jobs.as_ref() {
        let job_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT j.id FROM jobs j JOIN stages s ON s.id = j.stage_id WHERE s.pipeline_id = $1",
        )
        .bind(pipeline_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
        let mut guard = running.lock().await;
        for job_id in job_ids {
            if let Some(pid) = guard.remove(&job_id) {
                kill_running_job(job_id, pid).await;
            }
        }
    }
    sqlx::query("UPDATE pipelines SET status = 'canceled', finished_at = now() WHERE id = $1")
        .bind(pipeline_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE jobs SET status = 'canceled', finished_at = now() \
         WHERE status IN ('queued','running') AND stage_id IN \
         (SELECT id FROM stages WHERE pipeline_id = $1)",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE stages SET status = 'canceled' \
         WHERE status IN ('queued','running') AND pipeline_id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({"canceled": pipeline_id})))
}

/// Kill a running job process: try Docker container stop by name, then
/// SIGTERM and SIGKILL the child PID as fallback.
async fn kill_running_job(job_id: Uuid, pid: u32) {
    let container_name = format!("forge-job-{job_id}");
    let _ = tokio::process::Command::new("docker")
        .args(["stop", "-t", "2", &container_name])
        .status()
        .await;
    let _ = tokio::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {}).await;
    let _ = tokio::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .await;
}

#[utoipa::path(post, path="/api/v1/pipelines/{pipeline_id}/retry", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=Pipeline), (status=404)))]
async fn retry_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let pool = pool(&state)?;
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id = $1")
        .bind(pipeline_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if status != "failed" && status != "canceled" {
        return Err(ApiError::conflict(
            "only failed or canceled pipelines can be retried",
        ));
    }
    sqlx::query(
        "UPDATE jobs SET status = 'queued', started_at = NULL, finished_at = NULL WHERE status IN ('failed','canceled') AND stage_id IN (SELECT id FROM stages WHERE pipeline_id = $1)",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE stages SET status = 'queued' WHERE status IN ('failed','canceled') AND pipeline_id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE pipelines SET status = 'queued', started_at = NULL, finished_at = NULL WHERE id = $1",
    )
    .bind(pipeline_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({"retried": pipeline_id})))
}

#[utoipa::path(post, path="/api/v1/jobs/{job_id}/retry", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=Job), (status=404)))]
async fn retry_job(State(state): State<Arc<AppState>>, Path(job_id): Path<Uuid>) -> ApiResult<Job> {
    let pool = pool(&state)?;
    let job = sqlx::query_as::<_, Job>("SELECT id, stage_id, name, image, command, position, status, started_at, finished_at FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if job.status != "failed" && job.status != "canceled" {
        return Err(ApiError::conflict(
            "only failed or canceled jobs can be retried",
        ));
    }
    sqlx::query("DELETE FROM job_logs WHERE job_id = $1")
        .bind(job_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    let updated = sqlx::query_as::<_, Job>("UPDATE jobs SET status = 'queued', started_at = NULL, finished_at = NULL WHERE id = $1 RETURNING id, stage_id, name, image, command, position, status, started_at, finished_at")
        .bind(job_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE stages SET status = 'queued' WHERE id = $1 AND status IN ('failed','canceled')",
    )
    .bind(job.stage_id)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query("UPDATE pipelines SET status = 'running', finished_at = NULL WHERE id = (SELECT pipeline_id FROM stages WHERE id = $1) AND status IN ('failed','canceled')")
        .bind(job.stage_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(updated))
}

pub(crate) async fn refresh_statuses(pool: &PgPool, stage_id: Uuid) -> Result<(), ApiError> {
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

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
struct JobLog {
    id: i64,
    job_id: Uuid,
    sequence: i32,
    message: String,
    created_at: DateTime<Utc>,
}
#[derive(Deserialize, utoipa::ToSchema)]
struct AppendLog {
    message: String,
}
#[utoipa::path(get, path="/api/v1/jobs/{job_id}/logs", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobLog])))]
async fn list_logs(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<JobLog>> {
    let logs = sqlx::query_as::<_, JobLog>("SELECT id, job_id, sequence, message, created_at FROM job_logs WHERE job_id = $1 ORDER BY sequence").bind(job_id).fetch_all(pool(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(logs))
}
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/logs", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[JobLog])))]
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
