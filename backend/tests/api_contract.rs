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
