//! Auth vertical (login/refresh/logout + cookie helpers) — migrated out of the
//! api.rs monolith (ADR-0012). Token/session primitives live in `cicd_app::auth`.

use super::{
    AUTH_CSRF_COOKIE, AUTH_CSRF_HEADER, AUTH_REFRESH_COOKIE, ApiError, AppState, auth_secret, pool,
};
use crate::platform::audit;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, header};
use std::sync::Arc;
use uuid::Uuid;

#[utoipa::path(post, path="/api/v1/auth/login", tag="auth", request_body=crate::auth::LoginRequest, responses((status=200, body=crate::auth::TokenPair), (status=401)))]
pub(crate) async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::auth::LoginRequest>,
) -> Result<(HeaderMap, Json<crate::auth::TokenPair>), ApiError> {
    use crate::auth::*;
    crate::metrics::LOGIN_ATTEMPTS_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pool = pool(&state)?;
    // Central fleet auth first; local credential login remains the fallback
    // during the migration window (central_auth.rs).
    if let Some(pair) = crate::central_auth::try_login(&input.username, &input.password).await {
        if let Some(central) = crate::central_auth::try_central(&pair.access_token).await {
            let claims = crate::central_auth::link_central_user(pool, &central).await?; // shadow user ensured
            // Issue a local refresh session so the browser survives access-token
            // expiry (the central token itself has no refresh cookie here).
            let refresh = new_refresh_token();
            let csrf = new_csrf_token();
            let csrf_hash = hash_token(&csrf);
            let session_id =
                create_session_with_csrf(pool, claims.sub, &hash_token(&refresh), Some(&csrf_hash))
                    .await
                    .map_err(ApiError::from)?;
            let mut out = issue_access_with_secret_version(
                claims.sub,
                &claims.role,
                session_id,
                claims.ver,
                auth_secret(&state)?,
            )
            .map_err(|_| ApiError::unauthorized())?;
            out.refresh_token = refresh.clone();
            let _ = audit(
                pool,
                "auth.login_success",
                "user",
                claims.sub,
                Some(input.username.trim()),
            )
            .await;
            return Ok((auth_cookie_headers(&state, &refresh, &csrf), Json(out)));
        }
    }
    let row = sqlx::query_as::<_, (Uuid, String, bool, i64, String)>(
        "SELECT u.id, u.role, u.enabled, u.token_version, c.password_hash FROM users u \
         JOIN user_credentials c ON c.user_id = u.id WHERE u.username = $1",
    )
    .bind(input.username.trim())
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::unauthorized)?;
    let (user_id, role, enabled, token_version, password_hash) = row;
    if !enabled || !verify_password(&password_hash, &input.password) {
        let _ = audit(
            pool,
            "auth.login_failed",
            "user",
            user_id,
            Some(input.username.trim()),
        )
        .await;
        return Err(ApiError::unauthorized());
    }
    let _ = audit(
        pool,
        "auth.login_success",
        "user",
        user_id,
        Some(input.username.trim()),
    )
    .await;
    let refresh = new_refresh_token();
    let csrf = new_csrf_token();
    let csrf_hash = hash_token(&csrf);
    let session_id =
        create_session_with_csrf(pool, user_id, &hash_token(&refresh), Some(&csrf_hash))
            .await
            .map_err(ApiError::from)?;
    let mut pair = issue_access_with_secret_version(
        user_id,
        &role,
        session_id,
        token_version,
        auth_secret(&state)?,
    )
    .map_err(|_| ApiError::unauthorized())?;
    pair.refresh_token = refresh.clone();
    Ok((auth_cookie_headers(&state, &refresh, &csrf), Json(pair)))
}

#[utoipa::path(post, path="/api/v1/auth/refresh", tag="auth", request_body=crate::auth::RefreshRequest, responses((status=200, body=crate::auth::TokenPair), (status=401)))]
pub(crate) async fn auth_refresh(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<crate::auth::RefreshRequest>,
) -> Result<(HeaderMap, Json<crate::auth::TokenPair>), ApiError> {
    use crate::auth::*;
    let pool = pool(&state)?;
    let credential = refresh_credential(&headers, &input.refresh_token)?;
    let csrf_hash = credential
        .csrf_token
        .as_ref()
        .map(|token| hash_token(token));
    let rotated = rotate_session_with_csrf(
        pool,
        &hash_token(&credential.refresh_token),
        csrf_hash.as_deref(),
    )
    .await
    .map_err(|_| ApiError::unauthorized())?;
    let mut pair = issue_access_with_secret_version(
        rotated.user_id,
        &rotated.role,
        rotated.session_id,
        rotated.token_version,
        auth_secret(&state)?,
    )
    .map_err(|_| ApiError::unauthorized())?;
    pair.refresh_token = rotated.refresh_token.clone();
    Ok((
        auth_cookie_headers(&state, &rotated.refresh_token, &rotated.csrf_token),
        Json(pair),
    ))
}

#[utoipa::path(post, path="/api/v1/auth/logout", tag="auth", request_body=crate::auth::LogoutRequest, responses((status=200, body=crate::auth::LogoutResponse)))]
pub(crate) async fn auth_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<crate::auth::LogoutRequest>,
) -> Result<(HeaderMap, Json<crate::auth::LogoutResponse>), ApiError> {
    use crate::auth::*;
    let Some(credential) = logout_credential(&headers, &input.refresh_token)? else {
        return Ok((
            clear_auth_cookie_headers(&state),
            Json(LogoutResponse { revoked: false }),
        ));
    };
    let pool = pool(&state)?;
    let user_id = revoke_session(pool, &hash_token(&credential.refresh_token))
        .await
        .map_err(ApiError::from)?;
    if let Some(user_id) = user_id {
        let _ = audit(pool, "auth.logout", "session", user_id, None).await;
    }
    Ok((
        clear_auth_cookie_headers(&state),
        Json(LogoutResponse {
            revoked: user_id.is_some(),
        }),
    ))
}

struct RefreshCredential {
    refresh_token: String,
    csrf_token: Option<String>,
}

fn refresh_credential(
    headers: &HeaderMap,
    body_refresh_token: &str,
) -> Result<RefreshCredential, ApiError> {
    let trimmed = body_refresh_token.trim();
    if !trimmed.is_empty() {
        return Ok(RefreshCredential {
            refresh_token: trimmed.to_string(),
            csrf_token: None,
        });
    }
    cookie_refresh_credential(headers)?.ok_or_else(ApiError::unauthorized)
}

fn logout_credential(
    headers: &HeaderMap,
    body_refresh_token: &str,
) -> Result<Option<RefreshCredential>, ApiError> {
    let trimmed = body_refresh_token.trim();
    if !trimmed.is_empty() {
        return Ok(Some(RefreshCredential {
            refresh_token: trimmed.to_string(),
            csrf_token: None,
        }));
    }
    cookie_refresh_credential(headers)
}

fn cookie_refresh_credential(headers: &HeaderMap) -> Result<Option<RefreshCredential>, ApiError> {
    let Some(refresh_token) = cookie_value(headers, AUTH_REFRESH_COOKIE) else {
        return Ok(None);
    };
    let csrf_cookie = cookie_value(headers, AUTH_CSRF_COOKIE).ok_or_else(ApiError::unauthorized)?;
    let csrf_header = headers
        .get(AUTH_CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(ApiError::unauthorized)?;
    if !constant_time_eq(csrf_cookie.as_bytes(), csrf_header.as_bytes()) {
        return Err(ApiError::unauthorized());
    }
    Ok(Some(RefreshCredential {
        refresh_token,
        csrf_token: Some(csrf_cookie),
    }))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(candidate, value)| {
            if candidate == name && !value.is_empty() {
                Some(value.to_string())
            } else {
                None
            }
        })
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    left.ct_eq(right).into()
}

fn auth_cookie_headers(state: &AppState, refresh_token: &str, csrf_token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format_auth_cookie(
            AUTH_REFRESH_COOKIE,
            refresh_token,
            "/api/v1/auth",
            true,
            crate::auth::REFRESH_TTL_DAYS * 24 * 60 * 60,
            state.config.http.auth_cookie_secure,
        ))
        .expect("refresh cookie contains only safe ASCII"),
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format_auth_cookie(
            AUTH_CSRF_COOKIE,
            csrf_token,
            "/",
            false,
            crate::auth::REFRESH_TTL_DAYS * 24 * 60 * 60,
            state.config.http.auth_cookie_secure,
        ))
        .expect("csrf cookie contains only safe ASCII"),
    );
    headers
}

fn clear_auth_cookie_headers(state: &AppState) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format_auth_cookie(
            AUTH_REFRESH_COOKIE,
            "",
            "/api/v1/auth",
            true,
            0,
            state.config.http.auth_cookie_secure,
        ))
        .expect("refresh cookie contains only safe ASCII"),
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format_auth_cookie(
            AUTH_CSRF_COOKIE,
            "",
            "/",
            false,
            0,
            state.config.http.auth_cookie_secure,
        ))
        .expect("csrf cookie contains only safe ASCII"),
    );
    headers
}

fn format_auth_cookie(
    name: &str,
    value: &str,
    path: &str,
    http_only: bool,
    max_age_seconds: i64,
    secure: bool,
) -> String {
    let http_only = if http_only { "; HttpOnly" } else { "" };
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{name}={value}; Path={path}; Max-Age={max_age_seconds}; SameSite=Lax{http_only}{secure}"
    )
}
