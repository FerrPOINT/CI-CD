//! Forge CI/CD wiring of the shared central-auth bridge.
//!
//! JWKS validation and the login proxy live in
//! `sdlc_auth_core::service_bridge`; this file maps the central identity
//! to the local users table (shadow accounts by email-derived username).

use crate::auth::AccessClaims;
use sdlc_auth_core::AuthContext;
use sdlc_auth_core::service_bridge::{BridgeOutcome, ServiceBridge};
use sqlx::PgPool;
use uuid::Uuid;

/// Env prefix: CICD_AUTH__CENTRAL_{JWKS_URI,ISSUER,LOGIN_URL,TIMEOUT_SECS}.
pub static BRIDGE: ServiceBridge = ServiceBridge::new("CICD_AUTH__CENTRAL");

/// Central-first bearer check. `None` = fall back to PAT/legacy session.
pub async fn try_central(token: &str) -> Option<AuthContext> {
    match BRIDGE.try_token(token).await {
        BridgeOutcome::Validated(ctx) => Some(ctx),
        BridgeOutcome::Expired => None, // treated as not-ours; middleware 401s later
        BridgeOutcome::NotOurs | BridgeOutcome::NotConfigured => None,
        BridgeOutcome::Invalid(reason) => {
            tracing::debug!(reason, "bearer is not a valid central token; legacy path");
            None
        }
    }
}

/// Resolves the local user for a central identity (by email, falling back to
/// the local-part username), creating a shadow account on first use (no
/// credential row — local password login impossible for central users).
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
    let username = email.split('@').next().unwrap_or_default().to_string();
    if username.is_empty() {
        return Err(crate::api::ApiError::unauthorized());
    }
    // Existing user by email-derived username or literal email in username.
    let user: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, role FROM users WHERE lower(username) = $1 OR lower(username) = $2 LIMIT 1",
    )
    .bind(&username)
    .bind(&email)
    .fetch_optional(pool)
    .await
    .map_err(crate::api::ApiError::internal)?;
    let (user_id, role) = match user {
        Some(found) => found,
        None => {
            let id = Uuid::new_v4();
            let created: (Uuid, String) = sqlx::query_as(
                "INSERT INTO users (id, username, role, enabled) \
                 VALUES ($1, $2, 'developer', true) RETURNING id, role",
            )
            .bind(id)
            .bind(&username)
            .fetch_one(pool)
            .await
            .map_err(crate::api::ApiError::internal)?;
            tracing::info!(user_id = %created.0, "linked central identity as shadow user");
            created
        }
    };
    let now = chrono::Utc::now();
    Ok(AccessClaims {
        sub: user_id,
        sid: None, // central token; session invalidation is central-side
        token_id: None,
        token_project_id: None,
        token_scopes: Vec::new(),
        role,
        ver: 0,
        iat: now.timestamp(),
        exp: now.timestamp() + 900,
    })
}

/// Central login proxy; `None` = not configured / rejected / unreachable.
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
