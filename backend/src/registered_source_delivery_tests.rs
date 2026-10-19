//! Synthetic rows and real reader middleware; no live credentials or PostgreSQL claim.
use super::*;
use serde_json::json;

fn query() -> RepositoryQuery {
    serde_json::from_value(json!({"registry_instance_id":Uuid::from_u128(1),
        "namespace_id":Uuid::from_u128(2),"base_ref":"refs/heads/main",
        "base_commit":"a".repeat(40)}))
    .unwrap()
}

#[test]
fn delivery_configuration_selector_is_bounded_exact_and_source_only() {
    let mut value = query();
    for invalid in [
        "".into(),
        " pipeline".into(),
        "pipeline ".into(),
        "x\ny".into(),
        "x".repeat(257),
        "я".repeat(129),
    ] {
        value.delivery_configuration_name = Some(invalid);
        assert!(value.source().is_err());
    }
    value.delivery_configuration_name = Some("pipeline".into());
    assert!(value.source().is_ok());
    value.base_commit.clear();
    assert!(value.source().is_err());
    value.base_ref.clear();
    assert!(value.source().is_err());
    value.delivery_configuration_name = None;
    assert!(value.source().is_err());
    value.base_ref = "refs/heads/main".into();
    value.base_commit = "a".repeat(40);
    assert!(value.source().is_ok());
}

#[test]
fn delivery_configuration_projection_requires_one_exact_same_repo_and_four_fields() {
    let repository = Uuid::from_u128(3);
    let row = (Uuid::from_u128(4), "pipeline".to_owned(), repository);
    let selected =
        exact_delivery_configuration(vec![row.clone()], repository, "fixture/repo", "pipeline")
            .unwrap();
    assert_eq!(
        serde_json::to_value(selected).unwrap(),
        json!({
        "id":row.0,"name":"pipeline","repository_id":repository,"repository_name":"fixture/repo"})
    );
    let missing = exact_delivery_configuration(vec![], repository, "fixture/repo", "pipeline")
        .err()
        .unwrap();
    assert_eq!(missing.status, axum::http::StatusCode::NOT_FOUND);
    let duplicate = exact_delivery_configuration(
        vec![row.clone(), row.clone()],
        repository,
        "fixture/repo",
        "pipeline",
    )
    .err()
    .unwrap();
    assert_eq!(duplicate.status, axum::http::StatusCode::CONFLICT);
    for wrong in [
        (Uuid::nil(), row.1.clone(), repository),
        (row.0, "other".to_owned(), repository),
        (row.0, row.1.clone(), Uuid::from_u128(5)),
    ] {
        assert_eq!(
            exact_delivery_configuration(vec![wrong], repository, "fixture/repo", "pipeline")
                .err()
                .unwrap()
                .status,
            axum::http::StatusCode::FORBIDDEN
        );
    }
    let mut injected = json!({"id":row.0,"name":"pipeline","repository_id":repository,"repository_name":"fixture/repo"});
    injected["repository_url"] = json!("must-not-be-exposed");
    assert!(serde_json::from_value::<DeliveryConfigurationReadback>(injected).is_err());
}

#[test]
fn default_source_response_is_byte_identical_and_only_optin_adds_four_fields() {
    // The nine original fields, in their original serialization order.
    #[derive(Serialize)]
    struct LegacySource<'a> {
        namespace: &'a NamespaceRef,
        forge_instance_id: Uuid,
        repository_id: Uuid,
        public_name: &'a str,
        base_ref: &'a str,
        base_commit: &'a str,
        default_ref: &'a str,
        protected_refs: &'a [String],
        observed_at: DateTime<Utc>,
    }
    let mut source = VerifiedSource {
        namespace: query().namespace(),
        forge_instance_id: Uuid::from_u128(6),
        repository_id: Uuid::from_u128(3),
        public_name: "fixture/repo".into(),
        base_ref: "refs/heads/main".into(),
        base_commit: "a".repeat(40),
        default_ref: "refs/heads/main".into(),
        protected_refs: vec!["refs/heads/main".into()],
        observed_at: DateTime::parse_from_rfc3339("2026-10-17T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        delivery_configuration: None,
    };
    let old = LegacySource {
        namespace: &source.namespace,
        forge_instance_id: source.forge_instance_id,
        repository_id: source.repository_id,
        public_name: &source.public_name,
        base_ref: &source.base_ref,
        base_commit: &source.base_commit,
        default_ref: &source.default_ref,
        protected_refs: &source.protected_refs,
        observed_at: source.observed_at,
    };
    let bytes = serde_json::to_vec(&old).unwrap();
    assert_eq!(serde_json::to_vec(&source).unwrap(), bytes);
    assert!(
        serde_json::from_slice::<VerifiedSource>(&bytes)
            .unwrap()
            .delivery_configuration
            .is_none()
    );
    source.delivery_configuration = Some(
        exact_delivery_configuration(
            vec![(Uuid::from_u128(4), "pipeline".into(), source.repository_id)],
            source.repository_id,
            &source.public_name,
            "pipeline",
        )
        .unwrap(),
    );
    let value = serde_json::to_value(source).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 10);
    assert_eq!(
        value["delivery_configuration"].as_object().unwrap().len(),
        4
    );
}

#[tokio::test]
async fn namespace_delivery_reader_middleware_preserves_machine_and_human_boundaries() {
    use axum::{
        Json, Router,
        body::Body,
        extract::Query,
        http::{HeaderMap, Request, StatusCode},
        routing::get,
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::sync::Arc;
    use tower::ServiceExt;

    const CHILD: &str = "FORGE_NAMESPACE_DELIVERY_READER_TEST_PROCESS";
    const READER: &str = "00000000-0000-0000-0000-000000000007";
    const FOREIGN: &str = "00000000-0000-0000-0000-000000000008";
    // Synthetic test-only ES256 material, never a deployed signing key.
    const TEST_KEY: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgKX9P+FoJSDeTucv5++s77pW0E1Cxb7psEcXcHctu0/uhRANCAARdYQguyhhd0okqeh45rjc8IjgA+QBKkmGgTaWnnGyrq0maxz4o+qemr3hNKATX3eFZYN3kMhUglf9o6EVM0S8Z";
    let path = "/api/v1/namespace-repositories/00000000-0000-0000-0000-000000000003/source";
    let query = "?registry_instance_id=00000000-0000-0000-0000-000000000001&namespace_id=00000000-0000-0000-0000-000000000002&base_ref=refs%2Fheads%2Fmain&base_commit=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&delivery_configuration_name=";
    if std::env::var_os(CHILD).is_some() {
        async fn fixture(
            Query(query): Query<RepositoryQuery>,
        ) -> Result<Json<DeliveryConfigurationReadback>, ApiError> {
            query.source()?;
            let name = query
                .delivery_configuration_name
                .as_deref()
                .ok_or_else(|| ApiError::bad_request("name_required"))?;
            let repository = Uuid::from_u128(3);
            let row = (Uuid::from_u128(4), name.to_owned(), repository);
            let rows = match name {
                "missing" => vec![],
                "duplicate" => vec![row.clone(), row],
                "foreign-repo" => vec![(row.0, row.1, Uuid::from_u128(9))],
                _ => vec![row],
            };
            exact_delivery_configuration(rows, repository, "fixture/repo", name).map(Json)
        }
        let app = Router::new()
            .route("/api/v1/namespace-repositories/{id}/source", get(fixture))
            .route_layer(axum::middleware::from_fn(crate::namespace::reader_auth));
        for (token, name, expected) in [
            ("sdlc_pat_reader", "pipeline", StatusCode::OK),
            ("sdlc_pat_reader", "missing", StatusCode::NOT_FOUND),
            ("sdlc_pat_reader", "duplicate", StatusCode::CONFLICT),
            ("sdlc_pat_reader", "foreign-repo", StatusCode::FORBIDDEN),
            ("sdlc_pat_foreign", "pipeline", StatusCode::FORBIDDEN),
            ("sdlc_pat_write_only", "pipeline", StatusCode::FORBIDDEN),
            ("sdlc_pat_task_child", "pipeline", StatusCode::FORBIDDEN),
            ("sdlc_pat_revoked", "pipeline", StatusCode::UNAUTHORIZED),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("{path}{query}{name}"))
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{token}/{name}");
            if expected == StatusCode::OK {
                let body = axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&body)
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .len(),
                    4
                );
            }
        }
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some("namespace-delivery-fixture".into());
        let human = jsonwebtoken::encode(
            &header,
            &pdlc_auth_core::Claims {
                sub: READER.into(),
                aud: "sdlc".into(),
                iss: "http://namespace-fixture.test".into(),
                exp: Utc::now().timestamp() + 300,
                iat: Utc::now().timestamp(),
                role: Some("admin".into()),
                scopes: vec!["ci-cd:read".into()],
                sid: Some("human-session".into()),
                email: Some("fixture@example.invalid".into()),
            },
            &jsonwebtoken::EncodingKey::from_ec_der(&STANDARD.decode(TEST_KEY).unwrap()),
        )
        .unwrap();
        let verified_human = crate::central_auth::try_central(&human)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(verified_human.role.as_deref(), Some("admin"));
        assert!(verified_human.session_id.is_some());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{path}{query}pipeline"))
                    .header("authorization", format!("Bearer {human}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        for bearer in [None, Some("Bearer forge_sat_fixture_not_a_central_reader")] {
            let mut request = Request::builder().uri(format!("{path}{query}pipeline"));
            if let Some(value) = bearer {
                request = request.header("authorization", value);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        // The actual route reaches its fail-closed missing-DB boundary only after reader auth.
        let config = crate::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
        let actual = crate::namespace::routes()
            .merge(crate::registered_devops_evidence::routes())
            .with_state(Arc::new(AppState {
                pool: None,
                namespace_admission_pool: None,
                auth_secret: None,
                git: config.git.to_git_config(),
                config,
                running_jobs: None,
                rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()),
            }));
        for (selector, expected) in [
            ("pipeline", StatusCode::SERVICE_UNAVAILABLE),
            ("", StatusCode::BAD_REQUEST),
            (
                "pipeline&delivery_configuration_name=other",
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let response = actual
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("{path}{query}{selector}"))
                        .header("authorization", "Bearer sdlc_pat_reader")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{selector}");
        }
        let evidence_uri = format!(
            "/api/v1/namespace-repositories/00000000-0000-0000-0000-000000000003/devops-evidence{query}pipeline\
             &project_id=00000000-0000-0000-0000-000000000004\
             &source_pipeline_id=00000000-0000-0000-0000-000000000010\
             &operation_key=developer:original&source_commit={}\
             &request_hash={}&task_id=00000000-0000-0000-0000-000000000011\
             &root_task_id=00000000-0000-0000-0000-000000000011\
             &assignment_id=00000000-0000-0000-0000-000000000012\
             &execution_id=00000000-0000-0000-0000-000000000013&fencing_token=1",
            "b".repeat(40),
            "c".repeat(64),
        );
        for (token, expected) in [
            ("sdlc_pat_reader", StatusCode::SERVICE_UNAVAILABLE),
            ("sdlc_pat_foreign", StatusCode::FORBIDDEN),
            ("sdlc_pat_write_only", StatusCode::FORBIDDEN),
            ("sdlc_pat_task_child", StatusCode::FORBIDDEN),
            ("sdlc_pat_revoked", StatusCode::UNAUTHORIZED),
            (human.as_str(), StatusCode::FORBIDDEN),
            (
                "forge_sat_fixture_not_a_central_reader",
                StatusCode::UNAUTHORIZED,
            ),
        ] {
            let response = actual
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(&evidence_uri)
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                expected,
                "namespace DevOps reader boundary"
            );
        }
        return;
    }
    let auth = Router::new()
        .route("/oidc/jwks", get(|| async { Json(json!({"keys":[{
            "kty":"EC","crv":"P-256","use":"sig","alg":"ES256","kid":"namespace-delivery-fixture",
            "x":"XWEILsoYXdKJKnoeOa43PCI4APkASpJhoE2lp5xsq6s","y":"SZrHPij6p6aveE0oBNfd4Vlg3eQyFSCV_2joRUzRLxk"}]})) }))
        .route("/auth/me", get(|| async { Json(json!({"id":READER,"display_name":"Fixture human"})) }))
        .route("/auth/tokens/introspect", get(|headers: HeaderMap| async move {
            let token = headers.get("authorization").unwrap().to_str().unwrap();
            let mut value = json!({"sub":READER,"email":"fixture@example.invalid","scopes":["ci-cd:read"],"display_name":"Fixture"});
            match token {
                "Bearer sdlc_pat_foreign" => value["sub"] = json!(FOREIGN),
                "Bearer sdlc_pat_write_only" => value["scopes"] = json!(["ci-cd:write"]),
                "Bearer sdlc_pat_task_child" => value["scopes"] = json!(["ci-cd:read",format!("ci-cd:sdlc:task:v1:{}", "a".repeat(64))]),
                "Bearer sdlc_pat_revoked" => return (StatusCode::UNAUTHORIZED, Json(json!({}))),
                "Bearer sdlc_pat_reader" => (),
                _ => panic!("unexpected synthetic token"),
            }
            (StatusCode::OK, Json(value))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}/oidc/jwks", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, auth).await.unwrap() });
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "registered_source::delivery_configuration_tests::namespace_delivery_reader_middleware_preserves_machine_and_human_boundaries", "--nocapture"])
        .env(CHILD, "1").env("CICD_AUTH__CENTRAL_JWKS_URI", uri)
        .env("CICD_AUTH__CENTRAL_ISSUER", "http://namespace-fixture.test")
        .env("CICD_AUTH__CENTRAL_TIMEOUT_SECS", "5").env("CICD_NAMESPACE__READER_SUBJECTS", READER)
        .env("NO_PROXY", "127.0.0.1,localhost").env("no_proxy", "127.0.0.1,localhost").kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(40), command.output()).await;
    server.abort();
    let output = output
        .expect("isolated namespace reader test timed out")
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored;"));
}
