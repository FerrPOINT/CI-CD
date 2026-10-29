//! Actual sync HTTP routes under an enforced selected-space policy.
use super::*;
use pdlc_shared::{resource_context::*, space_access::SpaceRef, space_lifecycle::*};
use serde_json::{Value, json};

async fn request(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    expected: StatusCode,
) -> Value {
    request_as(
        router,
        method,
        path,
        body,
        expected,
        "cicd_fixture_sync_owner",
    )
    .await
}
async fn request_as(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    expected: StatusCode,
    token: &str,
) -> Value {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", Uuid::new_v4().to_string());
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(
        status,
        expected,
        "{method} {path}: {}",
        String::from_utf8_lossy(&bytes)
    );
    if status.is_success() {
        assert!(
            headers["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
    }
    serde_json::from_slice(&bytes).unwrap()
}

pub(super) async fn verify(
    pool: &sqlx::PgPool,
    config: &cicd::config::RuntimeConfig,
    repository: Uuid,
    owner: &required_owner::RequiredOwner,
) {
    let registry: Uuid = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
        .unwrap()
        .parse()
        .unwrap();
    let instance: Uuid = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
        .unwrap()
        .parse()
        .unwrap();
    let hidden = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(hidden)
        .bind(format!("hidden-sync-{hidden}"))
        .execute(pool)
        .await
        .unwrap();
    let mut spaces = Vec::new();
    for id in [repository, hidden] {
        let group = Uuid::new_v4();
        let space = SpaceRef {
            registry_instance_id: registry,
            space_id: Uuid::new_v4(),
        };
        let slug = format!("sync-group-{group}");
        sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Sync admission fixture')")
            .bind(group)
            .bind(&slug)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("UPDATE repository_catalog SET group_id=$1 WHERE id=$2")
            .bind(group)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        let legacy = OwnerCommand {
            schema_version: 1,
            namespace: NamespaceRef {
                registry_instance_id: registry,
                namespace_id: space.space_id,
            },
            resource: ResourceRef {
                kind: ResourceKind::GitGroup,
                instance_id: instance,
                resource_id: group,
            },
            operation_id: Uuid::new_v4(),
            generation: 1,
            state: "active".into(),
            create_spec: None,
        };
        sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)").bind(group).bind(registry).bind(space.space_id).bind(serde_json::to_value(legacy).unwrap()).execute(pool).await.unwrap();
        let command = SpaceOwnerCommandV2 {
            schema_version: 2,
            space: space.clone(),
            participant_instance_id: instance,
            operation_id: Uuid::new_v4(),
            revision: 1,
            desired_state: DesiredSpaceState::Active,
            metadata: SpaceMetadata {
                slug,
                name: "Sync admission fixture".into(),
                description: String::new(),
                responsible_subject: owner.subject.to_string(),
            },
        };
        sqlx::query("INSERT INTO forge_space_catalog(registry_instance_id,space_id,group_id,revision,state,operation_id,command) VALUES($1,$2,$3,1,'active',$4,$5)").bind(registry).bind(space.space_id).bind(group).bind(command.operation_id).bind(serde_json::to_value(command).unwrap()).execute(pool).await.unwrap();
        spaces.push(space);
    }
    let mut runtime = config
        .clone()
        .with_auth_secret(Some("fixture-sync-owner-auth".into()));
    runtime.git_sync.enabled = true;
    let router = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        runtime.git.to_git_config(),
        None,
        runtime,
    )
    .unwrap();
    *owner.spaces.write().unwrap() = vec![spaces[0].clone()];
    sqlx::query("UPDATE forge_space_admission_state SET active=true WHERE singleton")
        .execute(pool)
        .await
        .unwrap();
    let base = format!("/api/v1/catalog/repositories/{repository}/sync");
    let visible = request(&router, "GET", &base, None, StatusCode::OK).await;
    assert_eq!(visible["repository_id"], repository.to_string());
    let hidden_base = format!("/api/v1/catalog/repositories/{hidden}/sync");
    for suffix in [
        "",
        "/runs",
        "/runs/00000000-0000-4000-8000-000000000009",
        "/runs/00000000-0000-4000-8000-000000000009/refs",
    ] {
        let denied = request(
            &router,
            "GET",
            &format!("{hidden_base}{suffix}"),
            None,
            StatusCode::FORBIDDEN,
        )
        .await;
        assert!(!denied.to_string().contains("hidden-sync"));
    }
    let command = json!({"expected_version":1,"settings":visible["settings"]});
    request(
        &router,
        "PUT",
        &hidden_base,
        Some(command.clone()),
        StatusCode::FORBIDDEN,
    )
    .await;
    request(
        &router,
        "POST",
        &format!("{hidden_base}/check"),
        Some(json!({"settings":visible["settings"]})),
        StatusCode::FORBIDDEN,
    )
    .await;
    request(
        &router,
        "POST",
        &format!("{hidden_base}/runs"),
        None,
        StatusCode::FORBIDDEN,
    )
    .await;
    // Common read filters total/page. A current admin cannot bypass Selected [].
    let page = request(
        &router,
        "GET",
        "/api/v1/repository-syncs?limit=1&offset=1",
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"], json!([]));
    // Independent principals avoid contradicting a still-valid cached proof.
    let page = request_as(
        &router,
        "GET",
        "/api/v1/repository-syncs",
        None,
        StatusCode::OK,
        "cicd_fixture_sync_no_spaces",
    )
    .await;
    assert_eq!(page["total"], 0);
    assert_eq!(page["items"], json!([]));
    request_as(
        &router,
        "GET",
        &base,
        None,
        StatusCode::FORBIDDEN,
        "cicd_fixture_sync_no_spaces",
    )
    .await;
    let saved = request(&router, "PUT", &base, Some(command), StatusCode::OK).await;
    assert_eq!(saved["version"], 2);
    // No worker is running here; manual enqueue validates the effect boundary.
    let run = request(
        &router,
        "POST",
        &format!("{base}/runs"),
        None,
        StatusCode::ACCEPTED,
    )
    .await;
    let run_id = run["id"].as_str().unwrap();
    request(
        &router,
        "GET",
        &format!("{base}/runs/{run_id}"),
        None,
        StatusCode::OK,
    )
    .await;
    request(
        &router,
        "GET",
        &format!("{base}/runs/{run_id}/refs"),
        None,
        StatusCode::OK,
    )
    .await;
    sqlx::query("UPDATE repository_sync_runs SET state='canceled',cancel_requested=true,finished_at=now() WHERE id=$1").bind(Uuid::parse_str(run_id).unwrap()).execute(pool).await.unwrap();
    sqlx::query("UPDATE forge_space_admission_state SET active=false WHERE singleton")
        .execute(pool)
        .await
        .unwrap();
}
