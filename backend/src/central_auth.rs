//! Bridge between Forge CI/CD auth and the central fleet auth-server
//! (services-base/auth-server, ES256 + JWKS, audience `sdlc`).
//!
//! When `CICD_AUTH__CENTRAL_JWKS_URI` is configured, bearer validation tries
//! the central token first; the verified email claim links a local shadow
//! user by username (created on demand, no local credential). Legacy HS256
//! session JWTs and `cicd_` PATs keep working — zero-downtime cutover.

use crate::auth::AccessClaims;
use sdlc_auth_core::{AuthContext, JwksCache, Validator};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::OnceCell;
use uuid::Uuid;

pub struct CentralAuth {
    validator: Validator,
    #[allow(dead_code)] // kept for future direct JWKS access (rotation checks)
    jwks: Arc<JwksCache>,
}

static CENTRAL: OnceCell<Option<CentralAuth>> = OnceCell::const_new();

/// Reads `CICD_AUTH__CENTRAL_JWKS_URI` / `CICD_AUTH__CENTRAL_ISSUER` once.
/// `None` when central auth is not configured (legacy-only mode).
pub async fn central() -> Option<&'static CentralAuth> {
    CENTRAL
        .get_or_init(|| async {
            let uri = std::env::var("CICD_AUTH__CENTRAL_JWKS_URI").ok()?;
            let issuer: Arc<String> = Arc::new(
                std::env::var("CICD_AUTH__CENTRAL_ISSUER")
                    .unwrap_or_else(|_| "http://127.0.0.1:7701".into()),
            );
            match JwksCache::connect(&uri).await {
                Ok(jwks) => {
                    let jwks = Arc::new(jwks);
                    let validator = Validator::Jwks {
                        jwks: jwks.clone(),
                        issuer,
                    };
                    jwks.clone().spawn_refresh(std::time::Duration::from_secs(3600));
                    tracing::info!(jwks_uri = %uri, "central auth enabled");
                    Some(CentralAuth { validator, jwks })
                }
                Err(error) => {
                    tracing::warn!(%error, jwks_uri = %uri, "central auth unavailable; falling back to legacy sessions");
                    None
                }
            }
        })
        .await
        .as_ref()
}

/// Attempts central validation. `Ok(None)` = not a central token (caller
/// falls back to PAT/legacy paths).
pub async fn try_central(token: &str) -> Option<AuthContext> {
    let central = central().await?;
    match central.validator.validate(token) {
        Ok(ctx) => Some(ctx),
        // kid resolution failure = legacy token, not ours
        Err(sdlc_auth_core::AuthError::Jwks(_)) => None,
        Err(other) => {
            tracing::warn!(error = %other, "central token validation failed");
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

/// Login proxy to the central auth-server (`CICD_AUTH__CENTRAL_LOGIN_URL`).
#[derive(serde::Deserialize)]
pub struct CentralAuthPair {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub user: Option<CentralUser>,
}

#[derive(serde::Deserialize)]
pub struct CentralUser {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

pub async fn try_central_login(
    username: &str,
    password: &str,
) -> Result<Option<CentralAuthPair>, Option<String>> {
    let Some(url) = std::env::var("CICD_AUTH__CENTRAL_LOGIN_URL").ok() else {
        return Ok(None);
    };
    if url.trim().is_empty() {
        return Ok(None);
    }
    // The central server authenticates by email; accept the local-part login
    // by extending it with the configured central email domain, if set.
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
    let timeout = std::env::var("CICD_AUTH__CENTRAL_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5u64);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|e| Some(e.to_string()))?;
    let response = client
        .post(&url)
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, url = %url, "central login unreachable");
            Some(e.to_string())
        })?;
    if !response.status().is_success() {
        return Err(None);
    }
    let pair = response
        .json::<CentralAuthPair>()
        .await
        .map_err(|e| Some(e.to_string()))?;
    Ok(Some(pair))
}
