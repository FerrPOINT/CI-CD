//! Real private Smart HTTP discovery: central PAT scopes stay independent of local token IDs.
use super::Profile;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub(super) async fn verify(pool: &sqlx::PgPool, profile: Arc<RwLock<Profile>>) {
    *profile.write().unwrap() = Profile {
        subject: Uuid::new_v4(),
        role: "member".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    };
    let verified = cicd::central_auth::try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    let actor = cicd::central_auth::link_central_user(pool, &verified)
        .await
        .unwrap();
    assert_eq!(actor.role, "user");
    assert!(
        actor.token_id.is_none(),
        "central PAT must not invent a local API-token row"
    );
    let root = std::env::temp_dir().join(format!("forge-git-scope-{}", Uuid::new_v4()));
    let config = cicd::git_host::GitConfig {
        root: root.clone(),
        token: None,
        internal_token: None,
    };
    let name = format!("scoped-{}", Uuid::new_v4().simple());
    let repo = cicd::git_host::create_repository_core(pool, &config, &name, Some("private"))
        .await
        .unwrap();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://example.invalid/source.git',$3)")
        .bind(project).bind(format!("scope-{project}")).bind(repo.id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'viewer')")
        .bind(project)
        .bind(actor.sub)
        .execute(pool)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        config,
        Some("fixture-auth-secret-not-a-live-secret".into()),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let request = |service: &str| {
        http.get(format!("{origin}/git/{name}/info/refs?service={service}"))
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
    };
    profile.write().unwrap().scopes = vec!["git:read"];
    let denied = request("git-upload-pack").await.unwrap();
    assert_eq!(
        denied.status(),
        reqwest::StatusCode::FORBIDDEN,
        "a central PAT without ci-cd:read must not become an unrestricted browser session"
    );
    profile.write().unwrap().scopes = vec!["ci-cd:read"];
    let read = request("git-upload-pack").await.unwrap();
    assert_eq!(read.status(), reqwest::StatusCode::OK);
    assert_eq!(
        read.headers()["content-type"],
        "application/x-git-upload-pack-advertisement"
    );
    assert_eq!(read.headers()["cache-control"], "private, no-store");
    let read_only_write = request("git-receive-pack").await.unwrap();
    assert_eq!(read_only_write.status(), reqwest::StatusCode::FORBIDDEN);
    profile.write().unwrap().scopes = vec!["ci-cd:write"];
    assert_eq!(
        request("git-upload-pack").await.unwrap().status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(
        request("git-receive-pack").await.unwrap().status(),
        reqwest::StatusCode::FORBIDDEN,
        "write scope does not replace the confirmed project role"
    );
    sqlx::query(
        "UPDATE project_memberships SET role='developer' WHERE project_id=$1 AND user_id=$2",
    )
    .bind(project)
    .bind(actor.sub)
    .execute(pool)
    .await
    .unwrap();
    let write = request("git-receive-pack").await.unwrap();
    assert_eq!(write.status(), reqwest::StatusCode::OK);
    assert_eq!(
        write.headers()["content-type"],
        "application/x-git-receive-pack-advertisement"
    );
    // Hold the real ACL query until the original introspection window expires.
    // The successful membership lookup must not mint a new token lifetime.
    profile.write().unwrap().scopes = vec!["ci-cd:read"];
    profile.write().unwrap().expires_in = 1;
    let mut blocked = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE project_memberships IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocked)
        .await
        .unwrap();
    let delayed_http = http.clone();
    let delayed_url = format!("{origin}/git/{name}/info/refs?service=git-upload-pack");
    let delayed = tokio::spawn(async move {
        delayed_http
            .get(delayed_url)
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT m.role FROM projects p JOIN project_memberships%')")
                .fetch_one(pool).await.unwrap();
            if waiting { break; }
            assert!(!delayed.is_finished(), "request must reach the real ACL lock");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("Git request reached blocked membership query");
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    blocked.rollback().await.unwrap();
    let expired = delayed.await.unwrap().unwrap();
    assert_eq!(
        expired.status(),
        reqwest::StatusCode::FORBIDDEN,
        "expired original authority during ACL IO denies Git without a new SSO cycle"
    );
    // Exercise the actual HTTP Git transport, not only the discovery endpoint.
    profile.write().unwrap().expires_in = 300;
    profile.write().unwrap().scopes = vec!["ci-cd:read", "ci-cd:write"];
    let checkout = root.join("client");
    let client_git = |args: Vec<String>| {
        let checkout = checkout.clone();
        async move {
            tokio::process::Command::new("git")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_TERMINAL_PROMPT", "0")
                .arg("-c")
                .arg("http.extraHeader=Authorization: Bearer sdlc_pat_fixture_product_roles")
                .arg("-C")
                .arg(checkout)
                .args(args)
                .output()
                .await
                .unwrap()
        }
    };
    let cloned = tokio::process::Command::new("git")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("-c")
        .arg("http.extraHeader=Authorization: Bearer sdlc_pat_fixture_product_roles")
        .args(["clone", &format!("{origin}/git/{name}.git")])
        .arg(&checkout)
        .output()
        .await
        .unwrap();
    assert!(
        cloned.status.success(),
        "real clone: {}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    for args in [
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.invalid"],
    ] {
        assert!(
            client_git(args.into_iter().map(str::to_owned).collect())
                .await
                .status
                .success()
        );
    }
    tokio::fs::write(checkout.join("data"), "original proof\n")
        .await
        .unwrap();
    assert!(
        client_git(vec!["add".into(), "data".into()])
            .await
            .status
            .success()
    );
    assert!(
        client_git(vec!["commit".into(), "-m".into(), "fixture".into()])
            .await
            .status
            .success()
    );
    let push = |branch: &str| {
        client_git(vec![
            "push".into(),
            "origin".into(),
            format!("HEAD:refs/heads/task/{branch}"),
        ])
    };
    let accepted = push("current").await;
    assert!(
        accepted.status.success(),
        "current proof push: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let reference = |branch: &str| {
        let bare = root.join(format!("{name}.git"));
        let reference = format!("refs/heads/task/{branch}");
        async move {
            tokio::process::Command::new("git")
                .arg("--git-dir")
                .arg(bare)
                .args(["rev-parse", "--verify", &reference])
                .output()
                .await
                .unwrap()
        }
    };
    let original = reference("current").await;
    assert!(original.status.success());
    let custom = root.join("custom-hooks");
    tokio::fs::create_dir(&custom).await.unwrap();
    let configured = tokio::process::Command::new("git")
        .arg("--git-dir")
        .arg(root.join(format!("{name}.git")))
        .args(["config", "core.hooksPath"])
        .arg(&custom)
        .status()
        .await
        .unwrap();
    assert!(configured.success());
    let hook = custom.join("reference-transaction");
    let script = format!(
        "#!/bin/sh\nset -eu\ncat >/dev/null\nif test \"$1\" = prepared; then\n  touch '{}'\n  sleep 4\n  touch '{}'\nfi\n",
        root.join("prepared-started").display(),
        root.join("late-hook-effect").display()
    );
    tokio::fs::write(&hook, &script).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
    }
    profile.write().unwrap().expires_in = 3;
    let expired = push("expired").await;
    assert!(
        !expired.status.success(),
        "expired prepared hook must not publish a ref"
    );
    assert!(
        root.join("prepared-started").exists(),
        "real custom prepared hook was reached"
    );
    assert!(
        !reference("expired").await.status.success(),
        "expired invocation created no ref"
    );
    assert_eq!(
        original.stdout,
        reference("current").await.stdout,
        "neighbor ref unchanged"
    );
    tokio::time::sleep(std::time::Duration::from_millis(4300)).await;
    assert!(
        !root.join("late-hook-effect").exists(),
        "deadline must stop the owned hook process group"
    );
    assert_eq!(
        tokio::fs::read_to_string(&hook).await.unwrap(),
        script,
        "custom hook bytes preserved"
    );
    // The next independent invocation retains the original custom hook contract.
    tokio::fs::write(&hook, "#!/bin/sh\ncat >/dev/null\nexit 1\n")
        .await
        .unwrap();
    profile.write().unwrap().expires_in = 300;
    assert!(!push("custom-denied").await.status.success());
    assert!(!reference("custom-denied").await.status.success());
    tokio::fs::write(&hook, "#!/bin/sh\ncat >/dev/null\nexit 0\n")
        .await
        .unwrap();
    assert!(
        push("after-expiry").await.status.success(),
        "new valid proof and custom hook still work"
    );
    assert!(reference("after-expiry").await.status.success());
    // Timeout after publication is an unknown outcome, not proof of rollback.
    let post = custom.join("post-receive");
    let post_script = format!(
        "#!/bin/sh\nset -eu\ncat >/dev/null\ntouch '{}'\nsleep 4\ntouch '{}'\n",
        root.join("post-started").display(),
        root.join("late-post-effect").display()
    );
    tokio::fs::write(&post, &post_script).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&post, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
    }
    profile.write().unwrap().expires_in = 3;
    let unknown = push("published-before-timeout").await;
    assert!(
        !unknown.status.success(),
        "late post hook must not report confirmed success"
    );
    assert!(root.join("post-started").exists());
    assert!(
        reference("published-before-timeout").await.status.success(),
        "actual ref readback distinguishes committed publication from rollback"
    );
    tokio::time::sleep(std::time::Duration::from_millis(4300)).await;
    assert!(!root.join("late-post-effect").exists());
    assert_eq!(tokio::fs::read_to_string(&post).await.unwrap(), post_script);
    profile.write().unwrap().expires_in = 300;
    println!("Git post-publication timeout requires actual ref readback: PASS");
    super::git_space_admission::verify(pool, profile.clone(), &root, &name, repo.id, &origin).await;

    println!(
        "Git native clone/push original deadline, custom prepared hook and process-group cleanup: PASS"
    );
    server.abort();
    let _ = server.await;
    assert!(
        root.starts_with(std::env::temp_dir())
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("forge-git-scope-")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
