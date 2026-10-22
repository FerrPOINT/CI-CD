//! Common visibility is additional to Git write roles and credential scopes.
use crate::api::ApiError;
use pdlc_auth_core::service_bridge::{
    ConfirmedSpaceRead, SpaceAdmissionClient, SpaceAdmissionError,
};
use pdlc_shared::space_access::{SpaceRef, SpaceState};
use sqlx::PgPool;

static CLIENT: tokio::sync::OnceCell<SpaceAdmissionClient> = tokio::sync::OnceCell::const_new();

pub(crate) async fn active(pool: &PgPool) -> Result<bool, ApiError> {
    sqlx::query_scalar("SELECT active FROM forge_space_admission_state WHERE singleton")
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)
}

pub(crate) async fn client() -> Result<&'static SpaceAdmissionClient, ApiError> {
    CLIENT
        .get_or_try_init(|| async {
            let url = std::env::var("CICD_SPACES__ADMISSION_URL")
                .map_err(|_| ApiError::service_unavailable("space_admission_not_configured"))?;
            let token = std::env::var("CICD_SPACES__ADMISSION_TOKEN")
                .map_err(|_| ApiError::service_unavailable("space_admission_not_configured"))?;
            SpaceAdmissionClient::new(&url, token)
                .map_err(|_| ApiError::service_unavailable("space_admission_configuration_invalid"))
        })
        .await
}

pub(crate) fn owner_error(error: SpaceAdmissionError) -> ApiError {
    match error {
        SpaceAdmissionError::Expired => ApiError::forbidden(),
        _ => ApiError::service_unavailable("space_admission_unavailable"),
    }
}

/// Resolve only the immutable repository -> group -> owner Space mapping.
/// Legacy binding/name/first project cannot supply an unconfirmed replacement.
pub(crate) async fn repository(
    pool: &PgPool,
    storage: &str,
    actor: &crate::api::authz_mw::VerifiedRequestIdentity,
    write: bool,
) -> Result<ConfirmedSpaceRead, ApiError> {
    let row: Option<(uuid::Uuid, uuid::Uuid, i64, String)> = sqlx::query_as(
        "SELECT c.registry_instance_id,c.space_id,c.revision,c.state \
         FROM repository_catalog r JOIN forge_space_catalog c ON c.group_id=r.group_id \
         WHERE r.storage_name=$1",
    )
    .bind(storage)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    let (registry_instance_id, space_id, revision, state) = row.ok_or_else(ApiError::forbidden)?;
    let space = SpaceRef {
        registry_instance_id,
        space_id,
    };
    let proof = actor
        .confirm_spaces(client().await?, std::slice::from_ref(&space))
        .await?;
    let admission = proof.admission().map_err(owner_error)?;
    let now = chrono::Utc::now().timestamp();
    let identity = actor.space_identity()?;
    if !admission.can_read_space(&identity, &space, now, actor.claims.exp) {
        return Err(ApiError::forbidden());
    }
    let confirmed = admission
        .spaces
        .iter()
        .find(|item| item.space == space)
        .ok_or_else(ApiError::forbidden)?;
    if write && confirmed.revision != revision {
        return Err(ApiError::service_unavailable(
            "space_projection_revision_stale",
        ));
    }
    if write && (state != "active" || confirmed.state != SpaceState::Active) {
        return Err(ApiError::conflict("space_resource_read_only"));
    }
    if !matches!(state.as_str(), "active" | "archiving" | "archived") {
        return Err(ApiError::service_unavailable(
            "space_projection_state_invalid",
        ));
    }
    Ok(proof)
}

/// Private proof used only by catalog reads. Empty selected policy is an empty
/// SQL filter, never NULL/unrestricted. No DTO, role or bearer is exposed.
pub(crate) struct CatalogRead {
    proof: ConfirmedSpaceRead,
    pub(crate) groups: Vec<uuid::Uuid>,
}
pub(crate) async fn catalog_read(
    pool: &PgPool,
    identity: Option<crate::api::authz_mw::VerifiedRequestIdentity>,
) -> Result<Option<CatalogRead>, ApiError> {
    if !active(pool).await? {
        return Ok(None);
    }
    let identity = identity.ok_or_else(ApiError::forbidden)?;
    identity.require_scope("ci-cd:read")?;
    let rows: Vec<(uuid::Uuid, uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        "SELECT group_id,registry_instance_id,space_id FROM forge_space_catalog \
         WHERE group_id IS NOT NULL AND state IN ('active','archiving','archived') \
         ORDER BY registry_instance_id,space_id LIMIT 10001",
    )
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    if rows.len() > 10000 {
        return Err(ApiError::service_unavailable(
            "space_admission_catalog_too_large",
        ));
    }
    let spaces: Vec<_> = rows
        .iter()
        .map(|(_, registry_instance_id, space_id)| SpaceRef {
            registry_instance_id: *registry_instance_id,
            space_id: *space_id,
        })
        .collect();
    let proof = identity.confirm_spaces(client().await?, &spaces).await?;
    let readable: std::collections::HashSet<_> = proof
        .readable_spaces()
        .map_err(owner_error)?
        .into_iter()
        .collect();
    let groups = rows
        .into_iter()
        .filter_map(|(group, registry_instance_id, space_id)| {
            readable
                .contains(&SpaceRef {
                    registry_instance_id,
                    space_id,
                })
                .then_some(group)
        })
        .collect();
    Ok(Some(CatalogRead { proof, groups }))
}
/// Only these human GET/HEAD adapters consume this repository read proof.
/// Writes, CI, downloads and machine adapters need their own effect boundary.
pub(crate) fn repository_read_route(method: &axum::http::Method, route: &str) -> bool {
    matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD)
        && matches!(
            route,
            "/api/v1/repos/{repo}/refs"
                | "/api/v1/repos/{repo}/tree"
                | "/api/v1/repos/{repo}/blob"
                | "/api/v1/repos/{repo}/tags"
                | "/api/v1/repos/{repo}/commits"
                | "/api/v1/repos/{repo}/compare"
                | "/api/v1/repos/{repo}/pulls"
                | "/api/v1/repos/{repo}/pulls/page"
                | "/api/v1/repos/{repo}/pulls/{number}"
                | "/api/v1/catalog/repositories/{id}/pulls"
                | "/api/v1/catalog/repositories/{id}/pulls/{number}"
                | "/api/v1/catalog/repositories/{id}/pulls/{number}/tasks"
        )
}

pub(crate) fn repository_write_route(method: &axum::http::Method, route: &str) -> bool {
    *method == axum::http::Method::POST
        && matches!(
            route,
            "/api/v1/repos/{repo}/pulls"
                | "/api/v1/repos/{repo}/pulls/{number}/action"
                | "/api/v1/catalog/repositories/{id}/pulls"
                | "/api/v1/catalog/repositories/{id}/pulls/{number}/action"
                | "/api/v1/catalog/repositories/{id}/pulls/{number}/tasks"
        )
}

/// Opaque request-owned permit. Only verified middleware may construct it;
/// neither a caller DTO/header nor an internal metadata accessor supplies it.
#[derive(Clone)]
pub struct RepositoryWrite {
    proof: std::sync::Arc<ConfirmedSpaceRead>,
    storage: String,
    actor: uuid::Uuid,
    min_role: crate::authz::Role,
}
impl RepositoryWrite {
    pub(crate) fn confirmed(
        proof: ConfirmedSpaceRead,
        storage: String,
        actor: uuid::Uuid,
        min_role: crate::authz::Role,
    ) -> Self {
        Self {
            proof: std::sync::Arc::new(proof),
            storage,
            actor,
            min_role,
        }
    }
    pub(crate) async fn for_repository(
        pool: &PgPool,
        storage: &str,
        permit: Option<Self>,
    ) -> Result<Option<Self>, ApiError> {
        if let Some(permit) = permit {
            permit.require()?;
            if permit.storage != storage {
                return Err(ApiError::forbidden());
            }
            Ok(Some(permit))
        } else if active(pool).await? {
            Err(ApiError::forbidden())
        } else {
            Ok(None)
        }
    }
    pub(crate) fn require(&self) -> Result<(), ApiError> {
        self.proof.admission().map_err(owner_error).map(|_| ())
    }
    pub(crate) fn proof(&self) -> &ConfirmedSpaceRead {
        &self.proof
    }
    /// Keep confirmed local role and lifecycle state stable until commit.
    /// The existing namespace lease has already fenced archive and mapping changes.
    pub(crate) async fn lock(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<String, ApiError> {
        self.require()?;
        let row: Option<(String, String, bool)> =
            sqlx::query_as("SELECT username,role,enabled FROM users WHERE id=$1 FOR SHARE")
                .bind(self.actor)
                .fetch_optional(&mut **tx)
                .await
                .map_err(ApiError::internal)?;
        let (username, role, enabled) = row.ok_or_else(ApiError::forbidden)?;
        if !enabled || crate::authz::Role::parse(&role).is_none_or(|role| role < self.min_role) {
            return Err(ApiError::forbidden());
        }
        let row: Option<(uuid::Uuid,uuid::Uuid,i64,String)>=sqlx::query_as(
            "SELECT c.registry_instance_id,c.space_id,c.revision,c.state FROM repository_catalog r JOIN forge_space_catalog c ON c.group_id=r.group_id WHERE r.storage_name=$1 FOR SHARE OF c",
        ).bind(&self.storage).fetch_optional(&mut **tx).await.map_err(ApiError::internal)?;
        let (registry_instance_id, space_id, revision, state) =
            row.ok_or_else(ApiError::forbidden)?;
        let admission = self.proof.admission().map_err(owner_error)?;
        if state != "active" {
            return Err(ApiError::conflict("space_resource_read_only"));
        }
        if !admission.spaces.iter().any(|s| {
            s.space
                == SpaceRef {
                    registry_instance_id,
                    space_id,
                }
                && s.revision == revision
                && s.state == SpaceState::Active
        }) {
            return Err(ApiError::service_unavailable(
                "space_projection_revision_stale",
            ));
        }
        self.require()?;
        Ok(username)
    }
    pub(crate) fn response(
        &self,
        response: axum::response::Response,
    ) -> Result<axum::response::Response, ApiError> {
        if !self.proof.is_current() {
            // A known rejected operation stays rejected without protected detail.
            // A success or an unknown effect must require exact state readback.
            if response.status().is_client_error() {
                return Err(ApiError::forbidden());
            }
            return Err(ApiError::service_unavailable(
                "Результат операции PR не подтверждён; перечитайте состояние перед повтором",
            ));
        }
        // Keep the same proof across response handoff without cloning/minting it.
        let proof = self.proof.clone();
        let (mut parts, body) = response.into_parts();
        parts.headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        );
        use tokio_stream::StreamExt;
        let stream = body.into_data_stream().map(move |chunk| {
            if !proof.is_current() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "space_admission_expired",
                ))
            } else {
                chunk.map_err(std::io::Error::other)
            }
        });
        Ok(axum::response::Response::from_parts(
            parts,
            axum::body::Body::from_stream(stream),
        ))
    }
}
pub(crate) fn require_write(permit: Option<&RepositoryWrite>) -> Result<(), ApiError> {
    permit.map(RepositoryWrite::require).transpose().map(|_| ())
}

/// Match the framework route, then use the same decoded params as the handler.
/// Raw URI parsing must never turn an encoded owner into unscoped access.
pub(crate) async fn repository_request_storage(
    pool: &PgPool,
    parts: &mut axum::http::request::Parts,
) -> Result<Option<String>, ApiError> {
    let route = parts
        .extensions
        .get::<axum::extract::MatchedPath>()
        .map(|path| path.as_str())
        .unwrap_or("");
    if !repository_read_route(&parts.method, route) && !repository_write_route(&parts.method, route)
    {
        return Ok(None);
    }
    use axum::extract::FromRequestParts;
    let params = axum::extract::RawPathParams::from_request_parts(parts, &())
        .await
        .map_err(|_| ApiError::bad_request("invalid_repository_path"))?;
    // Resolve identity only. Kind/readiness are protected metadata and remain
    // the handler's responsibility after admission, never a pre-admission oracle.
    let storage: Option<String> = if let Some((_, repo)) =
        params.iter().find(|(key, _)| *key == "repo")
    {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT r.storage_name FROM repository_aliases a JOIN repository_catalog r ON r.id=a.repository_id WHERE a.alias=$1",
        ).bind(repo.trim_end_matches(".git")).fetch_optional(pool).await.map_err(ApiError::internal)?.flatten()
    } else {
        let id = params
            .iter()
            .find(|(key, _)| *key == "id")
            .and_then(|(_, value)| uuid::Uuid::parse_str(value).ok())
            .ok_or_else(|| ApiError::bad_request("invalid_repository_id"))?;
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT storage_name FROM repository_catalog WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .flatten()
    };
    storage.map(Some).ok_or_else(ApiError::not_found)
}

impl CatalogRead {
    pub(crate) fn response(
        self,
        response: axum::response::Response,
    ) -> Result<axum::response::Response, ApiError> {
        read_response(self.proof, response)
    }
}

pub(crate) fn read_response(
    proof: ConfirmedSpaceRead,
    mut response: axum::response::Response,
) -> Result<axum::response::Response, ApiError> {
    proof.admission().map_err(owner_error)?;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    let (parts, body) = response.into_parts();
    use tokio_stream::StreamExt;
    let stream = body.into_data_stream().map(move |chunk| {
        if !proof.is_current() {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "space_admission_expired",
            ))
        } else {
            chunk.map_err(std::io::Error::other)
        }
    });
    Ok(axum::response::Response::from_parts(
        parts,
        axum::body::Body::from_stream(stream),
    ))
}

/// A display descriptor, never a reusable repository/Git capability.
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct HumanAdmissionWindowV1 {
    pub schema_version: u32,
    pub enforced: bool,
    pub remaining_millis: Option<u64>,
    pub scope_revision: Option<String>,
}
#[utoipa::path(get,path="/api/v1/space-admission",tag="spaces",responses((status=200,body=HumanAdmissionWindowV1),(status=403),(status=503)))]
pub async fn human_window(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    if !active(pool).await? {
        return Ok((
            [(axum::http::header::CACHE_CONTROL, "private, no-store")],
            axum::Json(HumanAdmissionWindowV1 {
                schema_version: 1,
                enforced: false,
                remaining_millis: None,
                scope_revision: None,
            }),
        )
            .into_response());
    }
    let identity = request
        .extensions()
        .get::<crate::api::authz_mw::VerifiedRequestIdentity>()
        .cloned()
        .ok_or_else(ApiError::forbidden)?;
    drop(request);
    let scope = catalog_read(pool, Some(identity.clone()))
        .await?
        .ok_or_else(ApiError::forbidden)?;
    let admission = scope.proof.admission().map_err(owner_error)?;
    let mut read = scope.proof.readable_spaces().map_err(owner_error)?;
    read.sort_by_key(|s| (s.registry_instance_id, s.space_id));
    let subject = identity.space_identity()?;
    let write: Vec<_> = read
        .iter()
        .filter(|s| {
            admission.has_active_space(
                &subject,
                s,
                chrono::Utc::now().timestamp(),
                identity.claims.exp,
            )
        })
        .collect();
    let native: Vec<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT project_id,role FROM project_memberships WHERE user_id=$1 ORDER BY project_id",
    )
    .bind(identity.claims.sub)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let mut token_scopes = identity.claims.token_scopes.clone();
    token_scopes.sort_unstable();
    let bytes = serde_json::to_vec(&(
        &subject,
        &identity.claims.role,
        token_scopes,
        identity.claims.token_project_id,
        read.clone(),
        write,
        native,
    ))
    .map_err(|_| ApiError::service_unavailable("display_admission_unavailable"))?;
    use sha2::{Digest, Sha256};
    let revision = format!("{:x}", Sha256::digest(bytes));
    let remaining = u64::try_from(
        scope
            .proof
            .monotonic_deadline()
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(ApiError::forbidden)?
            .as_millis(),
    )
    .map_err(|_| ApiError::forbidden())?;
    if remaining == 0 || remaining > 60_000 {
        return Err(ApiError::forbidden());
    }
    scope.response(
        axum::Json(HumanAdmissionWindowV1 {
            schema_version: 1,
            enforced: true,
            remaining_millis: Some(remaining),
            scope_revision: Some(revision),
        })
        .into_response(),
    )
}
