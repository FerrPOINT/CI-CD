//! Common visibility is additional to Git write roles and credential scopes.
use crate::{api::ApiError, central_auth::BRIDGE};
use pdlc_auth_core::service_bridge::{
    ConfirmedSpaceRead, SpaceAdmissionClient, SpaceAdmissionError, VerifiedCentralToken,
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

async fn client() -> Result<&'static SpaceAdmissionClient, ApiError> {
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

fn owner_error(error: SpaceAdmissionError) -> ApiError {
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
    actor: &VerifiedCentralToken,
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
    let proof = client()
        .await?
        .confirm_many_read(&BRIDGE, actor, std::slice::from_ref(&space))
        .await
        .map_err(owner_error)?;
    let admission = proof.admission().map_err(owner_error)?;
    let now = chrono::Utc::now().timestamp();
    let identity = pdlc_shared::space_access::CentralIdentity {
        issuer: actor.issuer.clone(),
        subject: actor.context.user_id.clone(),
    };
    if !admission.can_read_space(&identity, &space, now, actor.lifetime.local_valid_until()) {
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
    let actor = identity.central.as_ref().ok_or_else(ApiError::forbidden)?;
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
    let proof = client()
        .await?
        .confirm_many_read(&BRIDGE, actor, &spaces)
        .await
        .map_err(owner_error)?;
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
impl CatalogRead {
    pub(crate) fn response(
        self,
        mut response: axum::response::Response,
    ) -> Result<axum::response::Response, ApiError> {
        self.proof.admission().map_err(owner_error)?;
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        );
        let (parts, body) = response.into_parts();
        use tokio_stream::StreamExt;
        let stream = body.into_data_stream().map(move |chunk| {
            if !self.proof.is_current() {
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
