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

async fn deployment_projection_request(
    app: &axum::Router,
    method: axum::http::Method,
    path: &str,
    input: Option<serde_json::Value>,
) -> serde_json::Value {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn deployment_projection_preserves_legacy_create_list_and_rollback_response() {
    use axum::http::Method;
    use serde_json::json;

    let pool = test_pool().await;
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://example.invalid/projection.git')")
        .bind(project).bind(format!("projection-{project}")).execute(&pool).await.unwrap();
    let app = authenticated_app(pool.clone()).await;
    let environment = deployment_projection_request(
        &app, Method::POST, &format!("/api/v1/projects/{project}/environments"),
        Some(json!({"name":"legacy","url":"https://legacy.example.invalid"})),
    ).await;
    let environment_id = environment["id"].as_str().unwrap();
    let path = format!("/api/v1/environments/{environment_id}/deployments");
    let source = deployment_projection_request(
        &app, Method::POST, &path,
        Some(json!({"git_ref":"legacy-release","status":"success"})),
    ).await;
    assert_eq!(source["status"], "success");
    assert_eq!(source["approval_state"], "not_required");
    assert_eq!(source["approval_count"], 0);
    assert_eq!(source["required_approvals"], 0);
    assert!(source.get("runtime_phase").is_none());
    assert!(source.get("runtime_receipt").is_none());

    let listed = deployment_projection_request(&app, Method::GET, &path, None).await;
    assert_eq!(listed.as_array().unwrap(), &vec![source.clone()]);
    let source_id = source["id"].as_str().unwrap();

    // This exercises the actual rollback HTTP response after its pipeline is
    // created. No worker runs and no external deployment success is fabricated.
    let rollback = deployment_projection_request(
        &app, Method::POST, &format!("/api/v1/deployments/{source_id}/rollback"),
        Some(json!({})),
    ).await;
    assert_ne!(rollback["id"], source["id"]);
    assert_eq!(rollback["rollback_of_id"], source["id"]);
    assert_eq!(rollback["git_ref"], "legacy-release");
    assert_eq!(rollback["status"], "pending");
    assert!(rollback.get("runtime_phase").is_none());
    assert!(rollback.get("runtime_receipt").is_none());
    let pipeline = Uuid::parse_str(rollback["pipeline_id"].as_str().unwrap()).unwrap();
    let (status, variables): (String, serde_json::Value) =
        sqlx::query_as("SELECT status,variables FROM pipelines WHERE id=$1 AND project_id=$2")
            .bind(pipeline).bind(project).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "queued");
    assert_eq!(variables["deployment_id"], rollback["id"]);
    assert_eq!(variables["rollback_of_deployment_id"], source["id"]);

    let listed = deployment_projection_request(&app, Method::GET, &path, None).await;
    let rows = listed.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(&source));
    assert!(rows.contains(&rollback));
    let source_after: (String, Option<String>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT status,runtime_phase,runtime_receipt FROM deployments WHERE id=$1",
    ).bind(Uuid::parse_str(source_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(source_after, ("success".into(), None, None));
    pool.close().await;
}

#[tokio::test]
async fn deployment_projection_preserves_kubernetes_runtime_on_list_and_fetch_response() {
    use axum::http::Method;
    use serde_json::json;

    let pool = test_pool().await;
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://example.invalid/kube-projection.git')")
        .bind(project).bind(format!("kube-projection-{project}")).execute(&pool).await.unwrap();
    let app = authenticated_app(pool.clone()).await;
    let environment = deployment_projection_request(
        &app, Method::POST, &format!("/api/v1/projects/{project}/environments"),
        Some(json!({
            "name":"kubernetes","protected":true,"required_approvals":1,
            "kubernetes_target":{
                "schema":"forge/kubernetes-environment/v1","cluster":"fixture-cluster",
                "namespace":"fixture-dev","release":"fixture-app","chart":"chart","values":[],
                "health_url":"https://fixture.example.invalid/health"
            }
        })),
    ).await;
    let environment_id = environment["id"].as_str().unwrap();
    let path = format!("/api/v1/environments/{environment_id}/deployments");
    let pending = deployment_projection_request(
        &app, Method::POST, &path, Some(json!({"git_ref":"main"})),
    ).await;
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["approval_state"], "pending");
    assert!(pending["pipeline_id"].is_null());
    assert!(pending.get("runtime_phase").is_none());
    assert!(pending.get("runtime_receipt").is_none());
    let deployment = Uuid::parse_str(pending["id"].as_str().unwrap()).unwrap();

    // Populate only the isolated PostgreSQL read-model fixture. This marker
    // claims neither execution nor a qualified successful runtime observation.
    let receipt = json!({"fixture":"projection-only","diagnostic":"no executor was run"});
    sqlx::query("UPDATE deployments SET runtime_phase='prepared',runtime_receipt=$2 WHERE id=$1")
        .bind(deployment).bind(&receipt).execute(&pool).await.unwrap();
    let listed = deployment_projection_request(&app, Method::GET, &path, None).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], pending["id"]);
    assert_eq!(listed[0]["runtime_phase"], "prepared");
    assert_eq!(listed[0]["runtime_receipt"], receipt);
    assert_eq!(listed[0]["status"], "pending");
    assert_eq!(listed[0]["approval_state"], "pending");
    assert_eq!(listed[0]["required_approvals"], 1);

    // Rejection invokes the shared fetch_deployment projection without starting
    // a pipeline, a Kubernetes client or an external execution.
    let rejected = deployment_projection_request(
        &app, Method::POST, &format!("/api/v1/deployments/{deployment}/approvals"),
        Some(json!({"decision":"rejected","comment":"projection regression fixture"})),
    ).await;
    assert_eq!(rejected["id"], pending["id"]);
    assert_eq!(rejected["status"], "failed");
    assert_eq!(rejected["approval_state"], "rejected");
    assert_eq!(rejected["runtime_phase"], "prepared");
    assert_eq!(rejected["runtime_receipt"], receipt);
    assert!(rejected["pipeline_id"].is_null());
    let pipelines: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(project).fetch_one(&pool).await.unwrap();
    assert_eq!(pipelines, 0);
    let listed = deployment_projection_request(&app, Method::GET, &path, None).await;
    assert_eq!(listed.as_array().unwrap(), &vec![rejected]);
    pool.close().await;
}
