//! Current human admission cannot issue or extend a Tracker task assignment.
use crate::api::ApiError;
use std::time::Instant;
use uuid::Uuid;

/// Call only after the registered Tracker response has passed the exact task,
/// assignment, binding and lease validation at the owning protocol boundary.
/// An optional bound belongs to a real credential, never to technical task ownership.
pub(crate) async fn intersect(
    subject: &str,
    native_until: Option<Instant>,
) -> Result<Option<Instant>, ApiError> {
    if native_until.is_some_and(|until| Instant::now() >= until) {
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
    .map(Some)
    .map_err(|error| match error {
        cicd_app::native_owner::NativeOwnerError::Forbidden => ApiError::forbidden(),
        cicd_app::native_owner::NativeOwnerError::Unavailable => {
            ApiError::service_unavailable("Current task owner admission unavailable")
        }
    })
}
