//! Central service scopes must survive Git's GET discovery and POST RPC mapping.
use super::*;

#[tokio::test]
async fn central_personal_tokens_preserve_git_scopes_on_bearer_and_basic() {
    const SELECTOR: &str =
        "git_platform_pat::central_personal_tokens_preserve_git_scopes_on_bearer_and_basic";
    const CHILD: &str = "FORGE_PLATFORM_PAT_SCOPE_TEST_CHILD";
    const SUBJECT: &str = "00000000-0000-4000-8000-000000000094";
    if std::env::var_os(CHILD).is_none() {
        let auth = axum::Router::new()
            .route("/oidc/jwks", axum::routing::get(|| async {
                axum::Json(serde_json::json!({"keys":[{
                    "kty":"EC","crv":"P-256","use":"sig","alg":"ES256","kid":"git-scope-fixture",
                    "x":"XWEILsoYXdKJKnoeOa43PCI4APkASpJhoE2lp5xsq6s",
                    "y":"SZrHPij6p6aveE0oBNfd4Vlg3eQyFSCV_2joRUzRLxk"}]}))
            }))
            .route("/auth/tokens/introspect", axum::routing::get(|headers: HeaderMap| async move {
                let token = headers["authorization"].to_str().unwrap();
                let scopes = match token {
                    "Bearer sdlc_pat_git_reader" => vec!["ci-cd:read"],
                    "Bearer sdlc_pat_git_writer" => vec!["ci-cd:write"],
                    "Bearer sdlc_pat_other_service" => vec!["wiki:read"],
                    "Bearer sdlc_pat_git_full" => vec!["ci-cd:read", "ci-cd:write"],
                    "Bearer sdlc_pat_empty" => vec![],
                    _ => return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({}))),
                };
                let subject = match token {
                    "Bearer sdlc_pat_other_service" => "00000000-0000-4000-8000-000000000095",
                    "Bearer sdlc_pat_empty" => "00000000-0000-4000-8000-000000000096",
                    _ => SUBJECT,
                };
                (StatusCode::OK, axum::Json(serde_json::json!({"sub":subject,
                    "email":"git-scope@example.invalid","display_name":"Git scope fixture","subject_role":"admin","scopes":scopes,
                    "observed_at":chrono::Utc::now(),"expires_at":chrono::Utc::now()+chrono::Duration::seconds(300)})))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}/oidc/jwks", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, auth).await.unwrap() });
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", SELECTOR, "--nocapture"])
                .env(CHILD, "1")
                .env("CICD_AUTH__CENTRAL_JWKS_URI", uri)
                .env(
                    "CICD_AUTH__CENTRAL_ISSUER",
                    "http://git-scope.example.invalid",
                )
                .env("NO_PROXY", "127.0.0.1,localhost")
                .env("no_proxy", "127.0.0.1,localhost")
                .kill_on_drop(true)
                .output(),
        )
        .await;
        server.abort();
        let output = output.expect("isolated Git scope case timed out").unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let pool = test_pool().await;
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    std::fs::create_dir(&work).unwrap();
    for args in [
        vec!["init", "--initial-branch=main"],
        vec!["config", "user.name", "QA"],
        vec!["config", "user.email", "qa@example.invalid"],
    ] {
        assert!(
            tokio::process::Command::new("git")
                .args(["-C"])
                .arg(&work)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .unwrap()
                .success()
        );
    }
    std::fs::write(work.join("README.md"), "Git scope fixture\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "fixture"]] {
        assert!(
            tokio::process::Command::new("git")
                .arg("-C")
                .arg(&work)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .unwrap()
                .success()
        );
    }
    let bare = root.path().join("scope-fixture.git");
    assert!(
        tokio::process::Command::new("git")
            .args(["clone", "--bare", "--no-hardlinks"])
            .arg(&work)
            .arg(&bare)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .unwrap()
            .success()
    );
    let user = Uuid::new_v4();
    let project = Uuid::new_v4();
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,'git-scope-fixture','developer',$2)")
        .bind(user).bind(SUBJECT).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO repositories(id,name,visibility) VALUES($1,'scope-fixture','private')",
    )
    .bind(repository)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'git-scope-fixture','https://fixture.invalid/repo',$2)")
        .bind(project).bind(repository).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')",
    )
    .bind(project)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some("git-scope-fixture-secret".into()));
    config.git.root = root.path().to_path_buf();
    let router = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    // The same developer membership permits writes. Only the credential's service
    // scope differs, so a 403 cannot be explained by an unrelated role failure.
    for (token, read, write) in [
        ("sdlc_pat_git_reader", true, false),
        ("sdlc_pat_git_writer", false, true),
        ("sdlc_pat_other_service", false, false),
        ("sdlc_pat_empty", false, false),
        ("sdlc_pat_git_full", true, true),
    ] {
        for basic in [false, true] {
            let authorization = if basic {
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(format!("fixture:{token}"))
                )
            } else {
                format!("Bearer {token}")
            };
            for (method, path, permitted) in [
                (
                    "GET",
                    "/git/scope-fixture.git/info/refs?service=git-upload-pack",
                    read,
                ),
                ("POST", "/git/scope-fixture.git/git-upload-pack", read),
                (
                    "GET",
                    "/git/scope-fixture.git/info/refs?service=git-receive-pack",
                    write,
                ),
                ("POST", "/git/scope-fixture.git/git-receive-pack", write),
            ] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(path)
                            .header("authorization", &authorization)
                            .header(
                                "content-type",
                                if path.contains("receive-pack") {
                                    "application/x-git-receive-pack-request"
                                } else {
                                    "application/x-git-upload-pack-request"
                                },
                            )
                            .body(if method == "POST" {
                                Body::from("0000")
                            } else {
                                Body::empty()
                            })
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let expected = if permitted {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                };
                assert_eq!(
                    response.status(),
                    expected,
                    "{token} basic={basic} {method} {path}"
                );
                let _ = to_bytes(response.into_body(), 65536).await.unwrap();
            }
        }
    }
    for (token, writable) in [("sdlc_pat_git_reader", false), ("sdlc_pat_git_full", true)] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/catalog/repositories/{repository}/sync"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(value["writable"], writable);
    }
    // REST reads/writes use the same PAT service capabilities, even for an admin.
    for (token, read, write) in [
        ("sdlc_pat_git_reader", true, false),
        ("sdlc_pat_git_writer", false, true),
        ("sdlc_pat_other_service", false, false),
        ("sdlc_pat_empty", false, false),
        ("sdlc_pat_git_full", true, true),
    ] {
        for (method, allowed) in [("GET", read), ("POST", write)] {
            let response = router.clone().oneshot(Request::builder()
                .method(method).uri("/api/v1/projects")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(if method == "POST" { Body::from(serde_json::json!({
                    "name":format!("Scope-{token}"),"repository_url":"https://fixture.invalid/scope.git"
                }).to_string()) } else { Body::empty() }).unwrap()).await.unwrap();
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
                "REST {method} {token}"
            );
            let _ = to_bytes(response.into_body(), 65536).await.unwrap();
        }
    }
    let unrelated_users: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE central_sub IN ('00000000-0000-4000-8000-000000000095','00000000-0000-4000-8000-000000000096')",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(
        unrelated_users, 0,
        "unrelated/empty PATs must be rejected before JIT"
    );
    // Human login scopes describe the session, not personal-token capabilities.
    let mut verified = cicd::central_auth::try_verified_central("sdlc_pat_git_full")
        .await
        .unwrap()
        .unwrap();
    verified.context.token = "human.jwt.session".into();
    verified.context.scopes = ["auth".to_owned()].into_iter().collect();
    verified.context.session_id = Some(Uuid::new_v4().to_string());
    let human = cicd::central_auth::link_central_user(&pool, &verified)
        .await
        .unwrap();
    assert!(human.token_id.is_none());
    assert!(human.token_scopes.is_empty());
    pool.close().await;
}
