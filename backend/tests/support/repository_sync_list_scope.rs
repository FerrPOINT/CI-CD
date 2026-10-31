//! Positive namespace filters on the saved-connections endpoint, not the picker.
use super::*;
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
use std::{collections::BTreeMap, sync::Arc};

async fn group(state: &Arc<cicd::api::AppState>, label: &str) -> OwnerCommand {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Scoped sync list fixture')")
        .bind(id)
        .bind(format!("sync-list-{label}-{}", id.simple()))
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

async fn page(router: &axum::Router, query: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/repository-syncs?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

fn identities(page: &serde_json::Value) -> BTreeMap<Uuid, (Uuid, String)> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert!(item["has_credential"].as_bool().unwrap());
            (
                item["repository_id"].as_str().unwrap().parse().unwrap(),
                (
                    item["namespace"]["namespace_id"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                    item["repository_name"].as_str().unwrap().to_owned(),
                ),
            )
        })
        .collect()
}

#[tokio::test]
async fn saved_connections_group_filter_preserves_uuid_namespace_and_combined_page_totals() {
    let pool = test_pool().await;
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false).unwrap();
    config.secrets.key = Some([37; 32]);
    config.git_sync.allowed_gitlab_origins = vec!["https://git-list.example.invalid".into()];
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
    let actor = Uuid::new_v4();
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: false,
        interval_seconds: 300,
    };
    let access = Access {
        actor_id: actor,
        project_id: None,
        admin: true,
        machine: false,
        writable: true,
    };
    let mut expected = [BTreeMap::new(), BTreeMap::new()];
    let mut named = BTreeMap::new();
    for (index, owner) in [&a, &b].into_iter().enumerate() {
        let group_slug: String = sqlx::query_scalar("SELECT slug FROM git_groups WHERE id=$1")
            .bind(owner.resource.resource_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        for (slug, provider, direction) in [
            ("shared-0", Provider::Github, Direction::Disabled),
            ("shared-1", Provider::Gitlab, Direction::Pull),
            ("other", Provider::Github, Direction::Pull),
            ("shared-2", Provider::Gitlab, Direction::Disabled),
        ] {
            if index == 1 && slug == "shared-2" {
                continue;
            }
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
                .bind(id)
                .bind(format!("sync_list_{}", id.simple()))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("UPDATE repository_catalog SET group_id=$2,slug=$3,ready=true WHERE id=$1")
                .bind(id)
                .bind(owner.resource.resource_id)
                .bind(slug)
                .execute(&pool)
                .await
                .unwrap();
            app::save(
                &store,
                &Secrets(state.config.secrets.key),
                id,
                Uuid::new_v4(),
                access,
                SaveConnection {
                    expected_version: 0,
                    settings: Settings {
                        provider,
                        instance_url: match provider {
                            Provider::Github => "https://github.com",
                            Provider::Gitlab => "https://git-list.example.invalid",
                        }
                        .into(),
                        repository_path: format!("fixture/{slug}"),
                        direction,
                        automatic: false,
                        protected_branch_updates:
                            cicd_domain::repository_sync::ProtectedBranchUpdates::Blocked,
                    },
                    token: Some("fictional-list-scope-credential".into()),
                },
                &state.config.git_sync.allowed_gitlab_origins,
            )
            .await
            .unwrap();
            expected[index].insert(
                id,
                (owner.namespace.namespace_id, format!("{group_slug}/{slug}")),
            );
            named.insert((index, slug), id);
        }
    }
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
    let router = cicd::repository_sync::routes()
        .with_state(state.clone())
        .layer(axum::Extension(claims));
    // These are real HTTP query extractors and the production saved-list SQL.
    // No group parameter is the explicit all-project contract.
    let all_expected: BTreeMap<_, _> = expected.iter().flat_map(|group| group.clone()).collect();
    let all = page(&router, "limit=100").await;
    assert_eq!(all["total"], 7);
    assert_eq!(identities(&all), all_expected);
    for (index, owner) in [&a, &b].into_iter().enumerate() {
        let result = page(
            &router,
            &format!("group_id={}&limit=100", owner.resource.resource_id),
        )
        .await;
        assert_eq!(result["total"], expected[index].len());
        assert_eq!(identities(&result), expected[index]);
        assert_eq!(
            result["items"].as_array().unwrap().len(),
            expected[index].len()
        );
    }
    let group_a = a.resource.resource_id;
    let group_b = b.resource.resource_id;
    let middle = page(
        &router,
        &format!("group_id={group_a}&search=ShArEd&limit=1&offset=1"),
    )
    .await;
    assert_eq!(middle["total"], 3, "total precedes pagination");
    assert_eq!(middle["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        middle["items"][0]["repository_id"],
        named[&(0, "shared-1")].to_string()
    );
    let filtered = page(
        &router,
        &format!("group_id={group_a}&provider=gitlab&state=disabled&search=SHARED&limit=1"),
    )
    .await;
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        filtered["items"][0]["repository_id"],
        named[&(0, "shared-2")].to_string()
    );
    let past = page(
        &router,
        &format!(
            "group_id={group_a}&provider=gitlab&state=disabled&search=shared&limit=1&offset=1"
        ),
    )
    .await;
    assert_eq!(past["total"], 1);
    assert!(past["items"].as_array().unwrap().is_empty());
    let active_b = page(
        &router,
        &format!("group_id={group_b}&provider=gitlab&state=enabled&search=shared&limit=1"),
    )
    .await;
    assert_eq!(active_b["total"], 1);
    assert_eq!(active_b["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        active_b["items"][0]["repository_id"],
        named[&(1, "shared-1")].to_string()
    );
    assert_eq!(
        active_b["items"][0]["namespace"]["namespace_id"],
        b.namespace.namespace_id.to_string()
    );
    let all_disabled = page(
        &router,
        "provider=github&state=disabled&search=shared&limit=1&offset=1",
    )
    .await;
    assert_eq!(all_disabled["total"], 2);
    assert_eq!(all_disabled["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        all_disabled["items"][0]["repository_id"],
        named[&(1, "shared-0")].to_string()
    );
    let absent = page(&router, &format!("group_id={}", Uuid::new_v4())).await;
    assert_eq!(absent["total"], 0);
    assert!(absent["items"].as_array().unwrap().is_empty());
    println!(
        "SYNC_SAVED_LIST_SCOPE_OK all_total=7 group_a_total=4 group_b_total=3 duplicate_slugs_distinct_uuid_namespace=true combined_filters=true offset_total_preserved=true"
    );
    drop(router);
    drop(state);
    pool.close().await;
}
