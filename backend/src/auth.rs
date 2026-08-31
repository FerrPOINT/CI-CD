//! Auth foundation (AUTHZ_CONTRACT, AUTH_IMPLEMENTATION_SPEC Phase 1).
//!
//! - argon2id password hashing (`user_credentials`)
//! - short-lived JWT access tokens (HS256, `CICD_AUTH_SECRET`)
//! - opaque refresh tokens with rotation (`sessions`)
//! - `Authorization: Bearer <jwt>` extraction
//!
//! Enforcement policy lives in `authz`; this module only proves identity.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::http::HeaderMap;
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
    /// role at issue time
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

pub(crate) fn is_configured() -> bool {
    configured_secret_from(std::env::var("CICD_AUTH_SECRET").ok()).is_ok()
}

fn encoding_secret() -> Result<EncodingKey, AuthError> {
    let secret = configured_secret_from(std::env::var("CICD_AUTH_SECRET").ok())?;
    Ok(EncodingKey::from_secret(secret.as_bytes()))
}

fn decoding_secret() -> Result<DecodingKey, AuthError> {
    let secret = configured_secret_from(std::env::var("CICD_AUTH_SECRET").ok())?;
    Ok(DecodingKey::from_secret(secret.as_bytes()))
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

pub fn issue_access(user_id: Uuid, role: &str) -> Result<TokenPair, AuthError> {
    // Note: refresh token is issued separately by the session layer.
    let now = Utc::now();
    let exp = now + Duration::minutes(ACCESS_TTL_MINUTES);
    let claims = AccessClaims {
        sub: user_id,
        role: role.to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    let token = jsonwebtoken::encode(&Header::default(), &claims, &encoding_secret()?)
        .map_err(|_| AuthError::Invalid)?;
    Ok(TokenPair {
        access_token: token,
        expires_at: exp.timestamp(),
        refresh_token: String::new(),
    })
}

pub fn verify_access(token: &str) -> Result<AccessClaims, AuthError> {
    let data =
        jsonwebtoken::decode::<AccessClaims>(token, &decoding_secret()?, &Validation::default())
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::Expired,
                _ => AuthError::Invalid,
            })?;
    Ok(data.claims)
}

/// Extracts and verifies a `Authorization: Bearer` JWT.
pub fn bearer_claims(headers: &HeaderMap) -> Result<AccessClaims, AuthError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::Invalid)?;
    let token = value.strip_prefix("Bearer ").ok_or(AuthError::Invalid)?;
    verify_access(token)
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

pub async fn create_session(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    refresh_hash: &str,
) -> Result<Uuid, AuthError> {
    let id = Uuid::new_v4();
    let expires = Utc::now() + Duration::days(REFRESH_TTL_DAYS);
    sqlx::query("INSERT INTO sessions (id, user_id, refresh_token_hash, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(user_id)
        .bind(refresh_hash)
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
    let row = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT u.id, u.role FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.refresh_token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now() AND u.enabled",
    )
    .bind(refresh_hash)
    .fetch_optional(pool)
    .await?;
    row.map(|(user_id, role)| SessionUser { user_id, role })
        .ok_or(AuthError::InvalidCredentials)
}

/// Rotate: revoke old session row, create a new one, return fresh pair.
pub async fn rotate_session(
    pool: &sqlx::PgPool,
    old_refresh_hash: &str,
) -> Result<(Uuid, String), AuthError> {
    let user = session_user(pool, old_refresh_hash).await?;
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE refresh_token_hash = $1")
        .bind(old_refresh_hash)
        .execute(pool)
        .await?;
    let new_refresh = new_refresh_token();
    create_session(pool, user.user_id, &hash_token(&new_refresh)).await?;
    Ok((user.user_id, new_refresh))
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
        let res = issue_access(Uuid::new_v4(), "admin");
        match res {
            Err(AuthError::NotConfigured) => {}
            _ => panic!("expected NotConfigured when secret is unset"),
        }
    }

    #[test]
    fn blank_secret_is_not_configured() {
        let res = configured_secret_from(Some("  ".to_string()));
        assert!(matches!(res, Err(AuthError::NotConfigured)));
    }
}
