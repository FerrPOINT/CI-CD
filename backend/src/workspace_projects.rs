//! Tracker owns presentation; Forge owns local bindings and execution data.
use crate::api::{ApiError, AppState};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Utc};
use sdlc_shared::resource_context::{ResourceContextSummary, ResourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct WorkspaceProject {
    pub registry_instance_id: Uuid,
    pub namespace_id: Uuid,
    pub tracker_instance_id: Uuid,
    pub tracker_project_id: Uuid,
    pub generation: i64,
    pub name: String,
    pub project_key: String,
    pub state: String,
    pub observed_at: DateTime<Utc>,
    pub stale: bool,
    pub group_id: Option<Uuid>,
    pub group_slug: Option<String>,
    pub git_state: Option<String>,
    pub repositories: i64,
    pub latest_status: Option<String>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct WorkspacePage {
    pub items: Vec<WorkspaceProject>,
    pub total: i64,
}
#[derive(Default, Deserialize, utoipa::IntoParams)]
pub struct WorkspaceQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub search: Option<String>,
    pub repository_id: Option<Uuid>,
    pub configuration_id: Option<Uuid>,
    pub status: Option<String>,
    pub git_ref: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}
impl WorkspaceQuery {
    fn bounds(&self) -> (i64, i64) {
        (
            self.limit.unwrap_or(50).clamp(1, 100),
            self.offset.unwrap_or(0).max(0),
        )
    }
    fn search(&self) -> String {
        self.search.as_deref().unwrap_or("").trim().to_lowercase()
    }
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct WorkspaceSummary {
    pub projects: i64,
    pub repositories: i64,
    pub configurations: i64,
    pub queued: i64,
    pub running: i64,
    pub failed: i64,
}
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct ExecutionRow {
    pub namespace: sdlc_shared::resource_context::NamespaceRef,
    pub id: Uuid,
    pub repository_id: Uuid,
    pub repository_name: String,
    pub configuration_id: Uuid,
    pub configuration_name: String,
    pub status: String,
    pub git_ref: String,
    pub created_at: DateTime<Utc>,
    pub pipeline_id: Option<Uuid>,
    pub commit_sha: Option<String>,
    pub environment_id: Option<Uuid>,
    pub environment_name: Option<String>,
    pub approval_state: Option<String>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct ExecutionPage {
    pub items: Vec<ExecutionRow>,
    pub total: i64,
}
pub async fn validate_catalog(pool: &PgPool) -> Result<(), ApiError> {
    let rows=sqlx::query("SELECT b.* FROM forge_namespace_bindings b JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id").fetch_all(pool).await.map_err(ApiError::internal)?;
    for row in rows {
        let command: sdlc_shared::resource_context::OwnerCommand =
            serde_json::from_value(row.get("command"))
                .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
        crate::namespace::validate_projection(&command)?;
    }
    Ok(())
}
#[utoipa::path(get,operation_id="forge_workspace_all_pipelines",path="/api/v1/workspace-pipelines",tag="workspace-projects",params(WorkspaceQuery),responses((status=200,body=ExecutionPage)))]
pub async fn all_pipelines(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<ExecutionPage>, ApiError> {
    executions(&state, None, None, query, false).await.map(Json)
}
#[utoipa::path(get,operation_id="forge_workspace_all_summary",path="/api/v1/workspace-summary",tag="workspace-projects",responses((status=200,body=WorkspaceSummary)))]
pub async fn all_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WorkspaceSummary>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    validate_catalog(pool).await?;
    let row=sqlx::query("SELECT (SELECT count(*) FROM forge_workspace_projects) AS projects,(SELECT count(*) FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id) AS repositories,(SELECT count(*) FROM projects p JOIN repository_catalog r ON r.id=p.repository_id JOIN forge_namespace_bindings b ON b.resource_id=r.group_id JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id) AS configurations,count(*) FILTER(WHERE pl.status='queued') AS queued,count(*) FILTER(WHERE pl.status='running') AS running,count(*) FILTER(WHERE pl.status='failed') AS failed FROM pipelines pl JOIN projects p ON p.id=pl.project_id JOIN repository_catalog r ON r.id=p.repository_id JOIN forge_namespace_bindings b ON b.resource_id=r.group_id JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id").fetch_one(pool).await.map_err(ApiError::internal)?;
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM forge_workspace_catalog_sync)")
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::service_unavailable(
            "tracker_project_catalog_unavailable",
        ));
    }
    Ok(Json(WorkspaceSummary {
        projects: row.get("projects"),
        repositories: row.get("repositories"),
        configurations: row.get("configurations"),
        queued: row.get("queued"),
        running: row.get("running"),
        failed: row.get("failed"),
    }))
}
const PROJECT_SELECT: &str = "SELECT w.*,(w.observed_at < now()-interval '5 minutes' OR NOT coalesce((SELECT available FROM forge_workspace_catalog_sync),false)) AS stale,b.resource_id AS group_id,g.slug AS group_slug,b.state AS git_state,(SELECT count(*) FROM repository_catalog r WHERE r.group_id=g.id) AS repositories,(SELECT pl.status FROM pipelines pl JOIN projects p ON p.id=pl.project_id JOIN repository_catalog r ON r.id=p.repository_id WHERE r.group_id=g.id ORDER BY pl.created_at DESC,pl.id DESC LIMIT 1) AS latest_status FROM forge_workspace_projects w LEFT JOIN forge_namespace_bindings b ON b.registry_instance_id=w.registry_instance_id AND b.namespace_id=w.namespace_id LEFT JOIN git_groups g ON g.id=b.resource_id";

pub async fn ensure_context(
    pool: &PgPool,
    registry: Uuid,
    namespace: Uuid,
) -> Result<WorkspaceProject, ApiError> {
    let sql = format!("{PROJECT_SELECT} WHERE w.registry_instance_id=$1 AND w.namespace_id=$2");
    let result: WorkspaceProject = sqlx::query_as(&sql)
        .bind(registry)
        .bind(namespace)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if let Some(group) = result.group_id {
        crate::namespace::binding(pool, group)
            .await?
            .ok_or_else(|| ApiError::service_unavailable("namespace_projection_missing"))?;
    }
    Ok(result)
}
#[utoipa::path(get,operation_id="forge_workspace_list",path="/api/v1/workspace-projects",tag="workspace-projects",params(WorkspaceQuery),responses((status=200,body=WorkspacePage),(status=503,description="Tracker project catalog unavailable")))]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<WorkspacePage>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    validate_catalog(pool).await?;
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM forge_workspace_projects")
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    // An unavailable source must not masquerade as a valid empty catalogue.
    if known == 0
        && !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM forge_workspace_catalog_sync)",
        )
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::service_unavailable(
            "tracker_project_catalog_unavailable",
        ));
    }
    let search = query.search();
    let (limit, offset) = query.bounds();
    let filter = " WHERE position($1 in lower(w.name||' '||w.project_key))>0";
    let sql = format!(
        "{PROJECT_SELECT}{filter} ORDER BY w.name,w.registry_instance_id,w.namespace_id LIMIT $2 OFFSET $3"
    );
    let items = sqlx::query_as(&sql)
        .bind(&search)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let total = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM forge_workspace_projects w{filter}"
    ))
    .bind(search)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(WorkspacePage { items, total }))
}
#[utoipa::path(get,operation_id="forge_workspace_get",path="/api/v1/workspace-projects/{registry}/{namespace}",tag="workspace-projects",params(("registry"=Uuid,Path),("namespace"=Uuid,Path)),responses((status=200,body=WorkspaceProject)))]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path((registry, namespace)): Path<(Uuid, Uuid)>,
) -> Result<Json<WorkspaceProject>, ApiError> {
    Ok(Json(
        ensure_context(
            state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
            registry,
            namespace,
        )
        .await?,
    ))
}
#[utoipa::path(get,operation_id="forge_workspace_summary",path="/api/v1/workspace-projects/{registry}/{namespace}/summary",tag="workspace-projects",params(("registry"=Uuid,Path),("namespace"=Uuid,Path)),responses((status=200,body=WorkspaceSummary)))]
pub async fn summary(
    State(state): State<Arc<AppState>>,
    Path((registry, namespace)): Path<(Uuid, Uuid)>,
) -> Result<Json<WorkspaceSummary>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let context = ensure_context(pool, registry, namespace).await?;
    let row=sqlx::query("SELECT (SELECT count(*) FROM repository_catalog WHERE group_id=$1) AS repositories,(SELECT count(*) FROM projects p JOIN repository_catalog r ON r.id=p.repository_id WHERE r.group_id=$1) AS configurations,count(*) FILTER(WHERE pl.status='queued') AS queued,count(*) FILTER(WHERE pl.status='running') AS running,count(*) FILTER(WHERE pl.status='failed') AS failed FROM pipelines pl JOIN projects p ON p.id=pl.project_id JOIN repository_catalog r ON r.id=p.repository_id WHERE r.group_id=$1").bind(context.group_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    Ok(Json(WorkspaceSummary {
        projects: 1,
        repositories: row.get("repositories"),
        configurations: row.get("configurations"),
        queued: row.get("queued"),
        running: row.get("running"),
        failed: row.get("failed"),
    }))
}
#[utoipa::path(get,operation_id="forge_workspace_pipelines",path="/api/v1/workspace-projects/{registry}/{namespace}/pipelines",tag="workspace-projects",params(("registry"=Uuid,Path),("namespace"=Uuid,Path),WorkspaceQuery),responses((status=200,body=ExecutionPage)))]
pub async fn pipelines(
    State(state): State<Arc<AppState>>,
    Path((registry, namespace)): Path<(Uuid, Uuid)>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<ExecutionPage>, ApiError> {
    executions(&state, Some((registry, namespace)), None, query, false)
        .await
        .map(Json)
}
#[utoipa::path(get,operation_id="forge_workspace_deployments",path="/api/v1/workspace-projects/{registry}/{namespace}/deployments",tag="workspace-projects",params(("registry"=Uuid,Path),("namespace"=Uuid,Path),WorkspaceQuery),responses((status=200,body=ExecutionPage)))]
pub async fn deployments(
    State(state): State<Arc<AppState>>,
    Path((registry, namespace)): Path<(Uuid, Uuid)>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<ExecutionPage>, ApiError> {
    executions(&state, Some((registry, namespace)), None, query, true)
        .await
        .map(Json)
}
#[utoipa::path(get,operation_id="forge_workspace_repository_pipelines",path="/api/v1/catalog/repositories/{id}/pipelines",tag="workspace-projects",params(("id"=Uuid,Path),WorkspaceQuery),responses((status=200,body=ExecutionPage)))]
pub async fn repository_pipelines(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<ExecutionPage>, ApiError> {
    executions(&state, None, Some(id), query, false)
        .await
        .map(Json)
}
#[utoipa::path(get,operation_id="forge_workspace_repository_deployments",path="/api/v1/catalog/repositories/{id}/deployments",tag="workspace-projects",params(("id"=Uuid,Path),WorkspaceQuery),responses((status=200,body=ExecutionPage)))]
pub async fn repository_deployments(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<ExecutionPage>, ApiError> {
    executions(&state, None, Some(id), query, true)
        .await
        .map(Json)
}
async fn executions(
    state: &AppState,
    namespace: Option<(Uuid, Uuid)>,
    repository: Option<Uuid>,
    query: WorkspaceQuery,
    deployment: bool,
) -> Result<ExecutionPage, ApiError> {
    if repository
        .zip(query.repository_id)
        .is_some_and(|(path, filter)| path != filter)
    {
        return Err(ApiError::conflict("repository_namespace_mismatch"));
    }
    if query
        .since
        .zip(query.until)
        .is_some_and(|(since, until)| since > until)
    {
        return Err(ApiError::bad_request("invalid_execution_period"));
    }
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let group = if let Some((registry, ns)) = namespace {
        let c = ensure_context(pool, registry, ns).await?;
        Some(
            c.group_id
                .ok_or_else(|| ApiError::conflict("git_not_connected"))?,
        )
    } else if repository.is_none() {
        validate_catalog(pool).await?;
        if !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM forge_workspace_catalog_sync)",
        )
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?
        {
            return Err(ApiError::service_unavailable(
                "tracker_project_catalog_unavailable",
            ));
        }
        None
    } else {
        let id = repository.ok_or_else(ApiError::not_found)?;
        let group: Option<Uuid> =
            sqlx::query_scalar("SELECT group_id FROM repository_catalog WHERE id=$1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(ApiError::not_found)?;
        let group = group.ok_or_else(|| ApiError::conflict("repository_not_attached"))?;
        crate::namespace::binding(pool, group)
            .await?
            .ok_or_else(|| ApiError::service_unavailable("namespace_projection_missing"))?;
        Some(group)
    };
    if let Some(repo) = query.repository_id.or(repository) {
        let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=$1 AND ($2::uuid IS NULL OR r.group_id=$2))").bind(repo).bind(group).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !matches {
            return Err(ApiError::conflict("repository_namespace_mismatch"));
        }
    }
    if let Some(config) = query.configuration_id {
        let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects p JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=$1 AND ($2::uuid IS NULL OR r.group_id=$2) AND ($3::uuid IS NULL OR r.id=$3))").bind(config).bind(group).bind(query.repository_id.or(repository)).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !matches {
            return Err(ApiError::conflict("configuration_repository_mismatch"));
        }
    }
    let (from, extra) = if deployment {
        (
            "deployments x JOIN environments e ON e.id=x.environment_id JOIN projects p ON p.id=e.project_id",
            format!(
                "'environment_id',e.id,'environment_name',e.name,'approval_state',(SELECT approval_state FROM ({}) approval),'pipeline_id',x.pipeline_id",
                crate::platform::deployment_select("d.id=x.id", "")
            ),
        )
    } else {
        (
            "pipelines x JOIN projects p ON p.id=x.project_id",
            "'commit_sha',x.commit_sha,'pipeline_id',x.id".to_owned(),
        )
    };
    let filter = " JOIN repository_catalog r ON r.id=p.repository_id JOIN git_groups g ON g.id=r.group_id JOIN forge_namespace_bindings b ON b.resource_id=g.id JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id WHERE ($1::uuid IS NULL OR r.group_id=$1) AND ($2::uuid IS NULL OR r.id=$2) AND ($3::uuid IS NULL OR p.id=$3) AND ($4::text IS NULL OR x.status=$4) AND ($5::text IS NULL OR x.git_ref=$5) AND ($6::timestamptz IS NULL OR x.created_at >= $6) AND ($7::timestamptz IS NULL OR x.created_at <= $7)";
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {from}{filter}"))
        .bind(group)
        .bind(query.repository_id.or(repository))
        .bind(query.configuration_id)
        .bind(&query.status)
        .bind(&query.git_ref)
        .bind(query.since)
        .bind(query.until)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let (limit, offset) = query.bounds();
    let sql = format!(
        "SELECT jsonb_build_object('namespace',jsonb_build_object('registry_instance_id',b.registry_instance_id,'namespace_id',b.namespace_id),'id',x.id,'repository_id',r.id,'repository_name',g.slug||'/'||r.slug,'configuration_id',p.id,'configuration_name',p.name,'status',x.status,'git_ref',x.git_ref,'created_at',x.created_at,{extra}) FROM {from}{filter} ORDER BY x.created_at DESC,x.id DESC LIMIT $8 OFFSET $9"
    );
    let values: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(group)
        .bind(query.repository_id.or(repository))
        .bind(query.configuration_id)
        .bind(query.status)
        .bind(query.git_ref)
        .bind(query.since)
        .bind(query.until)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let items = values
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|_| ApiError::service_unavailable("invalid_execution_projection"))
        })
        .collect::<Result<Vec<ExecutionRow>, _>>()?;
    Ok(ExecutionPage { items, total })
}

/// Complete snapshots commit atomically; an interrupted reader never replaces good metadata.
pub async fn refresh(pool: &PgPool) -> Result<(), ApiError> {
    let origin =
        std::env::var("CICD_NAMESPACE__TRACKER_URL").map_err(|_| ApiError::unavailable())?;
    let base = reqwest::Url::parse(&origin).map_err(|_| ApiError::unavailable())?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.path() != "/"
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(ApiError::bad_request("invalid_tracker_reader_endpoint"));
    }
    let instance = std::env::var("CICD_NAMESPACE__TRACKER_INSTANCE_ID")
        .ok()
        .and_then(|v| v.parse::<Uuid>().ok())
        .filter(|v| !v.is_nil())
        .ok_or_else(ApiError::unavailable)?;
    let registry = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
        .ok()
        .and_then(|v| v.parse::<Uuid>().ok())
        .filter(|v| !v.is_nil())
        .ok_or_else(ApiError::unavailable)?;
    let file =
        std::env::var("CICD_NAMESPACE__TRACKER_TOKEN_FILE").map_err(|_| ApiError::unavailable())?;
    let token = tokio::fs::read_to_string(file)
        .await
        .map_err(|_| ApiError::unavailable())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::service_unavailable("invalid_tracker_readback"))?;
    let mut projects = Vec::new();
    let mut complete = false;
    for offset in (0..10000).step_by(100) {
        let mut response = client
            .get(
                base.join("api/v1/namespace-projects")
                    .map_err(|_| ApiError::service_unavailable("invalid_tracker_readback"))?,
            )
            .query(&[("limit", 100), ("offset", offset)])
            .bearer_auth(token.trim())
            .send()
            .await
            .map_err(|_| ApiError::service_unavailable("tracker_reader_unavailable"))?;
        if !response.status().is_success() {
            return Err(ApiError::service_unavailable("tracker_reader_unavailable"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::service_unavailable("invalid_tracker_readback"))?
        {
            if bytes.len() + chunk.len() > 262144 {
                return Err(ApiError::service_unavailable(
                    "tracker_reader_response_too_large",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let page: Vec<ResourceContextSummary> = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::service_unavailable("invalid_tracker_readback"))?;
        let last = page.len() < 100;
        for item in page {
            validate_project(&item, registry, instance)?;
            if projects.iter().any(|p: &ResourceContextSummary| {
                p.binding.namespace == item.binding.namespace
                    || p.binding.resource == item.binding.resource
            }) {
                return Err(ApiError::conflict("duplicate_tracker_project_identity"));
            }
            projects.push(item);
        }
        if last {
            complete = true;
            break;
        }
    }
    if !complete {
        return Err(ApiError::service_unavailable(
            "tracker_catalog_limit_exceeded",
        ));
    }
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    for item in projects {
        let affected=sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,now()) ON CONFLICT(registry_instance_id,namespace_id) DO UPDATE SET name=EXCLUDED.name,project_key=EXCLUDED.project_key,state=EXCLUDED.state,generation=EXCLUDED.generation,observed_at=EXCLUDED.observed_at WHERE forge_workspace_projects.tracker_instance_id=EXCLUDED.tracker_instance_id AND forge_workspace_projects.tracker_project_id=EXCLUDED.tracker_project_id AND forge_workspace_projects.generation<=EXCLUDED.generation").bind(item.binding.namespace.registry_instance_id).bind(item.binding.namespace.namespace_id).bind(item.binding.resource.instance_id).bind(item.binding.resource.resource_id).bind(item.binding.generation).bind(item.label).bind(item.resource_key).bind(item.binding.state).execute(&mut *tx).await.map_err(ApiError::internal)?.rows_affected();
        if affected != 1 {
            return Err(ApiError::conflict(
                "tracker_projection_identity_or_generation_conflict",
            ));
        }
    }
    sqlx::query("INSERT INTO forge_workspace_catalog_sync(id,observed_at,available) VALUES(true,now(),true) ON CONFLICT(id) DO UPDATE SET observed_at=EXCLUDED.observed_at,available=true").execute(&mut *tx).await.map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)
}
pub fn validate_project(
    item: &ResourceContextSummary,
    registry: Uuid,
    instance: Uuid,
) -> Result<(), ApiError> {
    let b = &item.binding;
    if b.schema_version != 1
        || b.namespace.registry_instance_id != registry
        || b.namespace.namespace_id.is_nil()
        || b.resource.kind != ResourceKind::TrackerProject
        || b.resource.instance_id != instance
        || b.resource.resource_id.is_nil()
        || b.operation_id.is_nil()
        || b.generation < 1
        || !matches!(b.state.as_str(), "active" | "archived")
        || item.label.trim().is_empty()
        || item.label.len() > 1024
        || item.resource_key.is_empty()
        || item.resource_key.len() > 64
    {
        return Err(ApiError::service_unavailable(
            "invalid_tracker_project_readback",
        ));
    }
    Ok(())
}
/// A failed refresh marks retained metadata stale without turning an unknown catalog into an empty one.
pub async fn refresh_checked(pool: &PgPool) -> Result<(), ApiError> {
    let result = refresh(pool).await;
    if result.is_err() {
        sqlx::query("UPDATE forge_workspace_catalog_sync SET available=false WHERE id=true")
            .execute(pool)
            .await
            .map_err(ApiError::internal)?;
    }
    result
}
pub async fn supervisor(pool: PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        if refresh_checked(&pool).await.is_err() {
            tracing::warn!(
                "Tracker project catalog refresh unavailable; preserving local projection"
            );
        }
    }
}
