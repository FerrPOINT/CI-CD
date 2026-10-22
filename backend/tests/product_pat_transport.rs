//! The real router, owner-reader HTTP client, migrated PG and native clone/push.
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::json;
use sqlx::PgPool;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone)]
struct Owner {
    subject: Uuid,
    issuer: String,
    role: String,
    applications: Vec<String>,
    unavailable: bool,
    expires_ms: i64,
    spaces: Vec<pdlc_shared::space_access::SpaceRef>,
}
async fn decision(
    State(state): State<Arc<RwLock<Owner>>>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> axum::response::Response {
    assert_eq!(
        headers["authorization"],
        "Bearer fixture-restricted-owner-reader"
    );
    let state = state.read().unwrap().clone();
    assert_eq!(
        request,
        json!({"schema_version":1,"subjects":[state.subject]})
    );
    if state.unavailable {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let now = chrono::Utc::now();
    Json(json!({"schema_version":1,"decisions":[{
        "schema_version":1,"issuer":state.issuer,"subject":state.subject,"role":state.role,
        "applications":state.applications,"revision":1,"enforced":true,
        "observed_at":now,"expires_at":now+chrono::Duration::milliseconds(state.expires_ms)
    }]}))
    .into_response()
}
async fn space_decision(
    State(state): State<Arc<RwLock<Owner>>>,
    headers: HeaderMap,
    Json(request): Json<pdlc_shared::space_access::SpaceAdmissionRequest>,
) -> axum::response::Response {
    use pdlc_shared::space_access::*;
    assert_eq!(headers["authorization"], "Bearer fixture-space-reader");
    let state = state.read().unwrap().clone();
    assert!(request.valid_for(&state.issuer));
    assert_eq!(request.identity.subject, state.subject.to_string());
    let now = chrono::Utc::now().timestamp();
    Json(SpaceAdmission {
        schema_version: 1,
        identity: request.identity,
        identity_active: true,
        policy_revision: 1,
        policy: SpaceAccessPolicy::Selected {
            spaces: state.spaces,
        },
        inheritance: None,
        validity: AdmissionValidity {
            observed_at: now,
            valid_until: now + 30,
        },
        spaces: request
            .spaces
            .into_iter()
            .map(|space| ConfirmedSpace {
                space,
                revision: 1,
                state: SpaceState::Active,
            })
            .collect(),
    })
    .into_response()
}
async fn git(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new("git")
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .await
        .unwrap()
}
async fn git_ok(root: &std::path::Path, args: &[&str]) {
    let output = git(root, args).await;
    assert!(
        output.status.success(),
        "Git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[tokio::test]
#[ignore = "requires owned FORGE_PRODUCT_TRANSPORT_TEST_DATABASE_URL and isolated process"]
async fn current_owner_and_product_scopes_govern_real_git_and_direct_api() {
    let url = std::env::var("FORGE_PRODUCT_TRANSPORT_TEST_DATABASE_URL").unwrap();
    let pool = PgPool::connect(&url).await.unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        name, "forge_product_transport_test",
        "refuse working database"
    );
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    let local = Uuid::new_v4();
    let subject = Uuid::new_v4();
    let project = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let token = "cicd_native_owned_fixture";
    sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'fixture','admin',true,$2)")
        .bind(local).bind(subject.to_string()).execute(&pool).await.unwrap();
    let root = std::env::temp_dir().join(format!("forge-product-transport-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let config = cicd::git_host::GitConfig {
        root: root.join("repositories"),
        token: None,
        internal_token: None,
    };
    let bare = config.root.join("fixture.git");
    let repo = cicd::git_host::create_repository_core(&pool, &config, "fixture", Some("private"))
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'fixture','https://fixture.invalid/repo',$2)")
        .bind(project).bind(repo.id).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')",
    )
    .bind(project)
    .bind(local)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,project_id,scopes)
        VALUES($1,'fixture',$2,'fixture',$3,$4,ARRAY['git:read','git:write'])",
    )
    .bind(token_id)
    .bind(cicd::auth::hash_token(token))
    .bind(local)
    .bind(project)
    .execute(&pool)
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let owner = Arc::new(RwLock::new(Owner {
        subject,
        issuer: issuer.clone(),
        role: "user".into(),
        applications: vec!["ci-cd".into()],
        unavailable: false,
        expires_ms: 30000,
        spaces: Vec::new(),
    }));
    let reader = Router::new()
        .route("/auth/access/decisions", post(decision))
        .route("/admission", post(space_decision))
        .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
        .with_state(owner.clone());
    let registry = Uuid::new_v4();
    let instance = Uuid::new_v4();
    // This isolated test process initializes its configuration before any reader starts.
    unsafe {
        std::env::set_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string());
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
        std::env::set_var("CICD_AUTH__CENTRAL_ISSUER", &issuer);
        std::env::set_var("CICD_AUTH__CENTRAL_JWKS_URI", format!("{issuer}/oidc/jwks"));
        std::env::set_var("CICD_SPACES__ADMISSION_URL", format!("{issuer}/admission"));
        std::env::set_var("CICD_SPACES__ADMISSION_TOKEN", "fixture-space-reader");
        std::env::set_var("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_REQUIRED", "true");
        std::env::set_var(
            "CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN",
            "fixture-restricted-owner-reader",
        );
    }
    let reader_task = tokio::spawn(async move { axum::serve(listener, reader).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        config,
        Some("isolated-fixture-secret".into()),
    );
    let app_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let endpoint = format!("{origin}/git/fixture/info/refs?service=git-upload-pack");
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    // A git-only credential does not acquire API scopes through the owner reader.
    assert_eq!(
        http.get(format!("{origin}/api/v1/projects"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let header = format!("http.extraHeader=Authorization: Bearer {token}");
    let remote = format!("{origin}/git/fixture");
    git_ok(&root, &["-c", &header, "clone", &remote, "working"]).await;
    let work = root.join("working");
    git_ok(&work, &["config", "user.name", "Fixture"]).await;
    git_ok(&work, &["config", "user.email", "fixture@example.test"]).await;
    tokio::fs::write(work.join("proof.txt"), "owned fixture\n")
        .await
        .unwrap();
    git_ok(&work, &["add", "proof.txt"]).await;
    git_ok(&work, &["commit", "-m", "Owned fixture"]).await;
    git_ok(
        &work,
        &["-c", &header, "push", "origin", "HEAD:refs/heads/fixture"],
    )
    .await;
    let original_ref = git(&work, &["rev-parse", "HEAD"]).await.stdout;
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['git:read'] WHERE id=$1")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    tokio::fs::write(work.join("proof.txt"), "changed fixture\n")
        .await
        .unwrap();
    git_ok(&work, &["commit", "-am", "Next fixture"]).await;
    assert!(
        !git(
            &work,
            &["-c", &header, "push", "origin", "HEAD:refs/heads/fixture"]
        )
        .await
        .status
        .success()
    );
    // Independent app-policy protection: no namespace frame may be assumed.
    let spaces_active: bool =
        sqlx::query_scalar("SELECT active FROM forge_space_admission_state WHERE singleton")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!spaces_active);
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['api:read','api:write','git:read','git:write'] WHERE id=$1")
        .bind(token_id).execute(&pool).await.unwrap();
    git_ok(
        &work,
        &["-c", &header, "push", "origin", "HEAD:refs/heads/source"],
    )
    .await;
    let collection = format!("{origin}/api/v1/catalog/repositories/{}/pulls", repo.id);
    let input = json!({"title":"Application clock fixture","source_branch":"source","target_branch":"fixture"});
    let response = http
        .post(&collection)
        .bearer_auth(token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let review: serde_json::Value = response.json().await.unwrap();
    let action = format!("{collection}/{}/action", review["number"].as_i64().unwrap());
    let hook = bare.join("hooks/reference-transaction");
    let original_hook = tokio::fs::read(&hook).await.ok();
    let hook_script = format!(
        "#!/bin/sh\nset -eu\ncat >/dev/null\nif test \"$1\" = prepared; then\ntouch '{}'\nsleep 4\ntouch '{}'\nfi\n",
        root.join("prepared").display(),
        root.join("late-effect").display()
    );
    tokio::fs::write(&hook, hook_script).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
    }
    owner.write().unwrap().expires_ms = 2500;
    let result = http
        .post(&action)
        .bearer_auth(token)
        .json(&json!({"action":"merge"}))
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        root.join("prepared").exists(),
        "real ref preparation reached with app policy alone"
    );
    assert_eq!(
        git(
            &root,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                "refs/heads/fixture"
            ]
        )
        .await
        .stdout,
        original_ref
    );
    tokio::time::sleep(std::time::Duration::from_millis(4300)).await;
    assert!(
        !root.join("late-effect").exists(),
        "native hook cannot outlive original application clock"
    );
    owner.write().unwrap().expires_ms = 30000;
    assert_eq!(
        http.post(&action)
            .bearer_auth(token)
            .json(&json!({"action":"merge"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "unknown original preparation is reconciled, never blindly republished"
    );
    if let Some(bytes) = original_hook {
        tokio::fs::write(&hook, bytes).await.unwrap();
    } else {
        tokio::fs::remove_file(&hook).await.unwrap();
    }
    // A separate legitimate review exercises the real deferred application COMMIT fence.
    let response = http
        .post(&collection)
        .bearer_auth(token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let next: serde_json::Value = response.json().await.unwrap();
    let next_action = format!("{collection}/{}/action", next["number"].as_i64().unwrap());
    sqlx::raw_sql(
        "CREATE SEQUENCE qa_commit_seen;
        CREATE FUNCTION qa_commit_delay() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM nextval('qa_commit_seen'); PERFORM pg_sleep(2); RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER a_qa_commit_delay AFTER INSERT ON forge_request_commit_fences
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_commit_delay();",
    )
    .execute(&pool)
    .await
    .unwrap();
    owner.write().unwrap().expires_ms = 1500;
    assert_eq!(
        http.post(&next_action)
            .bearer_auth(token)
            .json(&json!({"action":"merge"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let seen: bool = sqlx::query_scalar("SELECT is_called FROM qa_commit_seen")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(seen, "real deferred COMMIT was entered");
    assert_eq!(
        git(
            &root,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                "refs/heads/fixture"
            ]
        )
        .await
        .stdout,
        original_ref
    );
    let status: String = sqlx::query_scalar("SELECT status FROM pull_requests WHERE id=$1")
        .bind(Uuid::parse_str(next["id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        status, "open",
        "expired application clock cannot commit a merged PR"
    );
    sqlx::raw_sql(
        "DROP TRIGGER a_qa_commit_delay ON forge_request_commit_fences;
        DROP FUNCTION qa_commit_delay(); DROP SEQUENCE qa_commit_seen;",
    )
    .execute(&pool)
    .await
    .unwrap();
    owner.write().unwrap().expires_ms = 30000;
    // A project-pinned token remains pinned even for a current central admin.
    let global_token = "cicd_native_global_fixture";
    sqlx::query(
        "INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,scopes)
        VALUES($1,'global-fixture',$2,'fixture',$3,ARRAY['api:read','api:write'])",
    )
    .bind(Uuid::new_v4())
    .bind(cicd::auth::hash_token(global_token))
    .bind(local)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        http.get(format!("{origin}/api/v1/users"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "old local admin cannot manage users under current owner=user"
    );
    // Reading build evidence never grants the right to manufacture runner output.
    for resource in ["logs", "test-report", "artifacts"] {
        assert_eq!(
            http.post(format!(
                "{origin}/api/v1/jobs/{}/{resource}",
                Uuid::new_v4()
            ))
            .bearer_auth(global_token)
            .json(&json!({"message":"untrusted evidence"}))
            .send()
            .await
            .unwrap()
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    owner.write().unwrap().role = "admin".into();
    assert_eq!(
        http.get(format!("{origin}/api/v1/runners"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "current admin does not remove the original project pin"
    );
    assert_eq!(
        http.get(format!("{origin}/api/v1/runners"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        http.get(format!("{origin}/api/v1/users"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "admin role does not change a product credential audience"
    );
    owner.write().unwrap().role = "user".into();
    assert_eq!(
        http.get(format!("{origin}/api/v1/runners"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http.get(format!("{origin}/api/v1/users"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "a product credential is not forwarded to the Auth directory"
    );
    // Compose independent owner-app and space policies using the same stored PAT.
    // These are owned fixture projections, never writes to an installed catalog.
    use pdlc_shared::space_access::SpaceRef;
    let visible = SpaceRef {
        registry_instance_id: registry,
        space_id: Uuid::new_v4(),
    };
    let hidden = SpaceRef {
        registry_instance_id: registry,
        space_id: Uuid::new_v4(),
    };
    owner.write().unwrap().spaces = vec![visible.clone()];
    let hidden_repo = cicd::git_host::create_repository_core(
        &pool,
        &cicd::git_host::GitConfig {
            root: root.join("repositories"),
            token: None,
            internal_token: None,
        },
        "hidden",
        Some("private"),
    )
    .await
    .unwrap();
    for (repository, space) in [(repo.id, visible), (hidden_repo.id, hidden)] {
        let group = Uuid::new_v4();
        sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Owned fixture space')")
            .bind(group)
            .bind(format!("fixture-{}", group.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE repository_catalog SET group_id=$1 WHERE id=$2")
            .bind(group)
            .bind(repository)
            .execute(&pool)
            .await
            .unwrap();
        let binding = json!({"schema_version":1,"operation_id":Uuid::new_v4(),
            "generation":1,"state":"active",
            "namespace":{"registry_instance_id":registry,"namespace_id":space.space_id},
            "resource":{"resource_id":group,"instance_id":instance,"kind":"git_group"}});
        sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
            .bind(group).bind(registry).bind(space.space_id).bind(binding).execute(&pool).await.unwrap();
        let operation = Uuid::new_v4();
        let command =
            json!({"schema_version":2,"space":space,"operation_id":operation,"revision":1});
        sqlx::query("INSERT INTO forge_space_catalog(registry_instance_id,space_id,group_id,revision,state,operation_id,command) VALUES($1,$2,$3,1,'active',$4,$5)")
            .bind(registry).bind(space.space_id).bind(group).bind(operation).bind(command).execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE forge_space_admission_state SET active=true WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['api:read','api:write','git:read','git:write'] WHERE token_hash=$1")
        .bind(cicd::auth::hash_token(global_token)).execute(&pool).await.unwrap();
    for (path, credential, expected) in [
        (
            "/git/fixture/info/refs?service=git-upload-pack".to_string(),
            token,
            StatusCode::OK,
        ),
        (
            "/git/hidden/info/refs?service=git-upload-pack".to_string(),
            global_token,
            StatusCode::FORBIDDEN,
        ),
        (
            format!("/api/v1/catalog/repositories/{}/pulls", repo.id),
            token,
            StatusCode::OK,
        ),
        (
            format!("/api/v1/catalog/repositories/{}/pulls", hidden_repo.id),
            global_token,
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = http
            .get(format!("{origin}{path}"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert_eq!(status, expected, "product owner +spaces {path}: {body}");
    }
    let response = http
        .get(format!("{origin}/api/v1/catalog/repositories?limit=1"))
        .bearer_auth(global_token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: serde_json::Value = response.json().await.unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], repo.id.to_string());
    git_ok(
        &work,
        &[
            "-c",
            &header,
            "push",
            "origin",
            "HEAD:refs/heads/product-with-spaces",
        ],
    )
    .await;
    let response = http
        .post(&collection)
        .bearer_auth(token)
        .json(&input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "API-scoped PAT also retains space write fence: {body}"
    );
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['git:read','git:write'] WHERE id=$1")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    owner.write().unwrap().applications.clear();
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(
        !git(&work, &["-c", &header, "fetch", "origin"])
            .await
            .status
            .success()
    );
    assert!(
        !git(
            &work,
            &["-c", &header, "push", "origin", "HEAD:refs/heads/fixture"]
        )
        .await
        .status
        .success(),
        "an application revoke also rejects a write-scoped product PAT"
    );
    let current_ref = git(
        &root,
        &[
            "--git-dir",
            bare.to_str().unwrap(),
            "rev-parse",
            "refs/heads/fixture",
        ],
    )
    .await
    .stdout;
    assert_eq!(
        current_ref, original_ref,
        "denied pushes cannot change the native ref"
    );
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    owner.write().unwrap().unavailable = true;
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    app_task.abort();
    reader_task.abort();
    pool.close().await;
}
