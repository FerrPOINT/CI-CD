//! Private coordinator transport. Human admin/PAT authority never substitutes.
use crate::{api::AppState, central_auth};
use std::sync::Arc;
type SharedState = Arc<AppState>;
use crate::store::identity_deletion::OwnerError;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use pdlc_shared::identity_lifecycle::DeletionRequest;
use serde_json::json;
use uuid::Uuid;

const SCOPE: &str = "ci-cd:identity-lifecycle";

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route(
            "/api/internal/v1/user-deletions/{operation_id}/{action}",
            post(handle),
        )
        .layer(axum::extract::DefaultBodyLimit::max(16_384))
        .layer(axum::middleware::map_response(
            |mut response: Response| async move {
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-store"),
                );
                response
            },
        ))
}
fn registered(subject: &str) -> bool {
    std::env::var("CICD_IDENTITY_DELETION_COORDINATOR_SUBJECTS")
        .unwrap_or_default()
        .split(',')
        .any(|item| !item.trim().is_empty() && item.trim() == subject)
}
fn failure(error: OwnerError) -> Response {
    let status = match error {
        OwnerError::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
        OwnerError::Conflict => StatusCode::CONFLICT,
        OwnerError::Expired => StatusCode::UNAUTHORIZED,
        OwnerError::Unbound | OwnerError::Unresolved | OwnerError::Storage => {
            StatusCode::SERVICE_UNAVAILABLE
        }
    };
    (status, Json(json!({"error":{"code":error.to_string()}}))).into_response()
}
async fn handle(
    State(state): State<SharedState>,
    Path((operation_id, action)): Path<(Uuid, String)>,
    headers: HeaderMap,
    Json(request): Json<DeletionRequest>,
) -> Response {
    let mut response = match tokio::time::timeout(
        std::time::Duration::from_secs(9),
        execute(state, operation_id, &action, &headers, request),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => failure(OwnerError::Storage),
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
async fn execute(
    state: SharedState,
    operation_id: Uuid,
    action: &str,
    headers: &HeaderMap,
    request: DeletionRequest,
) -> Response {
    if !request.valid()
        || request.operation_id != operation_id
        || !matches!(action, "check" | "freeze" | "release")
    {
        return failure(OwnerError::Invalid);
    }
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !pdlc_auth_core::is_personal_token(token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let verified = match central_auth::BRIDGE.try_verified_token(token).await {
        Ok(value) => value,
        Err(
            pdlc_auth_core::service_bridge::VerifiedBridgeError::Unavailable
            | pdlc_auth_core::service_bridge::VerifiedBridgeError::NotConfigured,
        ) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let ctx = &verified.context;
    if verified.subject_role.as_deref() != Some("service_account")
        || ctx.session_id.is_some()
        || !registered(&ctx.user_id)
        || !ctx.has_scope(SCOPE)
        || verified.issuer != request.identity.issuer
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(pool) = state.pool.as_ref() else {
        return failure(OwnerError::Storage);
    };
    let store = crate::store::identity_deletion::IdentityDeletionStore::new(pool.clone());
    let window = match crate::store::identity_deletion::IdentityDeletionWindow::confirmed(&verified)
    {
        Ok(window) => window,
        Err(error) => return failure(error),
    };
    let response = match action {
        "check" => match store.check(&request, &window).await {
            Ok(value) => Json(value).into_response(),
            Err(error) => failure(error),
        },
        "freeze" => match store.freeze(&request, &window).await {
            Ok(value) => Json(value).into_response(),
            Err(error) => failure(error),
        },
        "release" => match store.release(&request, &window).await {
            Ok(value) => Json(value).into_response(),
            Err(error) => failure(error),
        },
        _ => failure(OwnerError::Invalid),
    };
    if !verified.lifetime.is_current() {
        return failure(OwnerError::Expired);
    }
    response
}
