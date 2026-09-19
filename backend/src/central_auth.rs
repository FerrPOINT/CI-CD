//! Forge CI/CD wiring of the shared central-auth bridge.
//!
//! JWKS validation and the login proxy live in
//! `sdlc_auth_core::service_bridge`; this file maps the central identity
//! to a local profile keyed by the immutable central subject.

use crate::auth::AccessClaims;
use sdlc_auth_core::AuthContext;
use sdlc_auth_core::service_bridge::{BridgeOutcome, ServiceBridge};
use sqlx::PgPool;
use uuid::Uuid;

/// Env prefix: CICD_AUTH__CENTRAL_{JWKS_URI,ISSUER,LOGIN_URL,TIMEOUT_SECS}.
pub static BRIDGE: ServiceBridge = ServiceBridge::new("CICD_AUTH__CENTRAL");

/// Central-first bearer check. `None` means the token is explicitly outside
/// the central namespace; configured consumers still reject legacy human JWTs.
pub async fn try_central(token: &str) -> Result<Option<AuthContext>, crate::api::ApiError> {
    match BRIDGE.try_token(token).await {
        BridgeOutcome::Validated(ctx) => Ok(Some(ctx)),
        BridgeOutcome::Expired => Err(crate::api::ApiError::unauthorized()),
        BridgeOutcome::NotOurs | BridgeOutcome::NotConfigured => Ok(None),
        BridgeOutcome::Invalid(reason) => {
            tracing::debug!(reason, "central token rejected");
            Err(crate::api::ApiError::unauthorized())
        }
        BridgeOutcome::Unavailable => Err(crate::api::ApiError::service_unavailable(
            "Central Auth is temporarily unavailable",
        )),
    }
}

/// Resolves only by the immutable central subject; historical usernames stay
/// untouched even when they match a newly created central account.
pub async fn link_central_user(
    pool: &PgPool,
    ctx: &AuthContext,
) -> Result<AccessClaims, crate::api::ApiError> {
    let email = ctx
        .email
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if email.is_empty() {
        return Err(crate::api::ApiError::unauthorized());
    }
    if ctx.user_id.trim().is_empty() {
        return Err(crate::api::ApiError::unauthorized());
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, username, role, enabled, central_sub) \
         VALUES ($1, $2, 'developer', true, $3) \
         ON CONFLICT (central_sub) WHERE central_sub IS NOT NULL DO NOTHING",
    )
    .bind(id)
    .bind(format!("central-{}", id.simple()))
    .bind(&ctx.user_id)
    .execute(pool)
    .await
    .map_err(crate::api::ApiError::internal)?;
    let (user_id, _historical_role, enabled): (Uuid, String, bool) =
        sqlx::query_as("SELECT id, role, enabled FROM users WHERE central_sub = $1")
            .bind(&ctx.user_id)
            .fetch_one(pool)
            .await
            .map_err(crate::api::ApiError::internal)?;
    if !enabled {
        return Err(crate::api::ApiError::unauthorized());
    }
    let now = chrono::Utc::now();
    Ok(AccessClaims {
        service_account_name: None,
        sub: user_id,
        sid: ctx
            .session_id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok()),
        token_id: None,
        token_project_id: None,
        token_scopes: ctx.scopes.iter().cloned().collect(),
        role: "admin".to_string(),
        ver: 0,
        iat: now.timestamp(),
        exp: now.timestamp() + 900,
    })
}

/// Legacy login bridge used only when browser SSO is not configured.
pub async fn try_login(
    username: &str,
    password: &str,
) -> Option<sdlc_auth_core::service_bridge::CentralTokenPair> {
    // The central server authenticates by email; extend the bare username
    // with the configured domain, if any.
    let email = if username.contains('@') {
        username.to_string()
    } else {
        match std::env::var("CICD_AUTH__CENTRAL_EMAIL_DOMAIN").ok() {
            Some(domain) if !domain.trim().is_empty() => {
                format!("{username}@{}", domain.trim())
            }
            _ => username.to_string(),
        }
    };
    match BRIDGE.try_login(&email, password).await {
        Ok(pair) => pair,
        Err(transport) => {
            tracing::warn!(%transport, "central login failed; local fallback");
            None
        }
    }
}
