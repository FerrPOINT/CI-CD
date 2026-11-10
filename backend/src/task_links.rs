//! Forge owns PR/task links and exact-commit check evidence. Stored reads never call Tracker.
use crate::api::{ApiError, AppState};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use pdlc_shared::resource_context::*;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) async fn verify_task(
    namespace: &NamespaceRef,
    task: &TaskRef,
) -> Result<String, ApiError> {
    let raw = std::env::var("CICD_NAMESPACE__TRACKER_URL")
        .map_err(|_| ApiError::service_unavailable("tracker_reader_not_configured"))?;
    let base = reqwest::Url::parse(&raw)
        .map_err(|_| ApiError::service_unavailable("invalid_tracker_reader_endpoint"))?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.path() != "/"
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(ApiError::service_unavailable(
            "invalid_tracker_reader_endpoint",
        ));
    }
    let instance = std::env::var("CICD_NAMESPACE__TRACKER_INSTANCE_ID")
        .ok()
        .and_then(|value| value.parse::<Uuid>().ok())
        .ok_or_else(|| ApiError::service_unavailable("tracker_reader_not_configured"))?;
    if task.tracker_instance_id != instance || task.task_id.is_nil() || instance.is_nil() {
        return Err(ApiError::bad_request("unregistered_task_ref"));
    }
    let file = std::env::var("CICD_NAMESPACE__TRACKER_TOKEN_FILE")
        .map_err(|_| ApiError::service_unavailable("tracker_reader_not_configured"))?;
    let token = tokio::fs::read_to_string(file)
        .await
        .map_err(|_| ApiError::service_unavailable("tracker_reader_not_configured"))?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::service_unavailable("tracker_reader_unavailable"))?;
    let mut response = client
        .get(
            base.join(&format!("api/v1/namespace-tasks/{}", task.task_id))
                .map_err(|_| ApiError::bad_request("invalid_task_ref"))?,
        )
        .query(&[
            ("registry_instance_id", namespace.registry_instance_id),
            ("namespace_id", namespace.namespace_id),
        ])
        .bearer_auth(token.trim())
        .send()
        .await
        .map_err(|_| ApiError::service_unavailable("tracker_reader_unavailable"))?;
    if response.status() == reqwest::StatusCode::FORBIDDEN
        || response.status() == reqwest::StatusCode::NOT_FOUND
    {
        return Err(ApiError::bad_request("foreign_or_missing_task_ref"));
    }
    if !response.status().is_success() {
        return Err(ApiError::service_unavailable("tracker_reader_unavailable"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::service_unavailable("tracker_reader_unavailable"))?
    {
        if bytes.len() + chunk.len() > 65536 {
            return Err(ApiError::service_unavailable("invalid_tracker_readback"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::service_unavailable("invalid_tracker_readback"))?;
    if value["namespace"]
        != serde_json::to_value(namespace)
            .map_err(|_| ApiError::bad_request("invalid_namespace"))?
        || value["tracker_instance_id"] != task.tracker_instance_id.to_string()
        || value["task_id"] != task.task_id.to_string()
        || value["state"] != "active"
    {
        return Err(ApiError::bad_request("task_ref_not_verified"));
    }
    value["task_key"]
        .as_str()
        .filter(|key| !key.is_empty() && key.len() <= 64)
        .map(str::to_owned)
        .ok_or_else(|| ApiError::service_unavailable("invalid_tracker_readback"))
}

pub(crate) async fn verify_primary_task(
    pool: &sqlx::PgPool,
    storage: &str,
    task: &TaskRef,
) -> Result<(Uuid, NamespaceRef, String), ApiError> {
    let (repository, group): (Uuid, Option<Uuid>) =
        sqlx::query_as("SELECT id,group_id FROM repository_catalog WHERE storage_name=$1")
            .bind(storage)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    let binding = crate::namespace::binding(
        pool,
        group.ok_or_else(|| ApiError::bad_request("managed_repository_required"))?,
    )
    .await?
    .ok_or_else(|| ApiError::bad_request("managed_repository_required"))?;
    let key = verify_task(&binding.namespace, task).await?;
    Ok((repository, binding.namespace, key))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_link(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    pull: Uuid,
    repository: Uuid,
    namespace: &NamespaceRef,
    task: &TaskRef,
    key: &str,
    commit: &str,
    author: Uuid,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,created_by_user_id,task_key_snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(pull_request_id,tracker_instance_id,task_id) DO NOTHING")
        .bind(pull).bind(repository).bind(task.tracker_instance_id).bind(task.task_id)
        .bind(namespace.registry_instance_id).bind(namespace.namespace_id).bind(commit)
        .bind(author).bind(key).execute(&mut **tx).await.map_err(ApiError::internal)?;
    Ok(())
}

#[utoipa::path(post,operation_id="forge_link_task_pull",path="/api/v1/catalog/repositories/{id}/pulls/{number}/tasks",tag="namespaces",params(("id"=Uuid,Path),("number"=i32,Path)),request_body=LinkTaskRequest,responses((status=200,body=TaskRef)))]
pub async fn link(
    State(state): State<Arc<AppState>>,
    Path((repository, number)): Path<(Uuid, i32)>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    permit: Option<Extension<crate::space_admission::RepositoryWrite>>,
    Json(input): Json<LinkTaskRequest>,
) -> Result<Json<TaskRef>, ApiError> {
    let task = TaskRef {
        tracker_instance_id: input.tracker_instance_id,
        task_id: input.task_id,
    };
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let storage = crate::repository_catalog::storage_by_id(pool, repository).await?;
    let write = crate::space_admission::RepositoryWrite::for_repository(
        pool,
        &storage,
        permit.map(|permit| permit.0),
    )
    .await?;
    // The inactive policy preserves legacy readback of an already stored link.
    // Active-policy replays must retain current write role/lifecycle admission.
    let lease = if write.is_some() {
        Some(crate::repository_catalog::write_lease(&state, &storage).await?)
    } else {
        None
    };
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    if let Some(permit) = write.as_ref() {
        permit.lock(&mut transaction).await?;
    }
    // Stored identity is the original-key readback. Replay neither revalidates a
    // neighboring owner nor snapshots a newer source branch commit.
    let existing: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pull_task_links l JOIN pull_requests p ON p.id=l.pull_request_id WHERE p.repository_id=$1 AND p.number=$2 AND l.tracker_instance_id=$3 AND l.task_id=$4)").bind(repository).bind(number).bind(task.tracker_instance_id).bind(task.task_id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
    crate::space_admission::require_write(write.as_ref())?;
    if existing {
        if !input.primary {
            return Ok(Json(task));
        }
        let primary:(Option<Uuid>,Option<Uuid>)=sqlx::query_as("SELECT primary_tracker_instance_id,primary_task_id FROM pull_requests WHERE repository_id=$1 AND number=$2 FOR UPDATE")
            .bind(repository).bind(number).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
        crate::space_admission::require_write(write.as_ref())?;
        if primary == (Some(task.tracker_instance_id), Some(task.task_id)) {
            return Ok(Json(task));
        }
        if primary.1.is_some() {
            return Err(ApiError::conflict("pull_primary_task_already_selected"));
        }
        // An existing link is already verified history. Selecting it as primary
        // must not require a branch that may have been deleted since the review.
        sqlx::query("UPDATE pull_requests SET primary_tracker_instance_id=$3,primary_task_id=$4 WHERE repository_id=$1 AND number=$2")
            .bind(repository).bind(number).bind(task.tracker_instance_id).bind(task.task_id)
            .execute(&mut *transaction).await.map_err(ApiError::internal)?;
        crate::space_admission::require_write(write.as_ref())?;
        transaction.commit().await.map_err(ApiError::internal)?;
        if let Some(lease) = lease {
            lease.commit().await.map_err(ApiError::internal)?;
        }
        return Ok(Json(task));
    }
    let lease = match lease {
        Some(lease) => lease,
        None => crate::repository_catalog::write_lease(&state, &storage).await?,
    };
    let group: Uuid = sqlx::query_scalar("SELECT group_id FROM repository_catalog WHERE id=$1")
        .bind(repository)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let binding = crate::namespace::binding(pool, group)
        .await?
        .ok_or_else(|| ApiError::bad_request("managed_repository_required"))?;
    let task_key = verify_task(&binding.namespace, &task).await?;
    crate::space_admission::require_write(write.as_ref())?;
    let pull = sqlx::query(
        "SELECT id,source_branch FROM pull_requests WHERE repository_id=$1 AND number=$2 FOR UPDATE",
    )
    .bind(repository)
    .bind(number)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    let source: String = pull.get("source_branch");
    let output = tokio::process::Command::new("git")
        .arg(format!(
            "--git-dir={}",
            state.git.root.join(format!("{storage}.git")).display()
        ))
        .args([
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("refs/heads/{source}^{{commit}}"),
        ])
        .output()
        .await
        .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success()
        || !matches!(commit.len(), 40 | 64)
        || !commit.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Err(ApiError::conflict("pull_source_commit_unavailable"));
    }
    crate::space_admission::require_write(write.as_ref())?;
    sqlx::query("INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,created_by_user_id,task_key_snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(pull_request_id,tracker_instance_id,task_id) DO NOTHING").bind(pull.get::<Uuid,_>("id")).bind(repository).bind(task.tracker_instance_id).bind(task.task_id).bind(binding.namespace.registry_instance_id).bind(binding.namespace.namespace_id).bind(commit).bind(claims.sub).bind(task_key).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    if input.primary {
        let changed=sqlx::query("UPDATE pull_requests SET primary_tracker_instance_id=$2,primary_task_id=$3 WHERE id=$1 AND (primary_task_id IS NULL OR (primary_task_id=$3 AND primary_tracker_instance_id=$2))")
            .bind(pull.get::<Uuid,_>("id")).bind(task.tracker_instance_id).bind(task.task_id)
            .execute(&mut *transaction).await.map_err(ApiError::internal)?;
        if changed.rows_affected() != 1 {
            return Err(ApiError::conflict("pull_primary_task_already_selected"));
        }
    }
    crate::space_admission::require_write(write.as_ref())?;
    transaction.commit().await.map_err(ApiError::internal)?;
    lease.commit().await.map_err(ApiError::internal)?;
    Ok(Json(task))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkTaskRequest {
    pub tracker_instance_id: Uuid,
    pub task_id: Uuid,
    #[serde(default)]
    pub primary: bool,
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct TaskLookupQuery {
    pub identifier: String,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TaskLookup {
    pub task: TaskRef,
    pub task_key: String,
}

#[utoipa::path(get,operation_id="forge_repository_task_lookup",path="/api/v1/repos/{repo}/task",tag="pulls",params(("repo"=String,Path),TaskLookupQuery),
 responses((status=200,body=TaskLookup)))]
pub async fn lookup(
    State(state): State<Arc<AppState>>,
    Path(repo): Path<String>,
    Query(query): Query<TaskLookupQuery>,
) -> Result<Json<TaskLookup>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let storage = crate::repository_catalog::resolve_storage(pool, &repo).await?;
    let group: Option<Uuid> =
        sqlx::query_scalar("SELECT group_id FROM repository_catalog WHERE storage_name=$1")
            .bind(storage)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    let binding = crate::namespace::binding(
        pool,
        group.ok_or_else(|| ApiError::bad_request("managed_repository_required"))?,
    )
    .await?
    .ok_or_else(|| ApiError::bad_request("managed_repository_required"))?;
    let tracker = std::env::var("CICD_NAMESPACE__TRACKER_INSTANCE_ID")
        .ok()
        .and_then(|value| value.parse::<Uuid>().ok())
        .ok_or_else(|| ApiError::service_unavailable("tracker_reader_not_configured"))?;
    let (task_id, task_key) = crate::development_workspaces::tracker::read_namespace_task(
        &binding.namespace,
        tracker,
        &query.identifier,
    )
    .await?;
    Ok(Json(TaskLookup {
        task: TaskRef {
            tracker_instance_id: tracker,
            task_id,
        },
        task_key,
    }))
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct TaskPullQuery {
    pub tracker_instance_id: Uuid,
    pub task_id: Uuid,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[utoipa::path(get,operation_id="forge_task_pull_catalog",path="/api/v1/task-pulls",tag="pulls",params(TaskPullQuery),
    responses((status=200,body=crate::pulls::PullRequestPage)))]
pub async fn task_pulls(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::AccessClaims>,
    Query(query): Query<TaskPullQuery>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    if query.task_id.is_nil() || query.tracker_instance_id.is_nil() {
        return Err(ApiError::bad_request("invalid_task_ref"));
    }
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let identity = request
        .extensions()
        .get::<crate::api::authz_mw::VerifiedRequestIdentity>()
        .cloned();
    let native_actor = identity
        .as_ref()
        .filter(|i| crate::development_workspaces::machine::principal(i))
        .map(|i| i.claims.sub);
    let admission = if native_actor.is_some() {
        None
    } else {
        crate::space_admission::catalog_read(pool, identity).await?
    };
    let groups = admission.as_ref().map(|scope| scope.groups.clone());
    if let Some(actor) = native_actor {
        // A Reviewer must not mistake a filtered subset for the task's full PR set.
        let incomplete: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pull_task_links l JOIN pull_requests p ON p.id=l.pull_request_id WHERE l.tracker_instance_id=$1 AND l.task_id=$2 AND NOT EXISTS(SELECT 1 FROM projects fp JOIN project_memberships m ON m.project_id=fp.id JOIN users u ON u.id=m.user_id WHERE fp.repository_id=p.repository_id AND m.user_id=$3 AND u.enabled AND u.role='service_account'))")
            .bind(query.tracker_instance_id).bind(query.task_id).bind(actor).fetch_one(pool).await.map_err(ApiError::internal)?;
        if incomplete {
            return Err(ApiError::forbidden());
        }
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    if !(0..=i64::from(u32::MAX)).contains(&offset) {
        return Err(ApiError::bad_request("invalid_offset"));
    }
    let filter = "FROM pull_requests p JOIN repository_catalog r ON r.id=p.repository_id WHERE EXISTS(SELECT 1 FROM pull_task_links l WHERE l.pull_request_id=p.id AND l.tracker_instance_id=$1 AND l.task_id=$2) AND ($3::uuid[] IS NULL OR r.group_id=ANY($3)) AND ($4::uuid IS NULL OR EXISTS(SELECT 1 FROM projects fp WHERE fp.id=$4 AND fp.repository_id=p.repository_id)) AND ($5::uuid IS NULL OR EXISTS(SELECT 1 FROM projects fp JOIN project_memberships m ON m.project_id=fp.id JOIN users u ON u.id=m.user_id WHERE fp.repository_id=p.repository_id AND m.user_id=$5 AND u.enabled AND u.role='service_account'))";
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) {filter}"))
        .bind(query.tracker_instance_id)
        .bind(query.task_id)
        .bind(&groups)
        .bind(claims.token_project_id)
        .bind(native_actor)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let items = sqlx::query_as::<_, crate::pulls::PullRequest>(&format!(
        "SELECT p.id,p.repository_name,p.repository_id,p.number,p.title,p.description,p.source_branch,p.target_branch,p.status,p.created_by,p.created_at,p.updated_at,p.merged_at,p.merge_commit_sha,p.primary_tracker_instance_id,p.primary_task_id {filter} ORDER BY p.id LIMIT $6 OFFSET $7"
    ))
    .bind(query.tracker_instance_id)
    .bind(query.task_id)
    .bind(&groups)
    .bind(claims.token_project_id)
    .bind(native_actor)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let response = Json(crate::pulls::PullRequestPage {
        items,
        total,
        limit: limit as u32,
        offset: offset as u32,
    })
    .into_response();
    match admission {
        Some(scope) => scope.response(response),
        None => Ok(response),
    }
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct EvidenceQuery {
    pub registry_instance_id: Uuid,
    pub namespace_id: Uuid,
    pub offset: Option<u32>,
}
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct PullTaskLink {
    pub namespace: NamespaceRef,
    pub task: TaskRef,
    pub task_key: String,
    pub source_commit_sha: String,
}
#[utoipa::path(get,operation_id="forge_pull_task_links",path="/api/v1/catalog/repositories/{id}/pulls/{number}/tasks",tag="namespaces",params(("id"=Uuid,Path),("number"=i32,Path),crate::repository_catalog::Page),responses((status=200,body=Vec<PullTaskLink>)))]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path((repository, number)): Path<(Uuid, i32)>,
    Query(page): Query<crate::repository_catalog::Page>,
) -> Result<Json<Vec<PullTaskLink>>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let group: Option<Uuid> =
        sqlx::query_scalar("SELECT group_id FROM repository_catalog WHERE id=$1")
            .bind(repository)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::not_found)?;
    let Some(group) = group else {
        return Ok(Json(Vec::new()));
    };
    let binding = crate::namespace::binding(pool, group)
        .await?
        .ok_or_else(|| ApiError::service_unavailable("namespace_projection_missing"))?;
    let rows=sqlx::query("SELECT l.tracker_instance_id,l.task_id,l.registry_instance_id,l.namespace_id,l.task_key_snapshot,l.source_commit_sha FROM pull_task_links l JOIN pull_requests p ON p.id=l.pull_request_id WHERE p.repository_id=$1 AND p.number=$2 ORDER BY l.created_at,l.task_id LIMIT $3 OFFSET $4").bind(repository).bind(number).bind(page.limit.unwrap_or(50).clamp(1,100)).bind(page.offset.unwrap_or(0).max(0)).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut result = Vec::new();
    for row in rows {
        let namespace = NamespaceRef {
            registry_instance_id: row.get("registry_instance_id"),
            namespace_id: row.get("namespace_id"),
        };
        if namespace != binding.namespace {
            return Err(ApiError::service_unavailable(
                "invalid_task_link_projection",
            ));
        }
        result.push(PullTaskLink {
            namespace,
            task: TaskRef {
                tracker_instance_id: row.get("tracker_instance_id"),
                task_id: row.get("task_id"),
            },
            task_key: row.get("task_key_snapshot"),
            source_commit_sha: row.get("source_commit_sha"),
        });
    }
    Ok(Json(result))
}
#[utoipa::path(get,operation_id="forge_task_pull_evidence",path="/api/v1/namespace-task-evidence/{tracker}/{task}",tag="namespaces",params(("tracker"=Uuid,Path),("task"=Uuid,Path),EvidenceQuery),responses((status=200,body=Vec<TaskPullEvidence>)))]
pub async fn evidence(
    State(state): State<Arc<AppState>>,
    Path((tracker, task)): Path<(Uuid, Uuid)>,
    Query(query): Query<EvidenceQuery>,
) -> Result<Json<Vec<TaskPullEvidence>>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let rows=sqlx::query("SELECT p.id,p.number,p.title,p.status,p.merge_commit_sha,l.source_commit_sha,l.repository_id,r.group_id,b.command FROM pull_task_links l JOIN pull_requests p ON p.id=l.pull_request_id JOIN repository_catalog r ON r.id=l.repository_id LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE l.tracker_instance_id=$1 AND l.task_id=$2 AND l.registry_instance_id=$3 AND l.namespace_id=$4 ORDER BY l.created_at DESC,p.id LIMIT 10 OFFSET $5").bind(tracker).bind(task).bind(query.registry_instance_id).bind(query.namespace_id).bind(i64::from(query.offset.unwrap_or(0))).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut items = Vec::new();
    for row in rows {
        let command = row
            .get::<Option<serde_json::Value>, _>("command")
            .ok_or_else(|| ApiError::service_unavailable("namespace_projection_missing"))?;
        let binding: OwnerCommand = serde_json::from_value(command)
            .map_err(|_| ApiError::service_unavailable("invalid_namespace_projection"))?;
        crate::namespace::validate_projection(&binding)?;
        if binding.namespace.registry_instance_id != query.registry_instance_id
            || binding.namespace.namespace_id != query.namespace_id
            || row.get::<Option<Uuid>, _>("group_id") != Some(binding.resource.resource_id)
        {
            return Err(ApiError::service_unavailable(
                "invalid_task_link_projection",
            ));
        }
        let repository: Uuid = row.get("repository_id");
        let mut commits = vec![row.get::<String, _>("source_commit_sha")];
        if let Some(merge) = row.get::<Option<String>, _>("merge_commit_sha") {
            if !commits.contains(&merge) {
                commits.push(merge);
            }
        }
        let checks=sqlx::query("SELECT pl.id,pl.commit_sha,pl.status FROM pipelines pl JOIN projects p ON p.id=pl.project_id WHERE p.repository_id=$1 AND pl.commit_sha=ANY($2) ORDER BY pl.created_at DESC,pl.id LIMIT 5").bind(repository).bind(&commits).fetch_all(pool).await.map_err(ApiError::internal)?.into_iter().map(|row| RepositoryCheckRef {pipeline_id:row.get("id"),commit_sha:row.get("commit_sha"),status:row.get("status")}).collect();
        items.push(TaskPullEvidence {
            namespace: binding.namespace,
            repository: RepositoryRef {
                forge_instance_id: binding.resource.instance_id,
                repository_id: repository,
            },
            pull_request_id: row.get("id"),
            number: row.get("number"),
            title: row.get("title"),
            status: row.get("status"),
            commits,
            checks,
        });
    }
    Ok(Json(items))
}
