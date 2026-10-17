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
#[ignore = "requires isolated PostgreSQL and native Git"]
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
    let human = Uuid::new_v4();
    let viewer = Uuid::new_v4();
    for (user, role) in [(human, "developer"), (viewer, "viewer")] {
        sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,$3)")
            .bind(user)
            .bind(format!("git-human-{user}"))
            .bind(role)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
            .bind(user)
            .bind(cicd::auth::hash_password("SyntheticGitPass1!").unwrap())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$3,$4),($2,$3,$4)",
        )
        .bind(project)
        .bind(foreign)
        .bind(user)
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
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
    let human_token = login_access_token(
        app.clone(),
        &format!("git-human-{human}"),
        "SyntheticGitPass1!",
    )
    .await;
    let viewer_token = login_access_token(
        app.clone(),
        &format!("git-human-{viewer}"),
        "SyntheticGitPass1!",
    )
    .await;
    assert_eq!(
        discovery(&app, &name, &viewer_token, "git-receive-pack").await,
        StatusCode::FORBIDDEN
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
    let original = git(&source, &["--git-dir", bare.to_str().unwrap(), "show-ref"]).stdout;
    fs::write(source.join("data"), "second\n").unwrap();
    assert!(git(&source, &["commit", "-am", "second"]).status.success());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let url = format!("{origin}/git/{name}.git");
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
    // Human initial/default push keeps the pre-existing ACL and hook path, not the SAT guard.
    let initial_name = format!("human-initial-{}", Uuid::new_v4().simple());
    let initial_bare = root.join(format!("{initial_name}.git"));
    assert!(
        git(
            &source,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                initial_bare.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    let initial_repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(initial_repository)
        .bind(&initial_name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE projects SET repository_id=$1 WHERE id=$2")
        .bind(initial_repository)
        .bind(foreign)
        .execute(&pool)
        .await
        .unwrap();
    let initial_url = format!("{origin}/git/{initial_name}.git");
    assert!(
        !push(&source, &initial_url, &tokens[1], &["HEAD:refs/heads/main"])
            .await
            .status
            .success()
    );
    assert!(
        !git(
            &source,
            &[
                "--git-dir",
                initial_bare.to_str().unwrap(),
                "show-ref",
                "--verify",
                "refs/heads/main"
            ]
        )
        .status
        .success()
    );
    assert!(
        push(
            &source,
            &initial_url,
            &human_token,
            &["HEAD:refs/heads/main"]
        )
        .await
        .status
        .success()
    );
    assert!(
        push(&source, &url, &human_token, &["HEAD:refs/heads/main"])
            .await
            .status
            .success()
    );
    assert!(
        !push(
            &source,
            &url,
            &viewer_token,
            &["HEAD:refs/heads/task/viewer-denied"]
        )
        .await
        .status
        .success()
    );
    assert_eq!(
        fs::read_to_string(hook_dir.join("post-receive")).unwrap(),
        "#!/bin/sh\ncat >/dev/null\nexit 0\n"
    );
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
