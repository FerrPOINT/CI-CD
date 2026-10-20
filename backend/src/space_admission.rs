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
