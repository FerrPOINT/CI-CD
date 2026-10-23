//! Actual Forge router + PostgreSQL. Loopback Auth qualifies the private wire
//! boundary only; live Auth coordinator qualification is a separate gate.
#![cfg(feature = "integration")]
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::get,
};
use pdlc_shared::{
    identity_lifecycle::{DeletionRequest, OwnerFreezeReceipt},
    space_access::CentralIdentity,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

async fn call(
    app: &Router,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers.get("cache-control").unwrap(), "no-store");
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn coordinator_only_private_transport_and_no_store_rejections() {
    let url = std::env::var("CICD_TEST_DATABASE_URL").expect("dedicated URL");
    let admin = PgPool::connect(&url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(database, "forge_test_cicd", "refuse working DB");
    let schema = format!("identity_transport_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let configured_schema = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let schema = configured_schema.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {schema}"))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"),
    )
    .await
    .unwrap()
    .run(&pool)
    .await
    .unwrap();
    let coordinator = Uuid::new_v4();
    let stranger = Uuid::new_v4();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let central=Router::new().route("/oidc/jwks",get(||async{Json(json!({"keys":[]}))}))
        .route("/auth/tokens/introspect",get(move|headers:HeaderMap|async move {
            let token=headers.get("authorization").and_then(|v|v.to_str().ok()).unwrap_or("");
            let (subject,role,scopes,expired)=match token {
                "Bearer sdlc_pat_owner_slow"=>{tokio::time::sleep(std::time::Duration::from_secs(12)).await;(coordinator,"service_account",vec!["ci-cd:identity-lifecycle"],false)},
                "Bearer sdlc_pat_owner_valid"=>(coordinator,"service_account",vec!["ci-cd:identity-lifecycle"],false),
                "Bearer sdlc_pat_owner_human"=>(coordinator,"admin",vec!["ci-cd:identity-lifecycle"],false),
                "Bearer sdlc_pat_owner_unregistered"=>(stranger,"service_account",vec!["ci-cd:identity-lifecycle"],false),
                "Bearer sdlc_pat_owner_scope"=>(coordinator,"service_account",vec!["ci-cd:write"],false),
                "Bearer sdlc_pat_owner_expired"=>(coordinator,"service_account",vec!["ci-cd:identity-lifecycle"],true),
                _=>return (StatusCode::UNAUTHORIZED,Json(json!({}))),
            };
            let now=chrono::Utc::now();
            (StatusCode::OK,Json(json!({"sub":subject,"email":"fixture@example.test","subject_role":role,
                "scopes":scopes,"observed_at":now,"expires_at":now+chrono::Duration::seconds(if expired{-1}else{50})})))
        }));
    let server = tokio::spawn(async move { axum::serve(listener, central).await.unwrap() });
    unsafe {
        std::env::set_var("CICD_AUTH__CENTRAL_JWKS_URI", format!("{base}/oidc/jwks"));
        std::env::set_var("CICD_AUTH__CENTRAL_ISSUER", &base);
        std::env::set_var(
            "CICD_IDENTITY_DELETION_COORDINATOR_SUBJECTS",
            coordinator.to_string(),
        );
    }
    cicd::store::identity_deletion::IdentityDeletionStore::new(pool.clone())
        .bind_legacy_issuer(&base)
        .await
        .unwrap();
    let app = cicd::api::app_with_auth_secret(
        Some(pool.clone()),
        Some("fixture-owner-hmac-secret".into()),
    );
    let request = DeletionRequest {
        schema_version: 1,
        operation_id: Uuid::new_v4(),
        inventory_revision: Uuid::new_v4(),
        identity: CentralIdentity {
            issuer: base,
            subject: Uuid::new_v4().to_string(),
        },
    };
    let jwt=jsonwebtoken::encode(&jsonwebtoken::Header::default(),&json!({"iss":request.identity.issuer,"aud":"sdlc","sub":coordinator,"role":"service_account","scope":["ci-cd:identity-lifecycle"],"iat":chrono::Utc::now().timestamp(),"exp":chrono::Utc::now().timestamp()+60}),&jsonwebtoken::EncodingKey::from_secret(b"fixture-unaccepted-jwt-key")).unwrap();
    let body = serde_json::to_value(&request).unwrap();
    let path = format!(
        "/api/internal/v1/user-deletions/{}/check",
        request.operation_id
    );
    let checks = async move {
        assert_eq!(
            call(&app, &path, Some("sdlc_pat_owner_slow"), body.clone())
                .await
                .0,
            StatusCode::SERVICE_UNAVAILABLE,
            "overall source wait is bounded before any owner effect"
        );

        assert_eq!(
            call(&app, &path, Some(&jwt), body.clone()).await.0,
            StatusCode::FORBIDDEN,
            "service-account JWT cannot substitute coordinator PAT"
        );

        assert_eq!(
            call(&app, &path, None, body.clone()).await.0,
            StatusCode::UNAUTHORIZED
        );
        for token in [
            "sdlc_pat_owner_human",
            "sdlc_pat_owner_unregistered",
            "sdlc_pat_owner_scope",
        ] {
            assert_eq!(
                call(&app, &path, Some(token), body.clone()).await.0,
                StatusCode::FORBIDDEN,
                "{token}"
            );
        }
        assert_eq!(
            call(&app, &path, Some("sdlc_pat_owner_expired"), body.clone())
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        let mut foreign = body.clone();
        foreign["identity"]["issuer"] = "https://other.example.test".into();
        assert_eq!(
            call(&app, &path, Some("sdlc_pat_owner_valid"), foreign)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let mut wrong_operation = body.clone();
        wrong_operation["operation_id"] = Uuid::new_v4().to_string().into();
        assert_eq!(
            call(&app, &path, Some("sdlc_pat_owner_valid"), wrong_operation)
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            call(&app, &path, Some("sdlc_pat_owner_valid"), json!({}))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            call(
                &app,
                "/api/internal/v1/user-deletions/not-a-uuid/check",
                None,
                body.clone()
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        let mut oversized = body.clone();
        oversized["padding"] = "a".repeat(20_000).into();
        assert_eq!(
            call(&app, &path, None, oversized).await.0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            call(&app, &path, Some("sdlc_pat_owner_valid"), body.clone())
                .await
                .0,
            StatusCode::OK
        );
        let freeze_path = path.replace("/check", "/freeze");
        let response = call(
            &app,
            &freeze_path,
            Some("sdlc_pat_owner_valid"),
            body.clone(),
        )
        .await;
        assert_eq!(response.0, StatusCode::OK);
        let receipt: OwnerFreezeReceipt = serde_json::from_value(response.2.clone()).unwrap();
        assert_eq!(receipt.request, request);
        assert_eq!(receipt.reference_count, 0);
        assert_eq!(
            call(
                &app,
                &freeze_path,
                Some("sdlc_pat_owner_valid"),
                body.clone()
            )
            .await
            .2,
            response.2
        );
        let release = path.replace("/check", "/release");
        assert_eq!(
            call(&app, &release, Some("sdlc_pat_owner_valid"), body.clone())
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                &freeze_path,
                Some("sdlc_pat_owner_valid"),
                body.clone()
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    };
    let result = tokio::spawn(checks).await;
    server.abort();
    let _ = server.await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}
