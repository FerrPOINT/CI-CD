use super::*;

#[tokio::test]
async fn push_refs_are_exact_and_decisions_survive_filter_changes() {
    let pool = test_pool().await;
    let (_, repository, _) = workspace_fixture(&pool, Uuid::new_v4(), "generic-repository").await;
    let configuration = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_id,repository_url) VALUES($1,'Delivery',$2,'https://example.test/repo.git')")
        .bind(configuration).bind(repository).execute(&pool).await.unwrap();
    let app = authenticated_app(pool.clone()).await;
    let set = |refs: Option<Vec<&str>>| {
        let mut value = serde_json::json!({"configuration_id":configuration});
        if let Some(refs) = refs {
            value["refs"] = serde_json::json!(refs);
        }
        Request::put(format!(
            "/api/v1/catalog/repositories/{repository}/push-config"
        ))
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(set(Some(vec!["refs/heads/main"])))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    use cicd::delivery_configs::push_decision_for_ref as decide;
    assert_eq!(
        decide(&pool, repository, Some("main"), Some("refs/heads/main"))
            .await
            .unwrap(),
        Some(configuration)
    );
    for reference in ["refs/heads/main-old", "refs/tags/main", "refs/heads/other"] {
        assert_eq!(
            decide(&pool, repository, Some(reference), Some(reference))
                .await
                .unwrap(),
            None
        );
    }
    // An older client omitting refs does not silently broaden the saved policy.
    assert_eq!(
        app.clone().oneshot(set(None)).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        decide(
            &pool,
            repository,
            Some("other-fresh"),
            Some("refs/heads/other")
        )
        .await
        .unwrap(),
        None
    );
    assert_eq!(
        app.oneshot(set(Some(vec![]))).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        decide(
            &pool,
            repository,
            Some("refs/heads/other"),
            Some("refs/heads/other")
        )
        .await
        .unwrap(),
        None
    );
    assert_eq!(
        decide(
            &pool,
            repository,
            Some("all-ref-fresh"),
            Some("refs/heads/other")
        )
        .await
        .unwrap(),
        Some(configuration)
    );
}

#[tokio::test]
async fn environment_target_is_optional_preserved_and_cannot_claim_manual_success() {
    let pool = test_pool().await;
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,'Application','https://example.test/app.git')")
        .bind(project).execute(&pool).await.unwrap();
    let app = authenticated_app(pool.clone()).await;
    let create = Request::post(format!("/api/v1/projects/{project}/environments"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"name":"dev","url":"https://app.test"}).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(create).await.unwrap();
    assert!(response.status().is_success());
    let environment = response_json(response).await;
    assert!(environment.get("kubernetes_target").is_none());
    let id = environment["id"].as_str().unwrap();
    let target = serde_json::json!({"schema":"forge/kubernetes-environment/v1","cluster":"pdlc",
        "namespace":"app-dev","release":"app","chart":"chart","values":[],
        "health_url":"https://app.test/health"});
    let patch = |value: serde_json::Value| {
        Request::patch(format!("/api/v1/environments/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(patch(serde_json::json!({"kubernetes_target":target})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["kubernetes_target"], target);
    let response = app
        .clone()
        .oneshot(patch(serde_json::json!({"url":"https://app.test/new"})))
        .await
        .unwrap();
    assert_eq!(response_json(response).await["kubernetes_target"], target);
    let fake = Request::post(format!("/api/v1/environments/{id}/deployments"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"git_ref":"main","status":"success"}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(fake).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deployments")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let response = app
        .oneshot(patch(serde_json::json!({"kubernetes_target":null})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response_json(response)
            .await
            .get("kubernetes_target")
            .is_none()
    );
}
