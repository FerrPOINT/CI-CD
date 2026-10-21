//! Named live facets from the existing G-06/A-01 acceptance oracles.
use super::*;
use std::{os::unix::fs::PermissionsExt, time::Duration};

async fn non_commit_event(f: &Live, run: Uuid, name: &str) {
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let refs = f.store.refs(run, 1000, 0).await.unwrap().0;
            if refs.iter().any(|r| {
                r.ref_name == name
                    && r.state == "applied"
                    && r.event_delivered
                    && r.reason.as_deref() == Some("ci_non_commit_tag")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("incoming tree tag receives terminal CI decision without a pipeline");
}

#[tokio::test]
#[ignore = "Requires owned live provider; actual tree-object tags transfer both ways without CI"]
async fn tree_object_tags_transfer_both_directions_without_commit_pipeline() {
    let mut f = Live::new().await;
    // Bootstrap other provider refs before enabling CI for this specific facet.
    f.settings(Direction::Pull, false).await;
    assert_eq!(f.manual().await.state, "success");
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM repository_sync_refs ref JOIN repository_sync_runs run ON run.id=ref.run_id WHERE run.repository_id=$1 AND ref.direction='pull' AND ref.state='applied' AND NOT ref.event_delivered")
                .bind(f.repository).fetch_one(&f.pool).await.unwrap();
            if pending == 0 { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.unwrap();
    sqlx::query(
        "INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2)",
    )
    .bind(f.repository)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    let tree = git(&f.actor, &["mktree"]).await;
    assert_eq!(git(&f.actor, &["cat-file", "-t", &tree]).await, "tree");
    let incoming = format!("refs/tags/{}/tree-incoming", f.prefix);
    f.external(&incoming, None, &tree).await;
    let received = f.manual().await;
    assert_eq!(received.state, "success");
    f.ref_result(received.id, &incoming, "applied").await;
    f.same(&incoming, &tree).await;
    assert_eq!(git(&f.local, &["cat-file", "-t", &incoming]).await, "tree");
    non_commit_event(&f, received.id, &incoming).await;
    let outgoing = format!("refs/tags/{}/tree-outgoing", f.prefix);
    f.local(&outgoing, &tree).await;
    f.settings(Direction::Push, false).await;
    let sent = f.manual().await;
    assert_eq!(sent.state, "success");
    f.ref_result(sent.id, &outgoing, "applied").await;
    f.same(&outgoing, &tree).await;
    // Re-observe that outgoing object as incoming, so its non-commit CI decision
    // is independently exercised as well; deleting our local fixture ref does
    // not delete the provider ref.
    git(&f.local, &["update-ref", "-d", &outgoing, &tree]).await;
    f.settings(Direction::Pull, false).await;
    let returned = f.manual().await;
    assert_eq!(returned.state, "success");
    f.ref_result(returned.id, &outgoing, "applied").await;
    non_commit_event(&f, returned.id, &outgoing).await;
    f.same(&outgoing, &tree).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    f.stop_tasks().await;
    println!(
        "LIVE_TREE_TAGS_NO_CI_OK provider={:?} prefix={} received={} sent={} returned={} object_type=tree sha={tree} pipelines=0",
        f.settings.provider, f.prefix, received.id, sent.id, returned.id
    );
    f.pool.close().await;
}

#[tokio::test]
#[ignore = "Requires owned live provider; real receive-pack/PR merge in manual mode followed by >300s silence"]
async fn manual_mode_real_receive_and_merge_stay_idle_beyond_interval_then_manual_sync_works() {
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let mut f = Live::new().await;
    let developer = Uuid::new_v4();
    let username = format!("qa-manual-{}", developer.simple());
    let password = "Owned-Manual-HTTP-Fixture-Only!1";
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'developer')")
        .bind(developer)
        .bind(&username)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
        .bind(developer)
        .bind(cicd::auth::hash_password(password).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')",
    )
    .bind(f.project)
    .bind(developer)
    .execute(&f.pool)
    .await
    .unwrap();
    let authorization = login_access_token(f.api.clone(), &username, password).await;
    f.settings(Direction::Pull, false).await;
    assert_eq!(f.manual().await.state, "success");
    let git_config = cicd::git_host::GitConfig {
        root: f.root.clone(),
        token: None,
        internal_token: None,
    };
    cicd::git_host::ensure_post_receive_hooks(&f.pool, &git_config).await;
    std::fs::set_permissions(
        f.local.join("hooks/post-receive"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:22801")
        .await
        .unwrap();
    let receive_signals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_signals = receive_signals.clone();
    let api = f.api.clone().layer(axum::middleware::from_fn(
        move |request: Request<Body>, next: axum::middleware::Next| {
            let count = observed_signals.clone();
            async move {
                let signal = request.method() == axum::http::Method::POST
                    && request.uri().path() == "/api/v1/internal/git-push";
                let response = next.run(request).await;
                if signal && response.status().is_success() {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                response
            }
        },
    ));
    let server = tokio::spawn(async move { axum::serve(listener, api).await.unwrap() });
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    f.settings(Direction::Both, false).await;
    let previous = f.store.runs(f.repository, 100, 0).await.unwrap().1;
    let base = f.head("manual-http-base");
    let feature = f.head("manual-http-feature");
    let a = f.commit(None, "QA manual HTTP base").await;
    let b = f.commit(Some(&a), "QA manual HTTP feature").await;
    let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
    writer
        .write_all(format!("username=qa\npassword={authorization}\n\n").as_bytes())
        .unwrap();
    writer.shutdown(std::net::Shutdown::Write).unwrap();
    // SAFETY: reader owns its descriptor; duplicate is checked and wrapped once.
    let fd = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 200) };
    assert!(fd >= 0);
    // SAFETY: fd is the new owned descriptor returned by fcntl above.
    let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut push = tokio::process::Command::new("git");
    push.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(&f.actor)
        .args([
            "-c",
            "credential.helper=",
            "-c",
            "credential.helper=!f() { if test \"$1\" = get; then cat <&9; fi; }; f",
            "push",
        ])
        .arg(format!(
            "http://127.0.0.1:22801/git/{}.git",
            f.local.file_stem().unwrap().to_str().unwrap()
        ))
        .arg(format!("{a}:{base}"))
        .arg(format!("{b}:{feature}"));
    // SAFETY: only this child receives the owned credential channel on FD9.
    unsafe {
        push.pre_exec(move || {
            if libc::dup2(descriptor.as_raw_fd(), 9) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = tokio::time::timeout(Duration::from_secs(60), push.kill_on_drop(true).output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "real owned Git HTTP receive succeeded"
    );
    assert!(
        receive_signals.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "real post-receive hook reached the successful push-policy HTTP endpoint"
    );
    assert_eq!(git(&f.local, &["rev-parse", &base]).await, a);
    assert_eq!(git(&f.local, &["rev-parse", &feature]).await, b);
    assert_eq!(
        f.store.runs(f.repository, 100, 0).await.unwrap().1,
        previous
    );
    let storage = f.local.file_stem().unwrap().to_str().unwrap();
    let response = f.api.clone().oneshot(Request::post(format!("/api/v1/repos/{storage}/pulls"))
        .header("authorization",format!("Bearer {authorization}"))
        .header("content-type","application/json")
        .body(Body::from(serde_json::json!({"repository_name":storage,"title":"QA manual merge signal","source_branch":feature.strip_prefix("refs/heads/").unwrap(),"target_branch":base.strip_prefix("refs/heads/").unwrap()}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pr = response_json(response).await;
    let response = f
        .api
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/v1/repos/{storage}/pulls/{}/action",
                pr["number"]
            ))
            .header("authorization", format!("Bearer {authorization}"))
            .header("content-type", "application/json")
            .body(Body::from("{\"action\":\"merge\"}"))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let merged = response_json(response).await;
    assert_eq!(merged["status"], "merged");
    let merged_sha = merged["merge_commit_sha"].as_str().unwrap().to_owned();
    assert_eq!(git(&f.local, &["rev-parse", &base]).await, merged_sha);
    // This is real elapsed time with normal supervisors and the production
    // interval unchanged. The preceding receive and merge emitted real signals.
    let silence = tokio::time::Instant::now();
    while silence.elapsed() <= Duration::from_secs(300) {
        assert_eq!(
            f.store.runs(f.repository, 100, 0).await.unwrap().1,
            previous
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    let observed_silence = silence.elapsed();
    assert!(observed_silence > Duration::from_secs(300));
    assert_eq!(
        f.store.runs(f.repository, 100, 0).await.unwrap().1,
        previous
    );
    let refs = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert!(!refs.contains_key(&base));
    assert!(!refs.contains_key(&feature));
    let manual = f.manual().await;
    assert_eq!(manual.state, "success");
    assert_eq!(manual.source, "manual");
    assert!(!manual.settings.automatic);
    f.ref_result(manual.id, &base, "applied").await;
    f.ref_result(manual.id, &feature, "applied").await;
    f.same(&base, &merged_sha).await;
    f.same(&feature, &b).await;
    assert_eq!(
        f.store.runs(f.repository, 100, 0).await.unwrap().1,
        previous + 1
    );
    f.stop_tasks().await;
    println!(
        "LIVE_MANUAL_REAL_SIGNALS_SILENCE_OK provider={:?} prefix={} run={} silence_seconds={} receive_pack=true successful_post_receive_requests={} pr_merge=true automatic_runs=0",
        f.settings.provider,
        f.prefix,
        manual.id,
        observed_silence.as_secs_f64(),
        receive_signals.load(std::sync::atomic::Ordering::SeqCst)
    );
    f.pool.close().await;
}
