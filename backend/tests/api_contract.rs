use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use cicd::api::app;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_reports_service_ready() {
    let response = app(None)
        .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn readiness_endpoint_requires_database() {
    let response = app(None)
        .oneshot(
            Request::get("/api/v1/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn project_crud_requires_database() {
    // Without a DB pool all project endpoints must return 503, not panic.
    let response = app(None)
        .oneshot(
            Request::get("/api/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn project_update_with_empty_body_is_rejected() {
    let response = app(None)
        .oneshot(
            Request::patch("/api/v1/projects/00000000-0000-0000-0000-000000000000")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    // Body validation fires before the DB pool check: empty patch is a 400.
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pipeline_trigger_rejects_invalid_idempotency_key_before_database() {
    let response = app(None)
        .oneshot(
            Request::post("/api/v1/projects/00000000-0000-0000-0000-000000000000/pipelines")
                .header("content-type", "application/json")
                .header("Idempotency-Key", "not-a-uuid")
                .body(Body::from(r#"{"git_ref":"main"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn artifact_upload_allows_documented_payloads_above_axum_default() {
    let response = app(None)
        .oneshot(
            Request::post("/api/v1/jobs/00000000-0000-0000-0000-000000000000/artifacts")
                .header("content-type", "application/octet-stream")
                .header("x-artifact-name", "large.bin")
                .body(Body::from(vec![b'a'; 2 * 1024 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn runner_artifact_upload_allows_documented_payloads_above_axum_default() {
    let response = app(None)
        .oneshot(
            Request::post("/api/v1/runner/leases/00000000-0000-0000-0000-000000000000/artifacts")
                .header("content-type", "application/octet-stream")
                .header("x-runner-protocol-version", "1")
                .header("x-lease-token", "lease-token")
                .header("x-fencing-token", "1")
                .header("x-attempt-id", "00000000-0000-0000-0000-000000000000")
                .header("x-artifact-path", "target/result.bin")
                .header("x-artifact-name", "result.bin")
                .body(Body::from(vec![b'a'; 2 * 1024 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn git_rpc_allows_payloads_above_axum_default() {
    let response = app(None)
        .oneshot(
            Request::post("/git/demo.git/git-upload-pack")
                .header("content-type", "application/x-git-upload-pack-request")
                .body(Body::from(vec![b'a'; 2 * 1024 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_report_upload_allows_payloads_above_axum_default() {
    let body = format!("\"{}\"", "x".repeat(2 * 1024 * 1024 + 1));
    let response = app(None)
        .oneshot(
            Request::post("/api/v1/jobs/00000000-0000-0000-0000-000000000000/test-report")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn job_log_append_rejects_payloads_above_explicit_limit() {
    let body = format!(r#"{{"message":"{}"}}"#, "x".repeat(1024 * 1024));
    let response = app(None)
        .oneshot(
            Request::post("/api/v1/jobs/00000000-0000-0000-0000-000000000000/logs")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn runner_log_append_rejects_payloads_above_explicit_limit() {
    let body = format!(
        r#"{{"protocolVersion":1,"leaseToken":"lease-token","fencingToken":1,"attemptId":"00000000-0000-0000-0000-000000000000","lines":[{{"stream":"stdout","message":"{}"}}]}}"#,
        "x".repeat(1024 * 1024)
    );
    let response = app(None)
        .oneshot(
            Request::post("/api/v1/runner/leases/00000000-0000-0000-0000-000000000000/logs")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
