//! Disposable PostgreSQL, real HTTP authorization and Git; no live agent execution.
#![cfg(feature = "integration")]

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{borrow::Cow, future::IntoFuture, path::Path, process::Command, str::FromStr};
use tower::ServiceExt;
use uuid::Uuid;
#[path = "support/pull_fixture.rs"]
mod pull_fixture;

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "user.name=Reviewer fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

async fn call(
    app: &Router,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response.status(),
        to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
}

async fn ok(app: &Router, token: &str, method: &str, path: &str, body: Value) -> Value {
    let (status, bytes) = call(app, token, method, path, body).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{method} {path}: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

fn app(pool: sqlx::PgPool, root: &Path) -> Router {
    let config = cicd::config::RuntimeConfig::from_env_for_app()
        .unwrap()
        .with_auth_secret(Some("reviewer-disposable-test-secret".into()));
    cicd::api::app_with_git_and_config(
        Some(pool),
        cicd::git_host::GitConfig {
            root: root.into(),
            token: None,
            internal_token: None,
        },
        None,
        config,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reviewer_http_git_migration_paging_comments_and_restart() {
    let database =
        std::env::var("CICD_TEST_DATABASE_URL").expect("disposable Forge database required");
    let options = PgConnectOptions::from_str(&database).unwrap();
    assert!(options.get_database().unwrap().starts_with("forge_test_"));
    let admin = PgPoolOptions::new().connect(&database).await.unwrap();
    let schema = format!("reviewer_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options.options([("search_path", schema.as_str())]))
        .await
        .unwrap();
    let catalog = cicd::migrations().await.unwrap();
    sqlx::migrate::Migrator {
        migrations: Cow::Owned(
            catalog
                .iter()
                .filter(|m| m.version < 116)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("source");
    std::fs::create_dir(&work).unwrap();
    git(&work, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(work.join("deleted.txt"), "removed line\n").unwrap();
    std::fs::write(work.join("anchor-typechange.txt"), "one\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "--quiet", "-m", "base"]);
    let base = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["switch", "--quiet", "-c", "task"]);
    std::fs::remove_file(work.join("deleted.txt")).unwrap();
    std::fs::create_dir(work.join("dir")).unwrap();
    std::fs::write(work.join("dir/added.txt"), "new line\n").unwrap();
    std::fs::write(
        work.join("large.txt"),
        "review line with enough data\n".repeat(24000),
    )
    .unwrap();
    std::fs::write(work.join("anchor-typechange.txt"), "target").unwrap();
    git(&work, &["add", "."]);
    let link_blob = git(&work, &["rev-parse", ":anchor-typechange.txt"]);
    git(
        &work,
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{link_blob},anchor-typechange.txt"),
        ],
    );
    git(&work, &["commit", "--quiet", "-m", "change"]);
    let head = git(&work, &["rev-parse", "HEAD"]);
    git(
        &work,
        &[
            "clone",
            "--quiet",
            "--bare",
            ".",
            root.path().join("review.git").to_str().unwrap(),
        ],
    );
    let (repository, group, registry, namespace, tracker, task, user) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,'review-fixture','admin')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
        .bind(user)
        .bind(cicd::auth::hash_password("ReviewFixture1!").unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,'review-group','Review')")
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    let ns = json!({"registry_instance_id":registry,"namespace_id":namespace});
    let forge = Uuid::new_v4();
    let owner = json!({"schema_version":1,"namespace":ns,"resource":{"instance_id":forge,"kind":"git_group","resource_id":group},"operation_id":Uuid::new_v4(),"generation":1,"state":"active"});
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
        .bind(group).bind(registry).bind(namespace).bind(owner).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,'review')")
        .bind(repository)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(repository)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    // Pre-upgrade history: a unique verified link, an ambiguous pair and no link.
    for number in 1..=3 {
        let allocated: i32 = sqlx::query_scalar("SELECT allocate_repository_pr_number($1)")
            .bind(repository)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(allocated, number);
        let pull = Uuid::new_v4();
        sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,created_by) VALUES($1,'review',$2,'Historical PR','task','main','review-fixture')")
            .bind(pull).bind(number).execute(&pool).await.unwrap();
        for index in 0..(if number == 1 {
            1
        } else if number == 2 {
            2
        } else {
            0
        }) {
            let linked = if index == 0 { task } else { Uuid::new_v4() };
            sqlx::query("INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,task_key_snapshot,created_by_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,'RV-1',$8)")
                .bind(pull).bind(repository).bind(tracker).bind(linked).bind(registry).bind(namespace).bind(&head).bind(user).execute(&pool).await.unwrap();
        }
    }
    catalog.run(&pool).await.unwrap();
    catalog.run(&pool).await.unwrap();
    let mappings: Vec<Option<Uuid>> =
        sqlx::query_scalar("SELECT primary_task_id FROM pull_requests ORDER BY number")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(mappings, vec![Some(task), None, None]);
    assert!(sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch) VALUES($1,'review',4,'Missing Task','task','main')")
        .bind(Uuid::new_v4()).execute(&pool).await.is_err());

    let readback = json!({"schema_version":1,"namespace":ns,"tracker_instance_id":tracker,
        "task_id":task,"task_key":"RV-1","state":"active","project_id":Uuid::new_v4()});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new().route(
                "/api/v1/namespace-tasks/{id}",
                get(move || {
                    let readback = readback.clone();
                    async move { Json(readback) }
                }),
            ),
        )
        .into_future(),
    );
    let token_file = root.path().join("tracker-token");
    std::fs::write(&token_file, "fixture-reader").unwrap();
    // This integration binary has one test; no other test process shares these environment variables.
    unsafe {
        std::env::set_var("CICD_NAMESPACE__TRACKER_URL", format!("http://{address}"));
        std::env::set_var("CICD_NAMESPACE__TRACKER_INSTANCE_ID", tracker.to_string());
        std::env::set_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string());
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", forge.to_string());
        std::env::set_var("CICD_NAMESPACE__TRACKER_TOKEN_FILE", &token_file);
    }
    let router = app(pool.clone(), root.path());
    let login = ok(
        &router,
        "",
        "POST",
        "/api/v1/auth/login",
        json!({"username":"review-fixture","password":"ReviewFixture1!"}),
    )
    .await;
    let token = login["access_token"].as_str().unwrap();
    let lookup = ok(
        &router,
        token,
        "GET",
        &format!("/api/v1/repos/review/task?identifier={task}"),
        Value::Null,
    )
    .await;
    assert_eq!(lookup["task"]["task_id"], task.to_string());
    let create = json!({"title":"Task bound PR","source_branch":"task","target_branch":"main","primary_task":{"tracker_instance_id":tracker,"task_id":task}});
    let catalog_path = format!("/api/v1/catalog/repositories/{repository}/pulls");
    let missing = json!({"title":"Missing task","source_branch":"task","target_branch":"main"});
    assert_eq!(
        call(&router, token, "POST", &catalog_path, missing).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut foreign = create.clone();
    foreign["primary_task"]["task_id"] = json!(Uuid::new_v4());
    assert_eq!(
        call(&router, token, "POST", &catalog_path, foreign).await.0,
        StatusCode::BAD_REQUEST
    );
    let created = ok(&router, token, "POST", &catalog_path, create).await;
    assert_eq!(created["number"], 4);
    assert_eq!(created["primary_task_id"], task.to_string());
    let pull = format!("{catalog_path}/4");
    let context = ok(
        &router,
        token,
        "GET",
        &format!("{pull}/review-context"),
        Value::Null,
    )
    .await;
    assert_eq!(context["base_sha"], base);
    assert_eq!(context["head_sha"], head);
    let (status, patch) = call(
        &router,
        token,
        "GET",
        &format!("{pull}/patch?base_sha={base}&head_sha={head}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(patch.len() > 512 * 1024);
    assert!(
        String::from_utf8(patch)
            .unwrap()
            .ends_with("+review line with enough data\n")
    );
    // A type change produces two diff blocks for one literal path. Headers
    // between the blocks must never become anchors outside the actual blob.
    let invalid_type_change = json!({"operation_id":Uuid::new_v4(),"base_sha":base,"head_sha":head,
        "body":"Nonexistent old line","severity":"P1","anchor":{"path":"anchor-typechange.txt","side":"old","line":2}});
    assert_eq!(
        call(
            &router,
            token,
            "POST",
            &format!("{pull}/comments"),
            invalid_type_change
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut comment = json!({"operation_id":Uuid::new_v4(),"base_sha":base,"head_sha":head,"body":"Deleted line finding","severity":"P1","anchor":{"path":"deleted.txt","side":"old","line":1}});
    let saved = ok(
        &router,
        token,
        "POST",
        &format!("{pull}/comments"),
        comment.clone(),
    )
    .await;
    // Exercise the public endpoints over TCP as well as through the Router.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let http = tokio::spawn(axum::serve(listener, router.clone()).into_future());
    for path in [
        format!("{pull}/review-context"),
        format!("{pull}/comments"),
        format!("{pull}/checks?head_sha={head}"),
        format!("/api/v1/task-pulls?tracker_instance_id={tracker}&task_id={task}"),
        format!("{pull}/patch?base_sha={base}&head_sha={head}"),
    ] {
        let response = tokio::process::Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail-with-body",
                "--max-time",
                "20",
                "--header",
                &format!("Authorization: Bearer {token}"),
                &format!("http://{address}{path}"),
            ])
            .output()
            .await
            .unwrap();
        assert!(
            response.status.success(),
            "{}",
            String::from_utf8_lossy(&response.stderr)
        );
        if path.contains("/patch?") {
            assert!(response.stdout.len() > 512 * 1024);
            assert!(
                response
                    .stdout
                    .ends_with(b"+review line with enough data\n")
            );
        } else {
            let _: Value = serde_json::from_slice(&response.stdout).unwrap();
        }
    }
    let replay = tokio::process::Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--max-time",
            "20",
            "--header",
            &format!("Authorization: Bearer {token}"),
            "--header",
            "Content-Type: application/json",
            "--data-binary",
            &comment.to_string(),
            &format!("http://{address}{pull}/comments"),
        ])
        .output()
        .await
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.stdout).unwrap()["id"],
        saved["id"]
    );
    http.abort();
    assert_eq!(saved["author_id"], user.to_string());
    assert_eq!(
        ok(
            &router,
            token,
            "POST",
            &format!("{pull}/comments"),
            comment.clone()
        )
        .await["id"],
        saved["id"]
    );
    let mut different = comment.clone();
    different["body"] = json!("Changed payload");
    assert_eq!(
        call(
            &router,
            token,
            "POST",
            &format!("{pull}/comments"),
            different
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    comment["operation_id"] = json!(Uuid::new_v4());
    comment["anchor"]["side"] = json!("new");
    assert_eq!(
        call(
            &router,
            token,
            "POST",
            &format!("{pull}/comments"),
            comment.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    comment["operation_id"] = json!(Uuid::new_v4());
    comment["anchor"] = json!({"path":"dir","side":"new","line":1});
    assert_eq!(
        call(
            &router,
            token,
            "POST",
            &format!("{pull}/comments"),
            comment.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    comment["operation_id"] = json!(Uuid::new_v4());
    comment["anchor"] = json!({"path":"dir/added.txt","side":"new","line":1});
    ok(
        &router,
        token,
        "POST",
        &format!("{pull}/comments"),
        comment.clone(),
    )
    .await;
    comment["operation_id"] = json!(Uuid::new_v4());
    comment["anchor"] = Value::Null;
    ok(&router, token, "POST", &format!("{pull}/comments"), comment).await;
    // Restart the HTTP application after a head change; old operation/anchors remain exact.
    git(&work, &["commit", "--allow-empty", "--quiet", "-m", "next"]);
    let next = git(&work, &["rev-parse", "HEAD"]);
    git(
        &root.path().join("review.git"),
        &["fetch", work.to_str().unwrap(), "task:task"],
    );
    let restarted = app(pool.clone(), root.path());
    let replay = json!({"operation_id":saved["operation_id"],"base_sha":base,"head_sha":head,"body":"Deleted line finding","severity":"P1","anchor":{"path":"deleted.txt","side":"old","line":1}});
    assert_eq!(
        ok(
            &restarted,
            token,
            "POST",
            &format!("{pull}/comments"),
            replay.clone()
        )
        .await["id"],
        saved["id"]
    );
    let mut stale = replay;
    stale["operation_id"] = json!(Uuid::new_v4());
    assert_eq!(
        call(
            &restarted,
            token,
            "POST",
            &format!("{pull}/comments"),
            stale
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let history = ok(
        &restarted,
        token,
        "GET",
        &format!("{pull}/comments"),
        Value::Null,
    )
    .await;
    assert_eq!(history.as_array().unwrap().len(), 3);
    assert_eq!(history[0]["head_sha"], head);
    assert_eq!(
        ok(
            &restarted,
            token,
            "GET",
            &format!("{pull}/review-context"),
            Value::Null
        )
        .await["head_sha"],
        next
    );
    let configuration = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Review checks','https://example.invalid/review.git',$2)").bind(configuration).bind(repository).execute(&pool).await.unwrap();
    for sha in [&head, &next] {
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'task',$3,'success')").bind(Uuid::new_v4()).bind(configuration).bind(sha).execute(&pool).await.unwrap();
    }
    let checks = ok(
        &restarted,
        token,
        "GET",
        &format!("{pull}/checks?head_sha={next}"),
        Value::Null,
    )
    .await;
    assert_eq!(checks.as_array().unwrap().len(), 1);
    assert_eq!(checks[0]["commit_sha"], next);
    // More than one page, including closed PRs; extra task links cannot duplicate a PR.
    for number in 5..=109 {
        let id = Uuid::now_v7();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,status,created_by,primary_tracker_instance_id,primary_task_id) VALUES($1,'review',$2,'Paged','task','main',$3,'review-fixture',$4,$5)")
            .bind(id).bind(number).bind(if number % 2 == 0 {"closed"} else {"open"}).bind(tracker).bind(task).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,task_key_snapshot,created_by_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,'RV-1',$8)")
            .bind(id).bind(repository).bind(tracker).bind(task).bind(registry).bind(namespace).bind(&next).bind(user).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let query =
        format!("/api/v1/task-pulls?tracker_instance_id={tracker}&task_id={task}&limit=100");
    let first = ok(&restarted, token, "GET", &query, Value::Null).await;
    let second = ok(
        &restarted,
        token,
        "GET",
        &format!("{query}&offset=100"),
        Value::Null,
    )
    .await;
    assert_eq!(first["total"], 108);
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    assert_eq!(second["items"].as_array().unwrap().len(), 8);
    let ids: std::collections::BTreeSet<_> = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["items"].as_array().unwrap())
        .map(|pr| pr["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 108);
    // Selecting a confirmed primary on ambiguous history is explicit and immutable.
    git(
        &root.path().join("review.git"),
        &["update-ref", "-d", "refs/heads/task"],
    );
    server.abort();
    let mapping = json!({"tracker_instance_id":tracker,"task_id":task,"primary":true});
    let path = format!("{catalog_path}/2/tasks");
    ok(&restarted, token, "POST", &path, mapping.clone()).await;
    ok(&restarted, token, "POST", &path, mapping).await;
    assert_eq!(
        ok(
            &restarted,
            token,
            "GET",
            &format!("{catalog_path}/2"),
            Value::Null
        )
        .await["primary_task_id"],
        task.to_string()
    );
    assert!(
        sqlx::query("UPDATE pull_comments SET body='replacement'")
            .execute(&pool)
            .await
            .is_err()
    );
    server.abort();
    drop(restarted);
    drop(router);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reviewer_native_commands_reuse_project_membership_with_active_space_policy() {
    const CHILD: &str = "REVIEWER_NATIVE_TEST_CHILD";
    const SUBJECT: &str = "00000000-0000-4000-8000-000000000094";
    const TOKEN: &str = "sdlc_pat_reviewer_native_fixture_12345678901234567890";
    if std::env::var_os(CHILD).is_none() {
        let auth = Router::new()
            .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
            .route(
                "/auth/tokens/introspect",
                get(|| async {
                    Json(json!({"sub":SUBJECT,"subject_role":"service_account",
                    "email":"reviewer@example.invalid","display_name":"Reviewer fixture",
                    "scopes":["ci-cd:read","ci-cd:write"],"observed_at":chrono::Utc::now(),
                    "expires_at":chrono::Utc::now()+chrono::Duration::seconds(300)}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}/oidc/jwks", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, auth).await.unwrap() });
        let result = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "reviewer_native_commands_reuse_project_membership_with_active_space_policy",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("CICD_AUTH__CENTRAL_JWKS_URI", uri)
            .env(
                "CICD_AUTH__CENTRAL_ISSUER",
                "http://reviewer-auth.example.invalid",
            )
            .env("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_REQUIRED", "false")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        server.abort();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let database = std::env::var("CICD_TEST_DATABASE_URL").unwrap();
    let options = PgConnectOptions::from_str(&database).unwrap();
    assert!(options.get_database().unwrap().starts_with("forge_test_"));
    let admin = PgPoolOptions::new().connect(&database).await.unwrap();
    let schema = format!("reviewer_native_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options.options([("search_path", schema.as_str())]))
        .await
        .unwrap();
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("source");
    std::fs::create_dir(&work).unwrap();
    git(&work, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(work.join("data.txt"), "before\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "--quiet", "-m", "base"]);
    git(&work, &["switch", "--quiet", "-c", "task"]);
    std::fs::write(work.join("data.txt"), "after\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "--quiet", "-m", "change"]);
    git(
        &work,
        &[
            "clone",
            "--quiet",
            "--bare",
            ".",
            root.path().join("native.git").to_str().unwrap(),
        ],
    );
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let actor = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,'native')")
        .bind(repository)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Native Review','https://example.invalid/native.git',$2)").bind(project).bind(repository).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,'native-reviewer','service_account',$2)").bind(actor).bind(SUBJECT).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'viewer')")
        .bind(project)
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    let pull = pull_fixture::insert(
        &pool,
        "native",
        1,
        "Review",
        ("task", "main"),
        "open",
        "fixture",
    )
    .await;
    let (tracker, task): (Uuid, Uuid) = sqlx::query_as(
        "SELECT primary_tracker_instance_id,primary_task_id FROM pull_requests WHERE id=$1",
    )
    .bind(pull)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE forge_space_admission_state SET active=true WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    let router = app(pool.clone(), root.path());
    let prefix = format!("/api/v1/catalog/repositories/{repository}/pulls/1");
    let context = ok(
        &router,
        TOKEN,
        "GET",
        &format!("{prefix}/review-context"),
        Value::Null,
    )
    .await;
    // Ready CI was produced by another role, without this Reviewer's workspace.
    let pipeline = Uuid::new_v4();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'task',$3,'success')")
        .bind(pipeline).bind(project).bind(context["head_sha"].as_str().unwrap()).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'test',0,'success')",
    )
    .bind(stage)
    .bind(pipeline)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'test','fixture','true',0,'success')")
        .bind(job).bind(stage).execute(&pool).await.unwrap();
    let attempt = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'success')",
    )
    .bind(attempt)
    .bind(job)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO job_logs(job_id,attempt_id,sequence,message) VALUES($1,$2,1,'Ready test result')")
        .bind(job)
        .bind(attempt)
        .execute(&pool)
        .await
        .unwrap();
    let ci_paths = [
        format!("/api/v1/pipelines/{pipeline}"),
        format!("/api/v1/jobs/{job}/attempts"),
        format!("/api/v1/jobs/{job}/logs/page?limit=100"),
    ];
    for path in &ci_paths {
        ok(&router, TOKEN, "GET", path, Value::Null).await;
    }
    let checks = ok(
        &router,
        TOKEN,
        "GET",
        &format!(
            "{prefix}/checks?head_sha={}",
            context["head_sha"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await;
    assert_eq!(checks[0]["pipeline_id"], pipeline.to_string());
    let other_project = Uuid::new_v4();
    let other_pipeline = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,'Other CI','https://example.invalid/other.git')")
        .bind(other_project).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','success')",
    )
    .bind(other_pipeline)
    .bind(other_project)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        call(
            &router,
            TOKEN,
            "GET",
            &format!("/api/v1/pipelines/{other_pipeline}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let task_path = format!("/api/v1/task-pulls?tracker_instance_id={tracker}&task_id={task}");
    assert_eq!(
        ok(&router, TOKEN, "GET", &task_path, Value::Null).await["total"],
        1
    );
    ok(
        &router,
        TOKEN,
        "GET",
        "/api/v1/repos/native/tree?ref=task",
        Value::Null,
    )
    .await;
    let input = json!({"operation_id":Uuid::new_v4(),"base_sha":context["base_sha"],"head_sha":context["head_sha"],"severity":"P1","body":"Native finding","anchor":null});
    assert_eq!(
        call(
            &router,
            TOKEN,
            "POST",
            &format!("{prefix}/comments"),
            input.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query(
        "UPDATE project_memberships SET role='developer' WHERE project_id=$1 AND user_id=$2",
    )
    .bind(project)
    .bind(actor)
    .execute(&pool)
    .await
    .unwrap();
    let saved = ok(
        &router,
        TOKEN,
        "POST",
        &format!("{prefix}/comments"),
        input.clone(),
    )
    .await;
    assert_eq!(saved["author_id"], actor.to_string());
    assert_eq!(
        ok(
            &router,
            TOKEN,
            "POST",
            &format!("{prefix}/comments"),
            input.clone()
        )
        .await["id"],
        saved["id"]
    );
    for path in [
        format!("{prefix}/action"),
        format!("/api/v1/projects/{project}/pipelines"),
    ] {
        assert_eq!(
            call(&router, TOKEN, "POST", &path, json!({"action":"merge"}))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("DELETE FROM project_memberships WHERE project_id=$1 AND user_id=$2")
        .bind(project)
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    for path in &ci_paths {
        assert_eq!(
            call(&router, TOKEN, "GET", path, Value::Null).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        call(&router, TOKEN, "POST", &format!("{prefix}/comments"), input)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&router, TOKEN, "GET", &task_path, Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &router,
            TOKEN,
            "GET",
            &format!("{prefix}/comments"),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    drop(router);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
