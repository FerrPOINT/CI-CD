//! The real catalog query used by the new-connection hosted repository selector.
use super::*;
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

struct OwnedStorage(PathBuf);
impl Drop for OwnedStorage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn group(state: &Arc<cicd::api::AppState>, prefix: &str) -> OwnerCommand {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Available sync fixture')")
        .bind(id)
        .bind(format!("available-{prefix}-{}", id.simple()))
        .execute(state.pool.as_ref().unwrap())
        .await
        .unwrap();
    let command = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
                .unwrap()
                .parse()
                .unwrap(),
            namespace_id: Uuid::new_v4(),
        },
        resource: ResourceRef {
            instance_id: std::env::var("CICD_NAMESPACE__INSTANCE_ID")
                .unwrap()
                .parse()
                .unwrap(),
            kind: ResourceKind::GitGroup,
            resource_id: id,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: None,
    };
    let _ = cicd::namespace::apply(
        axum::extract::State(state.clone()),
        axum::extract::Path(id),
        axum::Json(command.clone()),
    )
    .await
    .unwrap();
    command
}

async fn repository(
    state: &cicd::api::AppState,
    group: Uuid,
    slug: &str,
    kind: &str,
    ready: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    let storage = format!("available_{}", id.simple());
    let pool = state.pool.as_ref().unwrap();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(id)
        .bind(&storage)
        .execute(pool)
        .await
        .unwrap();
    if kind == "hosted" && ready {
        let output = tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(state.git.root.join(format!("{storage}.git")))
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
    }
    sqlx::query("UPDATE repository_catalog SET group_id=$2,slug=$3,kind=$4,ready=$5,storage_name=CASE WHEN $4='hosted' THEN $6 ELSE NULL END,external_url=CASE WHEN $4='external' THEN 'https://fixture.invalid/external.git' ELSE NULL END WHERE id=$1")
        .bind(id).bind(group).bind(slug).bind(kind).bind(ready).bind(&storage)
        .execute(pool).await.unwrap();
    id
}

async fn page(router: &axum::Router, query: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/catalog/repositories?sync_available=true&{query}"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

#[tokio::test]
async fn available_hosted_catalog_pages_filter_exclusions_and_save_by_exact_uuid() {
    let pool = test_pool().await;
    let storage =
        OwnedStorage(std::env::temp_dir().join(format!("forge-available-{}", Uuid::new_v4())));
    std::fs::create_dir(&storage.0).unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.git.root = storage.0.clone();
    config.secrets.key = Some([23; 32]);
    let state = Arc::new(cicd::api::AppState {
        pool: Some(pool.clone()),
        namespace_admission_pool: Some(pool.clone()),
        auth_secret: None,
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: Arc::new(cicd::rate_limit::RateLimiter::default()),
    });
    let a = group(&state, "a").await;
    let b = group(&state, "b").await;
    let mut archived = group(&state, "c").await;
    let mut expected = BTreeMap::new();
    let mut duplicates = [Uuid::nil(); 2];
    for (position, (owner, count)) in [(&a, 23), (&b, 22)].into_iter().enumerate() {
        for index in 0..count {
            let slug = format!("same-{index:03}");
            let id = repository(&state, owner.resource.resource_id, &slug, "hosted", true).await;
            expected.insert(
                id,
                (
                    owner.resource.resource_id,
                    owner.namespace.namespace_id,
                    slug,
                ),
            );
            if index == 0 {
                duplicates[position] = id;
            }
        }
    }
    let connected = repository(
        &state,
        a.resource.resource_id,
        "omit-connected",
        "hosted",
        true,
    )
    .await;
    let external = repository(
        &state,
        a.resource.resource_id,
        "omit-external",
        "external",
        true,
    )
    .await;
    let pending = repository(
        &state,
        a.resource.resource_id,
        "omit-pending",
        "hosted",
        false,
    )
    .await;
    let archived_repo = repository(
        &state,
        archived.resource.resource_id,
        "omit-archived",
        "hosted",
        true,
    )
    .await;
    archived.generation += 1;
    archived.operation_id = Uuid::new_v4();
    archived.state = "archived".into();
    let _ = cicd::namespace::apply(
        axum::extract::State(state.clone()),
        axum::extract::Path(archived.resource.resource_id),
        axum::Json(archived),
    )
    .await
    .unwrap();
    let actor = Uuid::new_v4();
    app::save(
        &PgSyncStore {
            pool: pool.clone(),
            worker_enabled: false,
            interval_seconds: 300,
        },
        &Secrets(Some([23; 32])),
        connected,
        Uuid::new_v4(),
        Access {
            actor_id: actor,
            project_id: None,
            admin: true,
            machine: false,
            writable: true,
        },
        SaveConnection {
            expected_version: 0,
            settings: Settings {
                provider: Provider::Github,
                instance_url: "https://github.com".into(),
                repository_path: "fixture/already-connected".into(),
                direction: Direction::Disabled,
                automatic: false,
            },
            token: Some("fixture-available-credential".into()),
        },
        &[],
    )
    .await
    .unwrap();
    let claims = cicd::auth::AccessClaims {
        sub: actor,
        service_account_name: None,
        sid: None,
        token_id: None,
        token_project_id: None,
        token_scopes: vec![],
        role: "admin".into(),
        ver: 0,
        iat: 0,
        exp: i64::MAX,
    };
    let router = axum::Router::new()
        .route(
            "/api/v1/catalog/repositories",
            axum::routing::get(cicd::repository_catalog::all),
        )
        .merge(cicd::repository_sync::routes())
        .with_state(state.clone())
        .layer(axum::Extension(claims));
    let mut actual = BTreeMap::new();
    for (offset, length) in [(0, 20), (20, 20), (40, 6)] {
        let result = page(&router, &format!("limit=20&offset={offset}")).await;
        assert_eq!(result["total"], 46);
        assert_eq!(result["items"].as_array().unwrap().len(), length);
        for item in result["items"].as_array().unwrap() {
            let id: Uuid = item["id"].as_str().unwrap().parse().unwrap();
            assert_eq!(item["kind"], "hosted");
            assert_eq!(item["ready"], true);
            assert!(item["availability"].is_null());
            let group: Uuid = item["group_id"].as_str().unwrap().parse().unwrap();
            let namespace: Uuid = item["namespace"]["namespace_id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                actual
                    .insert(
                        id,
                        (group, namespace, item["slug"].as_str().unwrap().to_owned())
                    )
                    .is_none(),
                "pages must not duplicate UUIDs"
            );
        }
    }
    expected.insert(
        connected,
        (
            a.resource.resource_id,
            a.namespace.namespace_id,
            "omit-connected".into(),
        ),
    );
    assert_eq!(actual, expected);
    for excluded in [external, pending, archived_repo] {
        assert!(!actual.contains_key(&excluded));
    }
    let search = page(&router, "limit=20&search=SaMe-000").await;
    assert_eq!(search["total"], 2);
    assert_eq!(search["items"].as_array().unwrap().len(), 2);
    for (owner, count) in [(&a, 24), (&b, 22)] {
        let result = page(
            &router,
            &format!("group_id={}&limit=100", owner.resource.resource_id),
        )
        .await;
        assert_eq!(result["total"], count);
        assert!(
            result["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["group_id"] == owner.resource.resource_id.to_string())
        );
    }
    let selected = duplicates[1];
    let response = router.clone().oneshot(Request::put(format!("/api/v1/catalog/repositories/{selected}/sync"))
        .header("content-type", "application/json").header("Idempotency-Key", Uuid::new_v4().to_string())
        .body(Body::from(serde_json::json!({"expected_version":0,"settings":{"provider":"github","instance_url":"https://github.com","repository_path":"fixture/new-selection","direction":"disabled","automatic":false},"token":"fixture-available-credential"}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved = response_json(response).await;
    assert_eq!(saved["repository_id"], selected.to_string());
    assert_eq!(saved["version"], 1);
    assert_eq!(page(&router, "limit=100").await["total"], 46);
    let remaining = page(&router, "search=same-000").await;
    assert_eq!(remaining["total"], 2);
    assert_eq!(remaining["items"][0]["id"], duplicates[0].to_string());
    assert_eq!(
        page(
            &router,
            &format!("group_id={}&limit=100", b.resource.resource_id)
        )
        .await["total"],
        22
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM repository_sync_connections WHERE repository_id=$1"
        )
        .bind(duplicates[0])
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    println!(
        "SYNC_AVAILABLE_CATALOG_OK eligible_before=46 eligible_after=46 pages=3 duplicate_slug_uuids_distinct=true excluded=external,pending,archived selected_uuid={selected}"
    );
    drop(router);
    drop(state);
    pool.close().await;
}
