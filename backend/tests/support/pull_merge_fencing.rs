//! Real PostgreSQL + HTTP router + native Git: refs and data are the assertions.
use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Case {
    root: PathBuf,
    bare: PathBuf,
    repository: Uuid,
    name: String,
    initial: String,
    source: String,
    target: String,
    pool: sqlx::PgPool,
    app: axum::Router,
}

fn git(path: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}

impl Case {
    fn merge_barrier(&self) -> (PathBuf, PathBuf) {
        let entered = self.root.join("entered");
        let release = self.root.join("release");
        let driver = self.root.join("merge-driver.sh");
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        fs::write(&driver, format!("#!/bin/sh\nset -eu\n: > {}\nn=0\nwhile test ! -f {}; do n=$((n+1)); test \"$n\" -lt 1000 || exit 1; sleep 0.01; done\nprintf 'merged\\n' > \"$1\"\n", quote(&entered), quote(&release))).unwrap();
        fs::write(self.bare.join("info/attributes"), "shared merge=fence\n").unwrap();
        git(
            &self.bare,
            &[
                "config",
                "merge.fence.driver",
                &format!("sh {} %A", quote(&driver)),
            ],
        );
        (entered, release)
    }

    async fn new(conflicting: bool) -> Self {
        let pool = test_pool().await;
        let root = std::env::temp_dir().join(format!("forge-pr-fence-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let work = root.join("source");
        fs::create_dir(&work).unwrap();
        git(&work, &["init", "--quiet", "--initial-branch=main"]);
        fs::write(work.join("shared"), "base\n").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "--quiet", "-m", "base"]);
        let initial = git(&work, &["rev-parse", "HEAD"]);
        git(&work, &["switch", "--quiet", "-c", "task"]);
        fs::write(work.join("feature"), "feature\n").unwrap();
        if conflicting {
            fs::write(work.join("shared"), "source\n").unwrap();
        }
        git(&work, &["add", "."]);
        git(&work, &["commit", "--quiet", "-m", "source"]);
        let source = git(&work, &["rev-parse", "HEAD"]);
        git(&work, &["switch", "--quiet", "main"]);
        fs::write(work.join("main-data"), "existing-main-data\n").unwrap();
        if conflicting {
            fs::write(work.join("shared"), "target\n").unwrap();
        }
        git(&work, &["add", "."]);
        git(&work, &["commit", "--quiet", "-m", "target"]);
        let target = git(&work, &["rev-parse", "HEAD"]);
        for tag in ["main", "task", "tag-only"] {
            git(&work, &["tag", tag, &initial]);
        }
        let name = format!("merge-{}", Uuid::new_v4().simple());
        let bare = root.join(format!("{name}.git"));
        git(
            &work,
            &["clone", "--quiet", "--bare", ".", bare.to_str().unwrap()],
        );
        let repository = Uuid::new_v4();
        sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
            .bind(repository)
            .bind(&name)
            .execute(&pool)
            .await
            .unwrap();
        let app = authenticated_app_with_git(
            pool.clone(),
            cicd::git_host::GitConfig {
                root: root.clone(),
                token: None,
                internal_token: None,
            },
        )
        .await;
        Self {
            root,
            bare,
            repository,
            name,
            initial,
            source,
            target,
            pool,
            app,
        }
    }

    async fn configure_ci(&self, refs: &[&str]) -> Uuid {
        let configuration = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects(id,name,repository_url,repository_id,default_branch,protected_branches) \
             VALUES($1,$2,'https://example.invalid/fixture.git',$3,'main',ARRAY['main'])",
        )
        .bind(configuration)
        .bind(format!("CI {configuration}"))
        .bind(self.repository)
        .execute(&self.pool)
        .await
        .unwrap();
        // The existing protected-main rule still requires source-head success.
        sqlx::query(
            "INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'task',$3,'success')",
        )
        .bind(Uuid::new_v4())
        .bind(configuration)
        .bind(&self.source)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO repository_push_configs(repository_id,configuration_id,refs) VALUES($1,$2,$3) \
             ON CONFLICT(repository_id) DO UPDATE SET configuration_id=EXCLUDED.configuration_id,refs=EXCLUDED.refs",
        )
        .bind(self.repository)
        .bind(configuration)
        .bind(refs.iter().map(|reference| reference.to_string()).collect::<Vec<_>>())
        .execute(&self.pool)
        .await
        .unwrap();
        configuration
    }

    async fn ci_messages(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM outbox_messages WHERE channel='pipeline_trigger'")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn deliver_ci(&self) -> usize {
        cicd::outbox::deliver_due_with_git_root(
            &self.pool,
            &reqwest::Client::new(),
            &cicd::config::SmtpConfig::default(),
            &self.root,
        )
        .await
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        (status, response_json(response).await)
    }

    async fn create(&self, source: &str, target: &str) -> i64 {
        let (status, pr) = self.post(&format!("/api/v1/repos/{}/pulls", self.name),
            json!({"repository_name":self.name,"title":"Fenced merge fixture", "source_branch":source,"target_branch":target})).await;
        assert_eq!(status, StatusCode::OK, "{pr}");
        pr["number"].as_i64().unwrap()
    }

    fn merge_path(&self, number: i64) -> String {
        format!("/api/v1/repos/{}/pulls/{number}/action", self.name)
    }

    async fn is_open(&self, number: i64) {
        let status: String = sqlx::query_scalar(
            "SELECT status FROM pull_requests WHERE repository_id=$1 AND number=$2",
        )
        .bind(self.repository)
        .bind(i32::try_from(number).unwrap())
        .fetch_one(&self.pool)
        .await
        .unwrap();
        assert_eq!(status, "open");
    }
}

async fn wait_for_driver(entered: &Path) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !entered.is_file() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

fn request_action(
    app: axum::Router,
    path: String,
    action: &str,
) -> tokio::task::JoinHandle<axum::response::Response> {
    let body = json!({"action":action}).to_string();
    tokio::spawn(async move {
        app.oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_merge_keeps_its_owner_and_finishes_after_http_caller_disconnects() {
    let case = Case::new(true).await;
    let (entered, release) = case.merge_barrier();
    let number = case.create("task", "main").await;
    let request = request_action(case.app.clone(), case.merge_path(number), "merge");
    let waiting = wait_for_driver(&entered).await;
    if !waiting && request.is_finished() {
        let response = request.await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        panic!(
            "merge finished before driver barrier: {status}: {}",
            String::from_utf8_lossy(&body)
        );
    }
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let exclusive_available: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,3))")
            .bind(case.repository.to_string())
            .fetch_one(&case.pool)
            .await
            .unwrap();
    fs::write(&release, "continue\n").unwrap();
    let finished = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let status: String = sqlx::query_scalar(
                "SELECT status FROM pull_requests WHERE repository_id=$1 AND number=$2",
            )
            .bind(case.repository)
            .bind(i32::try_from(number).unwrap())
            .fetch_one(&case.pool)
            .await
            .unwrap();
            if status == "merged" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(waiting, "native merge never reached its barrier");
    assert!(
        !exclusive_available,
        "disconnect released admission while native Git was active"
    );
    assert!(finished.is_ok(), "accepted merge owner did not finish");
    assert_eq!(
        git(&case.bare, &["show", "refs/heads/main:main-data"]),
        "existing-main-data"
    );
    assert_eq!(
        git(&case.bare, &["show", "refs/heads/main:feature"]),
        "feature"
    );
    case.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_close_cannot_overwrite_the_terminal_merge_state() {
    let case = Case::new(true).await;
    let (entered, release) = case.merge_barrier();
    let number = case.create("task", "main").await;
    let merge = request_action(case.app.clone(), case.merge_path(number), "merge");
    let waiting = wait_for_driver(&entered).await;
    let close = request_action(case.app.clone(), case.merge_path(number), "close");
    let locked = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks w WHERE w.locktype='transactionid' AND NOT w.granted AND EXISTS(SELECT 1 FROM pg_locks r WHERE r.pid=w.pid AND r.relation='pull_requests'::regclass))")
                .fetch_one(&case.pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await;
    fs::write(&release, "continue\n").unwrap();
    let merged = tokio::time::timeout(std::time::Duration::from_secs(15), merge)
        .await
        .unwrap()
        .unwrap();
    let closed = tokio::time::timeout(std::time::Duration::from_secs(15), close)
        .await
        .unwrap()
        .unwrap();
    assert!(
        waiting && locked.is_ok(),
        "close did not wait for the active merge row lock"
    );
    assert_eq!(merged.status(), StatusCode::OK);
    assert_eq!(closed.status(), StatusCode::CONFLICT);
    let status: String =
        sqlx::query_scalar("SELECT status FROM pull_requests WHERE repository_id=$1 AND number=$2")
            .bind(case.repository)
            .bind(i32::try_from(number).unwrap())
            .fetch_one(&case.pool)
            .await
            .unwrap();
    assert_eq!(status, "merged");
    case.pool.close().await;
}

impl Drop for Case {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn pr_merge_uses_branch_heads_for_policy_and_preserves_data_despite_same_named_tags() {
    let case = Case::new(false).await;
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch,protected_branches) VALUES($1,'Merge fixture','https://example.invalid/fixture.git',$2,'main',ARRAY['main'])")
        .bind(project).bind(case.repository).execute(&case.pool).await.unwrap();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'task',$3,'success')")
        .bind(Uuid::new_v4()).bind(project).bind(&case.initial).execute(&case.pool).await.unwrap();
    let number = case.create("refs/heads/task", "refs/heads/main").await;
    let path = case.merge_path(number);
    let (status, _) = case.post(&path, json!({"action":"merge"})).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "green tag cannot approve a different branch commit"
    );
    case.is_open(number).await;
    assert_eq!(
        git(&case.bare, &["rev-parse", "refs/heads/main"]),
        case.target
    );
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'task',$3,'success')")
        .bind(Uuid::new_v4()).bind(project).bind(&case.source).execute(&case.pool).await.unwrap();
    let (status, merged) = case.post(&path, json!({"action":"merge"})).await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["status"], "merged");
    let head = git(&case.bare, &["rev-parse", "refs/heads/main"]);
    assert_eq!(merged["merge_commit_sha"], head);
    assert_eq!(
        git(&case.bare, &["show", "refs/heads/main:main-data"]),
        "existing-main-data"
    );
    assert_eq!(
        git(&case.bare, &["show", "refs/heads/main:feature"]),
        "feature"
    );
    assert_eq!(
        git(
            &case.bare,
            &["rev-list", "--parents", "-n", "1", "refs/heads/main"]
        ),
        format!("{head} {} {}", case.target, case.source)
    );
    for tag in ["main", "task"] {
        assert_eq!(
            git(&case.bare, &["rev-parse", &format!("refs/tags/{tag}")]),
            case.initial
        );
    }
    let tag_only = case.create("tag-only", "main").await;
    assert_eq!(
        case.post(&case.merge_path(tag_only), json!({"action":"merge"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    case.is_open(tag_only).await;
    let (invalid, _) = case.post(&format!("/api/v1/repos/{}/pulls", case.name),
        json!({"repository_name":case.name,"title":"Self alias", "source_branch":"task","target_branch":"refs/heads/task"})).await;
    assert_eq!(invalid, StatusCode::BAD_REQUEST);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pull_requests WHERE repository_id=$1")
            .bind(case.repository)
            .fetch_one(&case.pool)
            .await
            .unwrap();
    assert_eq!(count, 2, "invalid self-alias must not create another PR");
    assert_eq!(git(&case.bare, &["rev-parse", "refs/heads/main"]), head);
    case.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_source_or_target_change_returns_conflict_and_does_not_close_pr() {
    for changed in ["main", "task"] {
        let case = Case::new(true).await;
        case.configure_ci(&["refs/heads/main"]).await;
        let entered = case.root.join("entered");
        let release = case.root.join("release");
        let driver = case.root.join("merge-driver.sh");
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        fs::write(&driver, format!(
            "#!/bin/sh\nset -eu\n: > {}\nn=0\nwhile test ! -f {}; do n=$((n+1)); test \"$n\" -lt 1000 || exit 1; sleep 0.01; done\nprintf 'merged\\n' > \"$1\"\n",
            quote(&entered), quote(&release))).unwrap();
        fs::write(case.bare.join("info/attributes"), "shared merge=fence\n").unwrap();
        git(
            &case.bare,
            &[
                "config",
                "merge.fence.driver",
                &format!("sh {} %A", quote(&driver)),
            ],
        );
        let number = case.create("task", "main").await;
        let path = case.merge_path(number);
        let app = case.app.clone();
        let request = tokio::spawn(async move {
            app.oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"action":"merge"}"#))
                    .unwrap(),
            )
            .await
            .unwrap()
        });
        let waiting = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !entered.is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        // Unblock the bounded driver even when fixture setup did not reach the barrier.
        let parent = if changed == "main" {
            &case.target
        } else {
            &case.source
        };
        let tree = git(&case.bare, &["rev-parse", &format!("{parent}^{{tree}}")]);
        let advanced = git(
            &case.bare,
            &[
                "commit-tree",
                &tree,
                "-p",
                parent,
                "-m",
                "concurrent change",
            ],
        );
        git(
            &case.bare,
            &["update-ref", &format!("refs/heads/{changed}"), &advanced],
        );
        fs::write(&release, "continue\n").unwrap();
        let response = tokio::time::timeout(std::time::Duration::from_secs(15), request)
            .await
            .unwrap()
            .unwrap();
        assert!(
            waiting.is_ok(),
            "native merge never reached its driver barrier"
        );
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "stale {changed} must not publish"
        );
        case.is_open(number).await;
        assert_eq!(
            case.ci_messages().await,
            0,
            "failed publication must roll back CI dispatch"
        );
        let events: i64 =
            sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE event_type='git.merge'")
                .fetch_one(&case.pool)
                .await
                .unwrap();
        assert_eq!(events, 0);
        for (branch, initial) in [("main", &case.target), ("task", &case.source)] {
            let expected = if branch == changed {
                &advanced
            } else {
                initial
            };
            assert_eq!(
                git(&case.bare, &["rev-parse", &format!("refs/heads/{branch}")]),
                *expected
            );
        }
        case.pool.close().await;
    }
}

#[tokio::test]
async fn native_merge_dispatches_exact_main_ci_once() {
    let case = Case::new(false).await;
    let configuration = case.configure_ci(&["refs/heads/main"]).await;
    let number = case.create("task", "main").await;
    let (status, merged) = case
        .post(&case.merge_path(number), json!({"action":"merge"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    let sha = merged["merge_commit_sha"].as_str().unwrap();
    assert_eq!(git(&case.bare, &["rev-parse", "refs/heads/main"]), sha);
    assert_eq!(case.ci_messages().await, 1);
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1 AND git_ref='main'")
            .bind(configuration)
            .fetch_one(&case.pool)
            .await
            .unwrap();
    assert_eq!(
        before, 0,
        "the request captures intent; the worker starts CI"
    );
    let (destination, payload): (String, Value) = sqlx::query_as(
        "SELECT destination,payload FROM outbox_messages WHERE channel='pipeline_trigger'",
    )
    .fetch_one(&case.pool)
    .await
    .unwrap();
    assert!(destination.starts_with("forge-ci:"));
    assert_eq!(payload["configurationId"], configuration.to_string());
    assert_eq!(payload["reference"], "refs/heads/main");
    assert_eq!(payload["commitSha"], sha);

    assert_eq!(
        case.deliver_ci().await,
        1,
        "internal delivery cannot use its non-HTTP destination"
    );
    let (id, commit, key): (Uuid, String, String) = sqlx::query_as(
        "SELECT p.id,p.commit_sha,t.idempotency_key FROM pipelines p JOIN pipeline_triggers t ON t.pipeline_id=p.id \
         WHERE p.project_id=$1 AND p.git_ref='main' AND t.source='git-push'",
    ).bind(configuration).fetch_one(&case.pool).await.unwrap();
    assert_eq!(commit, sha);
    assert_eq!(key, payload["idempotencyKey"].as_str().unwrap());
    assert_eq!(case.deliver_ci().await, 0);
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM pipelines WHERE project_id=$1 AND git_ref='main'")
            .bind(configuration)
            .fetch_all(&case.pool)
            .await
            .unwrap();
    assert_eq!(ids, vec![id]);
    case.pool.close().await;
}

#[tokio::test]
async fn native_merge_ci_replays_captured_policy_and_commit_after_lost_ack() {
    let case = Case::new(false).await;
    let original = case.configure_ci(&["refs/heads/main"]).await;
    let number = case.create("task", "main").await;
    let (status, merged) = case
        .post(&case.merge_path(number), json!({"action":"merge"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    let sha = merged["merge_commit_sha"].as_str().unwrap().to_string();
    let changed = case.configure_ci(&["refs/heads/another"]).await;
    let tree = git(&case.bare, &["rev-parse", &format!("{sha}^{{tree}}")]);
    let advanced = git(
        &case.bare,
        &["commit-tree", &tree, "-p", &sha, "-m", "later main"],
    );
    git(&case.bare, &["update-ref", "refs/heads/main", &advanced]);

    assert_eq!(case.deliver_ci().await, 1);
    let (pipeline, commit): (Uuid, String) = sqlx::query_as(
        "SELECT id,commit_sha FROM pipelines WHERE project_id=$1 AND git_ref='main'",
    )
    .bind(original)
    .fetch_one(&case.pool)
    .await
    .unwrap();
    assert_eq!(commit, sha, "delivery must not substitute current main");
    let changed_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1 AND git_ref='main'")
            .bind(changed)
            .fetch_one(&case.pool)
            .await
            .unwrap();
    assert_eq!(
        changed_count, 0,
        "later policy must not retarget a captured event"
    );

    // Simulate loss of the delivery acknowledgement after pipeline creation.
    sqlx::query("UPDATE outbox_messages SET delivered_at=NULL,next_attempt_at=now() WHERE channel='pipeline_trigger'")
        .execute(&case.pool).await.unwrap();
    assert_eq!(case.deliver_ci().await, 1);
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM pipelines WHERE project_id=$1 AND git_ref='main'")
            .bind(original)
            .fetch_all(&case.pool)
            .await
            .unwrap();
    assert_eq!(
        ids,
        vec![pipeline],
        "retry must reuse the existing trigger receipt"
    );
    let attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_delivery_attempts a JOIN outbox_messages m ON m.id=a.message_id \
         WHERE m.channel='pipeline_trigger' AND a.outcome='delivered'",
    ).fetch_one(&case.pool).await.unwrap();
    assert_eq!(attempts, 2);
    case.pool.close().await;
}

#[tokio::test]
async fn native_merge_respects_ref_filter_and_preserves_legacy_delivery() {
    let case = Case::new(false).await;
    let configuration = case.configure_ci(&["refs/heads/other"]).await;
    let number = case.create("task", "main").await;
    let (status, merged) = case
        .post(&case.merge_path(number), json!({"action":"merge"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(
        case.ci_messages().await,
        0,
        "main is not an allowed push ref"
    );
    let event = Uuid::new_v4();
    let message = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO domain_events(id,event_type,aggregate_type,aggregate_id) VALUES($1,'fixture','project',$2)",
    ).bind(event).bind(configuration).execute(&case.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO outbox_messages(id,event_id,project_id,subscription_id,channel,destination,payload) \
         VALUES($1,$2,$3,'fixture','notification','fixture',$4)",
    ).bind(message).bind(event).bind(configuration)
        .bind(json!({"channel":"in_app","target":"fixture"}))
        .execute(&case.pool).await.unwrap();
    // The pre-existing public API and local notification channel remain supported.
    assert_eq!(
        cicd::outbox::deliver_due(
            &case.pool,
            &reqwest::Client::new(),
            &cicd::config::SmtpConfig::default(),
        )
        .await,
        1
    );
    let delivered: bool =
        sqlx::query_scalar("SELECT delivered_at IS NOT NULL FROM outbox_messages WHERE id=$1")
            .bind(message)
            .fetch_one(&case.pool)
            .await
            .unwrap();
    assert!(delivered);
    let main: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE git_ref='main'")
        .fetch_one(&case.pool)
        .await
        .unwrap();
    assert_eq!(main, 0);
    case.pool.close().await;
}
