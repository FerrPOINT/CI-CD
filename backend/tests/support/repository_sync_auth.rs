//! Production-router authentication; claims are never injected by the fixture.
use super::*;

#[tokio::test]
async fn sync_authenticates_project_tokens_without_opening_catalog_access() {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    let account = Uuid::new_v4();
    let name = format!("sync-auth-{}", repository.simple());
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Sync access','https://example.invalid/own.git',$2),($3,'Foreign access','https://example.invalid/foreign.git',NULL)")
        .bind(project).bind(repository).bind(foreign).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'sync-auth-fixture')")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    let mut tokens = Vec::new();
    for (bound, scopes) in [
        (Some(project), vec!["api:read", "api:write", "git:write"]),
        (Some(project), vec!["api:read"]),
        (Some(foreign), vec!["api:read", "api:write", "git:write"]),
        (None, vec!["api:read", "api:write", "git:write"]),
        (Some(project), Vec::new()),
        (Some(project), vec!["api:read", "api:write"]),
    ] {
        let id = Uuid::new_v4();
        let token = format!(
            "forge_sat_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes,expires_at) VALUES($1,'sync-auth-fixture',$2,'synthetic-only','service_account',$3,$4,$5,now()+interval '1 day')")
            .bind(id).bind(cicd::auth::hash_token(&token)).bind(account).bind(bound).bind(scopes).execute(&pool).await.unwrap();
        tokens.push((id, token));
    }
    let root = std::env::temp_dir().join(format!("forge-sync-auth-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(root.join(format!("{name}.git")))
            .status()
            .await
            .unwrap()
            .success()
    );
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some(format!("synthetic-sync-secret-{account}")));
    config.git.root = root.clone();
    config.git_sync.enabled = true;
    config.secrets.key = Some([13; 32]);
    let router = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    let path = format!("/api/v1/catalog/repositories/{repository}/sync");
    async fn request(
        router: &axum::Router,
        method: &str,
        path: &str,
        token: &str,
        body: serde_json::Value,
    ) -> StatusCode {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", Uuid::new_v4().to_string())
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        if status == StatusCode::BAD_REQUEST {
            let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
                .await
                .unwrap();
            panic!(
                "Unexpected invalid fixture: {}",
                String::from_utf8_lossy(&body)
            );
        }
        status
    }
    for index in [0, 1] {
        assert_eq!(
            request(
                &router,
                "GET",
                &path,
                &tokens[index].1,
                serde_json::Value::Null
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            request(
                &router,
                "GET",
                &format!("{path}/runs"),
                &tokens[index].1,
                serde_json::Value::Null
            )
            .await,
            StatusCode::OK
        );
    }
    let save = serde_json::json!({"expected_version":0,"token":"synthetic-provider-token","settings":{"provider":"github","instance_url":"https://github.com","repository_path":"synthetic/repo","direction":"disabled","automatic":false}});
    assert_eq!(
        request(&router, "PUT", &path, &tokens[1].1, save.clone()).await,
        StatusCode::FORBIDDEN
    );
    for index in [2, 3, 4] {
        assert_eq!(
            request(
                &router,
                "GET",
                &path,
                &tokens[index].1,
                serde_json::Value::Null
            )
            .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&router, "PUT", &path, &tokens[index].1, save.clone()).await,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(&router, "PUT", &path, &tokens[5].1, save.clone()).await,
        StatusCode::FORBIDDEN
    );
    let mut privileged = save.clone();
    privileged["settings"]["direction"] = serde_json::json!("pull");
    privileged["settings"]["protected_branch_updates"] = serde_json::json!("source_fast_forward");
    assert_eq!(
        request(&router, "PUT", &path, &tokens[0].1, privileged).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&router, "PUT", &path, &tokens[0].1, save).await,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/api/v1/catalog/repositories/{repository}"),
            &tokens[0].1,
            serde_json::Value::Null
        )
        .await,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE api_tokens SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(tokens[0].0)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&router, "GET", &path, &tokens[0].1, serde_json::Value::Null).await,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE api_tokens SET revoked_at=now() WHERE id=$1")
        .bind(tokens[1].0)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&router, "GET", &path, &tokens[1].1, serde_json::Value::Null).await,
        StatusCode::UNAUTHORIZED
    );
    drop(router);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
