//! Auth foundation (AUTHZ_CONTRACT, AUTH_IMPLEMENTATION_SPEC Phase 1).
//!
//! - argon2id password hashing (`user_credentials`)
//! - short-lived JWT access tokens (HS256, `CICD_AUTH_SECRET`)
//! - opaque refresh tokens with rotation (`sessions`)
//! - session lookup helpers for protected API middleware
//!
//! Enforcement policy lives in `authz`; this module only proves identity.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ACCESS_TTL_MINUTES: i64 = 15;
pub const REFRESH_TTL_DAYS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("token expired")]
    Expired,
    #[error("invalid token")]
    Invalid,
    #[error("auth is not configured (CICD_AUTH_SECRET missing or empty)")]
    NotConfigured,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessClaims {
    /// user id
    pub sub: Uuid,
    /// Session id for access-token invalidation; absent only for non-session credentials.
    #[serde(default)]
    pub sid: Option<Uuid>,
    /// API token id for PAT credentials; absent for browser/session JWTs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<Uuid>,
    /// Optional project binding for scoped PAT credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_project_id: Option<Uuid>,
    /// Explicit PAT scopes. Empty only for browser/session JWTs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_scopes: Vec<String>,
    /// Role hint at issue time; middleware refreshes it from DB for session JWTs.
    pub role: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RefreshRequest {
    /// The previously issued refresh token.
    pub refresh_token: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TokenPair {
    pub access_token: String,
    /// unix seconds
    pub expires_at: i64,
    pub refresh_token: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LogoutRequest {
    /// The refresh token issued by /auth/login or /auth/refresh.
    pub refresh_token: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LogoutResponse {
    pub revoked: bool,
}

fn configured_secret_from(value: Option<String>) -> Result<String, AuthError> {
    match value {
        Some(secret) if !secret.trim().is_empty() => Ok(secret),
        _ => Err(AuthError::NotConfigured),
    }
}

pub(crate) fn configured_secret() -> Result<String, AuthError> {
    configured_secret_from(std::env::var("CICD_AUTH_SECRET").ok())
}

fn encoding_secret(secret: &str) -> EncodingKey {
    EncodingKey::from_secret(secret.as_bytes())
}

fn decoding_secret(secret: &str) -> DecodingKey {
    DecodingKey::from_secret(secret.as_bytes())
}

pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut rand_core::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AuthError::Invalid)
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn issue_access(user_id: Uuid, role: &str, session_id: Uuid) -> Result<TokenPair, AuthError> {
    let secret = configured_secret()?;
    issue_access_with_secret(user_id, role, session_id, &secret)
}

pub(crate) fn issue_access_with_secret(
    user_id: Uuid,
    role: &str,
    session_id: Uuid,
    secret: &str,
) -> Result<TokenPair, AuthError> {
    // Note: refresh token is issued separately by the session layer.
    let now = Utc::now();
    let exp = now + Duration::minutes(ACCESS_TTL_MINUTES);
    let claims = AccessClaims {
        sub: user_id,
        sid: Some(session_id),
        token_id: None,
        token_project_id: None,
        token_scopes: Vec::new(),
        role: role.to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    let token = jsonwebtoken::encode(&Header::default(), &claims, &encoding_secret(secret))
        .map_err(|_| AuthError::Invalid)?;
    Ok(TokenPair {
        access_token: token,
        expires_at: exp.timestamp(),
        refresh_token: String::new(),
    })
}

/// Verifies only JWT signature and expiry. Protected API routes must also
/// validate `AccessClaims::sid` with `access_session_user`.
pub fn verify_access(token: &str) -> Result<AccessClaims, AuthError> {
    let secret = configured_secret()?;
    verify_access_with_secret(token, &secret)
}

pub(crate) fn verify_access_with_secret(
    token: &str,
    secret: &str,
) -> Result<AccessClaims, AuthError> {
    let data = jsonwebtoken::decode::<AccessClaims>(
        token,
        &decoding_secret(secret),
        &Validation::default(),
    )
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::Expired,
        _ => AuthError::Invalid,
    })?;
    Ok(data.claims)
}

// --- refresh sessions -------------------------------------------------------

fn sha256_hex(input: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(input.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_refresh_token() -> String {
    let raw = format!("{}-{}", Uuid::new_v4(), Uuid::new_v4());
    sha256_hex(&raw)
}

pub fn new_csrf_token() -> String {
    new_refresh_token()
}

pub async fn create_session(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    refresh_hash: &str,
) -> Result<Uuid, AuthError> {
    create_session_with_csrf(pool, user_id, refresh_hash, None).await
}

pub async fn create_session_with_csrf(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    refresh_hash: &str,
    csrf_hash: Option<&str>,
) -> Result<Uuid, AuthError> {
    let id = Uuid::new_v4();
    let expires = Utc::now() + Duration::days(REFRESH_TTL_DAYS);
    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token_hash, csrf_token_hash, expires_at) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(user_id)
    .bind(refresh_hash)
    .bind(csrf_hash)
    .bind(expires)
    .execute(pool)
    .await?;
    Ok(id)
}

pub struct SessionUser {
    pub user_id: Uuid,
    pub role: String,
}

/// Validates a refresh token hash, enforcing expiry/revocation/enabled user.
pub async fn session_user(
    pool: &sqlx::PgPool,
    refresh_hash: &str,
) -> Result<SessionUser, AuthError> {
    session_user_for_refresh(pool, refresh_hash, None).await
}

async fn session_user_for_refresh(
    pool: &sqlx::PgPool,
    refresh_hash: &str,
    csrf_hash: Option<&str>,
) -> Result<SessionUser, AuthError> {
    let row = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT u.id, u.role FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.refresh_token_hash = $1 \
           AND ($2::TEXT IS NULL OR s.csrf_token_hash = $2) \
           AND s.revoked_at IS NULL AND s.expires_at > now() AND u.enabled",
    )
    .bind(refresh_hash)
    .bind(csrf_hash)
    .fetch_optional(pool)
    .await?;
    row.map(|(user_id, role)| SessionUser { user_id, role })
        .ok_or(AuthError::InvalidCredentials)
}

/// Validates that an access JWT is still bound to an active refresh session.
pub async fn access_session_user(
    pool: &sqlx::PgPool,
    session_id: Uuid,
    user_id: Uuid,
) -> Result<SessionUser, AuthError> {
    let row = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT u.id, u.role FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.user_id = $2 AND s.revoked_at IS NULL \
           AND s.expires_at > now() AND u.enabled",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.map(|(user_id, role)| SessionUser { user_id, role })
        .ok_or(AuthError::InvalidCredentials)
}

/// Rotate: revoke old session row, create a new one, return fresh pair.
pub async fn rotate_session(
    pool: &sqlx::PgPool,
    old_refresh_hash: &str,
) -> Result<(Uuid, Uuid, String), AuthError> {
    let rotated = rotate_session_with_csrf(pool, old_refresh_hash, None).await?;
    Ok((rotated.user_id, rotated.session_id, rotated.refresh_token))
}

pub struct RotatedSession {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub refresh_token: String,
    pub csrf_token: String,
}

pub async fn rotate_session_with_csrf(
    pool: &sqlx::PgPool,
    old_refresh_hash: &str,
    old_csrf_hash: Option<&str>,
) -> Result<RotatedSession, AuthError> {
    let user = session_user_for_refresh(pool, old_refresh_hash, old_csrf_hash).await?;
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE refresh_token_hash = $1")
        .bind(old_refresh_hash)
        .execute(pool)
        .await?;
    let new_refresh = new_refresh_token();
    let new_csrf = new_csrf_token();
    let new_csrf_hash = hash_token(&new_csrf);
    let session_id = create_session_with_csrf(
        pool,
        user.user_id,
        &hash_token(&new_refresh),
        Some(&new_csrf_hash),
    )
    .await?;
    Ok(RotatedSession {
        user_id: user.user_id,
        session_id,
        refresh_token: new_refresh,
        csrf_token: new_csrf,
    })
}

/// Revoke a refresh session by stored refresh-token hash. Idempotent for callers.
pub async fn revoke_session(
    pool: &sqlx::PgPool,
    refresh_hash: &str,
) -> Result<Option<Uuid>, AuthError> {
    let user_id = sqlx::query_scalar::<_, Uuid>(
        "UPDATE sessions SET revoked_at = now() \
         WHERE refresh_token_hash = $1 AND revoked_at IS NULL RETURNING user_id",
    )
    .bind(refresh_hash)
    .fetch_optional(pool)
    .await?;
    Ok(user_id)
}

pub fn hash_token(token: &str) -> String {
    sha256_hex(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("s3cret!").expect("hash");
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password(&hash, "s3cret!"));
        assert!(!verify_password(&hash, "wrong"));
    }

    #[test]
    fn access_token_requires_secret() {
        // No CICD_AUTH_SECRET in test env by default.
        let res = issue_access(Uuid::new_v4(), "admin", Uuid::new_v4());
        match res {
            Err(AuthError::NotConfigured) => {}
            _ => panic!("expected NotConfigured when secret is unset"),
        }
    }

    #[test]
    fn access_token_roundtrip_includes_session_id() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let pair = issue_access_with_secret(user_id, "admin", session_id, "test-secret")
            .expect("issue token");
        let claims =
            verify_access_with_secret(&pair.access_token, "test-secret").expect("verify token");
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.sid, Some(session_id));
        assert_eq!(claims.role, "admin");
    }

    #[test]
    fn blank_secret_is_not_configured() {
        let res = configured_secret_from(Some("  ".to_string()));
        assert!(matches!(res, Err(AuthError::NotConfigured)));
    }
}
