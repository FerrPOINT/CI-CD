//! Product credential storage and the restricted central owner reader are distinct.
use crate::api::ApiError;
use cicd_app::product_token::{ConfirmedPersonalToken, ProductTokenError};
use pdlc_auth_core::application_access_client::{ApplicationAccessClient, ApplicationAccessError};
use sqlx::PgPool;
use std::time::Duration;

static READER: tokio::sync::OnceCell<ApplicationAccessClient> = tokio::sync::OnceCell::const_new();
async fn reader() -> Result<&'static ApplicationAccessClient, ApiError> {
    READER
        .get_or_try_init(|| async {
            let issuer = std::env::var("CICD_AUTH__CENTRAL_ISSUER").map_err(|_| {
                ApiError::service_unavailable("Current owner reader not configured")
            })?;
            let credential = std::env::var("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN")
                .map_err(|_| {
                    ApiError::service_unavailable("Current owner reader not configured")
                })?;
            ApplicationAccessClient::new(&issuer, credential, Duration::from_secs(5))
                .map_err(|_| ApiError::service_unavailable("Current owner reader invalid"))
        })
        .await
}
fn store_error(error: ProductTokenError) -> ApiError {
    match error {
        ProductTokenError::Invalid => ApiError::unauthorized(),
        ProductTokenError::Denied => ApiError::forbidden(),
        ProductTokenError::Storage(error) => ApiError::internal(error),
    }
}
pub(crate) async fn authenticate(
    pool: &PgPool,
    token: &str,
) -> Result<ConfirmedPersonalToken, ApiError> {
    let local = cicd_app::product_token::lookup(pool, token)
        .await
        .map_err(store_error)?;
    let mut owners =
        reader()
            .await?
            .resolve(&[local.owner()])
            .await
            .map_err(|error| match error {
                ApplicationAccessError::Forbidden => ApiError::forbidden(),
                _ => ApiError::service_unavailable("Current owner admission unavailable"),
            })?;
    let owner = owners
        .remove(&local.owner())
        .ok_or_else(ApiError::forbidden)?;
    local.confirm(owner).map_err(store_error)
}
