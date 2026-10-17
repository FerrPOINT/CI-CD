//! Real HTTP/Git regression; no Docker, pipeline dispatch or credential protocol.
use super::*;
use std::{fs, path::Path, process::Command};

struct Directory(std::path::PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn git(directory: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .unwrap()
}

async fn discovery(app: &axum::Router, name: &str, token: &str, service: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::get(format!("/git/{name}.git/info/refs?service={service}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn push(source: &Path, url: &str, token: &str, refs: &[&str]) -> std::process::Output {
    tokio::process::Command::new("git")
        .current_dir(source)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {token}"),
            "push",
            url,
        ])
        .args(refs)
        .output()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_sat_project_scope_and_native_receive_protect_refs_without_bypassing_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let pool = test_pool().await;
    let root = std::env::temp_dir().join(format!("forge-sat-git-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let _files = Directory(root.clone());
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    assert!(
        git(&source, &["init", "--initial-branch=main"])
            .status
            .success()
    );
    assert!(
        git(
            &source,
            &["config", "user.email", "fixture@example.invalid"]
        )
        .status
        .success()
    );
    assert!(
        git(&source, &["config", "user.name", "Fixture"])
            .status
            .success()
    );
    fs::write(source.join("data"), "first\n").unwrap();
    assert!(git(&source, &["add", "data"]).status.success());
    assert!(git(&source, &["commit", "-m", "first"]).status.success());
    let name = format!("sat-{}", Uuid::new_v4().simple());
    let bare = root.join(format!("{name}.git"));
    assert!(
        git(&source, &["clone", "--bare", ".", bare.to_str().unwrap()])
            .status
            .success()
    );
    let hook_dir = root.join("custom-hooks");
    fs::create_dir(&hook_dir).unwrap();
    fs::write(
        hook_dir.join("post-receive"),
        "#!/bin/sh\ncat >/dev/null\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(
        hook_dir.join("post-receive"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(
        git(
            &source,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "config",
                "core.hooksPath",
                hook_dir.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch,protected_branches) VALUES($1,'SAT Git','https://example.invalid/unused.git',$2,'main',ARRAY['release']),($3,'Foreign','https://example.invalid/foreign.git',NULL,'main',ARRAY['main'])")
        .bind(project).bind(repository).bind(foreign).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'git-fixture')")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    let mut tokens = Vec::new();
    for (bound, scopes) in [
        (Some(project), vec!["git:read", "git:write"]),
        (Some(foreign), vec!["git:read", "git:write"]),
        (Some(project), vec!["git:read"]),
        (Some(project), vec!["api:read", "api:write"]),
        (None, vec!["git:read", "git:write"]),
    ] {
        let token = format!(
            "forge_sat_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes,expires_at) VALUES($1,'git-fixture',$2,'synthetic-only','service_account',$3,$4,$5,now()+interval '1 day')")
            .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&token)).bind(account).bind(bound).bind(scopes).execute(&pool).await.unwrap();
        tokens.push(token);
    }
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.clone(),
            token: None,
            internal_token: None,
        },
        Some(format!("synthetic-secret-{account}")),
    );
    assert_eq!(
        discovery(&app, &name, &tokens[0], "git-upload-pack").await,
        StatusCode::OK
    );
    assert_eq!(
        discovery(&app, &name, &tokens[0], "git-receive-pack").await,
        StatusCode::OK
    );
    assert_eq!(
        discovery(&app, &name, &tokens[2], "git-upload-pack").await,
        StatusCode::OK
    );
    for index in [1, 3, 4] {
        assert_eq!(
            discovery(&app, &name, &tokens[index], "git-upload-pack").await,
            StatusCode::FORBIDDEN
        );
    }
    for index in [1, 2, 3, 4] {
        assert_eq!(
            discovery(&app, &name, &tokens[index], "git-receive-pack").await,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE service_accounts SET enabled=false WHERE id=$1")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        discovery(&app, &name, &tokens[0], "git-receive-pack").await,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE service_accounts SET enabled=true WHERE id=$1")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();
    let empty_name = format!("empty-{}", Uuid::new_v4().simple());
    let empty_bare = root.join(format!("{empty_name}.git"));
    assert!(
        git(
            &source,
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch=main",
                empty_bare.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    let empty_repository = Uuid::new_v4();
    let empty_project = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(empty_repository)
        .bind(&empty_name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch,protected_branches) VALUES($1,'Empty repository','https://example.invalid/empty.git',$2,'main',ARRAY['release'])")
        .bind(empty_project).bind(empty_repository).execute(&pool).await.unwrap();
    let empty_sat = format!(
        "forge_sat_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes) VALUES($1,'empty-sat',$2,'synthetic-only','service_account',$3,$4,ARRAY['git:read','git:write'])")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&empty_sat)).bind(account).bind(empty_project).execute(&pool).await.unwrap();
    let mut human_tokens = Vec::new();
    for (role, bound, scopes) in [
        ("admin", empty_project, vec!["git:read", "git:write"]),
        ("developer", empty_project, vec!["git:read", "git:write"]),
        ("admin", empty_project, vec!["git:read"]),
        ("admin", foreign, vec!["git:read", "git:write"]),
    ] {
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,$3)")
            .bind(user)
            .bind(format!("git-fixture-{user}"))
            .bind(role)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,$3)")
            .bind(empty_project)
            .bind(user)
            .bind(role)
            .execute(&pool)
            .await
            .unwrap();
        let token = format!(
            "cicd_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,project_id,scopes) VALUES($1,'empty-human',$2,'synthetic-only',$3,$4,$5)")
            .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&token)).bind(user).bind(bound).bind(scopes).execute(&pool).await.unwrap();
        human_tokens.push(token);
    }
    let original = git(&source, &["--git-dir", bare.to_str().unwrap(), "show-ref"]).stdout;
    fs::write(source.join("data"), "second\n").unwrap();
    assert!(git(&source, &["commit", "-am", "second"]).status.success());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/git/{name}.git", listener.local_addr().unwrap());
    let empty_url = format!(
        "http://{}/git/{empty_name}.git",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    for (token, refs) in [
        (
            &tokens[0],
            vec!["HEAD:refs/heads/main", "HEAD:refs/heads/task/denied"],
        ),
        (&tokens[0], vec!["HEAD:refs/heads/release"]),
        (&tokens[0], vec![":refs/heads/main"]),
        (&tokens[1], vec!["HEAD:refs/heads/task/foreign"]),
        (&tokens[2], vec!["HEAD:refs/heads/task/readonly"]),
    ] {
        assert!(!push(&source, &url, token, &refs).await.status.success());
        assert_eq!(
            git(&source, &["--git-dir", bare.to_str().unwrap(), "show-ref"]).stdout,
            original
        );
    }
    assert!(
        push(
            &source,
            &url,
            &tokens[0],
            &["HEAD:refs/heads/task/accepted"]
        )
        .await
        .status
        .success()
    );
    assert!(
        git(
            &source,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "show-ref",
                "--verify",
                "refs/heads/task/accepted"
            ]
        )
        .status
        .success()
    );
    assert_eq!(
        fs::read_to_string(hook_dir.join("post-receive")).unwrap(),
        "#!/bin/sh\ncat >/dev/null\nexit 0\n"
    );
    for token in [
        &empty_sat,
        &human_tokens[1],
        &human_tokens[2],
        &human_tokens[3],
    ] {
        assert!(
            !push(&source, &empty_url, token, &["HEAD:refs/heads/main"])
                .await
                .status
                .success()
        );
        assert!(
            !git(
                &source,
                &["--git-dir", empty_bare.to_str().unwrap(), "show-ref"]
            )
            .status
            .success()
        );
    }
    assert!(
        !push(
            &source,
            &empty_url,
            &human_tokens[0],
            &["HEAD:refs/heads/main", "HEAD:refs/heads/task/mixed"]
        )
        .await
        .status
        .success()
    );
    assert!(
        !push(
            &source,
            &empty_url,
            &human_tokens[0],
            &["HEAD:refs/heads/release"]
        )
        .await
        .status
        .success()
    );
    assert!(
        push(
            &source,
            &empty_url,
            &human_tokens[0],
            &["HEAD:refs/heads/main"]
        )
        .await
        .status
        .success()
    );
    let initial = git(
        &source,
        &["--git-dir", empty_bare.to_str().unwrap(), "show-ref"],
    )
    .stdout;
    fs::write(source.join("data"), "third\n").unwrap();
    assert!(git(&source, &["commit", "-am", "third"]).status.success());
    for token in [&empty_sat, &human_tokens[0]] {
        assert!(
            !push(&source, &empty_url, token, &["HEAD:refs/heads/main"])
                .await
                .status
                .success()
        );
        assert!(
            !push(&source, &empty_url, token, &[":refs/heads/main"])
                .await
                .status
                .success()
        );
        assert_eq!(
            git(
                &source,
                &["--git-dir", empty_bare.to_str().unwrap(), "show-ref"]
            )
            .stdout,
            initial
        );
    }
    assert!(!fs::read_dir(&bare).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".forge-receive-")
    }));
    server.abort();
    let _ = server.await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn namespace_write_lease_covers_receive_policy_and_hook_preparation() {
    let pool = test_pool().await;
    let root = std::env::temp_dir().join(format!("forge-git-lease-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let _files = Directory(root.clone());
    let name = format!("lease-{}", Uuid::new_v4().simple());
    let bare = root.join(format!("{name}.git"));
    assert!(
        git(
            &root,
            &["init", "--quiet", "--bare", bare.to_str().unwrap()]
        )
        .status
        .success()
    );
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch) VALUES($1,'Lease fixture','https://example.invalid/fixture.git',$2,'main')")
        .bind(Uuid::new_v4()).bind(repository).execute(&pool).await.unwrap();
    let projects_oid: i64 = sqlx::query_scalar("SELECT 'projects'::regclass::oid::bigint")
        .fetch_one(&pool)
        .await
        .unwrap();
    // Stall the policy snapshot so its namespace admission can be inspected deterministically.
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE projects IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let token = format!("synthetic-git-{}", Uuid::new_v4());
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.clone(),
            token: Some(token.clone()),
            internal_token: None,
        },
        Some("synthetic-auth-secret".into()),
    );
    let request = Request::post(format!("/git/{name}.git/git-receive-pack"))
        .header("content-type", "application/x-git-receive-pack-request")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from("0000"))
        .unwrap();
    let receive = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    let waiting = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='relation' AND relation=$1::bigint::oid AND NOT granted AND mode='AccessShareLock')",
            ).bind(projects_oid).fetch_one(&pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await;
    // Archive's exclusive repository fence must remain unavailable throughout preparation.
    let exclusive_available: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,3))")
            .bind(repository.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    blocker.rollback().await.unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(10), receive)
        .await
        .unwrap()
        .unwrap();
    assert!(waiting.is_ok(), "receive never reached the policy snapshot");
    assert!(
        !exclusive_available,
        "receive policy/hooks were prepared outside the namespace write lease"
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!fs::read_dir(&bare).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".forge-receive-")
    }));
    let released: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,3))")
            .bind(repository.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(released, "terminal receive retained namespace admission");
    pool.close().await;
}
