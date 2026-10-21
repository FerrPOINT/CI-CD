//! Exact command contracts through the production router and authentication.
use super::*;

struct HttpFixture {
    pool: sqlx::PgPool,
    router: axum::Router,
    root: std::path::PathBuf,
    repositories: [Uuid; 2],
    tokens: Vec<String>,
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        // No supervisors or Git effects are started by this HTTP-only fixture.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl HttpFixture {
    async fn new() -> Self {
        let pool = test_pool().await;
        let repositories = [Uuid::new_v4(), Uuid::new_v4()];
        let projects = [Uuid::new_v4(), Uuid::new_v4()];
        let actors = [Uuid::new_v4(), Uuid::new_v4()];
        let root = std::env::temp_dir().join(format!("forge-sync-contract-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for (repository, project) in repositories.iter().zip(projects) {
            let name = format!("contract-{}", repository.simple());
            sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
                .bind(repository)
                .bind(&name)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch) VALUES($1,$2,'https://forge.invalid/contract',$3,'qa-protected-default')")
                .bind(project).bind(format!("contract-{project}")).bind(repository).execute(&pool).await.unwrap();
            assert!(
                tokio::process::Command::new("git")
                    .args(["init", "--bare", "--initial-branch=main"])
                    .arg(root.join(format!("{name}.git")))
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await
                    .unwrap()
                    .success()
            );
        }
        for actor in actors {
            sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
                .bind(actor)
                .bind(format!("contract-{actor}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let mut tokens = Vec::new();
        // Tokens 0/1 are different actors authorized on the same repository;
        // token 2 is the first actor independently authorized on repository 2.
        // Cross-actor/repository replay must therefore reach 409, never merely
        // fail the caller's authorization with an unrelated 403.
        for (actor, project) in [
            (actors[0], projects[0]),
            (actors[1], projects[0]),
            (actors[0], projects[1]),
        ] {
            let token = format!("forge_sat_fixture_contract_{}", Uuid::new_v4().simple());
            sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes) VALUES($1,'Contract fixture',$2,'fictional','service_account',$3,$4,ARRAY['api:read','api:write','git:write'])")
                .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&token)).bind(actor).bind(project).execute(&pool).await.unwrap();
            tokens.push(token);
        }
        let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
            .unwrap()
            .with_auth_secret(Some("fictional-contract-auth-secret".into()));
        config.git.root = root.clone();
        config.git_sync.enabled = true;
        config.git_sync.allowed_gitlab_origins = vec![
            "https://gitlab-contract-one.example.invalid".into(),
            "https://gitlab-contract-two.example.invalid".into(),
        ];
        config.secrets.key = Some([29; 32]);
        let router = cicd::api::app_with_git_and_config(
            Some(pool.clone()),
            config.git.to_git_config(),
            None,
            config,
        )
        .unwrap();
        let fixture = Self {
            pool,
            router,
            root,
            repositories,
            tokens,
        };
        for (token, repository) in [(0, 0), (1, 0), (2, 1)] {
            let (status, body) = fixture.call(token, repository, "GET", "", None, None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["writable"], true);
        }
        fixture
    }

    async fn raw(
        &self,
        token: usize,
        method: &str,
        path: String,
        body: Option<serde_json::Value>,
        key: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {}", self.tokens[token]));
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = self
            .router
            .clone()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        for token in &self.tokens {
            assert!(
                !text.contains(token),
                "HTTP contract readback excludes caller tokens"
            );
        }
        assert!(!text.contains("fictional-provider-contract"));
        let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, body)
    }

    async fn call(
        &self,
        token: usize,
        repository: usize,
        method: &str,
        tail: &str,
        body: Option<serde_json::Value>,
        key: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        self.raw(
            token,
            method,
            format!(
                "/api/v1/catalog/repositories/{}/sync{tail}",
                self.repositories[repository]
            ),
            body,
            key,
        )
        .await
    }

    async fn saved(&self, repository: usize) -> serde_json::Value {
        let token = if repository == 0 { 0 } else { 2 };
        let (status, value) = self.call(token, repository, "GET", "", None, None).await;
        assert_eq!(status, StatusCode::OK);
        value
    }

    async fn encrypted(&self, repository: usize) -> String {
        sqlx::query_scalar(
            "SELECT encrypted_credential FROM repository_sync_connections WHERE repository_id=$1",
        )
        .bind(self.repositories[repository])
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }
}

fn settings(direction: &str) -> serde_json::Value {
    serde_json::json!({"provider":"github","instance_url":"https://github.com","repository_path":"fixture/contract","direction":direction,"automatic":false})
}

#[tokio::test]
async fn operation_keys_reject_other_authorized_actor_and_repository_over_http() {
    let f = HttpFixture::new().await;
    let save_key = Uuid::new_v4().to_string();
    let command = serde_json::json!({"expected_version":0,"settings":settings("pull"),"token":"fictional-provider-contract-initial"});
    let (status, original) = f
        .call(0, 0, "PUT", "", Some(command.clone()), Some(&save_key))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(original["version"], 1);
    assert_eq!(
        f.call(0, 0, "PUT", "", Some(command.clone()), Some(&save_key))
            .await,
        (StatusCode::OK, original.clone())
    );
    for (token, repository) in [(1, 0), (2, 1)] {
        let (status, error) = f
            .call(
                token,
                repository,
                "PUT",
                "",
                Some(command.clone()),
                Some(&save_key),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(error["error"]["message"], "idempotency_conflict");
    }
    assert_eq!(f.saved(0).await, original);
    assert_eq!(f.saved(1).await["version"], 0);
    let (status, _) = f
        .call(
            2,
            1,
            "PUT",
            "",
            Some(command),
            Some(&Uuid::new_v4().to_string()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let run_key = Uuid::new_v4().to_string();
    let (status, run) = f.call(0, 0, "POST", "/runs", None, Some(&run_key)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(run["state"], "queued");
    assert_eq!(
        f.call(0, 0, "POST", "/runs", None, Some(&run_key)).await,
        (StatusCode::ACCEPTED, run.clone())
    );
    for (token, repository) in [(1, 0), (2, 1)] {
        let (status, error) = f
            .call(token, repository, "POST", "/runs", None, Some(&run_key))
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(error["error"]["message"], "idempotency_conflict");
    }
    assert_eq!(f.call(0, 0, "GET", "/runs", None, None).await.1["total"], 1);
    assert_eq!(f.call(2, 1, "GET", "/runs", None, None).await.1["total"], 0);
    let operations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM repository_sync_operations WHERE operation_id IN ($1,$2)",
    )
    .bind(Uuid::parse_str(&save_key).unwrap())
    .bind(Uuid::parse_str(&run_key).unwrap())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        operations, 2,
        "rejected cross-principal replays never replace original receipts"
    );
    f.pool.close().await;
}

#[tokio::test]
async fn credential_and_malformed_command_rejections_preserve_saved_connection_over_http() {
    let f = HttpFixture::new().await;
    let (status, saved) = f.call(0, 0, "PUT", "", Some(serde_json::json!({"expected_version":0,"settings":settings("pull"),"token":"fictional-provider-contract-initial"})), Some(&Uuid::new_v4().to_string())).await;
    assert_eq!(status, StatusCode::OK);
    let encrypted = f.encrypted(0).await;
    let mut gitlab = settings("pull");
    gitlab["provider"] = "gitlab".into();
    gitlab["instance_url"] = "https://gitlab-contract-one.example.invalid".into();
    for command in [
        serde_json::json!({"expected_version":1,"settings":settings("pull"),"token":""}),
        serde_json::json!({"expected_version":1,"settings":settings("pull"),"token":"   "}),
        serde_json::json!({"expected_version":1,"settings":gitlab}),
    ] {
        assert_eq!(
            f.call(
                0,
                0,
                "PUT",
                "",
                Some(command),
                Some(&Uuid::new_v4().to_string())
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(f.saved(0).await, saved);
        assert!(
            f.encrypted(0).await == encrypted,
            "rejection preserves ciphertext without exposing it"
        );
    }
    for key in [
        None,
        Some("not-a-uuid"),
        Some("00000000-0000-0000-0000-000000000000"),
    ] {
        let command = serde_json::json!({"expected_version":1,"settings":settings("push")});
        assert_eq!(
            f.call(0, 0, "PUT", "", Some(command), key).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.call(0, 0, "POST", "/runs", None, key).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(f.saved(0).await, saved);
    }
    for suffix in ["", "/runs"] {
        let (status, _) = f
            .raw(
                0,
                "GET",
                format!("/api/v1/catalog/repositories/not-a-uuid/sync{suffix}"),
                None,
                None,
            )
            .await;
        assert!(
            status.is_client_error(),
            "malformed UUID never reaches a 500"
        );
    }
    assert_eq!(f.call(0, 0, "GET", "/runs", None, None).await.1["total"], 0);
    // A supplied replacement makes the very same provider change valid.
    let (status, rotated) = f.call(0, 0, "PUT", "", Some(serde_json::json!({"expected_version":1,"settings":gitlab,"token":"fictional-provider-contract-provider-rotation"})), Some(&Uuid::new_v4().to_string())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rotated["version"], 2);
    let encrypted = f.encrypted(0).await;
    gitlab["instance_url"] = "https://gitlab-contract-two.example.invalid".into();
    assert_eq!(
        f.call(
            0,
            0,
            "PUT",
            "",
            Some(serde_json::json!({"expected_version":2,"settings":gitlab})),
            Some(&Uuid::new_v4().to_string())
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(f.saved(0).await, rotated);
    assert!(f.encrypted(0).await == encrypted);
    let (status, origin_rotated) = f.call(0, 0, "PUT", "", Some(serde_json::json!({"expected_version":2,"settings":gitlab,"token":"fictional-provider-contract-origin-rotation"})), Some(&Uuid::new_v4().to_string())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(origin_rotated["version"], 3);
    let encrypted = f.encrypted(0).await;
    gitlab["repository_path"] = "fixture/path-only-edit".into();
    let (status, omitted) = f
        .call(
            0,
            0,
            "PUT",
            "",
            Some(serde_json::json!({"expected_version":3,"settings":gitlab})),
            Some(&Uuid::new_v4().to_string()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(omitted["version"], 4);
    assert!(
        f.encrypted(0).await == encrypted,
        "omitted credential survives path-only edit byte-for-byte"
    );
    assert_eq!(f.call(0, 0, "GET", "/runs", None, None).await.1["total"], 0);
    f.pool.close().await;
}

#[tokio::test]
async fn queued_running_and_reconciling_allow_only_disable_over_http() {
    for phase in ["queued", "running", "reconciling"] {
        let f = HttpFixture::new().await;
        let (status, _) = f.call(0, 0, "PUT", "", Some(serde_json::json!({"expected_version":0,"settings":settings("pull"),"token":"fictional-provider-contract-initial"})), Some(&Uuid::new_v4().to_string())).await;
        assert_eq!(status, StatusCode::OK);
        let (status, run) = f
            .call(
                0,
                0,
                "POST",
                "/runs",
                None,
                Some(&Uuid::new_v4().to_string()),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let run_id = Uuid::parse_str(run["id"].as_str().unwrap()).unwrap();
        let store = PgSyncStore {
            pool: f.pool.clone(),
            worker_enabled: true,
            interval_seconds: 300,
        };
        let owner = Uuid::new_v4();
        let claimed = if phase == "queued" {
            None
        } else {
            let value = store.claim(run_id, owner).await.unwrap().unwrap();
            if phase == "reconciling" {
                store
                    .finish(run_id, owner, value.1, "reconciling", None)
                    .await
                    .unwrap();
            }
            Some(value)
        };
        assert_eq!(
            store.run(f.repositories[0], run_id).await.unwrap().state,
            phase
        );
        let saved = f.saved(0).await;
        let encrypted = f.encrypted(0).await;
        let mut changed_path = settings("pull");
        changed_path["repository_path"] = "fixture/edited-during-run".into();
        let mut automatic = settings("pull");
        automatic["automatic"] = true.into();
        for command in [
            serde_json::json!({"expected_version":1,"settings":settings("push")}),
            serde_json::json!({"expected_version":1,"settings":changed_path}),
            serde_json::json!({"expected_version":1,"settings":automatic}),
            serde_json::json!({"expected_version":1,"settings":settings("pull"),"token":"fictional-provider-contract-busy-rotation"}),
        ] {
            let (status, error) = f
                .call(
                    0,
                    0,
                    "PUT",
                    "",
                    Some(command),
                    Some(&Uuid::new_v4().to_string()),
                )
                .await;
            assert_eq!(status, StatusCode::CONFLICT, "{phase}");
            assert_eq!(error["error"]["message"], "sync_running");
            assert_eq!(f.saved(0).await, saved);
            assert!(f.encrypted(0).await == encrypted);
        }
        let (status, disabled) = f
            .call(
                0,
                0,
                "PUT",
                "",
                Some(serde_json::json!({"expected_version":1,"settings":settings("disabled")})),
                Some(&Uuid::new_v4().to_string()),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{phase}");
        assert_eq!(disabled["version"], 2);
        assert_eq!(disabled["settings"]["direction"], "disabled");
        assert!(f.encrypted(0).await == encrypted);
        let stopped = store.run(f.repositories[0], run_id).await.unwrap();
        assert!(stopped.cancel_requested);
        assert_eq!(stopped.connection_version, 1);
        assert_eq!(stopped.settings.direction, Direction::Pull);
        if let Some((snapshot, generation)) = claimed {
            assert_eq!(stopped.state, "stopping");
            assert!(stopped.finished_at.is_none());
            assert!(store.heartbeat(run_id, owner, generation).await.unwrap());
            assert!(
                !store
                    .allowed_to_apply(&snapshot, owner, generation)
                    .await
                    .unwrap()
            );
        } else {
            assert_eq!(stopped.state, "canceled");
            assert!(stopped.finished_at.is_some());
            assert!(store.claim(run_id, owner).await.unwrap().is_none());
        }
        assert_eq!(f.call(0, 0, "GET", "/runs", None, None).await.1["total"], 1);
        f.pool.close().await;
    }
}
