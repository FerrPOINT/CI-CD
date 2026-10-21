//! Actual HTTP/SQL/native PR effects under opt-in common Space policy.
use super::Profile;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    path::Path,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

async fn git(bare: &Path, args: &[&str]) -> String {
    let output = tokio::process::Command::new("git")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("--git-dir")
        .arg(bare)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "fixture Git: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
async fn post(
    http: &reqwest::Client,
    origin: &str,
    path: &str,
    input: &Value,
    expected: u16,
) -> Value {
    let response = http
        .post(format!("{origin}{path}"))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .json(input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    if expected == 200 {
        assert_eq!(response.headers()["cache-control"], "private, no-store");
    }
    let text = response.text().await.unwrap();
    assert_eq!(status.as_u16(), expected, "PR {path}: {text}");
    serde_json::from_str(&text).unwrap()
}
async fn wait_query(pool: &PgPool, pattern: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let found:bool=sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE $1)",
            ).bind(pattern).fetch_one(pool).await.unwrap();
            if found {break}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("request reached actual protected SQL wait");
}
async fn delayed_post(
    http: &reqwest::Client,
    origin: &str,
    path: &str,
    input: Value,
) -> tokio::task::JoinHandle<reqwest::Response> {
    let client = http.clone();
    let url = format!("{origin}{path}");
    tokio::spawn(async move {
        client
            .post(url)
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .json(&input)
            .send()
            .await
            .unwrap()
    })
}
async fn hook(path: &Path, bytes: &[u8]) {
    tokio::fs::write(path, bytes).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
    }
}
pub(super) async fn verify(
    pool: &PgPool,
    profile: &Arc<RwLock<Profile>>,
    root: &Path,
    origin: &str,
    visible: (&str, Uuid, &str),
    hidden: (&str, Uuid),
) {
    let (name, repository, canonical) = visible;
    let (hidden_name, hidden_id) = hidden;
    let saved = profile.read().unwrap().clone();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap();
    let bare = root.join(format!("{name}.git"));
    let base = git(&bare, &["rev-parse", "refs/heads/task/current"]).await;
    let source = git(
        &bare,
        &[
            "commit-tree",
            &format!("{base}^{{tree}}"),
            "-p",
            &base,
            "-m",
            "PR fixture source",
        ],
    )
    .await;
    git(&bare, &["update-ref", "refs/heads/qa-pr-source", &source]).await;
    git(&bare, &["update-ref", "refs/heads/qa-pr-target", &base]).await;
    let collection = format!(
        "/api/v1/catalog/repositories/{}/pulls",
        repository.to_string().replace('-', "%2D")
    );
    let input = json!({"title":"Actual protected review","source_branch":"qa-pr-source","target_branch":"qa-pr-target"});
    post(&http, origin, &collection, &input, 403).await;
    profile.write().unwrap().scopes = vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"];
    sqlx::query("UPDATE users SET role='viewer' WHERE central_sub=$1")
        .bind(saved.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    post(&http, origin, &collection, &input, 403).await;
    sqlx::query("UPDATE users SET role='developer' WHERE central_sub=$1")
        .bind(saved.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    for path in [
        format!("/api/v1/catalog/repositories/{hidden_id}/pulls"),
        format!("/api/v1/repos/{}/pulls", hidden_name.replace('-', "%2D")),
    ] {
        post(&http, origin, &path, &input, 403).await;
    }
    let before: i32 = sqlx::query_scalar(
        "SELECT high_water_mark FROM repository_pr_counters WHERE repository_id=$1",
    )
    .bind(repository)
    .fetch_one(pool)
    .await
    .unwrap();
    let mut held = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT repository_id FROM repository_pr_counters WHERE repository_id=$1 FOR UPDATE",
    )
    .bind(repository)
    .fetch_one(&mut *held)
    .await
    .unwrap();
    profile.write().unwrap().expires_in = 2;
    let delayed = delayed_post(&http, origin, &collection, input.clone()).await;
    wait_query(pool, "SELECT allocate_repository_pr_number%").await;
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    held.rollback().await.unwrap();
    let response = delayed.await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 403, "expired create: {text}");
    let after: i32 = sqlx::query_scalar(
        "SELECT high_water_mark FROM repository_pr_counters WHERE repository_id=$1",
    )
    .bind(repository)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        before, after,
        "expired allocation is rolled back in the same transaction"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pull_requests WHERE repository_id=$1")
            .bind(repository)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "expired creation adds no review");
    profile.write().unwrap().expires_in = 300;
    let created = post(&http, origin, &collection, &input, 200).await;
    let number = created["number"].as_i64().unwrap();
    let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let action = format!("/api/v1/catalog/repositories/{repository}/pulls/{number}/action");
    let encoded_action = format!("/api/v1/repos/{canonical}%2Frepository/pulls/{number}/action");
    // Direct database inspection proves expiry did not silently commit the change.
    let mut held = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM pull_requests WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *held)
        .await
        .unwrap();
    profile.write().unwrap().expires_in = 2;
    let delayed = delayed_post(&http, origin, &action, json!({"action":"close"})).await;
    wait_query(
        pool,
        "SELECT id, repository_id, repository_name, number, title%FOR UPDATE",
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    held.rollback().await.unwrap();
    let response = delayed.await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 403, "expired close: {text}");
    let status: String = sqlx::query_scalar("SELECT status FROM pull_requests WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(status, "open");
    profile.write().unwrap().expires_in = 300;
    assert_eq!(
        post(
            &http,
            origin,
            &encoded_action,
            &json!({"action":"close"}),
            200
        )
        .await["status"],
        "closed"
    );
    assert_eq!(
        post(&http, origin, &action, &json!({"action":"reopen"}), 200).await["status"],
        "open"
    );
    println!("PR create/action scopes, owner aliases, viewer denial and SQL expiry rollback: PASS");
    // Task-link metadata is the same repository-owned resource, not a separate bypass.
    let tracker = Uuid::new_v4();
    let task = Uuid::new_v4();
    let inserted=sqlx::query(
        "INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,created_by_user_id,task_key_snapshot) SELECT p.id,p.repository_id,$2,$3,b.registry_instance_id,b.namespace_id,$4,u.id,'TASK-42' FROM pull_requests p JOIN repository_catalog r ON r.id=p.repository_id JOIN forge_namespace_bindings b ON b.resource_id=r.group_id JOIN users u ON u.central_sub=$5 WHERE p.repository_id=$1 AND p.number=1",
    ).bind(repository).bind(tracker).bind(task).bind(&base).bind(saved.subject.to_string()).execute(pool).await.unwrap();
    assert_eq!(inserted.rows_affected(), 1);
    for (path, expected) in [
        (
            format!("/api/v1/catalog/repositories/{repository}/pulls/1/tasks?limit=1"),
            200,
        ),
        (
            format!("/api/v1/catalog/repositories/{hidden_id}/pulls/1/tasks"),
            403,
        ),
    ] {
        let response = http
            .get(format!("{origin}{path}"))
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert_eq!(status.as_u16(), expected, "task links: {text}");
        if expected == 200 {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value[0]["task_key"], "TASK-42");
            assert_eq!(value[0]["task"]["task_id"], task.to_string());
        } else {
            assert!(!text.contains("TASK-42"));
        }
    }
    // A stored idempotency result is still protected by the current write proof.
    let task_route = format!("/api/v1/catalog/repositories/{repository}/pulls/1/tasks");
    let encoded_task_route = format!(
        "/api/v1/catalog/repositories/{}/pulls/1/tasks",
        repository.to_string().replace('-', "%2D")
    );
    let task_input = json!({"tracker_instance_id":tracker,"task_id":task});
    profile.write().unwrap().scopes = vec!["ci-cd:read"];
    post(&http, origin, &task_route, &task_input, 403).await;
    profile.write().unwrap().scopes = vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"];
    sqlx::query("UPDATE users SET role='viewer' WHERE central_sub=$1")
        .bind(saved.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    post(&http, origin, &task_route, &task_input, 403).await;
    sqlx::query("UPDATE users SET role='developer' WHERE central_sub=$1")
        .bind(saved.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        post(&http, origin, &encoded_task_route, &task_input, 200).await,
        task_input
    );
    // Seed an actual existing hidden link: replay must not bypass common visibility.
    let hidden_bare = root.join(format!("{hidden_name}.git"));
    let hidden_tree = git(&hidden_bare, &["mktree"]).await;
    let hidden_base = git(
        &hidden_bare,
        &["commit-tree", &hidden_tree, "-m", "Hidden fixture source"],
    )
    .await;
    git(
        &hidden_bare,
        &["update-ref", "refs/heads/task/current", &hidden_base],
    )
    .await;
    let inserted = sqlx::query(
        "INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,created_by_user_id,task_key_snapshot) SELECT p.id,p.repository_id,$2,$3,b.registry_instance_id,b.namespace_id,$4,u.id,'HIDDEN-42' FROM pull_requests p JOIN repository_catalog r ON r.id=p.repository_id JOIN forge_namespace_bindings b ON b.resource_id=r.group_id JOIN users u ON u.central_sub=$5 WHERE p.repository_id=$1 AND p.number=1",
    ).bind(hidden_id).bind(tracker).bind(task).bind(&hidden_base).bind(saved.subject.to_string()).execute(pool).await.unwrap();
    assert_eq!(inserted.rows_affected(), 1);
    post(
        &http,
        origin,
        &format!("/api/v1/catalog/repositories/{hidden_id}/pulls/1/tasks"),
        &task_input,
        403,
    )
    .await;
    let mut held = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE pull_task_links IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *held)
        .await
        .unwrap();
    profile.write().unwrap().expires_in = 2;
    let delayed = delayed_post(&http, origin, &task_route, task_input.clone()).await;
    wait_query(pool, "SELECT EXISTS(SELECT 1 FROM pull_task_links%").await;
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    held.rollback().await.unwrap();
    let response = delayed.await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 403, "expired task-link replay: {text}");
    profile.write().unwrap().expires_in = 300;
    assert_eq!(
        post(&http, origin, &task_route, &task_input, 200).await,
        task_input
    );
    let snapshot: String = sqlx::query_scalar(
        "SELECT source_commit_sha FROM pull_task_links WHERE repository_id=$1 AND tracker_instance_id=$2 AND task_id=$3"
    ).bind(repository).bind(tracker).bind(task).fetch_one(pool).await.unwrap();
    assert_eq!(
        snapshot, base,
        "replay preserves the original source commit"
    );
    println!(
        "PR task-link replay common visibility, scopes, role, encoded ID and original SQL deadline: PASS"
    );
    // Previous Git transport cases deliberately configure a custom hooksPath.
    // Exercise the actual hook Git resolves, rather than an inactive default file.
    let hooks = git(
        &bare,
        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )
    .await;
    let hook_path = std::path::PathBuf::from(hooks).join("reference-transaction");
    let original = tokio::fs::read(&hook_path).await.ok();
    let slow = format!(
        "#!/bin/sh\nset -eu\ncat >/dev/null\nif test \"$1\" = prepared; then\n touch '{}'\n sleep 4\n touch '{}'\nfi\n",
        root.join("pr-prepared-started").display(),
        root.join("pr-late-prepared").display()
    );
    hook(&hook_path, slow.as_bytes()).await;
    profile.write().unwrap().expires_in = 3;
    post(&http, origin, &action, &json!({"action":"merge"}), 503).await;
    assert!(
        root.join("pr-prepared-started").exists(),
        "actual native preparation was reached"
    );
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-target"]).await,
        base,
        "expired prepared hook cannot publish a ref"
    );
    tokio::time::sleep(std::time::Duration::from_millis(4300)).await;
    assert!(
        !root.join("pr-late-prepared").exists(),
        "native process group cleanup stops the late hook"
    );
    assert_eq!(tokio::fs::read(&hook_path).await.unwrap(), slow.as_bytes());
    for reference in ["qa-pr-source", "qa-pr-target"] {
        assert!(
            !bare.join(format!("refs/heads/{reference}.lock")).exists(),
            "terminated Git must clean its own {reference} lock before the next writer"
        );
    }
    profile.write().unwrap().expires_in = 300;
    // An unknown preparation is retained. A fresh readback must never republish it.
    post(&http, origin, &action, &json!({"action":"merge"}), 503).await;
    post(&http, origin, &action, &json!({"action":"close"}), 409).await;
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-target"]).await,
        base
    );
    // A separate legitimate review still reaches its actual custom deny hook.
    let fresh = post(&http, origin, &collection, &input, 200).await;
    let number = fresh["number"].as_i64().unwrap();
    let action = format!("/api/v1/catalog/repositories/{repository}/pulls/{number}/action");
    let encoded_action = format!("/api/v1/repos/{canonical}%2Frepository/pulls/{number}/action");
    let denied_marker = root.join("pr-denied-prepared");
    let deny = format!(
        "#!/bin/sh\ncat >/dev/null\nif test \"$1\" = prepared; then touch '{}'; exit 1; fi\n",
        denied_marker.display()
    );
    hook(&hook_path, deny.as_bytes()).await;
    post(&http, origin, &action, &json!({"action":"merge"}), 409).await;
    assert!(
        denied_marker.exists(),
        "custom hook, not a stale ref lock, denied merge"
    );
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-target"]).await,
        base
    );
    hook(&hook_path, b"#!/bin/sh\ncat >/dev/null\nexit 0\n").await;
    let merged = post(
        &http,
        origin,
        &encoded_action,
        &json!({"action":"merge"}),
        200,
    )
    .await;
    assert_eq!(merged["status"], "merged");
    assert_eq!(
        merged["merge_commit_sha"],
        git(&bare, &["rev-parse", "refs/heads/qa-pr-target"]).await
    );
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-source"]).await,
        source
    );
    println!(
        "PR native merge original deadline, custom reference hook, owned cleanup and exact ref publication: PASS"
    );
    // Timeout after ref publication is explicitly unknown, never an automatic retry.
    git(&bare, &["update-ref", "refs/heads/qa-pr-unknown", &base]).await;
    let unknown_input = json!({"title":"Unknown publication review","source_branch":"qa-pr-source","target_branch":"qa-pr-unknown"});
    let unknown = post(&http, origin, &collection, &unknown_input, 200).await;
    let number = unknown["number"].as_i64().unwrap();
    let unknown_id: Uuid = unknown["id"].as_str().unwrap().parse().unwrap();
    let late = format!(
        "#!/bin/sh\nset -eu\ncat >/dev/null\nif test \"$1\" = committed; then\n touch '{}'\n sleep 4\n touch '{}'\nfi\n",
        root.join("pr-committed-started").display(),
        root.join("pr-late-committed").display()
    );
    hook(&hook_path, late.as_bytes()).await;
    profile.write().unwrap().expires_in = 3;
    post(
        &http,
        origin,
        &format!("/api/v1/catalog/repositories/{repository}/pulls/{number}/action"),
        &json!({"action":"merge"}),
        503,
    )
    .await;
    assert!(
        root.join("pr-committed-started").exists(),
        "actual publication occurred before timeout"
    );
    assert_ne!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-unknown"]).await,
        base
    );
    let status: String = sqlx::query_scalar("SELECT status FROM pull_requests WHERE id=$1")
        .bind(unknown_id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        status, "open",
        "unknown native outcome cannot claim merged metadata"
    );
    tokio::time::sleep(std::time::Duration::from_millis(4300)).await;
    assert!(!root.join("pr-late-committed").exists());
    assert_eq!(tokio::fs::read(&hook_path).await.unwrap(), late.as_bytes());
    match original {
        Some(ref bytes) => hook(&hook_path, bytes).await,
        None => tokio::fs::remove_file(&hook_path).await.unwrap(),
    }
    profile.write().unwrap().expires_in = 300;
    let recovery_action =
        format!("/api/v1/catalog/repositories/{repository}/pulls/{number}/action");
    let published = git(&bare, &["rev-parse", "refs/heads/qa-pr-unknown"]).await;
    let intent: (String, String) = sqlx::query_as(
        "SELECT merge_oid,state FROM pull_request_merge_attempts WHERE pull_request_id=$1 AND state<>'rejected'",
    ).bind(unknown_id).fetch_one(pool).await.unwrap();
    assert_eq!(intent, (published.clone(), "prepared".into()));
    git(&bare, &["update-ref", "-d", "refs/heads/qa-pr-unknown"]).await;
    post(
        &http,
        origin,
        &recovery_action,
        &json!({"action":"merge"}),
        503,
    )
    .await;
    git(
        &bare,
        &["update-ref", "refs/heads/qa-pr-unknown", &published],
    )
    .await;
    let advanced = git(
        &bare,
        &[
            "commit-tree",
            &format!("{published}^{{tree}}"),
            "-p",
            &published,
            "-m",
            "later writer",
        ],
    )
    .await;
    git(
        &bare,
        &["update-ref", "refs/heads/qa-pr-unknown", &advanced],
    )
    .await;
    post(
        &http,
        origin,
        &recovery_action,
        &json!({"action":"merge"}),
        503,
    )
    .await;
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-unknown"]).await,
        advanced
    );
    git(
        &bare,
        &["update-ref", "refs/heads/qa-pr-unknown", &published],
    )
    .await;
    git(&bare, &["update-ref", "refs/heads/qa-pr-source", &base]).await;
    post(
        &http,
        origin,
        &recovery_action,
        &json!({"action":"merge"}),
        503,
    )
    .await;
    git(&bare, &["update-ref", "refs/heads/qa-pr-source", &source]).await;
    profile.write().unwrap().scopes = vec!["ci-cd:read", "git:read"];
    post(
        &http,
        origin,
        &recovery_action,
        &json!({"action":"merge"}),
        403,
    )
    .await;
    profile.write().unwrap().scopes = vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"];
    sqlx::query("UPDATE users SET role='viewer' WHERE central_sub=$1")
        .bind(saved.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    post(
        &http,
        origin,
        &recovery_action,
        &json!({"action":"merge"}),
        403,
    )
    .await;
    sqlx::query("UPDATE users SET role='developer' WHERE central_sub=$1")
        .bind(saved.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    let untouched = git(
        &bare,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    )
    .await;
    let forbidden_publication = root.join("pr-recovery-republished");
    hook(
        &hook_path,
        format!(
            "#!/bin/sh\ncat >/dev/null\ntouch '{}'\nexit 1\n",
            forbidden_publication.display()
        )
        .as_bytes(),
    )
    .await;
    let body = json!({"action":"merge"});
    let (first, second) = tokio::join!(
        post(&http, origin, &recovery_action, &body, 200),
        post(&http, origin, &recovery_action, &body, 200),
    );
    for result in [first, second] {
        assert_eq!(result["status"], "merged");
        assert_eq!(result["merge_commit_sha"], published);
    }
    assert!(
        !forbidden_publication.exists(),
        "recovery must not invoke a publication hook"
    );
    assert_eq!(
        git(
            &bare,
            &["for-each-ref", "--format=%(refname) %(objectname)"]
        )
        .await,
        untouched
    );
    let intent_state: String = sqlx::query_scalar(
        "SELECT state FROM pull_request_merge_attempts WHERE pull_request_id=$1 AND state<>'rejected'",
    ).bind(unknown_id).fetch_one(pool).await.unwrap();
    assert_eq!(intent_state, "confirmed");
    // SQL failure after successful publication leaves a recoverable durable intent.
    hook(&hook_path, b"#!/bin/sh\ncat >/dev/null\nexit 0\n").await;
    git(&bare, &["update-ref", "refs/heads/qa-pr-sql", &base]).await;
    let sql_review = post(&http, origin, &collection,
        &json!({"title":"SQL commit interruption","source_branch":"qa-pr-source","target_branch":"qa-pr-sql"}), 200).await;
    let sql_id: Uuid = sql_review["id"].as_str().unwrap().parse().unwrap();
    let sql_number = sql_review["number"].as_i64().unwrap();
    let sql_action = format!("/api/v1/catalog/repositories/{repository}/pulls/{sql_number}/action");
    sqlx::raw_sql(&format!(
        "CREATE FUNCTION qa_fail_merge_metadata() RETURNS trigger LANGUAGE plpgsql AS @fn@ BEGIN \
         IF NEW.id='{sql_id}'::uuid AND NEW.status='merged' THEN RAISE EXCEPTION 'fixture_metadata_failure'; END IF; RETURN NEW; END @fn@; \
         CREATE TRIGGER qa_fail_merge_metadata BEFORE UPDATE ON pull_requests FOR EACH ROW EXECUTE FUNCTION qa_fail_merge_metadata();"
    ).replace("@fn@", "$$")).execute(pool).await.unwrap();
    post(&http, origin, &sql_action, &json!({"action":"merge"}), 503).await;
    let sql_published = git(&bare, &["rev-parse", "refs/heads/qa-pr-sql"]).await;
    assert_ne!(sql_published, base);
    let sql_status: String = sqlx::query_scalar("SELECT status FROM pull_requests WHERE id=$1")
        .bind(sql_id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(sql_status, "open");
    sqlx::raw_sql("DROP TRIGGER qa_fail_merge_metadata ON pull_requests; DROP FUNCTION qa_fail_merge_metadata();")
        .execute(pool).await.unwrap();
    let recovered = post(&http, origin, &sql_action, &json!({"action":"merge"}), 200).await;
    assert_eq!(recovered["merge_commit_sha"], sql_published);
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-sql"]).await,
        sql_published
    );
    git(
        &bare,
        &["update-ref", "refs/heads/qa-pr-unknown", &advanced],
    )
    .await;
    let completed = post(
        &http,
        origin,
        &recovery_action,
        &json!({"action":"merge"}),
        200,
    )
    .await;
    assert_eq!(completed["merge_commit_sha"], published);
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/qa-pr-unknown"]).await,
        advanced
    );
    assert!(!forbidden_publication.exists());

    match original {
        Some(ref bytes) => hook(&hook_path, bytes).await,
        None => {
            if hook_path.exists() {
                tokio::fs::remove_file(&hook_path).await.unwrap();
            }
        }
    }
    *profile.write().unwrap() = saved;
    println!(
        "PR durable merge intent, exact refs, authority and concurrent recovery without republication: PASS"
    );
    println!(
        "PR post-publication timeout requires exact Git ref readback without retry; task-link read admission: PASS"
    );
}
