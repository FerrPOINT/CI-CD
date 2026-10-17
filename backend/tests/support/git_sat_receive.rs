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
        // Hook preservation only: this no-op does not prove the production callback.
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
        if role == "developer" {
            sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')")
                .bind(empty_project).bind(user).execute(&pool).await.unwrap();
        }
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

struct ReceiveRace {
    root: std::path::PathBuf,
    source: std::path::PathBuf,
    bare: std::path::PathBuf,
    entered: std::path::PathBuf,
    release: std::path::PathBuf,
    name: String,
    group: Uuid,
    command: pdlc_shared::resource_context::OwnerCommand,
    pool: sqlx::PgPool,
    admission: sqlx::PgPool,
    app: axum::Router,
    sat: String,
    admin: String,
}

impl Drop for ReceiveRace {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"release\n");
    }
}

impl ReceiveRace {
    async fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let pool = test_pool().await;
        let root = std::env::temp_dir().join(format!("forge-receive-race-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        for args in [
            vec!["init", "--initial-branch=main"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["config", "user.name", "Fixture"],
        ] {
            assert!(git(&source, &args).status.success());
        }
        fs::write(source.join("data"), "first\n").unwrap();
        assert!(git(&source, &["add", "data"]).status.success());
        assert!(git(&source, &["commit", "-m", "first"]).status.success());
        let slug = format!("race-{}", Uuid::new_v4().simple());
        let (group, _, command) = workspace_fixture(&pool, Uuid::new_v4(), &slug).await;
        let repository = Uuid::new_v4();
        let name = format!("ns-{}", repository.simple());
        sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
            .bind(repository)
            .bind(&name)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE repository_catalog SET group_id=$2,slug='receive-race' WHERE id=$1")
            .bind(repository)
            .bind(group)
            .execute(&pool)
            .await
            .unwrap();
        let project = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch) VALUES($1,'Receive race','https://example.invalid/race.git',$2,'main')")
            .bind(project).bind(repository).execute(&pool).await.unwrap();
        let account = Uuid::new_v4();
        sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'receive-race')")
            .bind(account)
            .execute(&pool)
            .await
            .unwrap();
        let sat = format!(
            "forge_sat_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes) VALUES($1,'receive-race',$2,'synthetic-only','service_account',$3,$4,ARRAY['git:read','git:write'])")
            .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&sat)).bind(account).bind(project).execute(&pool).await.unwrap();
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'admin')")
            .bind(user)
            .bind(format!("race-{user}"))
            .execute(&pool)
            .await
            .unwrap();
        let admin = format!(
            "cicd_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,project_id,scopes) VALUES($1,'receive-bootstrap',$2,'synthetic-only',$3,$4,ARRAY['git:read','git:write'])")
            .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&admin)).bind(user).bind(project).execute(&pool).await.unwrap();
        let bare = root.join(format!("{name}.git"));
        assert!(
            git(
                &root,
                &[
                    "init",
                    "--quiet",
                    "--bare",
                    "--initial-branch=main",
                    bare.to_str().unwrap()
                ]
            )
            .status
            .success()
        );
        let entered = root.join("entered");
        let release = root.join("release");
        // The custom hook pauses native Git, not a synthetic replacement process.
        // A deadline bounds the child even if the test runtime is interrupted.
        fs::write(bare.join("hooks/pre-receive"), format!(
            "#!/bin/sh\nset -eu\ncat >/dev/null\nif mkdir '{claimed}' 2>/dev/null; then\n  printf '%s\\n' \"$PPID\" > '{entered}.tmp'\n  mv '{entered}.tmp' '{entered}'\n  i=0\n  while ! test -f '{release}'; do\n    i=$((i+1))\n    if test \"$i\" -ge 400; then exit 1; fi\n    sleep 0.05\n  done\nfi\n",
            claimed=root.join("claimed").display(), entered=entered.display(), release=release.display()
        )).unwrap();
        fs::set_permissions(
            bare.join("hooks/pre-receive"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let git_config = cicd::git_host::GitConfig {
            root: root.clone(),
            token: None,
            internal_token: None,
        };
        let secret = format!("synthetic-race-{repository}");
        let config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
            .unwrap()
            .with_git_config(git_config.clone())
            .with_auth_secret(Some(secret.clone()));
        let admission = pool
            .options()
            .clone()
            .max_connections(2)
            .min_connections(0)
            .connect_lazy_with((*pool.connect_options()).clone());
        let state = std::sync::Arc::new(cicd::api::AppState {
            pool: Some(pool.clone()),
            namespace_admission_pool: Some(admission.clone()),
            auth_secret: Some(secret),
            git: git_config,
            config,
            running_jobs: None,
            rate_limiter: std::sync::Arc::new(cicd::rate_limit::RateLimiter::default()),
        });
        // Actual handlers and the same bounded admission pool. The local archive
        // route supplies a trusted synthetic owner; central-auth is not under test.
        let app = axum::Router::new()
            .route(
                "/git/{repo}/info/refs",
                axum::routing::get(cicd::git_host::git_info_refs),
            )
            .route(
                "/git/{repo}/git-receive-pack",
                axum::routing::post(cicd::git_host::git_service_endpoint),
            )
            .route("/archive/{id}", axum::routing::put(cicd::namespace::apply))
            .with_state(state);
        Self {
            root,
            source,
            bare,
            entered,
            release,
            name,
            group,
            command,
            pool,
            admission,
            app,
            sat,
            admin,
        }
    }

    async fn native_entered(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !self.entered.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("native pre-receive was not reached");
        let pid = fs::read_to_string(&self.entered).unwrap();
        assert_eq!(
            fs::read_to_string(format!("/proc/{}/comm", pid.trim()))
                .unwrap()
                .trim(),
            "git"
        );
    }

    async fn exclusive_available(&self) -> bool {
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(self.group.to_string())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    fn release(&self) {
        fs::write(&self.release, b"release\n").unwrap();
    }

    fn hooks_cleaned(&self) -> bool {
        !fs::read_dir(&self.bare).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".forge-receive-")
        })
    }

    async fn finish(&self) {
        assert!(self.exclusive_available().await);
        assert!(self.hooks_cleaned());
        self.admission.close().await;
        self.pool.close().await;
        // Only a known terminal writer permits filesystem cleanup. On panic,
        // Drop releases the pause but preserves files until outer QA cleanup.
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_http_receive_keeps_native_child_and_namespace_lease_until_exit() {
    let fixture = ReceiveRace::new().await;
    let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel();
    let app = fixture.app.clone().layer(axum::middleware::from_fn(
        move |request: Request<Body>, next: axum::middleware::Next| {
            let cancel_tx = cancel_tx.clone();
            async move {
                if !request.uri().path().ends_with("/git-receive-pack") {
                    return next.run(request).await;
                }
                let request = tokio::spawn(next.run(request));
                cancel_tx.send(request.abort_handle()).unwrap();
                request.await.unwrap_or_else(|_| {
                    axum::response::Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(Body::empty())
                        .unwrap()
                })
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/git/{}.git",
        listener.local_addr().unwrap(),
        fixture.name
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let source = fixture.source.clone();
    let token = fixture.sat.clone();
    let client = tokio::spawn(async move {
        push(&source, &url, &token, &["HEAD:refs/heads/task/cancelled"]).await
    });
    fixture.native_entered().await;
    assert!(!fixture.exclusive_available().await);
    assert!(!fixture.hooks_cleaned());
    let request = tokio::time::timeout(std::time::Duration::from_secs(10), cancel_rx.recv())
        .await
        .unwrap()
        .unwrap();
    request.abort();
    let response = tokio::time::timeout(std::time::Duration::from_secs(10), client)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !response.status.success(),
        "cancelled request unexpectedly acknowledged success"
    );
    fixture.native_entered().await;
    assert!(
        !fixture.exclusive_available().await,
        "HTTP cancellation released a live writer's lease"
    );
    assert!(
        !fixture.hooks_cleaned(),
        "HTTP cancellation removed live native hook files"
    );
    fixture.release();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !fixture.exclusive_available().await {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("terminal cancelled receive retained its namespace lease");
    assert!(fixture.hooks_cleaned());
    let head = git(&fixture.source, &["rev-parse", "HEAD"]).stdout;
    assert_eq!(
        git(&fixture.bare, &["rev-parse", "refs/heads/task/cancelled"]).stdout,
        head
    );
    server.abort();
    let _ = server.await;
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_bootstrap_and_receive_finish_around_waiting_namespace_archive() {
    let fixture = ReceiveRace::new().await;
    let (requests_tx, mut requests_rx) = tokio::sync::mpsc::unbounded_channel();
    let app = fixture.app.clone().layer(axum::middleware::from_fn(
        move |request: Request<Body>, next: axum::middleware::Next| {
            let requests_tx = requests_tx.clone();
            async move {
                let _ = requests_tx.send(request.uri().path().to_owned());
                next.run(request).await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/git/{}.git",
        listener.local_addr().unwrap(),
        fixture.name
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let source = fixture.source.clone();
    let token = fixture.admin.clone();
    let bootstrap_url = url.clone();
    let bootstrap = tokio::spawn(async move {
        push(&source, &bootstrap_url, &token, &["HEAD:refs/heads/main"]).await
    });
    fixture.native_entered().await;
    while requests_rx.try_recv().is_ok() {}
    let mut command = fixture.command.clone();
    command.generation += 1;
    command.operation_id = Uuid::new_v4();
    command.state = "archived".into();
    let request = Request::put(format!("/archive/{}", fixture.group))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&command).unwrap()))
        .unwrap();
    let archive_app = fixture.app.clone();
    let archive = tokio::spawn(async move { archive_app.oneshot(request).await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND mode='ExclusiveLock' AND NOT granted AND classid=((hashtextextended($1,0)>>32)&4294967295)::oid AND objid=(hashtextextended($1,0)&4294967295)::oid AND objsubid=1)")
                .bind(fixture.group.to_string()).fetch_one(&fixture.pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("archive did not queue behind native bootstrap");
    let source = fixture.source.clone();
    let token = fixture.sat.clone();
    let receive = tokio::spawn(async move {
        push(
            &source,
            &url,
            &token,
            &["HEAD:refs/heads/task/after-archive"],
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !requests_rx.recv().await.unwrap().ends_with("/info/refs") {}
    })
    .await
    .expect("concurrent receive did not reach HTTP discovery");
    assert!(!archive.is_finished());
    assert!(!receive.is_finished());
    assert!(
        git(&fixture.bare, &["for-each-ref", "--format=%(refname)"])
            .stdout
            .is_empty()
    );
    fixture.release();
    let (bootstrap, archive, receive) =
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(bootstrap, archive, receive)
        })
        .await
        .expect("bootstrap/receive/archive deadlocked");
    assert!(bootstrap.unwrap().status.success());
    assert_eq!(archive.unwrap().status(), StatusCode::OK);
    assert!(!receive.unwrap().status.success());
    let expected = git(&fixture.source, &["rev-parse", "HEAD"]).stdout;
    assert_eq!(
        git(&fixture.bare, &["rev-parse", "refs/heads/main"]).stdout,
        expected
    );
    assert!(
        !git(
            &fixture.bare,
            &["show-ref", "--verify", "refs/heads/task/after-archive"]
        )
        .status
        .success()
    );
    assert_eq!(
        git(&fixture.bare, &["for-each-ref", "--format=%(refname)"]).stdout,
        b"refs/heads/main\n"
    );
    let state: String =
        sqlx::query_scalar("SELECT state FROM forge_namespace_bindings WHERE resource_id=$1")
            .bind(fixture.group)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(state, "archived");
    assert!(fixture.exclusive_available().await);
    assert!(fixture.hooks_cleaned());
    server.abort();
    let _ = server.await;
    fixture.finish().await;
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
        .header("x-git-token", token)
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
    // SQLx queues rollback on Transaction drop; observe its completion rather than race it.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let released: bool =
                sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,3))")
                    .bind(repository.to_string())
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            if released {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("terminal receive retained namespace admission");
    pool.close().await;
}
