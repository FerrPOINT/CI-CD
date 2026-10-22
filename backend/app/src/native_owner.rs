//! Current owner gate for already confirmed task stages; no machine grant is issued here.
use pdlc_auth_core::application_access_client::{ApplicationAccessClient, ApplicationAccessError};
use std::time::Instant;
use uuid::Uuid;
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum NativeOwnerError {
    #[error("Current task owner admission denied")]
    Forbidden,
    #[error("Current task owner admission unavailable")]
    Unavailable,
}
/// The caller first validates the exact task/assignment/binding/lease readback.
/// This gate intersects its fixed lifetime with current human applications.
pub async fn admit(
    client: &ApplicationAccessClient,
    subject: Uuid,
    native_until: Instant,
) -> Result<Instant, NativeOwnerError> {
    if subject.is_nil() || Instant::now() >= native_until {
        return Err(NativeOwnerError::Forbidden);
    }
    let mut decisions = client
        .resolve(&[subject])
        .await
        .map_err(|error| match error {
            ApplicationAccessError::Forbidden => NativeOwnerError::Forbidden,
            _ => NativeOwnerError::Unavailable,
        })?;
    let owner = decisions
        .remove(&subject)
        .ok_or(NativeOwnerError::Forbidden)?;
    if !owner.decision.enforced
        || !owner.permits(&["ci-cd", "task-tracker", "fleet-control"], false)
    {
        return Err(NativeOwnerError::Forbidden);
    }
    let until = native_until.min(owner.monotonic_deadline());
    if Instant::now() >= until {
        return Err(NativeOwnerError::Forbidden);
    }
    Ok(until)
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{get, post},
    };
    use std::{
        sync::{
            Arc, RwLock,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use uuid::Uuid;
    #[derive(Clone)]
    struct Owner {
        subject: Uuid,
        issuer: String,
        apps: Vec<&'static str>,
        role: &'static str,
        expiry_ms: i64,
        delay_ms: u64,
        unavailable: bool,
        calls: Arc<AtomicUsize>,
    }
    async fn decision(
        State(state): State<Arc<RwLock<Owner>>>,
        headers: HeaderMap,
        Json(request): Json<serde_json::Value>,
    ) -> axum::response::Response {
        assert_eq!(
            headers["authorization"],
            "Bearer fixture-restricted-forge-owner"
        );
        let owner = state.read().unwrap().clone();
        assert_eq!(
            request,
            serde_json::json!({"schema_version":1,"subjects":[owner.subject]})
        );
        owner.calls.fetch_add(1, Ordering::SeqCst);
        if owner.unavailable {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let now = chrono::Utc::now();
        tokio::time::sleep(Duration::from_millis(owner.delay_ms)).await;
        Json(serde_json::json!({"schema_version":1,"decisions":[{"schema_version":1,"issuer":owner.issuer,"subject":owner.subject,"role":owner.role,"applications":owner.apps,"revision":1,"enforced":true,"observed_at":now,"expires_at":now+chrono::Duration::milliseconds(owner.expiry_ms)}]})).into_response()
    }
    #[tokio::test]
    async fn confirmed_task_owner_limits_native_stages_and_each_required_application() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let owner = Uuid::new_v4();
        let calls = Arc::new(AtomicUsize::new(0));
        let state = Arc::new(RwLock::new(Owner {
            subject: owner,
            issuer: issuer.clone(),
            apps: vec!["ci-cd", "task-tracker", "fleet-control"],
            role: "user",
            expiry_ms: 30000,
            delay_ms: 0,
            unavailable: false,
            calls: calls.clone(),
        }));
        let router = Router::new()
            .route(
                "/oidc/jwks",
                get(|| async { Json(serde_json::json!({"keys":[]})) }),
            )
            .route("/auth/access/decisions", post(decision))
            .with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = pdlc_auth_core::application_access_client::ApplicationAccessClient::new(
            &issuer,
            "fixture-restricted-forge-owner".into(),
            Duration::from_secs(2),
        )
        .unwrap();
        async fn admit_http(
            client: &pdlc_auth_core::application_access_client::ApplicationAccessClient,
            owner: Uuid,
            until: Instant,
        ) -> Result<Instant, NativeOwnerError> {
            super::admit(client, owner, until).await
        }
        let native = Instant::now() + Duration::from_secs(2);
        assert_eq!(
            admit_http(&client, owner, native).await.unwrap(),
            native,
            "owner read cannot renew the native lease"
        );
        for removed in ["ci-cd", "task-tracker", "fleet-control"] {
            state.write().unwrap().apps = vec!["ci-cd", "task-tracker", "fleet-control"]
                .into_iter()
                .filter(|app| *app != removed)
                .collect();
            assert_eq!(
                admit_http(&client, owner, Instant::now() + Duration::from_secs(30))
                    .await
                    .unwrap_err(),
                NativeOwnerError::Forbidden,
                "missing {removed}"
            );
        }
        state.write().unwrap().role = "admin";
        state.write().unwrap().apps.clear();
        assert!(
            admit_http(&client, owner, Instant::now() + Duration::from_secs(30))
                .await
                .is_ok()
        );
        state.write().unwrap().role = "user";
        state.write().unwrap().apps = vec!["ci-cd", "task-tracker", "fleet-control"];
        state.write().unwrap().expiry_ms = 140;
        let started = Instant::now();
        let until = admit_http(&client, owner, started + Duration::from_secs(30))
            .await
            .unwrap();
        assert!(until <= started + Duration::from_millis(140));
        state.write().unwrap().expiry_ms = 30000;
        state.write().unwrap().delay_ms = 100;
        assert_eq!(
            admit_http(&client, owner, Instant::now() + Duration::from_millis(40))
                .await
                .unwrap_err(),
            NativeOwnerError::Forbidden,
            "Auth read cannot renew an elapsed lease"
        );
        state.write().unwrap().delay_ms = 0;
        state.write().unwrap().unavailable = true;
        assert_eq!(
            admit_http(&client, owner, Instant::now() + Duration::from_secs(30))
                .await
                .unwrap_err(),
            NativeOwnerError::Unavailable
        );
        let before = calls.load(Ordering::SeqCst);
        assert_eq!(
            admit_http(
                &client,
                Uuid::nil(),
                Instant::now() + Duration::from_secs(30)
            )
            .await
            .unwrap_err(),
            NativeOwnerError::Forbidden
        );
        assert_eq!(
            admit_http(&client, owner, Instant::now())
                .await
                .unwrap_err(),
            NativeOwnerError::Forbidden
        );
        assert_eq!(calls.load(Ordering::SeqCst), before);
        assert_eq!(pdlc_auth_core::human_request::current_role(), Ok(None));
        server.abort();
    }
}
