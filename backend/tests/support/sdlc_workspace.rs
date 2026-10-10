use super::*;
use cicd::domain::sdlc_workspace::*;

#[path = "candidate_evidence.rs"]
mod candidate_evidence;
#[cfg(unix)]
#[path = "task_delivery.rs"]
mod task_delivery;

struct Fixture {
    pool: sqlx::PgPool,
    app: axum::Router,
    project: Uuid,
    subject: Uuid,
    token_id: Uuid,
    token: String,
    read_token: String,
    request: WorkspaceOperationRequest,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_repository_binding(true).await
    }

    async fn with_repository_binding(bind_repository: bool) -> Self {
        let url = std::env::var("CICD_TEST_DATABASE_URL").unwrap();
        assert!(
            url.rsplit('/').next().unwrap().starts_with("forge_test"),
            "disposable database only"
        );
        let pool = test_pool().await;
        let project = Uuid::new_v4();
        let repo = Uuid::new_v4();
        let subject = Uuid::new_v4();
        let pipeline = Uuid::new_v4();
        let stage = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let lease = Uuid::new_v4();
        let token_id = Uuid::new_v4();
        let repo_name = format!("sdlc-{}", repo.simple());
        sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,$3)")
            .bind(project)
            .bind(format!("workspace-{project}"))
            .bind(format!("https://forge.invalid/git/{repo_name}.git"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
            .bind(repo)
            .bind(repo_name)
            .execute(&pool)
            .await
            .unwrap();
        if bind_repository {
            sqlx::query("UPDATE projects SET repository_id=$2 WHERE id=$1")
                .bind(project)
                .bind(repo)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
            .bind(subject)
            .bind(format!("workspace-{subject}"))
            .execute(&pool)
            .await
            .unwrap();
        let token = format!("forge_sat_{}", Uuid::new_v4().simple());
        let read_token = format!("forge_sat_{}", Uuid::new_v4().simple());
        for (id, raw, scopes) in [
            (token_id, &token, vec!["api:read", "api:write"]),
            (Uuid::new_v4(), &read_token, vec!["api:read"]),
        ] {
            sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,project_id,scopes,principal_type,service_account_id) VALUES($1,'workspace', $2,'qa-only',$3,$4,'service_account',$5)")
                .bind(id).bind(cicd::auth::hash_token(raw)).bind(project).bind(scopes).bind(subject)
                .execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,status,commit_sha) VALUES($1,$2,'main','running',$3)")
            .bind(pipeline).bind(project).bind("b".repeat(40)).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'workspace',0,'running')")
            .bind(stage).bind(pipeline).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'workspace','unused','unused',0,'running')")
            .bind(job).bind(stage).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'running')",
        )
        .bind(attempt)
        .bind(job)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_expires_at,acknowledged_at) VALUES($1,$2,$3,'workspace-qa',2,now()+interval '5 minutes',now())")
            .bind(lease).bind(job).bind(attempt).execute(&pool).await.unwrap();
        let request = WorkspaceOperationRequest {
            contract_version: 1,
            operation_key: "prepare:workspace:1".into(),
            binding: WorkspaceTaskBinding {
                tracker_instance_id: "tracker-qa".into(),
                tracker_project_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                root_task_id: Uuid::new_v4(),
                assignment_id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
                routing_snapshot_id: Uuid::new_v4(),
                requirement_revision: 3,
                fencing_token: 11,
                assignment_hash: "a".repeat(64),
                workflow_task_ref: "SDLC-17".into(),
            },
            repository_id: repo,
            source_commit: "b".repeat(40),
            lease_id: lease,
            attempt_id: attempt,
            workspace_generation: 2,
            workspace_id: format!("attempt-{attempt}-2-{}", Uuid::new_v4().simple()),
            role: WorkspaceRole::Analyst,
            access: WorkspaceAccess::ReadOnly,
        };
        let mut config = cicd::config::RuntimeConfig::test_default()
            .with_auth_secret(Some("workspace-test-secret".into()));
        config.sdlc_workspace.operation_subject = Some(subject);
        let app = cicd::api::app_with_git_and_config(
            Some(pool.clone()),
            cicd::git_host::GitConfig::default(),
            None,
            config,
        )
        .unwrap();
        Self {
            pool,
            app,
            project,
            subject,
            token_id,
            token,
            read_token,
            request,
        }
    }
    async fn post(&self, request: &WorkspaceOperationRequest) -> (StatusCode, serde_json::Value) {
        call(
            &self.app,
            "POST",
            &format!(
                "/api/v1/projects/{}/sdlc/workspace-operations",
                self.project
            ),
            &self.token,
            serde_json::to_string(request).unwrap(),
        )
        .await
    }
    fn lookup_url(&self, receipt: &serde_json::Value) -> String {
        let b = &self.request.binding;
        format!(
            "/api/v1/projects/{}/sdlc/workspace-operations/{}?requestHash={}&taskId={}&rootTaskId={}&assignmentId={}&executionId={}&fencingToken={}",
            self.project,
            self.request.operation_key,
            receipt["requestHash"].as_str().unwrap(),
            b.task_id,
            b.root_task_id,
            b.assignment_id,
            b.execution_id,
            b.fencing_token
        )
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    payload: String,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 32 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::json!({"error": "non-json rejection"})),
    )
}

#[tokio::test]
async fn sdlc_workspace_replay_conflict_restart_and_expiry_readback() {
    let f = Fixture::new().await;
    let (status, first) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["status"], "blocked");
    assert_eq!(first["dispatchAllowed"], false);
    assert_eq!(first["physicalSourceObserved"], false);
    assert!(
        first["blockers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("tracker_admission_unavailable"))
    );
    assert_eq!(f.post(&f.request).await.1, first);
    if let Ok(path) = std::env::var("FORGE_WORKSPACE_FIXTURE_PATH") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({
            "post":{"request":f.request,"status":200,"response":first},
            "get":{"status":200,"response":call(&f.app,"GET",&f.lookup_url(&first),&f.read_token,String::new()).await.1}
        })).unwrap()).unwrap();
    }
    let mut changed = f.request.clone();
    changed.binding.fencing_token += 1;
    assert_eq!(f.post(&changed).await.0, StatusCode::CONFLICT);
    changed = f.request.clone();
    changed.binding.task_id = Uuid::new_v4();
    assert_eq!(f.post(&changed).await.0, StatusCode::CONFLICT);
    let mut config = cicd::config::RuntimeConfig::test_default()
        .with_auth_secret(Some("workspace-test-secret".into()));
    config.sdlc_workspace.operation_subject = Some(f.subject);
    let restarted = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        cicd::git_host::GitConfig::default(),
        None,
        config,
    )
    .unwrap();
    let url = f.lookup_url(&first);
    assert_eq!(
        call(&restarted, "GET", &url, &f.read_token, String::new())
            .await
            .1["receipt"],
        first
    );
    sqlx::query("UPDATE job_leases SET lease_expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(f.request.lease_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let expiry: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT lease_expires_at FROM job_leases WHERE id=$1")
            .bind(f.request.lease_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(f.post(&f.request).await.1, first);
    let (status, read) = call(&restarted, "GET", &url, &f.read_token, String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["receipt"], first);
    assert_eq!(read["expired"], true);
    assert_eq!(read["reconciliationNeeded"], true);
    assert_eq!(
        sqlx::query_scalar::<_, chrono::DateTime<Utc>>(
            "SELECT lease_expires_at FROM job_leases WHERE id=$1"
        )
        .bind(f.request.lease_id)
        .fetch_one(&f.pool)
        .await
        .unwrap(),
        expiry
    );
    let mut new_key = f.request.clone();
    new_key.operation_key = "new-after-expiry".into();
    assert_eq!(f.post(&new_key).await.0, StatusCode::CONFLICT);
    sqlx::query("UPDATE job_leases SET lease_status='expired',completed_at=now(),terminal_status='canceled' WHERE id=$1")
        .bind(f.request.lease_id).execute(&f.pool).await.unwrap();
    let next_attempt = Uuid::new_v4();
    sqlx::query("UPDATE execution_attempts SET status='canceled',finished_at=now() WHERE id=$1")
        .bind(f.request.attempt_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status) SELECT $1,job_id,2,'running' FROM job_leases WHERE id=$2")
        .bind(next_attempt).bind(f.request.lease_id).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_expires_at,acknowledged_at) SELECT $1,job_id,$2,'later-generation',3,now()+interval '5 minutes',now() FROM job_leases WHERE id=$3")
        .bind(Uuid::new_v4()).bind(next_attempt).bind(f.request.lease_id).execute(&f.pool).await.unwrap();
    let read = call(&restarted, "GET", &url, &f.read_token, String::new())
        .await
        .1;
    assert_eq!(read["receipt"], first);
    assert_eq!(read["currentGeneration"], 3);
    assert_eq!(read["leaseStatus"], "expired");
    assert_eq!(read["reconciliationNeeded"], true);
    assert_eq!(f.post(&f.request).await.1, first);
    assert!(
        sqlx::query("UPDATE sdlc_workspace_operations SET request_hash=repeat('c',64)")
            .execute(&f.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM sdlc_workspace_operations")
            .execute(&f.pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sdlc_workspace_concurrent_original_key_has_one_receipt_and_payload_winner() {
    let f = Fixture::new().await;
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let uri = format!("/api/v1/projects/{}/sdlc/workspace-operations", f.project);
    let mut handles = Vec::new();
    for request in [f.request.clone(), f.request.clone()] {
        let app = f.app.clone();
        let token = f.token.clone();
        let uri = uri.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            call(
                &app,
                "POST",
                &uri,
                &token,
                serde_json::to_string(&request).unwrap(),
            )
            .await
        }));
    }
    barrier.wait().await;
    let a = handles.remove(0).await.unwrap();
    let b = handles.remove(0).await.unwrap();
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(a, b);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sdlc_workspace_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    let mut x = f.request.clone();
    x.operation_key = "concurrent-different".into();
    let mut y = x.clone();
    y.binding.execution_id = Uuid::new_v4();
    let app = f.app.clone();
    let token = f.token.clone();
    let path = uri.clone();
    let h = tokio::spawn(async move {
        call(
            &app,
            "POST",
            &path,
            &token,
            serde_json::to_string(&x).unwrap(),
        )
        .await
    });
    let other = f.post(&y).await;
    let one = h.await.unwrap();
    assert!(matches!(
        (one.0, other.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
}

#[tokio::test]
async fn sdlc_workspace_permissions_foreign_stale_lookup_and_revocation() {
    let f = Fixture::new().await;
    let uri = format!("/api/v1/projects/{}/sdlc/workspace-operations", f.project);
    assert_eq!(
        call(
            &f.app,
            "POST",
            &uri,
            &f.read_token,
            serde_json::to_string(&f.request).unwrap()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let first = f.post(&f.request).await.1;
    let url = f.lookup_url(&first);
    assert_eq!(
        call(&f.app, "GET", &url, &f.read_token, String::new())
            .await
            .0,
        StatusCode::OK
    );
    for (old, new) in [
        (
            f.request.binding.task_id.to_string(),
            Uuid::new_v4().to_string(),
        ),
        (
            f.request.binding.assignment_id.to_string(),
            Uuid::new_v4().to_string(),
        ),
        ("fencingToken=11".into(), "fencingToken=12".into()),
    ] {
        assert_eq!(
            call(
                &f.app,
                "GET",
                &url.replace(&old, &new),
                &f.read_token,
                String::new()
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    }
    assert_eq!(
        call(
            &f.app,
            "GET",
            &url.replace(&f.project.to_string(), &Uuid::new_v4().to_string()),
            &f.read_token,
            String::new()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let other_subject = Uuid::new_v4();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'foreign-subject')")
        .bind(other_subject)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE api_tokens SET service_account_id=$1 WHERE id=$2")
        .bind(other_subject)
        .bind(f.token_id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(f.post(&f.request).await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE api_tokens SET revoked_at=now() WHERE id=$1")
        .bind(f.token_id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(f.post(&f.request).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sdlc_workspace_malformed_claim_and_local_fences_fail_without_receipt() {
    let f = Fixture::new().await;
    for mutation in 0..7 {
        let mut r = f.request.clone();
        match mutation {
            0 => r.attempt_id = Uuid::new_v4(),
            1 => r.lease_id = Uuid::new_v4(),
            2 => r.source_commit = "c".repeat(40),
            3 => r.repository_id = Uuid::new_v4(),
            4 => {
                r.workspace_generation = 3;
                r.workspace_id = format!("attempt-{}-3-{}", r.attempt_id, Uuid::new_v4().simple());
            }
            5 => r.access = WorkspaceAccess::ReadWrite,
            _ => r.binding.assignment_hash = "a".repeat(63),
        }
        assert!(matches!(
            f.post(&r).await.0,
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT
        ));
    }
    let uri = format!("/api/v1/projects/{}/sdlc/workspace-operations", f.project);
    let mut json = serde_json::to_value(&f.request).unwrap();
    json["receipt"] = serde_json::json!({"status":"prepared"});
    assert_eq!(
        call(&f.app, "POST", &uri, &f.token, json.to_string())
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&f.app, "POST", &uri, &f.token, " ".repeat(17 * 1024))
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let human = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'admin')")
        .bind(human)
        .bind(format!("human-{human}"))
        .execute(&f.pool)
        .await
        .unwrap();
    let pat = format!("cicd_{}", Uuid::new_v4().simple());
    sqlx::query("INSERT INTO api_tokens(id,name,user_id,token_hash,token_hint,project_id,scopes) VALUES($1,'human',$2,$3,'qa-only',$4,ARRAY['api:read','api:write'])")
        .bind(Uuid::new_v4()).bind(human).bind(cicd::auth::hash_token(&pat)).bind(f.project).execute(&f.pool).await.unwrap();
    assert_eq!(
        call(
            &f.app,
            "POST",
            &uri,
            &pat,
            serde_json::to_string(&f.request).unwrap()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sdlc_workspace_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn sdlc_workspace_migration_40_preserves_default_38_and_local_39_history() {
    let catalog = cicd::migrations().await.unwrap();
    for baseline in [38, 39] {
        let (pool, admin, schema) = migration_catalog_empty_pool().await;
        migration_catalog_subset(&catalog, baseline)
            .run(&pool)
            .await
            .unwrap();
        let before = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT version,checksum FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        catalog.run(&pool).await.unwrap();
        let after = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT version,checksum FROM _sqlx_migrations WHERE version <= $1 ORDER BY version",
        )
        .bind(baseline)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sdlc_workspace_operations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        catalog.run(&pool).await.unwrap();
        migration_catalog_cleanup(pool, admin, schema).await;
    }
}

struct PhysicalFixture {
    f: Fixture,
    temp: std::path::PathBuf,
    source: std::path::PathBuf,
    bare: std::path::PathBuf,
    pin: String,
    owned: cicd::runner_workspace::OwnedWorkspace,
}

fn physical_git(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(output.status.success(), "test Git command failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

impl Drop for PhysicalFixture {
    fn drop(&mut self) {
        assert!(self.temp.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(&self.temp).unwrap();
    }
}

impl PhysicalFixture {
    async fn new() -> Self {
        let mut f = Fixture::new().await;
        let temp = std::env::temp_dir().join(format!("forge-workspace-qa-{}", Uuid::new_v4()));
        let source = temp.join("source");
        let git_root = temp.join("git");
        let workspaces = temp.join("workspaces");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&git_root).unwrap();
        std::fs::create_dir_all(&workspaces).unwrap();
        let git = physical_git;
        git(&source, &["init", "--initial-branch=main"]);
        std::fs::write(source.join("artifact.txt"), "pinned source\n").unwrap();
        git(&source, &["add", "."]);
        git(
            &source,
            &[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "commit",
                "-m",
                "pin",
            ],
        );
        let pin = git(&source, &["rev-parse", "HEAD"]);
        let name: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id=$1")
            .bind(f.request.repository_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        let bare = git_root.join(format!("{name}.git"));
        git(
            &temp,
            &[
                "clone",
                "--bare",
                source.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        sqlx::query("UPDATE pipelines SET commit_sha=$1 WHERE project_id=$2")
            .bind(&pin)
            .bind(f.project)
            .execute(&f.pool)
            .await
            .unwrap();
        f.request.source_commit = pin.clone();
        let owned = cicd::runner_workspace::OwnedWorkspace::create(
            &workspaces,
            f.request.attempt_id,
            f.request.lease_id,
            2,
        )
        .unwrap();
        owned
            .clone_checkout(bare.to_str().unwrap(), Some(&pin), "main")
            .await
            .unwrap();
        f.request.workspace_id = owned
            .checkout()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .into();
        let mut config = cicd::config::RuntimeConfig::test_default()
            .with_auth_secret(Some("workspace-test-secret".into()));
        config.sdlc_workspace.operation_subject = Some(f.subject);
        config.sdlc_workspace.observation_root = Some(workspaces.clone());
        f.app = cicd::api::app_with_git_and_config(
            Some(f.pool.clone()),
            cicd::git_host::GitConfig {
                root: git_root,
                token: None,
                internal_token: None,
            },
            None,
            config,
        )
        .unwrap();
        Self {
            f,
            temp,
            source,
            bare,
            pin,
            owned,
        }
    }
}

#[tokio::test]
async fn sdlc_workspace_physical_pin_marker_clean_and_role_are_not_admission() {
    let p = PhysicalFixture::new().await;
    let f = &p.f;
    let owned = &p.owned;
    let source = &p.source;
    let bare = &p.bare;
    let pin = &p.pin;
    let workspaces = p.temp.join("workspaces");
    let git = physical_git;
    let (status, receipt) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(receipt["physicalSourceObserved"], true);
    assert_eq!(receipt["dispatchAllowed"], false);
    assert_eq!(
        receipt["blockers"],
        serde_json::json!([
            "tracker_admission_unavailable",
            "tracker_workspace_binding_unavailable"
        ])
    );
    let mut next = f.request.clone();
    next.operation_key = "physical-new-key".into();
    git(owned.checkout(), &["checkout", "-b", "mutable-source"]);
    assert_eq!(f.post(&next).await.0, StatusCode::CONFLICT);
    git(owned.checkout(), &["checkout", "--detach", pin]);
    std::fs::write(owned.checkout().join("artifact.txt"), "dirty\n").unwrap();
    assert_eq!(f.post(&next).await.0, StatusCode::CONFLICT);
    // Original replay never observes/changes a physical resource again.
    assert_eq!(f.post(&f.request).await.1, receipt);
    std::fs::write(owned.checkout().join("artifact.txt"), "pinned source\n").unwrap();
    git(
        owned.checkout(),
        &["remote", "set-url", "origin", source.to_str().unwrap()],
    );
    assert_eq!(f.post(&next).await.0, StatusCode::CONFLICT);
    git(
        owned.checkout(),
        &["remote", "set-url", "origin", bare.to_str().unwrap()],
    );
    for i in 0..100 {
        std::fs::write(
            owned
                .checkout()
                .join(format!("untracked-{i:03}-{}", "x".repeat(50))),
            "bounded observation",
        )
        .unwrap();
    }
    assert_eq!(f.post(&next).await.0, StatusCode::CONFLICT);
    for i in 0..100 {
        std::fs::remove_file(
            owned
                .checkout()
                .join(format!("untracked-{i:03}-{}", "x".repeat(50))),
        )
        .unwrap();
    }
    let foreign = cicd::runner_workspace::OwnedWorkspace::create(
        &workspaces,
        f.request.attempt_id,
        Uuid::new_v4(),
        2,
    )
    .unwrap();
    next.workspace_id = foreign
        .checkout()
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .into();
    assert_eq!(f.post(&next).await.0, StatusCode::CONFLICT);
    next.workspace_id = f.request.workspace_id.clone();
    next.source_commit = "c".repeat(40);
    sqlx::query("UPDATE pipelines SET commit_sha=$1 WHERE project_id=$2")
        .bind(&next.source_commit)
        .bind(f.project)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(f.post(&next).await.0, StatusCode::CONFLICT);
    // No new attempt/lease/queue or cleanup was produced by the operation API.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sdlc_workspace_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_leases")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    assert!(owned.checkout().exists());
    assert!(foreign.checkout().parent().unwrap().exists());
}

struct HttpServer(tokio::task::JoinHandle<()>);
impl Drop for HttpServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn socket_post(
    client: &reqwest::Client,
    url: &str,
    f: &Fixture,
    request: &WorkspaceOperationRequest,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = client
        .post(url)
        .bearer_auth(&f.token)
        .json(request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.bytes().await.unwrap();
    assert!(body.len() <= 32 * 1024);
    (status, serde_json::from_slice(&body).unwrap())
}

#[cfg(unix)]
#[tokio::test]
async fn sdlc_workspace_http_hidden_bytes_and_config_never_append_false_physical_observation() {
    use sha1::{Digest, Sha1};
    use std::{fs, os::unix::fs::MetadataExt, time::Duration};
    let p = PhysicalFixture::new().await;
    let f = &p.f;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = f.app.clone();
    let _server = HttpServer(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let origin = format!("http://{address}");
    let url = format!(
        "{origin}/api/v1/projects/{}/sdlc/workspace-operations",
        f.project
    );
    let checkout = p.owned.checkout();
    let index_path = checkout.join(".git/index");
    let file_path = checkout.join("artifact.txt");
    let (status, receipt) = socket_post(&client, &url, f, &f.request).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{receipt}");
    assert_eq!(receipt["physicalSourceObserved"], true);
    assert_eq!(receipt["status"], "blocked");
    assert_eq!(receipt["dispatchAllowed"], false);
    assert_eq!(receipt["blockers"].as_array().unwrap().len(), 2);
    let mut next = f.request.clone();
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        next.operation_key = format!("hidden-{flag}");
        physical_git(checkout, &["update-index", flag, "artifact.txt"]);
        fs::write(&file_path, "forged source\n").unwrap();
        assert_eq!(physical_git(checkout, &["status", "--porcelain"]), "");
        let index = fs::read(&index_path).unwrap();
        let (status, rejection) = socket_post(&client, &url, f, &next).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{rejection}");
        assert!(rejection.get("physicalSourceObserved").is_none());
        assert_eq!(fs::read(&index_path).unwrap(), index);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "forged source\n");
        let (status, replay) = socket_post(&client, &url, f, &f.request).await;
        assert_eq!(status, reqwest::StatusCode::OK);
        assert_eq!(replay, receipt);
        // Fixture-only restoration; production observation performs neither of these mutations.
        fs::write(&file_path, "pinned source\n").unwrap();
        let undo = if flag == "--assume-unchanged" {
            "--no-assume-unchanged"
        } else {
            "--no-skip-worktree"
        };
        physical_git(checkout, &["update-index", undo, "artifact.txt"]);
    }
    physical_git(checkout, &["update-index", "--index-version=2"]);
    let before = fs::metadata(&file_path).unwrap();
    fs::write(&file_path, "forged source\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&file_path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    let meta = fs::metadata(&file_path).unwrap();
    assert_eq!(meta.len(), before.len());
    let mut index = fs::read(&index_path).unwrap();
    assert_eq!(&index[..12], b"DIRC\0\0\0\x02\0\0\0\x01");
    for (slot, value) in index[12..52].chunks_exact_mut(4).zip([
        meta.ctime() as u32,
        meta.ctime_nsec() as u32,
        meta.mtime() as u32,
        meta.mtime_nsec() as u32,
        meta.dev() as u32,
        meta.ino() as u32,
        meta.mode(),
        meta.uid(),
        meta.gid(),
        meta.len() as u32,
    ]) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    let end = index.len() - 20;
    let checksum = Sha1::digest(&index[..end]);
    index[end..].copy_from_slice(&checksum);
    fs::write(&index_path, &index).unwrap();
    fs::File::options()
        .write(true)
        .open(&index_path)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(before.modified().unwrap() + Duration::from_secs(10)),
        )
        .unwrap();
    assert_eq!(physical_git(checkout, &["status", "--porcelain"]), "");
    next.operation_key = "spoofed-stat-cache".into();
    assert_eq!(
        socket_post(&client, &url, f, &next).await.0,
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(fs::read(&index_path).unwrap(), index);
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "forged source\n");
    let read = client
        .get(format!("{origin}{}", f.lookup_url(&receipt)))
        .bearer_auth(&f.read_token)
        .send()
        .await
        .unwrap();
    assert_eq!(read.status(), reqwest::StatusCode::OK);
    assert_eq!(
        read.json::<serde_json::Value>().await.unwrap()["receipt"],
        receipt
    );
    fs::write(&file_path, "pinned source\n").unwrap();
    for (config_path, key) in [
        (checkout.to_path_buf(), "filter.unsafe.clean"),
        (p.bare.clone(), "include.path"),
    ] {
        physical_git(&config_path, &["config", key, "unsupported-test-fixture"]);
        next.operation_key = format!("unsupported-{key}");
        assert_eq!(
            socket_post(&client, &url, f, &next).await.0,
            reqwest::StatusCode::CONFLICT
        );
        physical_git(&config_path, &["config", "--unset", key]);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sdlc_workspace_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    next.operation_key = "clean-after-fixture-restoration".into();
    assert_eq!(
        socket_post(&client, &url, f, &next).await.1["physicalSourceObserved"],
        true
    );
    next.operation_key = "wrong-row-lease-generation".into();
    next.workspace_generation += 1;
    next.workspace_id = format!(
        "attempt-{}-{}-{}",
        next.attempt_id,
        next.workspace_generation,
        Uuid::new_v4().simple()
    );
    assert_eq!(
        socket_post(&client, &url, f, &next).await.0,
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sdlc_workspace_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn sdlc_workspace_fail_closed_config_subject_rotation_write_only_and_json_guard() {
    let f = Fixture::new().await;
    let first = f.post(&f.request).await.1;
    let url = f.lookup_url(&first);
    let write_token = format!("forge_sat_{}", Uuid::new_v4().simple());
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,project_id,scopes,principal_type,service_account_id) VALUES($1,'write-only',$2,'qa-only',$3,ARRAY['api:write'],'service_account',$4)")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&write_token)).bind(f.project).bind(f.subject).execute(&f.pool).await.unwrap();
    assert_eq!(
        call(&f.app, "GET", &url, &write_token, String::new())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let unconfigured = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        cicd::git_host::GitConfig::default(),
        None,
        cicd::config::RuntimeConfig::test_default()
            .with_auth_secret(Some("workspace-test-secret".into())),
    )
    .unwrap();
    let path = format!("/api/v1/projects/{}/sdlc/workspace-operations", f.project);
    assert_eq!(
        call(
            &unconfigured,
            "POST",
            &path,
            &f.token,
            serde_json::to_string(&f.request).unwrap()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for (key, value) in [
        ("ready", serde_json::json!(true)),
        ("stopped", serde_json::json!(true)),
        ("role", serde_json::json!("project_manager")),
        ("binding", serde_json::Value::Null),
    ] {
        let mut json = serde_json::to_value(&f.request).unwrap();
        json[key] = value;
        assert_eq!(
            call(&f.app, "POST", &path, &f.token, json.to_string())
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let malformed=sqlx::query("INSERT INTO sdlc_workspace_operations SELECT $1,project_id,service_account_id,'missing-schema',request_hash,task_id,root_task_id,assignment_id,execution_id,fencing_token,lease_id,attempt_id,workspace_generation,repository_id,source_commit,jsonb_set(jsonb_set(receipt-'schema','{operationId}',to_jsonb($1::text)),'{request,operationKey}','\"missing-schema\"'::jsonb),recorded_at FROM sdlc_workspace_operations WHERE operation_key=$2")
        .bind(Uuid::new_v4()).bind(&f.request.operation_key).execute(&f.pool).await.unwrap_err();
    assert_eq!(
        malformed.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let rotated = Uuid::new_v4();
    let token = format!("forge_sat_{}", Uuid::new_v4().simple());
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'rotated-owner')")
        .bind(rotated)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,project_id,scopes,principal_type,service_account_id) VALUES($1,'rotated',$2,'qa-only',$3,ARRAY['api:read','api:write'],'service_account',$4)")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&token)).bind(f.project).bind(rotated).execute(&f.pool).await.unwrap();
    let mut config = cicd::config::RuntimeConfig::test_default()
        .with_auth_secret(Some("workspace-test-secret".into()));
    config.sdlc_workspace.operation_subject = Some(rotated);
    let app = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        cicd::git_host::GitConfig::default(),
        None,
        config,
    )
    .unwrap();
    assert_eq!(
        call(&app, "GET", &url, &token, String::new()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &path,
            &token,
            serde_json::to_string(&f.request).unwrap()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn sdlc_workspace_repository_uuid_wins_over_a_conflicting_url_tail() {
    let f = Fixture::with_repository_binding(false).await;
    let other = Uuid::new_v4();
    let other_name = format!("other-{}", other.simple());
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(other)
        .bind(&other_name)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE projects SET repository_url=$2,repository_id=$3 WHERE id=$1")
        .bind(f.project)
        .bind(format!("https://foreign.invalid/git/{other_name}.git"))
        .bind(f.request.repository_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let mut wrong = f.request.clone();
    wrong.repository_id = other;
    assert_eq!(f.post(&wrong).await.0, StatusCode::CONFLICT);
    let (status, receipt) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(
        receipt["request"]["repositoryId"],
        f.request.repository_id.to_string()
    );
}

#[tokio::test]
async fn sdlc_workspace_matching_url_without_explicit_repository_binding_is_rejected() {
    let f = Fixture::with_repository_binding(false).await;
    assert_eq!(f.post(&f.request).await.0, StatusCode::CONFLICT);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sdlc_workspace_operations WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    sqlx::query("UPDATE projects SET repository_id=$2 WHERE id=$1")
        .bind(f.project)
        .bind(f.request.repository_id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(f.post(&f.request).await.0, StatusCode::OK);
}
