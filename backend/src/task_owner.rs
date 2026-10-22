//! Current human admission intersects, but cannot issue, a confirmed native task lease.
use crate::api::ApiError;
use std::time::Instant;
use uuid::Uuid;

/// Call only after the registered Tracker response has passed the exact task,
/// assignment, binding and lease validation at the owning protocol boundary.
/// This function returns freshness, not a native credential or human role.
pub(crate) async fn intersect(subject: &str, native_until: Instant) -> Result<Instant, ApiError> {
    if Instant::now() >= native_until {
        return Err(ApiError::forbidden());
    }
    if !crate::central_auth::BRIDGE
        .application_access_required()
        .await
        .map_err(|_| ApiError::service_unavailable("Current task owner admission unavailable"))?
    {
        return Ok(native_until);
    }
    let subject_id = Uuid::parse_str(subject).map_err(|_| ApiError::forbidden())?;
    if subject_id.is_nil() || subject_id.to_string() != subject {
        return Err(ApiError::forbidden());
    }
    cicd_app::native_owner::admit(
        crate::product_tokens::reader().await?,
        subject_id,
        native_until,
    )
    .await
    .map_err(|error| match error {
        cicd_app::native_owner::NativeOwnerError::Forbidden => ApiError::forbidden(),
        cicd_app::native_owner::NativeOwnerError::Unavailable => {
            ApiError::service_unavailable("Current task owner admission unavailable")
        }
    })
}
