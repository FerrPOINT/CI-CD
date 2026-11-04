//! Workspace metadata is a project read, not configuration administration.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(app: &axum::Router, method: &str, path: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", "Bearer sdlc_pat_fixture_product_roles")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub(super) async fn verify(pool: &PgPool, actor: Uuid) {
    let root = tempfile::tempdir().unwrap();
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.path().to_owned(),
            token: None,
            internal_token: None,
        },
        Some("workspace-metadata-test-secret".into()),
    );
    let prefix = format!("workspace-metadata-{}", Uuid::new_v4());
    let readable = Uuid::new_v4();
    let neighbor = Uuid::new_v4();
    for (id, suffix) in [(readable, "readable"), (neighbor, "neighbor")] {
        sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://fixture.invalid/source.git')")
            .bind(id).bind(format!("{prefix}-{suffix}")).execute(pool).await.unwrap();
    }
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'viewer')")
        .bind(readable)
        .bind(actor)
        .execute(pool)
        .await
        .unwrap();
    let list = format!("/api/v1/delivery-configurations?search={prefix}");
    let own = format!("/api/v1/delivery-configurations/{readable}");
    let other = format!("/api/v1/delivery-configurations/{neighbor}");
    let (status, page) = request(&app, "GET", &list).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], readable.to_string());
    assert_eq!(request(&app, "GET", &own).await.0, StatusCode::OK);
    assert_eq!(request(&app, "GET", &other).await.0, StatusCode::FORBIDDEN);
    for path in [
        "/api/v1/runners".to_owned(),
        format!("/api/v1/projects/{readable}/secrets"),
    ] {
        assert_eq!(request(&app, "GET", &path).await.0, StatusCode::FORBIDDEN);
    }
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("/api/v1/catalog/repositories/{neighbor}/delivery-configs")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("DELETE FROM project_memberships WHERE project_id=$1 AND user_id=$2")
        .bind(readable)
        .bind(actor)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(request(&app, "GET", &own).await.0, StatusCode::FORBIDDEN);
    let (status, page) = request(&app, "GET", &list).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 0);
    sqlx::query("DELETE FROM projects WHERE id=ANY($1)")
        .bind(vec![readable, neighbor])
        .execute(pool)
        .await
        .unwrap();
}
