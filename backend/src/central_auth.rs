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
pub static BRIDGE: ServiceBridge = ServiceBridge::for_application("CICD_AUTH__CENTRAL", "ci-cd");

/// Central-first bearer check. `None` means the token is explicitly outside
/// the central namespace; configured consumers still reject legacy human JWTs.
pub async fn try_central(token: &str) -> Result<Option<AuthContext>, crate::api::ApiError> {
    Ok(try_central_with_policy(token)
        .await?
        .map(|(context, _)| context))
}

pub async fn try_central_with_policy(
    token: &str,
) -> Result<
    Option<(
        AuthContext,
        Option<pdlc_auth_core::application_access::VerifiedApplicationAccess>,
    )>,
    crate::api::ApiError,
> {
    let (outcome, _, proof) = BRIDGE.try_token_with_policy(token).await;
    match outcome {
        BridgeOutcome::Validated(ctx) => Ok(Some((ctx, proof))),
        BridgeOutcome::Forbidden => Err(crate::api::ApiError::forbidden()),
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

/// Token kind remains explicit even when synthetic local claims have no local token id.
pub(crate) fn personal_scope_allows(token: &str, scopes: &[String], method: &str) -> bool {
    !pdlc_auth_core::is_personal_token(token)
        || scopes.iter().any(|scope| {
            scope
                == if matches!(method, "GET" | "HEAD" | "OPTIONS") {
                    "ci-cd:read"
                } else {
                    "ci-cd:write"
                }
        })
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
    url.set_query(Some("include_lifetime=true"));
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
        role: if ctx.is_admin() { "admin" } else { "user" }.to_string(),
        ver: 0,
        iat: now.timestamp(),
        exp: now.timestamp() + 60,
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
        use axum::{Json, Router, http::HeaderMap, http::Uri, routing::get};
        use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering};

        const CHILD_PROCESS: &str = "FORGE_CHILD_INTROSPECTION_TEST_PROCESS";
        const SUBJECT: &str = "00000000-0000-0000-0000-000000000001";
        if std::env::var_os(CHILD_PROCESS).is_some() {
            let token = "sdlc_pat_fixture_valid";
            let legacy = try_central(token).await.unwrap().unwrap();
            assert_eq!(legacy.user_id, SUBJECT);
            assert_eq!(legacy.email.as_deref(), Some("fixture@example.invalid"));
            assert_eq!(legacy.scopes.len(), 1);
            assert!(legacy.scopes.contains("ci-cd:read"));
            let (identity, until) = try_central_child(token).await.unwrap().unwrap();
            assert_eq!(identity.user_id, legacy.user_id);
            assert_eq!(identity.email, legacy.email);
            assert_eq!(identity.scopes, legacy.scopes);
            assert_eq!(identity.token, token);
            assert!(until > Instant::now());
            for case in [
                "missing",
                "no_expiry",
                "no_observed",
                "expired",
                "null",
                "malformed",
            ] {
                assert!(
                    try_central_child(&format!("sdlc_pat_fixture_{case}"))
                        .await
                        .is_err(),
                    "{case}"
                );
            }
            return;
        }

        let legacy_calls = Arc::new(AtomicUsize::new(0));
        let child_calls = Arc::new(AtomicUsize::new(0));
        let legacy_count = legacy_calls.clone();
        let child_count = child_calls.clone();
        let router = Router::new()
            .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
            .route(
                "/auth/tokens/introspect",
                get(move |uri: Uri, headers: HeaderMap| {
                    let legacy_count = legacy_count.clone();
                    let child_count = child_count.clone();
                    async move {
                        let mut wire = json!({"sub":SUBJECT,"email":"fixture@example.invalid",
                            "scopes":["ci-cd:read"],"display_name":"Fixture"});
                        if uri.query().is_none() {
                            legacy_count.fetch_add(1, Ordering::SeqCst);
                            return Json(wire);
                        }
                        assert_eq!(uri.query(), Some("include_lifetime=true"));
                        child_count.fetch_add(1, Ordering::SeqCst);
                        let observed = chrono::Utc::now();
                        wire["observed_at"] = json!(observed);
                        wire["expires_at"] = json!(observed + chrono::Duration::seconds(30));
                        match headers.get("authorization").unwrap().to_str().unwrap() {
                            "Bearer sdlc_pat_fixture_missing" => {
                                wire.as_object_mut().unwrap().remove("expires_at");
                                wire.as_object_mut().unwrap().remove("observed_at");
                            }
                            "Bearer sdlc_pat_fixture_no_expiry" => {
                                wire.as_object_mut().unwrap().remove("expires_at");
                            }
                            "Bearer sdlc_pat_fixture_no_observed" => {
                                wire.as_object_mut().unwrap().remove("observed_at");
                            }
                            "Bearer sdlc_pat_fixture_expired" => {
                                wire["expires_at"] = json!(observed)
                            }
                            "Bearer sdlc_pat_fixture_null" => {
                                wire["observed_at"] = serde_json::Value::Null
                            }
                            "Bearer sdlc_pat_fixture_malformed" => {
                                wire["expires_at"] = json!("not-a-clock")
                            }
                            "Bearer sdlc_pat_fixture_valid" => (),
                            _ => panic!("unexpected fixture bearer"),
                        }
                        Json(wire)
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!(
            "http://{}/oidc/jwks?discard=this",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        // Isolate the SDK's env-configured OnceCell without mutating the test process env.
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "central_auth::child_expiry_tests::child_introspection_http_requires_authoritative_expiry_on_same_identity_response", "--nocapture"])
            .env(CHILD_PROCESS, "1")
            .env("CICD_AUTH__CENTRAL_JWKS_URI", uri)
            .env("CICD_AUTH__CENTRAL_TIMEOUT_SECS", "5")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(30), command.output()).await;
        server.abort();
        let output = output
            .expect("isolated introspection test timed out")
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(legacy_calls.load(Ordering::SeqCst), 1);
        assert_eq!(child_calls.load(Ordering::SeqCst), 7);
    }
}

#[cfg(test)]
mod personal_scope_tests {
    use super::*;
    #[test]
    fn synthetic_central_pat_claims_do_not_turn_missing_local_token_id_into_git_write() {
        for token in ["sdlc_pat_synthetic", "pdlc_pat_synthetic"] {
            assert!(personal_scope_allows(token, &["ci-cd:read".into()], "GET"));
            assert!(!personal_scope_allows(
                token,
                &["ci-cd:read".into()],
                "POST"
            ));
            assert!(personal_scope_allows(
                token,
                &["ci-cd:write".into()],
                "POST"
            ));
            assert!(!personal_scope_allows(
                token,
                &["ci-cd:write".into()],
                "GET"
            ));
            assert!(!personal_scope_allows(token, &[], "POST"));
            assert!(!personal_scope_allows(token, &["api:write".into()], "POST"));
        }
        assert!(personal_scope_allows("browser-session", &[], "POST"));
    }
}
