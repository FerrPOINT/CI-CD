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
use chrono::{DateTime, Duration, Utc};
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
    /// User token version at issue time. Reuse detection bumps it server-side.
    #[serde(default)]
    pub ver: i64,
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
    issue_access_with_secret_version(user_id, role, session_id, 0, secret)
}

pub(crate) fn issue_access_with_secret_version(
    user_id: Uuid,
    role: &str,
    session_id: Uuid,
    token_version: i64,
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
        ver: token_version,
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
        "INSERT INTO sessions (id, user_id, refresh_token_hash, csrf_token_hash, expires_at, family_id) \
         VALUES ($1, $2, $3, $4, $5, $1)",
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
    pub token_version: i64,
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
    let row = sqlx::query_as::<_, (Uuid, String, i64)>(
        "SELECT u.id, u.role, u.token_version FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.refresh_token_hash = $1 \
           AND ($2::TEXT IS NULL OR s.csrf_token_hash = $2) \
           AND s.revoked_at IS NULL AND s.expires_at > now() AND u.enabled",
    )
    .bind(refresh_hash)
    .bind(csrf_hash)
    .fetch_optional(pool)
    .await?;
    row.map(|(user_id, role, token_version)| SessionUser {
        user_id,
        role,
        token_version,
    })
    .ok_or(AuthError::InvalidCredentials)
}

/// Validates that an access JWT is still bound to an active refresh session.
pub async fn access_session_user(
    pool: &sqlx::PgPool,
    session_id: Uuid,
    user_id: Uuid,
) -> Result<SessionUser, AuthError> {
    access_session_user_with_version(pool, session_id, user_id, None).await
}

pub async fn access_session_user_with_version(
    pool: &sqlx::PgPool,
    session_id: Uuid,
    user_id: Uuid,
    token_version: Option<i64>,
) -> Result<SessionUser, AuthError> {
    let row = sqlx::query_as::<_, (Uuid, String, i64)>(
        "SELECT u.id, u.role, u.token_version FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.user_id = $2 AND s.revoked_at IS NULL \
           AND s.expires_at > now() AND u.enabled \
           AND ($3::BIGINT IS NULL OR u.token_version = $3)",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(token_version)
    .fetch_optional(pool)
    .await?;
    row.map(|(user_id, role, token_version)| SessionUser {
        user_id,
        role,
        token_version,
    })
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
    pub role: String,
    pub token_version: i64,
    pub session_id: Uuid,
    pub refresh_token: String,
    pub csrf_token: String,
}

struct RefreshSessionRow {
    id: Uuid,
    user_id: Uuid,
    role: String,
    token_version: i64,
    enabled: bool,
    family_id: Uuid,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    replaced_by: Option<Uuid>,
    csrf_token_hash: Option<String>,
}

pub async fn rotate_session_with_csrf(
    pool: &sqlx::PgPool,
    old_refresh_hash: &str,
    old_csrf_hash: Option<&str>,
) -> Result<RotatedSession, AuthError> {
    let mut tx = pool.begin().await?;
    let family_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT family_id FROM sessions WHERE refresh_token_hash = $1",
    )
    .bind(old_refresh_hash)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AuthError::InvalidCredentials)?;
    lock_session_family(&mut tx, family_id).await?;

    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            i64,
            bool,
            Uuid,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            Option<Uuid>,
            Option<String>,
        ),
    >(
        "SELECT s.id, s.user_id, u.role, u.token_version, u.enabled, s.family_id, \
                s.expires_at, s.revoked_at, s.replaced_by, s.csrf_token_hash \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.refresh_token_hash = $1 \
         FOR UPDATE OF s",
    )
    .bind(old_refresh_hash)
    .fetch_optional(&mut *tx)
    .await?
    .map(
        |(
            id,
            user_id,
            role,
            token_version,
            enabled,
            family_id,
            expires_at,
            revoked_at,
            replaced_by,
            csrf_token_hash,
        )| RefreshSessionRow {
            id,
            user_id,
            role,
            token_version,
            enabled,
            family_id,
            expires_at,
            revoked_at,
            replaced_by,
            csrf_token_hash,
        },
    )
    .ok_or(AuthError::InvalidCredentials)?;

    if let Some(csrf_hash) = old_csrf_hash {
        if row.csrf_token_hash.as_deref() != Some(csrf_hash) {
            return Err(AuthError::InvalidCredentials);
        }
    }

    if row.replaced_by.is_some() {
        revoke_session_family(&mut tx, row.family_id, row.user_id).await?;
        tx.commit().await?;
        return Err(AuthError::InvalidCredentials);
    }

    if !row.enabled || row.revoked_at.is_some() || row.expires_at <= Utc::now() {
        return Err(AuthError::InvalidCredentials);
    }

    let new_refresh = new_refresh_token();
    let new_csrf = new_csrf_token();
    let new_csrf_hash = hash_token(&new_csrf);
    let session_id = Uuid::new_v4();
    let expires = Utc::now() + Duration::days(REFRESH_TTL_DAYS);
    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token_hash, csrf_token_hash, expires_at, family_id) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(session_id)
    .bind(row.user_id)
    .bind(hash_token(&new_refresh))
    .bind(Some(new_csrf_hash.as_str()))
    .bind(expires)
    .bind(row.family_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE sessions SET revoked_at = now(), replaced_by = $2 WHERE id = $1")
        .bind(row.id)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(RotatedSession {
        user_id: row.user_id,
        role: row.role,
        token_version: row.token_version,
        session_id,
        refresh_token: new_refresh,
        csrf_token: new_csrf,
    })
}

async fn revoke_session_family(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    family_id: Uuid,
    user_id: Uuid,
) -> Result<(), AuthError> {
    let newly_marked = sqlx::query_scalar::<_, i64>(
        "WITH marked AS ( \
             UPDATE sessions \
             SET revoked_at = COALESCE(revoked_at, now()), \
                 reuse_detected_at = now() \
             WHERE family_id = $1 AND reuse_detected_at IS NULL \
             RETURNING 1 \
         ) SELECT COUNT(*)::BIGINT FROM marked",
    )
    .bind(family_id)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query(
        "UPDATE sessions \
         SET revoked_at = COALESCE(revoked_at, now()) \
         WHERE family_id = $1",
    )
    .bind(family_id)
    .execute(&mut **tx)
    .await?;

    if newly_marked > 0 {
        sqlx::query("UPDATE users SET token_version = token_version + 1 WHERE id = $1")
            .bind(user_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn lock_session_family(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    family_id: Uuid,
) -> Result<(), AuthError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(session_family_lock_key(family_id))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn session_family_lock_key(family_id: Uuid) -> i64 {
    let bytes = family_id.as_bytes();
    let mut high = [0_u8; 8];
    let mut low = [0_u8; 8];
    high.copy_from_slice(&bytes[..8]);
    low.copy_from_slice(&bytes[8..]);
    (u64::from_be_bytes(high) ^ u64::from_be_bytes(low)) as i64
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
