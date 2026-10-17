//! Forge CI/CD wiring of the shared central-auth bridge.
//!
//! JWKS validation and the login proxy live in
//! `pdlc_auth_core::service_bridge`; this file maps the central identity
//! to a local profile keyed by the immutable central subject.

use crate::auth::AccessClaims;
use pdlc_auth_core::AuthContext;
use pdlc_auth_core::service_bridge::{BridgeOutcome, ServiceBridge};
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

/// Same configured PAT endpoint, with authoritative expiry for delegated effects.
/// The pinned SDK intentionally ignores additive introspection metadata.
pub(crate) async fn try_central_child(
    token: &str,
) -> Result<Option<(AuthContext, std::time::Instant)>, crate::api::ApiError> {
    use crate::api::ApiError;
    let unavailable = || ApiError::service_unavailable("Central child expiry unavailable");
    if !pdlc_auth_core::is_personal_token(token) {
        return Err(ApiError::forbidden());
    }
    let uri = std::env::var("CICD_AUTH__CENTRAL_JWKS_URI").map_err(|_| unavailable())?;
    let mut url = reqwest::Url::parse(&uri).map_err(|_| unavailable())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(unavailable());
    }
    url.set_path("/auth/tokens/introspect");
    url.set_query(None);
    url.set_fragment(None);
    let timeout = std::env::var("CICD_AUTH__CENTRAL_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5u64);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| unavailable())?;
    introspect_child(&client, url, token).await
}

#[derive(serde::Deserialize)]
struct ChildIntrospection {
    sub: String,
    email: String,
    scopes: Vec<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
    observed_at: chrono::DateTime<chrono::Utc>,
}

fn child_deadline(
    started: std::time::Instant,
    metadata: &ChildIntrospection,
) -> Result<std::time::Instant, crate::api::ApiError> {
    let remaining = metadata
        .expires_at
        .signed_duration_since(metadata.observed_at);
    if metadata.sub.is_empty()
        || metadata.email.is_empty()
        || remaining <= chrono::Duration::zero()
        || remaining > chrono::Duration::seconds(1800)
    {
        return Err(crate::api::ApiError::forbidden());
    }
    let deadline = started
        .checked_add(
            remaining
                .to_std()
                .map_err(|_| crate::api::ApiError::forbidden())?,
        )
        .ok_or_else(crate::api::ApiError::forbidden)?;
    if std::time::Instant::now() >= deadline {
        return Err(crate::api::ApiError::forbidden());
    }
    Ok(deadline)
}

async fn introspect_child(
    client: &reqwest::Client,
    url: reqwest::Url,
    token: &str,
) -> Result<Option<(AuthContext, std::time::Instant)>, crate::api::ApiError> {
    use crate::api::ApiError;
    let unavailable = || ApiError::service_unavailable("Central child expiry unavailable");
    let started = std::time::Instant::now();
    let mut response = client
        .get(url)
        .bearer_auth(token)
        .header("accept-encoding", "identity")
        .header("cache-control", "no-store")
        .send()
        .await
        .map_err(|_| unavailable())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ApiError::unauthorized());
    }
    if response.status() != reqwest::StatusCode::OK
        || response.content_length().is_some_and(|size| size > 16_384)
        || response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v != "identity")
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len().saturating_add(chunk.len()) > 16_384 {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    let metadata: ChildIntrospection = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    let until = child_deadline(started, &metadata)?;
    Ok(Some((
        AuthContext {
            user_id: metadata.sub,
            email: Some(metadata.email),
            scopes: metadata.scopes.into_iter().collect(),
            role: None,
            session_id: None,
            token: token.into(),
        },
        until,
    )))
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
) -> Option<pdlc_auth_core::service_bridge::CentralTokenPair> {
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

#[cfg(test)]
mod child_expiry_tests {
    use super::*;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn metadata() -> ChildIntrospection {
        let observed = chrono::Utc::now();
        serde_json::from_value(
            json!({"sub":Uuid::new_v4(),"email":"fixture@example.invalid",
            "scopes":["ci-cd:read"],"display_name":"Fixture",
            "observed_at":observed,"expires_at":observed+chrono::Duration::seconds(30)}),
        )
        .unwrap()
    }

    #[test]
    fn child_expiry_wire_is_required_and_bounded_without_using_client_wall_clock() {
        let started = Instant::now();
        let mut value = metadata();
        assert_eq!(
            child_deadline(started, &value).unwrap(),
            started + Duration::from_secs(30)
        );
        value.expires_at = value.observed_at;
        assert!(child_deadline(started, &value).is_err());
        value.expires_at = value.observed_at + chrono::Duration::seconds(1801);
        assert!(child_deadline(started, &value).is_err());
        for missing in ["observed_at", "expires_at"] {
            let mut raw = json!({"sub":Uuid::new_v4(),"email":"fixture@example.invalid",
                "scopes":["ci-cd:read"],"observed_at":value.observed_at,"expires_at":value.expires_at});
            raw.as_object_mut().unwrap().remove(missing);
            assert!(serde_json::from_value::<ChildIntrospection>(raw).is_err());
        }
    }

    #[test]
    fn child_deadline_subtracts_elapsed_transport_and_decode_time() {
        let mut value = metadata();
        value.expires_at = value.observed_at + chrono::Duration::seconds(1);
        assert!(child_deadline(Instant::now() - Duration::from_secs(2), &value).is_err());
    }

    #[tokio::test]
    async fn child_introspection_http_requires_authoritative_expiry_on_same_identity_response() {
        use axum::{Json, Router, routing::get};
        for with_expiry in [true, false] {
            let observed = chrono::Utc::now();
            let subject = Uuid::new_v4().to_string();
            let mut wire = json!({"sub":subject,"email":"fixture@example.invalid",
                "scopes":["ci-cd:read"],"display_name":"Fixture",
                "expires_at":observed+chrono::Duration::seconds(30),"observed_at":observed});
            if !with_expiry {
                wire.as_object_mut().unwrap().remove("expires_at");
                wire.as_object_mut().unwrap().remove("observed_at");
            }
            let router = Router::new().route(
                "/auth/tokens/introspect",
                get(move || {
                    let wire = wire.clone();
                    async move { Json(wire) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!(
                "http://{}/auth/tokens/introspect",
                listener.local_addr().unwrap()
            );
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap();
            let result = introspect_child(&client, url.parse().unwrap(), "fixture-child").await;
            server.abort();
            if with_expiry {
                let (identity, until) = result.unwrap().unwrap();
                assert_eq!(identity.user_id, subject);
                assert_eq!(identity.token, "fixture-child");
                assert!(until > Instant::now());
            } else {
                assert!(result.is_err());
            }
        }
    }
}
