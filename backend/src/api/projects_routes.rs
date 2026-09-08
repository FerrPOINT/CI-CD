//! Projects + memberships vertical (ADR-0012).

use super::dto::{CreateProject, Project, ProjectMembership, ProjectMembershipInput};
use super::pipelines_routes::valid_project_role;
use super::{ApiError, ApiResult, AppState, PageParams, list_projects_for_claims, pool};
use crate::platform::audit;
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Deserializer};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[utoipa::path(post, path="/api/v1/projects", tag="projects", request_body=CreateProject, responses((status=200, body=Project), (status=400, description="validation error")))]
pub(crate) async fn create_project(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreateProject>,
) -> ApiResult<Project> {
    if input.name.trim().is_empty() || input.repository_url.trim().is_empty() {
        return Err(ApiError::bad_request(
            "name and repository_url are required",
        ));
    }
    let db = pool(&state)?;
    let project = sqlx::query_as::<_, Project>(
        "INSERT INTO projects (id, name, repository_url, default_branch) VALUES ($1, $2, $3, $4) RETURNING id, name, repository_url, default_branch, max_running_jobs, created_at"
    ).bind(Uuid::new_v4()).bind(input.name.trim()).bind(input.repository_url.trim()).bind(input.default_branch.unwrap_or_else(|| "main".into())).fetch_one(db).await.map_err(ApiError::internal)?;
    if let Some(axum::Extension(claims)) = claims {
        if let Some(role) = default_project_role(&claims.role) {
            upsert_project_membership_record(db, project.id, claims.sub, role).await?;
        }
    }
    Ok(Json(project))
}

#[utoipa::path(get, path="/api/v1/projects", tag="projects", params(PageParams), responses((status=200, body=[Project])))]
pub(crate) async fn list_projects(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    axum::extract::Query(page): axum::extract::Query<PageParams>,
) -> ApiResult<Vec<Project>> {
    let (limit, offset) = page.bounded();
    let db = pool(&state)?;
    let projects = if state.auth_secret.is_some() {
        let claims = claims.ok_or_else(ApiError::unauthorized)?.0;
        let role = crate::authz::Role::parse(&claims.role).ok_or_else(ApiError::unauthorized)?;
        list_projects_for_claims(db, &claims, role, limit, offset).await?
    } else {
        sqlx::query_as::<_, Project>("SELECT id, name, repository_url, default_branch, max_running_jobs, created_at FROM projects ORDER BY created_at DESC LIMIT $1 OFFSET $2")
            .bind(limit)
            .bind(offset)
            .fetch_all(db)
            .await
            .map_err(ApiError::internal)?
    };
    Ok(Json(projects))
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}/memberships", tag="memberships", params(("project_id"=Uuid, Path)), responses((status=200, body=[ProjectMembership]), (status=404)))]
pub(crate) async fn list_project_memberships(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Vec<ProjectMembership>> {
    let db = pool(&state)?;
    ensure_project_exists(db, project_id).await?;
    let rows = sqlx::query_as::<_, ProjectMembership>(
        "SELECT m.project_id, m.user_id, u.username, u.enabled AS user_enabled, \
                m.role, m.created_at, m.updated_at \
         FROM project_memberships m \
         JOIN users u ON u.id = m.user_id \
         WHERE m.project_id = $1 \
         ORDER BY m.role, u.username",
    )
    .bind(project_id)
    .fetch_all(db)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

#[utoipa::path(post, path="/api/v1/projects/{project_id}/memberships", tag="memberships", request_body=ProjectMembershipInput, params(("project_id"=Uuid, Path)), responses((status=200, body=ProjectMembership), (status=400), (status=404)))]
pub(crate) async fn upsert_project_membership(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<ProjectMembershipInput>,
) -> ApiResult<ProjectMembership> {
    let role = input.role.trim();
    if !valid_project_role(role) {
        return Err(ApiError::bad_request(
            "role (maintainer, developer, viewer) is required",
        ));
    }
    let db = pool(&state)?;
    ensure_project_exists(db, project_id).await?;
    ensure_user_exists(db, input.user_id).await?;
    let membership = upsert_project_membership_record(db, project_id, input.user_id, role).await?;
    audit(
        db,
        "project_membership.upserted",
        "project",
        project_id,
        Some(&format!("{}:{}", membership.user_id, membership.role)),
    )
    .await?;
    Ok(Json(membership))
}

#[utoipa::path(delete, path="/api/v1/projects/{project_id}/memberships/{user_id}", tag="memberships", params(("project_id"=Uuid, Path), ("user_id"=Uuid, Path)), responses((status=200), (status=404), (status=409)))]
pub(crate) async fn delete_project_membership(
    State(state): State<Arc<AppState>>,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<serde_json::Value> {
    let db = pool(&state)?;
    ensure_project_exists(db, project_id).await?;
    let Some(role) = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_memberships WHERE project_id = $1 AND user_id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .map_err(ApiError::internal)?
    else {
        return Err(ApiError::not_found());
    };
    if role == "maintainer" {
        let maintainer_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM project_memberships WHERE project_id = $1 AND role = 'maintainer'",
        )
        .bind(project_id)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
        if maintainer_count <= 1 {
            return Err(ApiError::conflict(
                "project must keep at least one maintainer",
            ));
        }
    }
    sqlx::query("DELETE FROM project_memberships WHERE project_id = $1 AND user_id = $2")
        .bind(project_id)
        .bind(user_id)
        .execute(db)
        .await
        .map_err(ApiError::internal)?;
    audit(
        db,
        "project_membership.deleted",
        "project",
        project_id,
        Some(&user_id.to_string()),
    )
    .await?;
    Ok(Json(
        serde_json::json!({"deleted": user_id, "project_id": project_id}),
    ))
}

async fn ensure_project_exists(db: &PgPool, project_id: Uuid) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1)")
        .bind(project_id)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

async fn ensure_user_exists(db: &PgPool, user_id: Uuid) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(user_id)
        .fetch_one(db)
        .await
        .map_err(ApiError::internal)?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::not_found_named("user"))
    }
}

async fn upsert_project_membership_record(
    db: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<ProjectMembership, ApiError> {
    sqlx::query_as::<_, ProjectMembership>(
        "WITH upsert AS ( \
             INSERT INTO project_memberships (project_id, user_id, role) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (project_id, user_id) DO UPDATE \
                 SET role = EXCLUDED.role, updated_at = now() \
             RETURNING project_id, user_id, role, created_at, updated_at \
         ) \
         SELECT upsert.project_id, upsert.user_id, u.username, u.enabled AS user_enabled, \
                upsert.role, upsert.created_at, upsert.updated_at \
         FROM upsert JOIN users u ON u.id = upsert.user_id",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(role)
    .fetch_one(db)
    .await
    .map_err(ApiError::internal)
}

pub(crate) fn default_project_role(global_role: &str) -> Option<&'static str> {
    match crate::authz::Role::parse(global_role)? {
        crate::authz::Role::Admin | crate::authz::Role::Maintainer => Some("maintainer"),
        crate::authz::Role::Developer => Some("developer"),
        crate::authz::Role::Viewer => None,
    }
}

#[utoipa::path(get, path="/api/v1/projects/{project_id}", tag="projects", params(("project_id"=Uuid, Path)), responses((status=200, body=Project), (status=404)))]
pub(crate) async fn get_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Project> {
    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, repository_url, default_branch, max_running_jobs, created_at FROM projects WHERE id = $1",
    )
    .bind(project_id)
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(project))
}

fn deserialize_present_nullable_i32<'de, D>(
    deserializer: D,
) -> Result<Option<Option<i32>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<i32>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
pub(crate) struct UpdateProject {
    pub(crate) name: Option<String>,
    pub(crate) repository_url: Option<String>,
    pub(crate) default_branch: Option<String>,
    /// K4.3 dispatch fairness: max concurrently active-leased jobs for this
    /// project (>=1). Omit to keep; null clears the limit.
    #[serde(default, deserialize_with = "deserialize_present_nullable_i32")]
    pub(crate) max_running_jobs: Option<Option<i32>>,
}

#[utoipa::path(patch, path="/api/v1/projects/{project_id}", tag="projects", request_body=UpdateProject, params(("project_id"=Uuid, Path)), responses((status=200, body=Project), (status=404)))]
pub(crate) async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<UpdateProject>,
) -> ApiResult<Project> {
    if let (None, None, None, None) = (
        &input.name,
        &input.repository_url,
        &input.default_branch,
        &input.max_running_jobs,
    ) {
        return Err(ApiError::bad_request(
            "at least one of name, repository_url, default_branch, max_running_jobs is required",
        ));
    }
    if let Some(cap) = input.max_running_jobs.flatten()
        && !(1..=4096).contains(&cap)
    {
        return Err(ApiError::bad_request(
            "max_running_jobs must be between 1 and 4096",
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
        "UPDATE projects SET name = COALESCE($2, name), repository_url = COALESCE($3, repository_url), default_branch = COALESCE($4, default_branch), max_running_jobs = CASE WHEN $5 THEN $6 ELSE max_running_jobs END WHERE id = $1 RETURNING id, name, repository_url, default_branch, max_running_jobs, created_at",
    )
    .bind(project_id)
    .bind(input.name.as_deref().map(str::trim))
    .bind(input.repository_url.as_deref().map(str::trim))
    .bind(input.default_branch.as_deref().map(str::trim))
    .bind(input.max_running_jobs.is_some())
    .bind(input.max_running_jobs.flatten())
    .fetch_optional(pool(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(project))
}

#[utoipa::path(delete, path="/api/v1/projects/{project_id}", tag="projects", params(("project_id"=Uuid, Path)), responses((status=200), (status=404)))]
pub(crate) async fn delete_project(
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
