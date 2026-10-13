use super::*;
#[cfg(unix)]
use sha2::{Digest, Sha256};

#[tokio::test]
async fn sdlc_workspace_history_delete_returns_conflict_and_preserves_owner_resources() {
    let p = PhysicalFixture::new().await;
    let f = &p.f;
    assert_eq!(f.post(&f.request).await.0, StatusCode::OK);
    let app = authenticated_app(f.pool.clone()).await;
    let response = app
        .oneshot(
            Request::delete(format!("/api/v1/projects/{}", f.project))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let name: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id=$1")
        .bind(f.request.repository_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let error = cicd::git_host::delete_repository_core(
        &f.pool,
        &cicd::git_host::GitConfig {
            root: p.temp.join("git"),
            token: None,
            internal_token: None,
        },
        &name,
    )
    .await
    .unwrap_err();
    let response = axum::response::IntoResponse::into_response(error);
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(p.bare.is_dir());
    assert!(p.owned.checkout().is_dir());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sdlc_workspace_operations WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[cfg(unix)]
#[tokio::test]
async fn sdlc_candidate_actual_runner_pipeline_artifacts_and_blocked_delivery() {
    let mut f = Fixture::with_repository_binding(false).await;
    let temp = std::env::temp_dir().join(format!("forge-candidate-actual-{}", Uuid::new_v4()));
    let source = temp.join("source");
    let git_root = temp.join("git");
    let workspaces = temp.join("workspaces");
    let artifacts = temp.join("artifacts");
    for path in [&source, &git_root, &workspaces, &artifacts] {
        std::fs::create_dir_all(path).unwrap();
    }
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp.clone());
    let barrier = temp.join("release");
    let tag = format!("candidate-{}", Uuid::new_v4().simple());
    let command = format!(
        "while test ! -f '{}'; do sleep 0.1; done; printf 'actual candidate artifact\\n' > product.txt",
        barrier.display()
    );
    let yaml = serde_yaml::to_string(&serde_json::json!({"version":1,"jobs":{"build":{
        "tags":[tag], "commands":[command], "artifacts":{"paths":["product.txt"]}}}}))
    .unwrap();
    physical_git(&source, &["init", "--initial-branch=main"]);
    std::fs::write(source.join(".forge-ci.yml"), &yaml).unwrap();
    physical_git(&source, &["add", "."]);
    physical_git(
        &source,
        &[
            "-c",
            "user.name=QA",
            "-c",
            "user.email=qa@example.invalid",
            "commit",
            "-m",
            "candidate pipeline",
        ],
    );
    let pin = physical_git(&source, &["rev-parse", "HEAD"]);
    let name: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id=$1")
        .bind(f.request.repository_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let bare = git_root.join(format!("{name}.git"));
    physical_git(
        &temp,
        &[
            "clone",
            "--bare",
            source.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    sqlx::query("UPDATE projects SET repository_url=$1,repository_id=$3 WHERE id=$2")
        .bind(format!("file://{}", bare.display()))
        .bind(f.project)
        .bind(f.request.repository_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let mut config = cicd::config::RuntimeConfig::test_default()
        .with_auth_secret(Some("workspace-test-secret".into()));
    config.sdlc_workspace.operation_subject = Some(f.subject);
    config.artifacts.root = artifacts.clone();
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
    let (status, pipeline) = call(
        &f.app,
        "POST",
        &format!("/api/v1/projects/{}/pipelines", f.project),
        &f.token,
        serde_json::json!({"git_ref":pin}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pipeline}");
    assert_eq!(pipeline["plan"]["config_source"], "repository");
    assert_eq!(pipeline["plan"]["resolved_commit_sha"], pin);
    let pipeline_id: Uuid = serde_json::from_value(pipeline["pipeline"]["id"].clone()).unwrap();
    let runner_id = Uuid::new_v4();
    let credential = format!("cicd_runner_{}", runner_id.simple());
    sqlx::query("INSERT INTO runners(id,name,tags,status,last_seen_at,credential_hash,credential_expires_at,capabilities) VALUES($1,$2,$3,'online',now(),$4,now()+interval '1 hour','{}')")
        .bind(runner_id).bind(format!("actual-{runner_id}")).bind(vec![tag.clone()])
        .bind(cicd::auth::hash_token(&credential)).execute(&f.pool).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = f.app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    let mut runner = tokio::process::Command::new(env!("CARGO_BIN_EXE_forge-runner"));
    runner
        .kill_on_drop(true)
        .args([
            "--api-url",
            &format!("http://{address}"),
            "--name",
            &format!("actual-{runner_id}"),
            "--tags",
            &tag,
            "--once",
            "--keep-workspace",
            "--work-dir",
        ])
        .arg(&workspaces)
        .env("CICD_RUNNER_CREDENTIAL", credential)
        .env_remove("CICD_RUNNER_REGISTRATION_TOKEN")
        .env_remove("CICD_RUNNER_NO_CHECKOUT")
        .env_remove("CICD_RUNNER_INSPECT_WORKSPACES")
        .env_remove("CICD_RUNNER_RECONCILE_WORKSPACES");
    let child = runner.spawn().unwrap();
    let wait = tokio::spawn(async move { child.wait_with_output().await.unwrap() });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let (lease, attempt, generation) = loop {
        let row: Option<(Uuid,Uuid,i64)> = sqlx::query_as("SELECT l.id,l.attempt_id,l.generation FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 AND l.acknowledged_at IS NOT NULL AND l.lease_status='active'")
            .bind(pipeline_id).fetch_optional(&f.pool).await.unwrap();
        if let Some(row) = row {
            break row;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "actual runner did not acknowledge lease"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    f.request.lease_id = lease;
    f.request.attempt_id = attempt;
    f.request.workspace_generation = generation;
    f.request.source_commit = pin.clone();
    f.request.workspace_id = format!("attempt-{attempt}-{generation}-{}", Uuid::new_v4().simple());
    let (status, receipt) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    let original_url = f.lookup_url(&receipt);
    let candidate_url = original_url.replace('?', "/candidate-evidence?");
    let (status, before) = call(&f.app, "GET", &candidate_url, &f.read_token, String::new()).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_eq!(before["candidateObserved"], false);
    std::fs::write(&barrier, b"release").unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), wait)
        .await
        .unwrap()
        .unwrap();
    assert!(result.status.success(), "actual runner failed");
    let response = tokio::process::Command::new("curl")
        .args([
            "-fsS",
            "-H",
            &format!("Authorization: Bearer {}", f.read_token),
            &format!("http://{address}{candidate_url}"),
        ])
        .output()
        .await
        .unwrap();
    assert!(response.status.success());
    let evidence: serde_json::Value = serde_json::from_slice(&response.stdout).unwrap();
    assert_eq!(evidence["candidateObserved"], true, "{evidence}");
    assert_eq!(evidence["pipelineId"], pipeline_id.to_string());
    assert_eq!(evidence["operationReceipt"], receipt);
    assert_eq!(
        evidence["configSha256"],
        format!("{:x}", Sha256::digest(yaml.as_bytes()))
    );
    assert_eq!(evidence["artifacts"][0]["attemptId"], attempt.to_string());
    assert_eq!(
        evidence["artifacts"][0]["sha256"],
        format!("{:x}", Sha256::digest(b"actual candidate artifact\n"))
    );
    for flag in [
        "dispatchAllowed",
        "deploymentVerified",
        "acceptanceVerified",
    ] {
        assert_eq!(evidence[flag], false);
    }
    assert_eq!(evidence["blockers"].as_array().unwrap().len(), 7);
    let (status, replay) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, receipt);
    let path: String = sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE attempt_id=$1")
        .bind(attempt)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    std::fs::write(&path, b"forged candidate artifact\n").unwrap();
    let (_, tampered) = call(&f.app, "GET", &candidate_url, &f.read_token, String::new()).await;
    assert_eq!(tampered["candidateObserved"], false);
    assert!(tampered["artifacts"].as_array().unwrap().is_empty());
    assert!(
        tampered["blockers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("attempt_artifact_bytes_unverified"))
    );
    std::fs::write(&path, b"actual candidate artifact\n").unwrap();
    sqlx::query("UPDATE job_leases SET completion_received_at=NULL WHERE id=$1")
        .bind(lease)
        .execute(&f.pool)
        .await
        .unwrap();
    let (_, unack) = call(&f.app, "GET", &candidate_url, &f.read_token, String::new()).await;
    assert_eq!(unack["candidateObserved"], false);
    sqlx::query("UPDATE job_leases SET completion_received_at=now() WHERE id=$1")
        .bind(lease)
        .execute(&f.pool)
        .await
        .unwrap();
    let (_, missing) = call(
        &f.app,
        "GET",
        &candidate_url.replace(receipt["requestHash"].as_str().unwrap(), &"c".repeat(64)),
        &f.read_token,
        String::new(),
    )
    .await;
    assert!(missing.get("error").is_some());
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_expires_at) SELECT $1,job_id,attempt_id,'superseding',generation+1,now()+interval '1 minute' FROM job_leases WHERE id=$2")
        .bind(Uuid::new_v4()).bind(lease).execute(&f.pool).await.unwrap();
    let (status, _) = call(&f.app, "GET", &candidate_url, &f.read_token, String::new()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, historical) = f.post(&f.request).await;
    assert_eq!(historical, receipt);
}

#[tokio::test]
async fn sdlc_candidate_missing_plan_and_completion_never_pass() {
    let f = Fixture::new().await;
    let (status, receipt) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK);
    let url = f.lookup_url(&receipt).replace('?', "/candidate-evidence?");
    let (status, evidence) = call(&f.app, "GET", &url, &f.read_token, String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(evidence["candidateObserved"], false);
    assert!(
        evidence["blockers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(
                "exact_repository_pipeline_config_unverified"
            ))
    );
    let (status, _) = call(
        &f.app,
        "GET",
        &url,
        &f.token.replace("forge_sat_", "foreign_"),
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sdlc_workspace_operations WHERE project_id=$1")
            .bind(f.project)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn sdlc_workspace_database_rejects_missing_null_and_invalid_revision_identity() {
    let f = Fixture::new().await;
    let (status, original) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK);
    for field in [
        "trackerInstanceId",
        "trackerProjectId",
        "routingSnapshotId",
        "requirementRevision",
        "assignmentHash",
        "workflowTaskRef",
    ] {
        for null in [false, true] {
            let mut receipt = original.clone();
            let id = Uuid::new_v4();
            let key = format!("missing:{field}:{null}");
            receipt["operationId"] = serde_json::json!(id);
            receipt["request"]["operationKey"] = serde_json::json!(key);
            if null {
                receipt["request"]["binding"][field] = serde_json::Value::Null;
            } else {
                receipt["request"]["binding"]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
            }
            let result=sqlx::query("INSERT INTO sdlc_workspace_operations SELECT $1,project_id,service_account_id,$2,request_hash,task_id,root_task_id,assignment_id,execution_id,fencing_token,lease_id,attempt_id,workspace_generation,repository_id,source_commit,$3,recorded_at FROM sdlc_workspace_operations WHERE project_id=$4")
                .bind(id).bind(&key).bind(sqlx::types::Json(receipt)).bind(f.project).execute(&f.pool).await;
            assert!(result.is_err(), "missing/null {field} accepted");
        }
    }
    let (_, replay) = f.post(&f.request).await;
    assert_eq!(replay, original);
}
