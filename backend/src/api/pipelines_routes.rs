//! Pipelines + trigger + config parser vertical (ADR-0012).

use super::dto::{Pipeline, PipelineDetail, TriggerPipeline};
use super::{
    ApiError, ApiResult, AppState, IDEMPOTENCY_KEY_HEADER, LEGACY_PIPELINE_PLAN_PARSER_VERSION,
    LEGACY_TEMPLATE_CONFIG, PIPELINE_TRIGGER_SOURCE_API, PageParams,
    V1_PIPELINE_PLAN_PARSER_VERSION, pipeline_detail, pool,
};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path as FsPath;
use std::sync::Arc;
use uuid::Uuid;

#[utoipa::path(
    post,
    path="/api/v1/projects/{project_id}/pipelines",
    tag="pipelines",
    request_body=TriggerPipeline,
    tag="pipelines",
    request_body=TriggerPipeline,
    params(
        ("project_id"=Uuid, Path),
        ("Idempotency-Key" = Option<Uuid>, Header, description = "Optional UUID idempotency key for retry-safe pipeline trigger")
    ),
    responses((status=200, body=PipelineDetail), (status=400), (status=404), (status=409))
)]
pub(crate) async fn trigger_pipeline(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<TriggerPipeline>,
) -> Result<(HeaderMap, Json<PipelineDetail>), ApiError> {
    let idempotency_key = pipeline_idempotency_key(&headers)?;
    let pool = pool(&state)?;
    let git_ref = input.git_ref.unwrap_or_else(|| "main".into());
    let outcome = create_pipeline_with_vars_idempotent(
        pool,
        project_id,
        git_ref,
        serde_json::to_value(input.variables.clone().unwrap_or_default())
            .unwrap_or_else(|_| serde_json::json!({})),
        PIPELINE_TRIGGER_SOURCE_API,
        idempotency_key.as_deref(),
        &state.config.git.root,
    )
    .await?;
    let mut response_headers = HeaderMap::new();
    if outcome.replayed {
        response_headers.insert("idempotency-replayed", HeaderValue::from_static("true"));
    }
    pipeline_detail(pool, outcome.pipeline.id)
        .await
        .map(Json)
        .map(|body| (response_headers, body))
}

#[derive(Debug)]
pub(crate) struct PipelineTriggerOutcome {
    pub(crate) pipeline: Pipeline,
    pub(crate) replayed: bool,
}

pub(crate) async fn create_pipeline_with_vars_idempotent(
    pool: &PgPool,
    project_id: Uuid,
    git_ref: String,
    variables: serde_json::Value,
    source: &str,
    idempotency_key: Option<&str>,
    git_root: &FsPath,
) -> Result<PipelineTriggerOutcome, ApiError> {
    let repository_url: String =
        sqlx::query_scalar("SELECT repository_url FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    // Never clone here: this path is called by post-receive and must return
    // before git-receive-pack finishes. Local config is read from the bare repo.
    let commit_sha = resolve_commit_sha(Some(repository_url.as_str()), &git_ref, git_root).await;
    let config_ref = commit_sha.as_deref().unwrap_or(&git_ref);
    let config =
        read_local_forge_ci_config(Some(repository_url.as_str()), config_ref, git_root).await;
    let (config_source, raw_config) = match config {
        Some(raw_config) => ("repository", raw_config),
        None => ("legacy_template", LEGACY_TEMPLATE_CONFIG.to_string()),
    };
    let parsed_config = parse_pipeline_config(Some(&raw_config)).map_err(ApiError::bad_request)?;
    let plan_snapshot = build_pipeline_plan_snapshot(
        &git_ref,
        commit_sha.as_deref(),
        config_source,
        raw_config,
        &parsed_config,
    );
    let fingerprint = pipeline_trigger_fingerprint(&git_ref, &variables);
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;

    if let Some(idempotency_key) = idempotency_key {
        let lock_key = format!("pipeline-trigger:{project_id}:{source}:{idempotency_key}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&lock_key)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;

        if let Some((pipeline_id, existing_fingerprint)) = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT pipeline_id, request_fingerprint \
                 FROM pipeline_triggers \
                 WHERE project_id = $1 AND source = $2 AND idempotency_key = $3",
        )
        .bind(project_id)
        .bind(source)
        .bind(idempotency_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ApiError::internal)?
        {
            if existing_fingerprint != fingerprint {
                return Err(ApiError::conflict(
                    "idempotency key was already used for a different pipeline trigger",
                ));
            }
            let pipeline = sqlx::query_as::<_, Pipeline>(
                "SELECT id, project_id, git_ref, status, created_at, started_at, finished_at \
                 FROM pipelines WHERE id = $1",
            )
            .bind(pipeline_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
            tx.commit().await.map_err(ApiError::internal)?;
            return Ok(PipelineTriggerOutcome {
                pipeline,
                replayed: true,
            });
        }
    }

    let pipeline = sqlx::query_as::<_, Pipeline>(
        "INSERT INTO pipelines (id, project_id, git_ref, commit_sha, variables, status) VALUES ($1, $2, $3, $4, $5, 'queued') RETURNING id, project_id, git_ref, status, created_at, started_at, finished_at",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(&git_ref)
    .bind(&commit_sha)
    .bind(&variables)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO pipeline_plans \
         (pipeline_id, config_source, parser_version, git_ref, resolved_commit_sha, config_sha256, plan_sha256, raw_config, plan) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(pipeline.id)
    .bind(plan_snapshot.config_source)
    .bind(plan_snapshot.parser_version)
    .bind(&plan_snapshot.git_ref)
    .bind(&plan_snapshot.resolved_commit_sha)
    .bind(&plan_snapshot.config_sha256)
    .bind(&plan_snapshot.plan_sha256)
    .bind(&plan_snapshot.raw_config)
    .bind(&plan_snapshot.plan)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    for (position, stage) in parsed_config.stages.iter().enumerate() {
        let stage_id = Uuid::new_v4();
        sqlx::query("INSERT INTO stages (id, pipeline_id, name, position, status) VALUES ($1, $2, $3, $4, 'queued')")
            .bind(stage_id).bind(pipeline.id).bind(&stage.name).bind(position as i32).execute(&mut *tx).await.map_err(ApiError::internal)?;
        for (job_position, job) in stage.jobs.iter().enumerate() {
            let job_id = Uuid::new_v4();
            sqlx::query("INSERT INTO jobs (id, stage_id, name, image, command, required_tags, required_secrets, artifact_paths, position, status, timeout_seconds, allow_failure, manual) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'queued', $10, $11, $12)")
                .bind(job_id).bind(stage_id).bind(&job.name).bind(&job.image).bind(&job.command).bind(&job.required_tags).bind(&job.required_secrets).bind(&job.artifact_paths)
                .bind(job_position as i32).bind(job.timeout_seconds).bind(job.allow_failure).bind(job.manual)
                .execute(&mut *tx).await.map_err(ApiError::internal)?;
            let attempt_id = Uuid::new_v4();
            sqlx::query("INSERT INTO execution_attempts (id, job_id, attempt_no, status, trigger) VALUES ($1, $2, 1, 'queued', 'initial')")
                .bind(attempt_id).bind(job_id).execute(&mut *tx).await.map_err(ApiError::internal)?;
            crate::store::enqueue_job_attempt_tx(&mut tx, job_id, attempt_id)
                .await
                .map_err(ApiError::internal)?;
        }
    }
    if let Some(idempotency_key) = idempotency_key {
        sqlx::query(
            "INSERT INTO pipeline_triggers \
             (id, project_id, source, idempotency_key, request_fingerprint, pipeline_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(source)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(pipeline.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    crate::dispatch_signal::notify_runner_work_available();
    Ok(PipelineTriggerOutcome {
        pipeline,
        replayed: false,
    })
}

fn pipeline_idempotency_key(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get(IDEMPOTENCY_KEY_HEADER) else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| ApiError::bad_request("idempotency key must be a UUID"))?
        .trim();
    if raw.is_empty() {
        return Err(ApiError::bad_request("idempotency key must be a UUID"));
    }
    Uuid::parse_str(raw)
        .map(|uuid| Some(uuid.to_string()))
        .map_err(|_| ApiError::bad_request("idempotency key must be a UUID"))
}

fn pipeline_trigger_fingerprint(git_ref: &str, variables: &serde_json::Value) -> String {
    let payload = serde_json::json!({
        "git_ref": git_ref,
        "variables": variables,
    });
    sha256_hex(&serde_json::to_vec(&payload).unwrap_or_default())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
struct ParsedPipelineConfig {
    stages: Vec<CiStage>,
    plan: ParsedPipelinePlan,
}

#[derive(Debug)]
enum ParsedPipelinePlan {
    Legacy,
    V1(V1PlanData),
}

impl ParsedPipelineConfig {
    fn parser_version(&self) -> &'static str {
        match self.plan {
            ParsedPipelinePlan::Legacy => LEGACY_PIPELINE_PLAN_PARSER_VERSION,
            ParsedPipelinePlan::V1(_) => V1_PIPELINE_PLAN_PARSER_VERSION,
        }
    }
}

#[derive(Debug)]
struct PipelinePlanSnapshot {
    config_source: &'static str,
    parser_version: &'static str,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    config_sha256: String,
    plan_sha256: String,
    raw_config: String,
    plan: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct LegacyExecutionPlan {
    format: &'static str,
    parser_version: &'static str,
    config_source: &'static str,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    stages: Vec<LegacyPlanStage>,
    dependencies: Vec<LegacyPlanDependency>,
}

#[derive(Debug, Serialize)]
struct LegacyPlanStage {
    name: String,
    position: i32,
    jobs: Vec<LegacyPlanJob>,
}

#[derive(Debug, Serialize)]
struct LegacyPlanJob {
    key: String,
    name: String,
    stage: String,
    stage_position: i32,
    position: i32,
    image: String,
    command: String,
    required_tags: Vec<String>,
    required_secrets: Vec<String>,
    artifact_paths: Vec<String>,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    manual: bool,
    needs: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct LegacyPlanDependency {
    from: String,
    to: String,
}

#[derive(Debug)]
struct V1PlanData {
    jobs: Vec<V1PlanJob>,
    dependencies: Vec<LegacyPlanDependency>,
}

#[derive(Debug, Serialize)]
struct V1ExecutionPlan {
    format: &'static str,
    version: u8,
    parser_version: &'static str,
    config_source: &'static str,
    git_ref: String,
    resolved_commit_sha: Option<String>,
    jobs: Vec<V1PlanJob>,
    dependencies: Vec<LegacyPlanDependency>,
}

#[derive(Clone, Debug, Serialize)]
struct V1PlanJob {
    key: String,
    stage: String,
    stage_position: i32,
    position: i32,
    image: String,
    commands: Vec<String>,
    command: String,
    required_tags: Vec<String>,
    required_secrets: Vec<String>,
    artifact_paths: Vec<String>,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    needs: Vec<String>,
}

fn build_pipeline_plan_snapshot(
    git_ref: &str,
    resolved_commit_sha: Option<&str>,
    config_source: &'static str,
    raw_config: String,
    parsed_config: &ParsedPipelineConfig,
) -> PipelinePlanSnapshot {
    let parser_version = parsed_config.parser_version();
    let plan = match &parsed_config.plan {
        ParsedPipelinePlan::Legacy => serde_json::to_value(build_legacy_execution_plan(
            git_ref,
            resolved_commit_sha,
            config_source,
            &parsed_config.stages,
        ))
        .expect("serialize legacy execution plan"),
        ParsedPipelinePlan::V1(v1_plan) => serde_json::to_value(build_v1_execution_plan(
            git_ref,
            resolved_commit_sha,
            config_source,
            v1_plan,
        ))
        .expect("serialize v1 execution plan"),
    };
    let plan_bytes = serde_json::to_vec(&plan).expect("serialize execution plan value");
    PipelinePlanSnapshot {
        config_source,
        parser_version,
        git_ref: git_ref.to_string(),
        resolved_commit_sha: resolved_commit_sha.map(ToOwned::to_owned),
        config_sha256: sha256_hex(raw_config.as_bytes()),
        plan_sha256: sha256_hex(&plan_bytes),
        raw_config,
        plan,
    }
}

fn build_legacy_execution_plan(
    git_ref: &str,
    resolved_commit_sha: Option<&str>,
    config_source: &'static str,
    stages: &[CiStage],
) -> LegacyExecutionPlan {
    let mut planned_stages = Vec::with_capacity(stages.len());
    let mut dependencies = Vec::new();
    let mut previous_stage_keys: Vec<String> = Vec::new();

    for (stage_position, stage) in stages.iter().enumerate() {
        let stage_position = stage_position as i32;
        let mut current_stage_keys = Vec::with_capacity(stage.jobs.len());
        let mut planned_jobs = Vec::with_capacity(stage.jobs.len());
        for (job_position, job) in stage.jobs.iter().enumerate() {
            let job_position = job_position as i32;
            let key = format!("stage-{stage_position}/job-{job_position}");
            let needs = previous_stage_keys.clone();
            for predecessor in &needs {
                dependencies.push(LegacyPlanDependency {
                    from: predecessor.clone(),
                    to: key.clone(),
                });
            }
            current_stage_keys.push(key.clone());
            planned_jobs.push(LegacyPlanJob {
                key,
                name: job.name.clone(),
                stage: stage.name.clone(),
                stage_position,
                position: job_position,
                image: job.image.clone(),
                command: job.command.clone(),
                required_tags: job.required_tags.clone(),
                required_secrets: job.required_secrets.clone(),
                artifact_paths: job.artifact_paths.clone(),
                timeout_seconds: job.timeout_seconds,
                allow_failure: job.allow_failure,
                manual: job.manual,
                needs,
            });
        }
        previous_stage_keys = current_stage_keys;
        planned_stages.push(LegacyPlanStage {
            name: stage.name.clone(),
            position: stage_position,
            jobs: planned_jobs,
        });
    }

    LegacyExecutionPlan {
        format: "legacy-linear",
        parser_version: LEGACY_PIPELINE_PLAN_PARSER_VERSION,
        config_source,
        git_ref: git_ref.to_string(),
        resolved_commit_sha: resolved_commit_sha.map(ToOwned::to_owned),
        stages: planned_stages,
        dependencies,
    }
}

fn build_v1_execution_plan(
    git_ref: &str,
    resolved_commit_sha: Option<&str>,
    config_source: &'static str,
    plan: &V1PlanData,
) -> V1ExecutionPlan {
    V1ExecutionPlan {
        format: "v1-dag",
        version: 1,
        parser_version: V1_PIPELINE_PLAN_PARSER_VERSION,
        config_source,
        git_ref: git_ref.to_string(),
        resolved_commit_sha: resolved_commit_sha.map(ToOwned::to_owned),
        jobs: plan.jobs.clone(),
        dependencies: plan.dependencies.clone(),
    }
}

#[derive(Clone, Debug, Default)]
struct CiStage {
    name: String,
    jobs: Vec<CiJob>,
}
#[derive(Clone, Debug)]
struct CiJob {
    name: String,
    image: String,
    command: String,
    required_tags: Vec<String>,
    required_secrets: Vec<String>,
    artifact_paths: Vec<String>,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    manual: bool,
}

/// Reads `.forge-ci.yml` from an already-pushed local bare repository.
/// External URLs deliberately use the template: cloning during post-receive
/// could wait on the same Smart HTTP request that is still completing.
async fn read_local_forge_ci_config(
    repo_url: Option<&str>,
    git_ref: &str,
    git_root: &FsPath,
) -> Option<String> {
    let name = extract_repo_name_from_url(repo_url?)?;
    let bare_path = git_root.join(format!("{name}.git"));
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

/// Resolves a ref to a commit sha in the local bare repo (best-effort).
async fn resolve_commit_sha(
    repo_url: Option<&str>,
    git_ref: &str,
    git_root: &FsPath,
) -> Option<String> {
    let name = extract_repo_name_from_url(repo_url?)?;
    let bare_path = git_root.join(format!("{name}.git"));
    if !bare_path.is_dir() {
        return None;
    }
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare_path.display()))
        .args(["rev-parse", git_ref])
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
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
#[cfg(test)]
fn parse_forge_ci(raw: Option<&str>) -> Result<Vec<CiStage>, String> {
    Ok(parse_pipeline_config(raw)?.stages)
}

fn parse_pipeline_config(raw: Option<&str>) -> Result<ParsedPipelineConfig, String> {
    let Some(raw) = raw else {
        return Ok(ParsedPipelineConfig {
            stages: default_pipeline(),
            plan: ParsedPipelinePlan::Legacy,
        });
    };
    if raw.len() > 1024 * 1024 {
        return Err(".forge-ci.yml must be no larger than 1 MiB".into());
    }

    let root: serde_yaml::Value =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid .forge-ci.yml: {error}"))?;
    let has_version = match root {
        serde_yaml::Value::Mapping(mapping) => {
            mapping.contains_key(serde_yaml::Value::String("version".into()))
        }
        _ => return Err(".forge-ci.yml must be a YAML object".into()),
    };
    if has_version {
        parse_v1_pipeline_config(raw)
    } else {
        parse_legacy_pipeline_config(raw)
    }
}

fn parse_legacy_pipeline_config(raw: &str) -> Result<ParsedPipelineConfig, String> {
    #[derive(serde::Deserialize)]
    struct YamlJob {
        name: String,
        #[serde(default = "default_image")]
        image: String,
        command: String,
        /// e.g. "30s", "5m", "1h" — default 1h.
        #[serde(default)]
        timeout: Option<String>,
        /// Job failure does not fail the stage/pipeline.
        #[serde(default)]
        allow_failure: bool,
        /// Waits for an explicit start (approval gate).
        #[serde(default)]
        when: Option<String>,
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
                    required_tags: Vec::new(),
                    required_secrets: Vec::new(),
                    artifact_paths: Vec::new(),
                    timeout_seconds: parse_timeout(job.timeout.as_deref()),
                    allow_failure: job.allow_failure,
                    manual: job.when.as_deref() == Some("manual"),
                })
                .collect(),
        })
        .collect();
    // Drop stages without jobs (nothing to execute).
    stages.retain(|stage| !stage.jobs.is_empty());
    validate_ci_stages(&stages)?;
    Ok(ParsedPipelineConfig {
        stages,
        plan: ParsedPipelinePlan::Legacy,
    })
}

#[derive(Clone, Debug)]
struct NormalizedV1Job {
    key: String,
    image: String,
    commands: Vec<String>,
    required_tags: Vec<String>,
    required_secrets: Vec<String>,
    artifact_paths: Vec<String>,
    timeout_seconds: Option<i32>,
    allow_failure: bool,
    needs: Vec<String>,
}

fn parse_v1_pipeline_config(raw: &str) -> Result<ParsedPipelineConfig, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Config {
        version: u8,
        #[serde(default)]
        defaults: YamlV1Defaults,
        jobs: BTreeMap<String, YamlV1Job>,
    }

    #[derive(Default, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Defaults {
        image: Option<String>,
        timeout: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Job {
        #[serde(default)]
        needs: Vec<String>,
        image: Option<String>,
        commands: Vec<String>,
        timeout: Option<String>,
        tags: Option<Vec<String>>,
        #[serde(default)]
        secrets: Vec<String>,
        #[serde(default)]
        artifacts: Option<YamlV1Artifacts>,
        #[serde(default)]
        allow_failure: bool,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct YamlV1Artifacts {
        #[serde(default)]
        paths: Vec<String>,
    }

    let parsed: YamlV1Config =
        serde_yaml::from_str(raw).map_err(|error| format!("invalid .forge-ci.yml v1: {error}"))?;
    if parsed.version != 1 {
        return Err("unsupported .forge-ci.yml version; only version: 1 is supported".into());
    }
    if parsed.jobs.is_empty() {
        return Err(".forge-ci.yml v1 must define at least one job".into());
    }
    if parsed.jobs.len() > 500 {
        return Err(".forge-ci.yml v1 supports at most 500 jobs".into());
    }

    let default_image = parsed
        .defaults
        .image
        .unwrap_or_else(|| "alpine:3.21".into());
    if !is_safe_image_reference(&default_image) {
        return Err(".forge-ci.yml v1 defaults.image must be a safe image reference".into());
    }
    let default_timeout = parse_v1_timeout("defaults.timeout", parsed.defaults.timeout.as_deref())?;
    let default_tags = normalize_v1_tags("defaults.tags", parsed.defaults.tags)?;

    let mut jobs = BTreeMap::new();
    let mut edge_count = 0usize;
    for (key, job) in parsed.jobs {
        if !is_valid_v1_job_key(&key) {
            return Err(format!(
                ".forge-ci.yml v1 job key '{key}' must match ^[a-zA-Z][a-zA-Z0-9_.-]{{0,62}}$"
            ));
        }
        if job.commands.is_empty() || job.commands.len() > 64 {
            return Err(format!(
                ".forge-ci.yml v1 job '{key}' must define 1..64 commands"
            ));
        }
        for command in &job.commands {
            if command.trim().is_empty() || command.len() > 16 * 1024 || command.contains('\0') {
                return Err(format!(
                    ".forge-ci.yml v1 job '{key}' has an empty or oversized command"
                ));
            }
        }

        let image = job.image.unwrap_or_else(|| default_image.clone());
        if !is_safe_image_reference(&image) {
            return Err(format!(
                ".forge-ci.yml v1 job '{key}' image must be a safe image reference"
            ));
        }
        let timeout_seconds = match job.timeout.as_deref() {
            Some(raw) => parse_v1_timeout(&format!("jobs.{key}.timeout"), Some(raw))?,
            None => default_timeout,
        };
        let required_tags = match job.tags {
            Some(tags) => normalize_v1_tags(&format!("jobs.{key}.tags"), tags)?,
            None => default_tags.clone(),
        };
        let required_secrets =
            normalize_v1_secret_names(&format!("jobs.{key}.secrets"), job.secrets)?;
        let artifact_paths = match job.artifacts {
            Some(artifacts) => {
                if artifacts.paths.is_empty() {
                    return Err(format!(
                        ".forge-ci.yml v1 jobs.{key}.artifacts.paths must define at least one path"
                    ));
                }
                normalize_v1_artifact_paths(
                    &format!("jobs.{key}.artifacts.paths"),
                    artifacts.paths,
                )?
            }
            None => Vec::new(),
        };

        if job.needs.len() > 64 {
            return Err(format!(
                ".forge-ci.yml v1 job '{key}' can depend on at most 64 jobs"
            ));
        }
        let mut unique_needs = BTreeSet::new();
        let mut needs = Vec::with_capacity(job.needs.len());
        for need in job.needs {
            if !is_valid_v1_job_key(&need) {
                return Err(format!(
                    ".forge-ci.yml v1 job '{key}' has invalid need key '{need}'"
                ));
            }
            if !unique_needs.insert(need.clone()) {
                return Err(format!(
                    ".forge-ci.yml v1 job '{key}' lists dependency '{need}' more than once"
                ));
            }
            needs.push(need);
        }
        needs.sort();
        edge_count += needs.len();
        if edge_count > 10_000 {
            return Err(".forge-ci.yml v1 supports at most 10000 DAG edges".into());
        }

        jobs.insert(
            key.clone(),
            NormalizedV1Job {
                key,
                image,
                commands: job.commands,
                required_tags,
                required_secrets,
                artifact_paths,
                timeout_seconds,
                allow_failure: job.allow_failure,
                needs,
            },
        );
    }

    for job in jobs.values() {
        for need in &job.needs {
            if need == &job.key {
                return Err(format!(
                    ".forge-ci.yml v1 job '{}' cannot depend on itself",
                    job.key
                ));
            }
            if !jobs.contains_key(need) {
                return Err(format!(
                    ".forge-ci.yml v1 job '{}' depends on missing job '{need}'",
                    job.key
                ));
            }
        }
    }

    let levels = compute_v1_job_levels(&jobs)?;
    let mut grouped: BTreeMap<i32, Vec<NormalizedV1Job>> = BTreeMap::new();
    for (key, job) in jobs {
        let level = *levels
            .get(&key)
            .expect("every v1 job must have a computed topological level");
        grouped.entry(level).or_default().push(job);
    }

    let mut stages = Vec::with_capacity(grouped.len());
    let mut plan_jobs = Vec::new();
    for (stage_position, jobs) in grouped.into_values().enumerate() {
        let stage_position = stage_position as i32;
        let stage_name = format!("dag-{stage_position}");
        let mut ci_jobs = Vec::with_capacity(jobs.len());
        for (job_position, job) in jobs.into_iter().enumerate() {
            let job_position = job_position as i32;
            let command = v1_runtime_command(&job.commands);
            ci_jobs.push(CiJob {
                name: job.key.clone(),
                image: job.image.clone(),
                command: command.clone(),
                required_tags: job.required_tags.clone(),
                required_secrets: job.required_secrets.clone(),
                artifact_paths: job.artifact_paths.clone(),
                timeout_seconds: job.timeout_seconds,
                allow_failure: job.allow_failure,
                manual: false,
            });
            plan_jobs.push(V1PlanJob {
                key: job.key,
                stage: stage_name.clone(),
                stage_position,
                position: job_position,
                image: job.image,
                commands: job.commands,
                command,
                required_tags: job.required_tags,
                required_secrets: job.required_secrets,
                artifact_paths: job.artifact_paths,
                timeout_seconds: job.timeout_seconds,
                allow_failure: job.allow_failure,
                needs: job.needs,
            });
        }
        stages.push(CiStage {
            name: stage_name,
            jobs: ci_jobs,
        });
    }
    validate_ci_stages(&stages)?;

    let mut dependencies = Vec::with_capacity(edge_count);
    for job in &plan_jobs {
        for need in &job.needs {
            dependencies.push(LegacyPlanDependency {
                from: need.clone(),
                to: job.key.clone(),
            });
        }
    }

    Ok(ParsedPipelineConfig {
        stages,
        plan: ParsedPipelinePlan::V1(V1PlanData {
            jobs: plan_jobs,
            dependencies,
        }),
    })
}

fn v1_runtime_command(commands: &[String]) -> String {
    format!("set -e\n{}", commands.join("\n"))
}

fn compute_v1_job_levels(
    jobs: &BTreeMap<String, NormalizedV1Job>,
) -> Result<BTreeMap<String, i32>, String> {
    fn visit(
        key: &str,
        jobs: &BTreeMap<String, NormalizedV1Job>,
        levels: &mut BTreeMap<String, i32>,
        visiting: &mut BTreeSet<String>,
    ) -> Result<i32, String> {
        if let Some(level) = levels.get(key) {
            return Ok(*level);
        }
        if !visiting.insert(key.to_string()) {
            return Err(format!(
                ".forge-ci.yml v1 jobs.needs contains a dependency cycle at '{key}'"
            ));
        }
        let job = jobs
            .get(key)
            .ok_or_else(|| format!(".forge-ci.yml v1 references missing job '{key}'"))?;
        let mut level = 0;
        for need in &job.needs {
            level = level.max(visit(need, jobs, levels, visiting)? + 1);
        }
        visiting.remove(key);
        levels.insert(key.to_string(), level);
        Ok(level)
    }

    let mut levels = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for key in jobs.keys() {
        visit(key, jobs, &mut levels, &mut visiting)?;
    }
    Ok(levels)
}

fn parse_timeout(raw: Option<&str>) -> Option<i32> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let (value, unit) = raw.split_at(raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len()));
    let value: i32 = value.parse().ok()?;
    match unit.trim() {
        "s" | "sec" | "secs" | "" => Some(value),
        "m" | "min" | "mins" => value.checked_mul(60),
        "h" | "hour" | "hours" => value.checked_mul(3600),
        _ => None,
    }
}

fn parse_v1_timeout(field: &str, raw: Option<&str>) -> Result<Option<i32>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let digit_count = raw
        .char_indices()
        .find(|(_, character)| !character.is_ascii_digit())
        .map(|(index, _)| index)
        .unwrap_or(raw.len());
    if digit_count == 0 || digit_count == raw.len() {
        return Err(format!(
            ".forge-ci.yml v1 {field} must use a positive duration with unit s, m, h, or d"
        ));
    }
    let value: i32 = raw[..digit_count]
        .parse()
        .map_err(|_| format!(".forge-ci.yml v1 {field} duration is too large"))?;
    if value <= 0 {
        return Err(format!(
            ".forge-ci.yml v1 {field} must use a positive duration"
        ));
    }
    let seconds = match &raw[digit_count..] {
        "s" => value,
        "m" => value
            .checked_mul(60)
            .ok_or_else(|| format!(".forge-ci.yml v1 {field} duration is too large"))?,
        "h" => value
            .checked_mul(3600)
            .ok_or_else(|| format!(".forge-ci.yml v1 {field} duration is too large"))?,
        "d" => value
            .checked_mul(86_400)
            .ok_or_else(|| format!(".forge-ci.yml v1 {field} duration is too large"))?,
        _ => {
            return Err(format!(
                ".forge-ci.yml v1 {field} must use duration unit s, m, h, or d"
            ));
        }
    };
    if seconds > 86_400 {
        return Err(format!(".forge-ci.yml v1 {field} must not exceed 24h"));
    }
    Ok(Some(seconds))
}

fn normalize_v1_tags(field: &str, tags: Vec<String>) -> Result<Vec<String>, String> {
    if tags.len() > 64 {
        return Err(format!(".forge-ci.yml v1 {field} supports at most 64 tags"));
    }
    let mut normalized = BTreeSet::new();
    for tag in tags {
        let tag = tag.trim();
        if !is_valid_runner_tag(tag) {
            return Err(format!(
                ".forge-ci.yml v1 {field} tag '{tag}' must match ^[a-z0-9][a-z0-9._-]{{0,62}}$"
            ));
        }
        normalized.insert(tag.to_string());
    }
    Ok(normalized.into_iter().collect())
}

fn normalize_v1_secret_names(field: &str, names: Vec<String>) -> Result<Vec<String>, String> {
    if names.len() > 64 {
        return Err(format!(
            ".forge-ci.yml v1 {field} supports at most 64 secrets"
        ));
    }
    let mut normalized = BTreeSet::new();
    for name in names {
        let name = name.trim();
        if !is_valid_v1_secret_name(name) {
            return Err(format!(
                ".forge-ci.yml v1 {field} secret '{name}' must match ^[A-Z][A-Z0-9_]{{0,127}}$"
            ));
        }
        normalized.insert(name.to_string());
    }
    Ok(normalized.into_iter().collect())
}

fn normalize_v1_artifact_paths(field: &str, paths: Vec<String>) -> Result<Vec<String>, String> {
    if paths.len() > 32 {
        return Err(format!(
            ".forge-ci.yml v1 {field} supports at most 32 paths"
        ));
    }
    let mut normalized = BTreeSet::new();
    for path in paths {
        let path = path.trim();
        if !is_valid_v1_artifact_path(path) {
            return Err(format!(
                ".forge-ci.yml v1 {field} path '{path}' must be a safe relative file path"
            ));
        }
        normalized.insert(path.to_string());
    }
    Ok(normalized.into_iter().collect())
}

fn is_valid_runner_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

fn is_valid_v1_secret_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_uppercase() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

fn is_valid_v1_artifact_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 255
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn is_valid_v1_job_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    key.len() <= 63
        && first.is_ascii_alphabetic()
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
}

fn default_pipeline() -> Vec<CiStage> {
    vec![
        CiStage {
            name: "build".into(),
            jobs: vec![CiJob {
                name: "checkout".into(),
                image: "alpine/git:latest".into(),
                command: "git fetch --all".into(),
                required_tags: Vec::new(),
                required_secrets: Vec::new(),
                artifact_paths: Vec::new(),
                timeout_seconds: None,
                allow_failure: false,
                manual: false,
            }],
        },
        CiStage {
            name: "test".into(),
            jobs: vec![CiJob {
                name: "unit-tests".into(),
                image: "rust:1.86".into(),
                command: "cargo test".into(),
                required_tags: Vec::new(),
                required_secrets: Vec::new(),
                artifact_paths: Vec::new(),
                timeout_seconds: None,
                allow_failure: false,
                manual: false,
            }],
        },
        CiStage {
            name: "deploy".into(),
            jobs: vec![CiJob {
                name: "deploy".into(),
                image: "alpine:3.21".into(),
                required_tags: Vec::new(),
                required_secrets: Vec::new(),
                artifact_paths: Vec::new(),
                timeout_seconds: None,
                allow_failure: false,
                manual: false,
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
                || job
                    .required_tags
                    .iter()
                    .any(|tag| !is_valid_runner_tag(tag))
                || job
                    .required_secrets
                    .iter()
                    .any(|secret| !is_valid_v1_secret_name(secret))
            {
                return Err(
                    "every job needs a name, command, safe image reference, safe runner tags and safe secret names"
                        .into(),
                );
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

// ---- Releases (P1 git-server parity) ----

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub(crate) struct Release {
    pub(crate) id: Uuid,
    pub(crate) repository_name: String,
    pub(crate) tag_name: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) prerelease: bool,
    pub(crate) created_by: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateRelease {
    pub(crate) tag_name: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) prerelease: bool,
}

#[utoipa::path(get, path="/api/v1/repos/{repo}/releases", tag="releases", params(("repo"=String, Path)), responses((status=200, body=[Release])))]
pub(crate) async fn list_releases(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(repo): axum::extract::Path<String>,
) -> ApiResult<Vec<Release>> {
    let rows = sqlx::query_as::<_, Release>(
        "SELECT id, repository_name, tag_name, name, description, prerelease, created_by, created_at \
         FROM releases WHERE repository_name = $1 ORDER BY created_at DESC",
    )
    .bind(repo)
    .fetch_all(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

#[utoipa::path(post, path="/api/v1/repos/{repo}/releases", tag="releases", params(("repo"=String, Path)), request_body=CreateRelease, responses((status=200, body=Release)))]
pub(crate) async fn create_release(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(repo): axum::extract::Path<String>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreateRelease>,
) -> ApiResult<Release> {
    let created_by = claims.map(|c| c.0.sub.to_string());
    if input.tag_name.trim().is_empty() || input.name.trim().is_empty() {
        return Err(ApiError::bad_request("tag_name and name are required"));
    }
    let row = sqlx::query_as::<_, Release>(
        "INSERT INTO releases (id, repository_name, tag_name, name, description, prerelease, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (repository_name, tag_name) DO UPDATE SET \
           name = EXCLUDED.name, description = EXCLUDED.description, prerelease = EXCLUDED.prerelease \
         RETURNING id, repository_name, tag_name, name, description, prerelease, created_by, created_at",
    )
    .bind(Uuid::new_v4())
    .bind(repo.trim())
    .bind(input.tag_name.trim())
    .bind(input.name.trim())
    .bind(input.description)
    .bind(input.prerelease)
    .bind(created_by)
    .fetch_one(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(row))
}

#[utoipa::path(get, path="/api/v1/repos/{repo}/releases/{tag}", tag="releases", params(("repo"=String, Path), ("tag"=String, Path)), responses((status=200, body=Release), (status=404)))]
pub(crate) async fn get_release(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((repo, tag)): axum::extract::Path<(String, String)>,
) -> ApiResult<Release> {
    let row = sqlx::query_as::<_, Release>(
        "SELECT id, repository_name, tag_name, name, description, prerelease, created_by, created_at \
         FROM releases WHERE repository_name = $1 AND tag_name = $2",
    )
    .bind(repo)
    .bind(tag)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[utoipa::path(delete, path="/api/v1/repos/{repo}/releases/{tag}", tag="releases", params(("repo"=String, Path), ("tag"=String, Path)), responses((status=200), (status=404)))]
pub(crate) async fn delete_release(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((repo, tag)): axum::extract::Path<(String, String)>,
) -> ApiResult<serde_json::Value> {
    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM releases WHERE repository_name = $1 AND tag_name = $2 RETURNING id",
    )
    .bind(repo)
    .bind(tag)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(serde_json::json!({ "deleted": deleted.to_string() })))
}

// ---- Pipeline badge (P1, public read-only) ----

#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}/badge.svg", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, description="SVG badge")))]
pub(crate) async fn pipeline_badge(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> axum::response::Response {
    let status: String = match state.pool.as_ref() {
        Some(pool) => sqlx::query_scalar("SELECT status FROM pipelines WHERE id = $1")
            .bind(pipeline_id)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| "unknown".into()),
        None => "unknown".into(),
    };
    let (label, color) = match status.as_str() {
        "success" => ("passing", "#2ea44f"),
        "failed" => ("failed", "#d73a4a"),
        "running" | "queued" => ("running", "#dfb317"),
        "canceled" => ("canceled", "#959da5"),
        _ => ("unknown", "#959da5"),
    };
    let width = 60 + label.len() * 7;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"20\">\
<linearGradient id=\"s\" x2=\"0\" y2=\"100%\"><stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/><stop offset=\"1\" stop-opacity=\".1\"/></linearGradient>\
<clipPath id=\"r\"><rect width=\"{width}\" height=\"20\" rx=\"3\" fill=\"#fff\"/></clipPath>\
<g clip-path=\"url(#r)\"><rect width=\"46\" height=\"20\" fill=\"#555\"/><rect x=\"46\" width=\"{rw}\" height=\"20\" fill=\"{color}\"/><rect width=\"{width}\" height=\"20\" fill=\"url(#s)\"/></g>\
<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,sans-serif\" font-size=\"11\">\
<text x=\"23\" y=\"15\">build</text><text x=\"{tx}\" y=\"15\">{label}</text></g></svg>",
        width = width,
        rw = width - 46,
        tx = 46 + (width - 46) / 2,
        color = color,
        label = label,
    );
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "image/svg+xml")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from(svg))
        .unwrap()
}

// ---- Pipeline variables (P1) ----

#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}/variables", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200)))]
pub(crate) async fn pipeline_variables(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let vars: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT variables FROM pipelines WHERE id = $1")
            .bind(pipeline_id)
            .fetch_optional(pool(&state)?)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    Ok(Json(vars.unwrap_or_else(|| serde_json::json!({}))))
}

// ---- JUnit test reports (P1) ----

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub(crate) struct TestReport {
    pub(crate) id: Uuid,
    pub(crate) job_id: Uuid,
    pub(crate) suite_name: String,
    pub(crate) tests_total: i32,
    pub(crate) tests_passed: i32,
    pub(crate) tests_failed: i32,
    pub(crate) tests_skipped: i32,
    pub(crate) duration_ms: Option<i32>,
    pub(crate) created_at: DateTime<Utc>,
}

#[utoipa::path(get, path="/api/v1/jobs/{job_id}/test-report", tag="jobs", params(("job_id"=Uuid, Path)), responses((status=200, body=[TestReport])))]
pub(crate) async fn get_test_report(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Vec<TestReport>> {
    let rows = sqlx::query_as::<_, TestReport>(
        "SELECT id, job_id, suite_name, tests_total, tests_passed, tests_failed, tests_skipped, duration_ms, created_at \
         FROM test_reports WHERE job_id = $1 ORDER BY suite_name",
    )
    .bind(job_id)
    .fetch_all(pool(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

/// Minimal JUnit XML parser: sums testsuite/testcase counts and failures.
#[utoipa::path(post, path="/api/v1/jobs/{job_id}/test-report", tag="jobs", params(("job_id"=Uuid, Path)), request_body=String, responses((status=200, body=[TestReport]), (status=413, description="test report body exceeds 10 MiB")))]
pub(crate) async fn upload_test_report(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Json(body): Json<String>,
) -> ApiResult<Vec<TestReport>> {
    // Delete previous reports for the job (idempotent re-upload).
    sqlx::query("DELETE FROM test_reports WHERE job_id = $1")
        .bind(job_id)
        .execute(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
    let mut reports = Vec::new();
    for suite in parse_junit(&body) {
        let row = sqlx::query_as::<_, TestReport>(
            "INSERT INTO test_reports (id, job_id, suite_name, tests_total, tests_passed, tests_failed, tests_skipped, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id, job_id, suite_name, tests_total, tests_passed, tests_failed, tests_skipped, duration_ms, created_at",
        )
        .bind(Uuid::new_v4())
        .bind(job_id)
        .bind(&suite.name)
        .bind(suite.total)
        .bind(suite.passed)
        .bind(suite.failed)
        .bind(suite.skipped)
        .bind(suite.duration_ms)
        .fetch_one(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
        reports.push(row);
    }
    Ok(Json(reports))
}

#[derive(Debug)]
struct JunitSuite {
    name: String,
    total: i32,
    passed: i32,
    failed: i32,
    skipped: i32,
    duration_ms: Option<i32>,
}

/// Extracts `<testsuite ...>` attributes and counts `<testcase>` children with
/// failure/skipped children. String-scanning keeps the parser allocation-light
/// and dependency-free.
fn parse_junit(xml: &str) -> Vec<JunitSuite> {
    let mut out = Vec::new();
    // Iterate over each <testsuite ...> ... </testsuite> block by scanning
    // opening tags and slicing to the matching close.
    let mut search_from = 0usize;
    let mut suite_idx = 0;
    while let Some(open_rel) = xml[search_from..]
        .find("<testsuite ")
        .or_else(|| xml[search_from..].find("<testsuite>"))
    {
        let open_at = search_from + open_rel;
        let attrs_start = open_at
            + if xml[open_at..].starts_with("<testsuite ") {
                "<testsuite ".len()
            } else {
                "<testsuite>".len()
            };
        let attrs_end = xml[attrs_start..]
            .find('>')
            .map(|e| attrs_start + e)
            .unwrap_or(xml.len());
        let attrs = &xml[attrs_start..attrs_end];
        let body_start = (attrs_end + 1).min(xml.len());
        let close_at = xml[body_start..]
            .find("</testsuite>")
            .map(|e| body_start + e)
            .unwrap_or(xml.len());
        let body = &xml[body_start..close_at];
        suite_idx += 1;

        let get_int = |key: &str| -> Option<i32> {
            let pat = format!("{key}=\"");
            let start = attrs.find(&pat)? + pat.len();
            let rest = &attrs[start..];
            let end = rest.find('"')?;
            rest[..end].parse().ok()
        };
        let get_float = |key: &str| -> Option<f64> {
            let pat = format!("{key}=\"");
            let start = attrs.find(&pat)? + pat.len();
            let rest = &attrs[start..];
            let end = rest.find('"')?;
            rest[..end].parse().ok()
        };
        let name = {
            let pat = "name=\"";
            attrs.find(pat).and_then(|p| {
                let rest = &attrs[p + pat.len()..];
                rest.find('"').map(|e| rest[..e].to_string())
            })
        }
        .unwrap_or_else(|| format!("suite-{suite_idx}"));

        let total_cases = body.matches("<testcase").count() as i32;
        let failed = (body.matches("<failure").count() + body.matches("<error").count()) as i32;
        let skipped = body.matches("<skipped").count() as i32;
        let total = get_int("tests").unwrap_or(total_cases);
        out.push(JunitSuite {
            name,
            total,
            passed: (total - failed - skipped).max(0),
            failed,
            skipped,
            duration_ms: get_float("time").map(|s| (s * 1000.0) as i32),
        });
        search_from = (close_at + "</testsuite>".len()).min(xml.len());
    }
    out
}

pub(crate) fn valid_project_role(role: &str) -> bool {
    crate::authz::Role::parse(role).is_some_and(crate::authz::Role::is_project_role)
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/pipelines", tag="pipelines", params(("project_id"=Uuid, Path)), responses((status=200, body=[Pipeline])))]
pub(crate) async fn list_pipelines(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    axum::extract::Query(page): axum::extract::Query<PageParams>,
) -> ApiResult<Vec<Pipeline>> {
    let (limit, offset) = page.bounded();
    let pipelines = sqlx::query_as::<_, Pipeline>("SELECT id, project_id, git_ref, status, created_at, started_at, finished_at FROM pipelines WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3")
        .bind(project_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(pipelines))
}

#[utoipa::path(get, path="/api/v1/pipelines/{pipeline_id}", tag="pipelines", params(("pipeline_id"=Uuid, Path)), responses((status=200, body=PipelineDetail), (status=404)))]
pub(crate) async fn get_pipeline(
    State(state): State<Arc<AppState>>,
    Path(pipeline_id): Path<Uuid>,
) -> ApiResult<PipelineDetail> {
    pipeline_detail(pool(&state)?, pipeline_id).await.map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::projects_routes::default_project_role;

    #[test]
    fn parses_execution_controls() {
        let stages = parse_forge_ci(Some(
            r#"
stages:
  - name: build
    jobs:
      - name: compile
        command: make
        timeout: 30s
      - name: lint
        command: make lint
        allow_failure: true
  - name: deploy
    jobs:
      - name: prod
        command: ./deploy.sh
        when: manual
"#,
        ))
        .expect("valid configuration");
        assert_eq!(stages[0].jobs[0].timeout_seconds, Some(30));
        assert!(!stages[0].jobs[0].allow_failure);
        assert!(stages[0].jobs[1].allow_failure);
        assert!(stages[1].jobs[0].manual);
        assert_eq!(stages[1].jobs[0].timeout_seconds, None);
    }

    #[test]
    fn timeout_units_parse() {
        // parse_timeout lives inside parse_forge_ci; verify via full config.
        let stages = parse_forge_ci(Some(
            "stages:\n  - name: a\n    jobs:\n      - name: j\n        command: x\n        timeout: 5m\n",
        ))
        .expect("valid");
        assert_eq!(stages[0].jobs[0].timeout_seconds, Some(300));
    }

    #[test]
    fn parse_junit_extracts_suite_names_and_counts() {
        let xml = r#"<?xml version="1.0"?>
<testsuites>
  <testsuite name="unit::core" tests="4" time="0.845">
    <testcase name="a"/><testcase name="b"/>
    <testcase name="c"><failure message="x"/></testcase>
    <testcase name="d"><skipped/></testcase>
  </testsuite>
  <testsuite name="integration::api" tests="2" time="1.9">
    <testcase name="e"/><testcase name="f"><error message="y"/></testcase>
  </testsuite>
</testsuites>"#;
        let suites = parse_junit(xml);
        assert_eq!(suites.len(), 2, "expected 2 suites, got {suites:?}");
        assert_eq!(suites[0].name, "unit::core");
        assert_eq!(suites[0].total, 4);
        assert_eq!(suites[0].failed, 1);
        assert_eq!(suites[0].skipped, 1);
        assert_eq!(suites[0].passed, 2);
        assert_eq!(suites[0].duration_ms, Some(845));
        assert_eq!(suites[1].name, "integration::api");
        assert_eq!(suites[1].failed, 1);
        assert_eq!(suites[1].duration_ms, Some(1900));
    }

    #[test]
    fn project_membership_roles_exclude_instance_admin() {
        assert!(valid_project_role("maintainer"));
        assert!(valid_project_role("developer"));
        assert!(valid_project_role("viewer"));
        assert!(!valid_project_role("admin"));
        assert_eq!(default_project_role("admin"), Some("maintainer"));
        assert_eq!(default_project_role("developer"), Some("developer"));
        assert_eq!(default_project_role("viewer"), None);
    }

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

    #[test]
    fn parses_v1_dag_into_topological_stages() {
        let parsed = parse_pipeline_config(Some(
            r#"
version: 1
defaults:
  image: alpine:3.21
  timeout: 20m
  tags: [linux, docker]
jobs:
  build:
    commands:
      - cargo build --release
  lint:
    commands:
      - cargo fmt --check
    allow_failure: true
  test:
    needs: [build, lint]
    image: rust:1.86
    timeout: 45m
    tags: [linux]
    secrets: [DEPLOY_TOKEN, AWS_ACCESS_KEY_ID, DEPLOY_TOKEN]
    artifacts:
      paths: [target/release/app.tar.gz, reports/junit.xml, reports/junit.xml]
    commands:
      - cargo test
      - cargo clippy --all-targets
"#,
        ))
        .expect("valid v1 configuration");

        assert_eq!(parsed.parser_version(), "forge-dsl/1.0.0");
        assert_eq!(
            parsed
                .stages
                .iter()
                .map(|stage| stage.name.as_str())
                .collect::<Vec<_>>(),
            ["dag-0", "dag-1"]
        );
        assert_eq!(
            parsed.stages[0]
                .jobs
                .iter()
                .map(|job| job.name.as_str())
                .collect::<Vec<_>>(),
            ["build", "lint"]
        );
        assert_eq!(parsed.stages[0].jobs[0].timeout_seconds, Some(1200));
        assert_eq!(
            parsed.stages[0].jobs[0].required_tags,
            vec!["docker".to_string(), "linux".to_string()]
        );
        assert!(parsed.stages[0].jobs[1].allow_failure);
        assert_eq!(
            parsed.stages[0].jobs[1].required_tags,
            vec!["docker".to_string(), "linux".to_string()]
        );
        assert_eq!(parsed.stages[1].jobs[0].name, "test");
        assert_eq!(parsed.stages[1].jobs[0].image, "rust:1.86");
        assert_eq!(parsed.stages[1].jobs[0].timeout_seconds, Some(2700));
        assert_eq!(
            parsed.stages[1].jobs[0].required_tags,
            vec!["linux".to_string()]
        );
        assert_eq!(
            parsed.stages[1].jobs[0].required_secrets,
            vec!["AWS_ACCESS_KEY_ID".to_string(), "DEPLOY_TOKEN".to_string()]
        );
        assert_eq!(
            parsed.stages[1].jobs[0].artifact_paths,
            vec![
                "reports/junit.xml".to_string(),
                "target/release/app.tar.gz".to_string()
            ]
        );
        assert_eq!(
            parsed.stages[1].jobs[0].command,
            "set -e\ncargo test\ncargo clippy --all-targets"
        );
    }

    #[test]
    fn v1_pipeline_plan_is_deterministic_and_records_needs() {
        let raw_config = r#"
version: 1
jobs:
  package:
    needs: [test]
    commands: ["tar -cf app.tar target/release/app"]
  build:
    commands: ["cargo build --release"]
  test:
    needs: [build]
    commands: ["cargo test"]
"#;
        let parsed = parse_pipeline_config(Some(raw_config)).expect("valid v1");
        let first = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "repository",
            raw_config.into(),
            &parsed,
        );
        let second = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "repository",
            raw_config.into(),
            &parsed,
        );

        assert_eq!(first.parser_version, "forge-dsl/1.0.0");
        assert_eq!(first.plan_sha256, second.plan_sha256);
        assert_eq!(first.config_sha256, second.config_sha256);
        assert_eq!(first.plan["format"], "v1-dag");
        assert_eq!(first.plan["version"], 1);
        assert_eq!(first.plan["jobs"].as_array().unwrap().len(), 3);
        assert_eq!(
            first.plan["jobs"][0]["required_tags"],
            serde_json::json!([])
        );
        assert_eq!(
            first.plan["jobs"][0]["required_secrets"],
            serde_json::json!([])
        );
        assert_eq!(
            first.plan["jobs"][0]["artifact_paths"],
            serde_json::json!([])
        );
        assert_eq!(first.plan["dependencies"].as_array().unwrap().len(), 2);
        assert_eq!(first.plan["dependencies"][0]["from"], "build");
        assert_eq!(first.plan["dependencies"][0]["to"], "test");
        assert_eq!(first.plan["dependencies"][1]["from"], "test");
        assert_eq!(first.plan["dependencies"][1]["to"], "package");
    }

    #[test]
    fn rejects_invalid_v1_dag_configuration() {
        for source in [
            "version: 1\njobs: {}\n",
            "version: 2\njobs:\n  build:\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    needs: [missing]\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    needs: [test]\n    commands: [echo build]\n  test:\n    needs: [build]\n    commands: [echo test]\n",
            "version: 1\njobs:\n  build:\n    commands: []\n",
            "version: 1\njobs:\n  build:\n    timeout: 25h\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    tags: [Prod]\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    secrets: [deploy_token]\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    artifacts:\n      paths: []\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    artifacts:\n      paths: [../target]\n    commands: [echo build]\n",
            "version: 1\njobs:\n  build:\n    artifacts:\n      paths: [target\\\\app.bin]\n    commands: [echo build]\n",
        ] {
            assert!(parse_pipeline_config(Some(source)).is_err(), "{source}");
        }
    }

    #[test]
    fn legacy_pipeline_plan_is_deterministic_and_records_stage_edges() {
        let parsed = ParsedPipelineConfig {
            stages: default_pipeline(),
            plan: ParsedPipelinePlan::Legacy,
        };
        let first = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "legacy_template",
            LEGACY_TEMPLATE_CONFIG.to_string(),
            &parsed,
        );
        let second = build_pipeline_plan_snapshot(
            "main",
            Some("abc123"),
            "legacy_template",
            LEGACY_TEMPLATE_CONFIG.to_string(),
            &parsed,
        );

        assert_eq!(first.plan_sha256, second.plan_sha256);
        assert_eq!(first.config_sha256, second.config_sha256);
        assert_eq!(first.plan["format"], "legacy-linear");
        assert_eq!(first.plan["stages"].as_array().unwrap().len(), 3);
        assert_eq!(first.plan["dependencies"].as_array().unwrap().len(), 2);
        assert_eq!(first.plan["dependencies"][0]["from"], "stage-0/job-0");
        assert_eq!(first.plan["dependencies"][0]["to"], "stage-1/job-0");
        assert_eq!(first.plan["dependencies"][1]["from"], "stage-1/job-0");
        assert_eq!(first.plan["dependencies"][1]["to"], "stage-2/job-0");
    }
}
