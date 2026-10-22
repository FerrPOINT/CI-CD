//! The real router, owner-reader HTTP client, migrated PG and native clone/push.
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::json;
use sqlx::PgPool;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone)]
struct Owner {
    subject: Uuid,
    issuer: String,
    role: String,
    applications: Vec<String>,
    unavailable: bool,
    expires_ms: i64,
    spaces: Vec<pdlc_shared::space_access::SpaceRef>,
}
async fn decision(
    State(state): State<Arc<RwLock<Owner>>>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> axum::response::Response {
    assert_eq!(
        headers["authorization"],
        "Bearer fixture-restricted-owner-reader"
    );
    let state = state.read().unwrap().clone();
    assert_eq!(
        request,
        json!({"schema_version":1,"subjects":[state.subject]})
    );
    if state.unavailable {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let now = chrono::Utc::now();
    Json(json!({"schema_version":1,"decisions":[{
        "schema_version":1,"issuer":state.issuer,"subject":state.subject,"role":state.role,
        "applications":state.applications,"revision":1,"enforced":true,
        "observed_at":now,"expires_at":now+chrono::Duration::milliseconds(state.expires_ms)
    }]}))
    .into_response()
}
async fn space_decision(
    State(state): State<Arc<RwLock<Owner>>>,
    headers: HeaderMap,
    Json(request): Json<pdlc_shared::space_access::SpaceAdmissionRequest>,
) -> axum::response::Response {
    use pdlc_shared::space_access::*;
    assert_eq!(headers["authorization"], "Bearer fixture-space-reader");
    let state = state.read().unwrap().clone();
    assert!(request.valid_for(&state.issuer));
    assert_eq!(request.identity.subject, state.subject.to_string());
    let now = chrono::Utc::now().timestamp();
    Json(SpaceAdmission {
        schema_version: 1,
        identity: request.identity,
        identity_active: true,
        policy_revision: 1,
        policy: SpaceAccessPolicy::Selected {
            spaces: state.spaces,
        },
        inheritance: None,
        validity: AdmissionValidity {
            observed_at: now,
            valid_until: now + 30,
        },
        spaces: request
            .spaces
            .into_iter()
            .map(|space| ConfirmedSpace {
                space,
                revision: 1,
                state: SpaceState::Active,
            })
            .collect(),
    })
    .into_response()
}
async fn git(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new("git")
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .await
        .unwrap()
}
async fn git_ok(root: &std::path::Path, args: &[&str]) {
    let output = git(root, args).await;
    assert!(
        output.status.success(),
        "Git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[tokio::test]
#[ignore = "requires owned FORGE_PRODUCT_TRANSPORT_TEST_DATABASE_URL and isolated process"]
async fn current_owner_and_product_scopes_govern_real_git_and_direct_api() {
    let url = std::env::var("FORGE_PRODUCT_TRANSPORT_TEST_DATABASE_URL").unwrap();
    let pool = cicd::request_database::pool_options()
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        name, "forge_product_transport_test",
        "refuse working database"
    );
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    let uncovered: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname<>'_sqlx_migrations'
         AND NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=c.oid AND t.tgname='forge_human_request_clock')"
    ).fetch_all(&pool).await.unwrap();
    assert!(
        uncovered.is_empty(),
        "new product tables must retain human clock coverage: {uncovered:?}"
    );

    let local = Uuid::new_v4();
    let subject = Uuid::new_v4();
    let project = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let token = "cicd_native_owned_fixture";
    sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'fixture','admin',true,$2)")
        .bind(local).bind(subject.to_string()).execute(&pool).await.unwrap();
    let root = std::env::temp_dir().join(format!("forge-product-transport-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let config = cicd::git_host::GitConfig {
        root: root.join("repositories"),
        token: None,
        internal_token: None,
    };
    let bare = config.root.join("fixture.git");
    let repo = cicd::git_host::create_repository_core(&pool, &config, "fixture", Some("private"))
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'fixture','https://fixture.invalid/repo',$2)")
        .bind(project).bind(repo.id).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')",
    )
    .bind(project)
    .bind(local)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,project_id,scopes)
        VALUES($1,'fixture',$2,'fixture',$3,$4,ARRAY['git:read','git:write'])",
    )
    .bind(token_id)
    .bind(cicd::auth::hash_token(token))
    .bind(local)
    .bind(project)
    .execute(&pool)
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let owner = Arc::new(RwLock::new(Owner {
        subject,
        issuer: issuer.clone(),
        role: "user".into(),
        applications: vec!["ci-cd".into()],
        unavailable: false,
        expires_ms: 30000,
        spaces: Vec::new(),
    }));
    let reader = Router::new()
        .route("/auth/access/decisions", post(decision))
        .route("/admission", post(space_decision))
        .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
        .with_state(owner.clone());
    let registry = Uuid::new_v4();
    let instance = Uuid::new_v4();
    // This isolated test process initializes its configuration before any reader starts.
    unsafe {
        std::env::set_var(
            "CICD_SECRETS_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        );
        std::env::set_var("CICD_ARTIFACTS_DIR", root.join("public-artifacts"));
        std::env::set_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string());
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
        std::env::set_var("CICD_AUTH__CENTRAL_ISSUER", &issuer);
        std::env::set_var("CICD_AUTH__CENTRAL_JWKS_URI", format!("{issuer}/oidc/jwks"));
        std::env::set_var("CICD_SPACES__ADMISSION_URL", format!("{issuer}/admission"));
        std::env::set_var("CICD_SPACES__ADMISSION_TOKEN", "fixture-space-reader");
        std::env::set_var("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_REQUIRED", "true");
        std::env::set_var(
            "CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN",
            "fixture-restricted-owner-reader",
        );
    }
    let reader_task = tokio::spawn(async move { axum::serve(listener, reader).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        config,
        Some("isolated-fixture-secret".into()),
    );
    let app_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let endpoint = format!("{origin}/git/fixture/info/refs?service=git-upload-pack");
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    // A git-only credential does not acquire API scopes through the owner reader.
    assert_eq!(
        http.get(format!("{origin}/api/v1/projects"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let header = format!("http.extraHeader=Authorization: Bearer {token}");
    let remote = format!("{origin}/git/fixture");
    git_ok(&root, &["-c", &header, "clone", &remote, "working"]).await;
    let work = root.join("working");
    git_ok(&work, &["config", "user.name", "Fixture"]).await;
    git_ok(&work, &["config", "user.email", "fixture@example.test"]).await;
    tokio::fs::write(work.join("proof.txt"), "owned fixture\n")
        .await
        .unwrap();
    git_ok(&work, &["add", "proof.txt"]).await;
    git_ok(&work, &["commit", "-m", "Owned fixture"]).await;
    git_ok(
        &work,
        &["-c", &header, "push", "origin", "HEAD:refs/heads/fixture"],
    )
    .await;
    let original_ref = git(&work, &["rev-parse", "HEAD"]).await.stdout;
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['git:read'] WHERE id=$1")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    tokio::fs::write(work.join("proof.txt"), "changed fixture\n")
        .await
        .unwrap();
    git_ok(&work, &["commit", "-am", "Next fixture"]).await;
    assert!(
        !git(
            &work,
            &["-c", &header, "push", "origin", "HEAD:refs/heads/fixture"]
        )
        .await
        .status
        .success()
    );
    // Independent app-policy protection: no namespace frame may be assumed.
    let spaces_active: bool =
        sqlx::query_scalar("SELECT active FROM forge_space_admission_state WHERE singleton")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!spaces_active);
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['api:read','api:write','git:read','git:write'] WHERE id=$1")
        .bind(token_id).execute(&pool).await.unwrap();
    git_ok(
        &work,
        &["-c", &header, "push", "origin", "HEAD:refs/heads/source"],
    )
    .await;
    sqlx::query("INSERT INTO repository_push_configs(repository_id,configuration_id) VALUES($1,$2) ON CONFLICT(repository_id) DO UPDATE SET configuration_id=excluded.configuration_id")
        .bind(repo.id).bind(project).execute(&pool).await.unwrap();
    git_ok(
        &work,
        &["-c", &header, "push", "origin", "HEAD:refs/heads/ci-owner"],
    )
    .await;
    let git_pipeline: Uuid =
        sqlx::query_scalar("SELECT id FROM pipelines WHERE project_id=$1 AND git_ref='ci-owner'")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
    let saved: Uuid =
        sqlx::query_scalar("SELECT owner_subject FROM pipeline_human_owners WHERE pipeline_id=$1")
            .bind(git_pipeline)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        saved, subject,
        "native Git completion retains its authenticated central owner"
    );
    let response = http
        .post(format!("{origin}/api/v1/projects/{project}/pipelines"))
        .bearer_auth(token)
        .json(&json!({"git_ref":"source"}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    let api_pipeline: Uuid = serde_json::from_value(value["pipeline"]["id"].clone()).unwrap();
    let saved: Uuid =
        sqlx::query_scalar("SELECT owner_subject FROM pipeline_human_owners WHERE pipeline_id=$1")
            .bind(api_pipeline)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(saved, subject);
    let denied_job:Uuid=sqlx::query_scalar("SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 ORDER BY s.position,j.position LIMIT 1").bind(api_pipeline).fetch_one(&pool).await.unwrap();
    let allowed_job:Uuid=sqlx::query_scalar("SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 ORDER BY s.position,j.position LIMIT 1").bind(git_pipeline).fetch_one(&pool).await.unwrap();
    let allowed_marker = root.join("allowed-ci-effect");
    sqlx::query("UPDATE jobs SET command=$2 WHERE id=$1")
        .bind(allowed_job)
        .bind(format!("touch '{}'", allowed_marker.display()))
        .execute(&pool)
        .await
        .unwrap();
    let mut allowed_runtime = cicd::config::RuntimeConfig::test_default();
    allowed_runtime.git.root = bare.parent().unwrap().to_path_buf();
    allowed_runtime.artifacts.root = root.join("allowed-artifacts");
    allowed_runtime.runner.mode = cicd::config::RunnerMode::HostShell;
    cicd::runner::run_job_with_config(
        pool.clone(),
        allowed_job,
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        cicd::runner::RuntimeRunnerConfig::from_config(&allowed_runtime),
    )
    .await;
    let allowed_status: String = sqlx::query_scalar("SELECT status FROM jobs WHERE id=$1")
        .bind(allowed_job)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        allowed_status, "success",
        "admitted ordinary CI runs through native workspace, lease and completion"
    );
    assert!(allowed_marker.exists());
    let marker = root.join("revoked-ci-effect");
    sqlx::query("UPDATE jobs SET command=$2 WHERE id=$1")
        .bind(denied_job)
        .bind(format!("touch '{}'", marker.display()))
        .execute(&pool)
        .await
        .unwrap();
    let mut runtime = cicd::config::RuntimeConfig::test_default();
    runtime.git.root = bare.parent().unwrap().to_path_buf();
    runtime.artifacts.root = root.join("artifacts");
    runtime.runner.mode = cicd::config::RunnerMode::HostShell;
    owner.write().unwrap().applications.clear();
    cicd::runner::run_job_with_config(
        pool.clone(),
        denied_job,
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        cicd::runner::RuntimeRunnerConfig::from_config(&runtime),
    )
    .await;
    assert!(
        !marker.exists(),
        "a saved pipeline cannot start a command after app revocation"
    );
    let leases: i64 = sqlx::query_scalar("SELECT count(*) FROM job_leases WHERE job_id=$1")
        .bind(denied_job)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        leases, 0,
        "owner admission precedes native lease/workspace allocation"
    );
    // An admission denial does not fabricate a terminal process result. Create
    // an actual failed native attempt before exercising the public retry rule.
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    sqlx::query("UPDATE jobs SET command='exit 1' WHERE id=$1")
        .bind(denied_job)
        .execute(&pool)
        .await
        .unwrap();
    cicd::runner::run_job_with_config(
        pool.clone(),
        denied_job,
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        cicd::runner::RuntimeRunnerConfig::from_config(&runtime),
    )
    .await;
    let failed: String = sqlx::query_scalar("SELECT status FROM pipelines WHERE id=$1")
        .bind(api_pipeline)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(failed, "failed");
    // A retry is a new operation by its current actor, not the pipeline's historical creator.
    let other = Uuid::new_v4();
    let other_local = Uuid::new_v4();
    let other_token = "cicd_retry_actor_fixture";
    sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'retry-actor','developer',true,$2)").bind(other_local).bind(other.to_string()).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer')",
    )
    .bind(project)
    .bind(other_local)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,project_id,scopes) VALUES($1,'retry-actor',$2,'fixture',$3,$4,ARRAY['api:read','api:write'])").bind(Uuid::new_v4()).bind(cicd::auth::hash_token(other_token)).bind(other_local).bind(project).execute(&pool).await.unwrap();
    owner.write().unwrap().subject = other;
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    let response = http
        .post(format!("{origin}/api/v1/pipelines/{api_pipeline}/retry"))
        .bearer_auth(other_token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "new retry actor: {text}");
    let retry_jobs:Vec<Uuid>=sqlx::query_scalar("SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 ORDER BY s.position,j.position").bind(api_pipeline).fetch_all(&pool).await.unwrap();
    for job in &retry_jobs {
        let (attempt,actor):(i32,Uuid)=sqlx::query_as("SELECT a.attempt_no,o.owner_subject FROM execution_attempts a JOIN execution_attempt_human_owners o ON o.attempt_id=a.id WHERE a.job_id=$1 ORDER BY a.attempt_no DESC LIMIT 1").bind(job).fetch_one(&pool).await.unwrap();
        assert!(attempt >= 2);
        assert_eq!(
            actor, other,
            "queued later stages also retain the explicit retry actor"
        );
        let marker = root.join(format!("retry-{job}"));
        sqlx::query("UPDATE jobs SET command=$2 WHERE id=$1")
            .bind(job)
            .bind(format!("touch '{}'", marker.display()))
            .execute(&pool)
            .await
            .unwrap();
        cicd::runner::run_job_with_config(
            pool.clone(),
            *job,
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            cicd::runner::RuntimeRunnerConfig::from_config(&runtime),
        )
        .await;
        assert!(
            marker.exists(),
            "authorized retry executes as its new actor"
        );
    }
    let original: Uuid =
        sqlx::query_scalar("SELECT owner_subject FROM pipeline_human_owners WHERE pipeline_id=$1")
            .bind(api_pipeline)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        original, subject,
        "original pipeline ownership remains history"
    );
    let manual:Uuid=sqlx::query_scalar("SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 AND j.status='queued' ORDER BY s.position,j.position LIMIT 1").bind(git_pipeline).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE jobs SET manual=true WHERE id=$1")
        .bind(manual)
        .execute(&pool)
        .await
        .unwrap();
    let response = http
        .post(format!("{origin}/api/v1/jobs/{manual}/start"))
        .bearer_auth(other_token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "explicit manual actor: {text}");
    let actor:Uuid=sqlx::query_scalar("SELECT o.owner_subject FROM execution_attempt_human_owners o JOIN execution_attempts a ON a.id=o.attempt_id WHERE a.job_id=$1 ORDER BY a.attempt_no DESC LIMIT 1").bind(manual).fetch_one(&pool).await.unwrap();
    assert_eq!(actor, other);
    let marker = root.join("manual-actor");
    sqlx::query("UPDATE jobs SET command=$2 WHERE id=$1")
        .bind(manual)
        .bind(format!("touch '{}'", marker.display()))
        .execute(&pool)
        .await
        .unwrap();
    cicd::runner::run_job_with_config(
        pool.clone(),
        manual,
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        cicd::runner::RuntimeRunnerConfig::from_config(&runtime),
    )
    .await;
    assert!(marker.exists());
    owner.write().unwrap().subject = subject;
    // Real external runner protocol: a machine lease does not preserve revoked human access.
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    let response = http
        .post(format!("{origin}/api/v1/projects/{project}/pipelines"))
        .bearer_auth(token)
        .json(&json!({"git_ref":"source"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let remote_value: serde_json::Value = response.json().await.unwrap();
    let remote_pipeline: Uuid =
        serde_json::from_value(remote_value["pipeline"]["id"].clone()).unwrap();
    let remote_job:Uuid=sqlx::query_scalar("SELECT j.id FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1 ORDER BY s.position,j.position LIMIT 1").bind(remote_pipeline).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE jobs SET required_secrets=ARRAY['FIXTURE_TOKEN'],artifact_paths=ARRAY['fixture.bin'] WHERE id=$1")
        .bind(remote_job)
        .execute(&pool)
        .await
        .unwrap();
    // Generic administrative writes must not commit after their original owner proof.
    // Hold a real row lock until the short proof expires, then observe durable state.
    owner.write().unwrap().role = "admin".into();
    let original_name: String = sqlx::query_scalar("SELECT name FROM projects WHERE id=$1")
        .bind(project)
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut row_lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM projects WHERE id=$1 FOR UPDATE")
        .bind(project)
        .execute(&mut *row_lock)
        .await
        .unwrap();
    owner.write().unwrap().expires_ms = 1500;
    let request = http
        .patch(format!("{origin}/api/v1/projects/{project}"))
        .bearer_auth(token)
        .json(&json!({"name":"must-not-survive-expired-proof"}));
    let pending = tokio::spawn(async move { request.send().await.unwrap() });
    let mut blocked = false;
    for _ in 0..100 {
        blocked = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'UPDATE projects SET name%')")
            .fetch_one(&pool).await.unwrap();
        if blocked {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        blocked,
        "the real administrative SQL must reach the held row lock"
    );
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    row_lock.rollback().await.unwrap();
    let response = pending.await.unwrap();
    assert!(
        !response.status().is_success(),
        "expired generic write must fail"
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let actual: String = sqlx::query_scalar("SELECT name FROM projects WHERE id=$1")
        .bind(project)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        actual, original_name,
        "HTTP expiry must prevent the durable generic write"
    );
    // A later deferred trigger must not let COMMIT outlive the original clock.
    sqlx::raw_sql(
        "CREATE FUNCTION qa_late_generic_commit() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_sleep(2); RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER zz_qa_late_generic_commit AFTER UPDATE ON projects
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_late_generic_commit();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let response = http
        .patch(format!("{origin}/api/v1/projects/{project}"))
        .bearer_auth(token)
        .json(&json!({"name":"must-not-survive-late-commit"}))
        .send()
        .await
        .unwrap();
    assert!(!response.status().is_success());
    tokio::time::sleep(std::time::Duration::from_millis(2300)).await;
    let actual: String = sqlx::query_scalar("SELECT name FROM projects WHERE id=$1")
        .bind(project)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        actual, original_name,
        "a late deferred trigger cannot outlive the original proof"
    );
    sqlx::raw_sql("DROP TRIGGER zz_qa_late_generic_commit ON projects; DROP FUNCTION qa_late_generic_commit();")
        .execute(&pool).await.unwrap();
    owner.write().unwrap().expires_ms = 30000;
    let response = http
        .patch(format!("{origin}/api/v1/projects/{project}"))
        .bearer_auth(token)
        .json(&json!({"name":"fresh-administrator-write"}))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "a fresh current administrator may still update the project"
    );
    let actual: String = sqlx::query_scalar("SELECT name FROM projects WHERE id=$1")
        .bind(project)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        actual, "fresh-administrator-write",
        "pool reuse does not retain an expired prior request"
    );
    owner.write().unwrap().role = "admin".into();
    let response = http
        .post(format!("{origin}/api/v1/projects/{project}/secrets"))
        .bearer_auth(token)
        .json(&json!({"key":"FIXTURE_TOKEN","value":"fixture-only-secret"}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "fixture secret setup: {status} {body}");
    owner.write().unwrap().role = "user".into();
    sqlx::query("UPDATE job_queue SET priority=10000 WHERE job_id=$1")
        .bind(remote_job)
        .execute(&pool)
        .await
        .unwrap();
    let runner = Uuid::new_v4();
    let runner_token = "cicd_runner_owned_fixture";
    sqlx::query("INSERT INTO runners(id,name,status,credential_hash,credential_expires_at,capabilities,last_seen_at) VALUES($1,'owned-protocol','online',$2,now()+interval '1 hour',$3,now())").bind(runner).bind(cicd::auth::hash_token(runner_token)).bind(json!({"executorKinds":["shell"]})).execute(&pool).await.unwrap();
    let poll_url = format!("{origin}/api/v1/runner/work:poll");
    let poll = json!({"protocolVersion":1,"capacity":{"freeSlots":1},"waitSeconds":0,"tags":[]});
    owner.write().unwrap().applications.clear();
    assert_eq!(
        http.post(&poll_url)
            .bearer_auth(runner_token)
            .json(&poll)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let pending: String = sqlx::query_scalar("SELECT status FROM jobs WHERE id=$1")
        .bind(remote_job)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        pending, "queued",
        "denied remote offer transaction rolls back its claim"
    );
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    let response = http
        .post(&poll_url)
        .bearer_auth(runner_token)
        .json(&poll)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let offer: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{offer}");
    assert_eq!(offer["attempt"]["jobId"], remote_job.to_string());
    let lease = offer["leaseId"].as_str().unwrap();
    let credential = offer["leaseToken"].as_str().unwrap();
    let fencing = offer["fencingToken"].as_i64().unwrap();
    let control = json!({"protocolVersion":1,"leaseToken":credential,"fencingToken":fencing});
    for action in ["ack", "renew"] {
        let response = http
            .post(format!("{origin}/api/v1/runner/leases/{lease}/{action}"))
            .bearer_auth(runner_token)
            .json(&control)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: serde_json::Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{action}: {value}");
        let until = chrono::DateTime::parse_from_rfc3339(value["leaseExpiresAt"].as_str().unwrap())
            .unwrap();
        assert!(
            until <= chrono::Utc::now() + chrono::Duration::seconds(30),
            "runner renew cannot add a 120s lease to a 30s owner proof"
        );
    }
    let secrets = json!({"protocolVersion":1,"leaseToken":credential,"fencingToken":fencing,"secretNames":["FIXTURE_TOKEN"]});
    let secret_url = format!("{origin}/api/v1/runner/leases/{lease}/secrets:resolve");
    let response = http
        .post(&secret_url)
        .bearer_auth(runner_token)
        .json(&secrets)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    let until = chrono::DateTime::parse_from_rfc3339(value["expiresAt"].as_str().unwrap()).unwrap();
    assert!(
        until <= chrono::Utc::now() + chrono::Duration::seconds(30),
        "secret envelope preserves original owner clock"
    );
    // Artifact publication is a transaction, not a response-only freshness check.
    let remote_attempt = offer["attempt"]["id"].as_str().unwrap();
    let artifact_url = format!("{origin}/api/v1/runner/leases/{lease}/artifacts");
    let upload = http
        .post(&artifact_url)
        .bearer_auth(runner_token)
        .header("x-runner-protocol-version", "1")
        .header("x-lease-token", credential)
        .header("x-fencing-token", fencing.to_string())
        .header("x-attempt-id", remote_attempt)
        .header("x-artifact-path", "fixture.bin")
        .header("x-artifact-name", "fixture.bin")
        .body("fixture");
    let response = upload.try_clone().unwrap().send().await.unwrap();
    let status = response.status();
    let published: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "artifact publication: {published}");
    let before_count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let before_files = std::fs::read_dir(root.join("public-artifacts"))
        .unwrap()
        .count();
    sqlx::raw_sql("CREATE SEQUENCE qa_artifact_commit_seen;
        CREATE FUNCTION qa_artifact_commit_delay() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM nextval('qa_artifact_commit_seen'); PERFORM pg_sleep(2); RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER a_qa_artifact_commit_delay AFTER INSERT ON forge_request_commit_fences
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_artifact_commit_delay();")
        .execute(&pool).await.unwrap();
    owner.write().unwrap().expires_ms = 1500;
    assert_eq!(
        upload.try_clone().unwrap().send().await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let seen: bool = sqlx::query_scalar("SELECT is_called FROM qa_artifact_commit_seen")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(seen);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        before_count
    );
    assert_eq!(
        std::fs::read_dir(root.join("public-artifacts"))
            .unwrap()
            .count(),
        before_files,
        "failed COMMIT removes only its unpublished file"
    );
    sqlx::raw_sql("DROP TRIGGER a_qa_artifact_commit_delay ON forge_request_commit_fences; DROP FUNCTION qa_artifact_commit_delay(); DROP SEQUENCE qa_artifact_commit_seen;").execute(&pool).await.unwrap();
    owner.write().unwrap().expires_ms = 30000;
    let begin_url = format!("{origin}/api/v1/runner/leases/{lease}/artifact-sessions:begin");
    let begin_input = json!({"protocolVersion":1,"leaseToken":credential,"fencingToken":fencing,"attemptId":remote_attempt,"artifactPath":"fixture.bin","artifactName":"session.bin","declaredSize":7});
    let response = http
        .post(&begin_url)
        .bearer_auth(runner_token)
        .json(&begin_input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let session: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "session begin: {session}");
    let sid = session["sessionId"].as_str().unwrap();
    let complete_input = json!({"protocolVersion":1,"leaseToken":credential,"fencingToken":fencing,"attemptId":remote_attempt});
    let chunk = http
        .post(format!(
            "{origin}/api/v1/runner/leases/{lease}/artifact-sessions/{sid}/chunks"
        ))
        .bearer_auth(runner_token)
        .header("x-runner-protocol-version", "1")
        .header("x-lease-token", credential)
        .header("x-fencing-token", fencing.to_string())
        .header("x-attempt-id", remote_attempt)
        .header("x-chunk-index", "0")
        .header(
            "x-chunk-sha256",
            format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(b"fixture")),
        )
        .body("fixture");
    owner.write().unwrap().applications.clear();
    assert_eq!(
        chunk.try_clone().unwrap().send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    sqlx::raw_sql("CREATE FUNCTION qa_artifact_commit_delay() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(2); RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER a_qa_artifact_commit_delay AFTER INSERT ON forge_request_commit_fences DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_artifact_commit_delay();").execute(&pool).await.unwrap();
    owner.write().unwrap().expires_ms = 1500;
    assert_eq!(
        chunk.try_clone().unwrap().send().await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT received_bytes FROM artifact_upload_sessions WHERE id=$1"
        )
        .bind(Uuid::parse_str(sid).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM artifact_upload_chunks WHERE session_id=$1"
        )
        .bind(Uuid::parse_str(sid).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    sqlx::raw_sql("DROP TRIGGER a_qa_artifact_commit_delay ON forge_request_commit_fences; DROP FUNCTION qa_artifact_commit_delay();").execute(&pool).await.unwrap();
    owner.write().unwrap().expires_ms = 30000;
    let response = chunk.try_clone().unwrap().send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "chunk: {text}");
    let complete_url =
        format!("{origin}/api/v1/runner/leases/{lease}/artifact-sessions/{sid}/complete");
    let response = http
        .post(&complete_url)
        .bearer_auth(runner_token)
        .json(&complete_input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let completed: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "complete: {completed}");
    let response = http
        .post(&complete_url)
        .bearer_auth(runner_token)
        .json(&complete_input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let repeated: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["id"], repeated["id"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        before_count + 1,
        "session completion replay cannot create another artifact"
    );
    owner.write().unwrap().applications.clear();
    assert_eq!(
        http.post(format!("{origin}/api/v1/runner/leases/{lease}/renew"))
            .bearer_auth(runner_token)
            .json(&control)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http.post(&secret_url)
            .bearer_auth(runner_token)
            .json(&secrets)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    owner.write().unwrap().unavailable = true;
    assert_eq!(
        http.post(format!("{origin}/api/v1/runner/leases/{lease}/renew"))
            .bearer_auth(runner_token)
            .json(&control)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    owner.write().unwrap().unavailable = false;
    // Administrator-owned schedule fires resolve a current role, not a saved admin bit.
    owner.write().unwrap().role = "admin".into();
    let schedule_input = json!({"cron":"* * * * *","git_ref":"source","enabled":true});
    let response = http
        .post(format!("{origin}/api/v1/projects/{project}/schedules"))
        .bearer_auth(token)
        .json(&schedule_input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "schedule: {value}");
    let schedule: Uuid = serde_json::from_value(value["id"].clone()).unwrap();
    let actual_owner: Uuid = sqlx::query_scalar("SELECT owner_subject FROM schedules WHERE id=$1")
        .bind(schedule)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual_owner, subject);
    sqlx::query("UPDATE schedules SET next_fire_at=now()-interval '1 minute' WHERE id=$1")
        .bind(schedule)
        .execute(&pool)
        .await
        .unwrap();
    owner.write().unwrap().unavailable = true;
    assert_eq!(
        cicd::outbox::fire_due_schedules_with_git_root(&pool, bare.parent().unwrap()).await,
        0
    );
    let (fire, state): (Uuid, String) =
        sqlx::query_as("SELECT id,status FROM schedule_fires WHERE schedule_id=$1")
            .bind(schedule)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        state, "pending",
        "Auth outage retains the original fire/key for safe retry"
    );
    let fire_owner: Uuid =
        sqlx::query_scalar("SELECT owner_subject FROM schedule_fires WHERE id=$1")
            .bind(fire)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(fire_owner, subject);
    owner.write().unwrap().unavailable = false;
    owner.write().unwrap().role = "user".into();
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    assert_eq!(
        cicd::outbox::fire_due_schedules_with_git_root(&pool, bare.parent().unwrap()).await,
        0
    );
    let state: String = sqlx::query_scalar("SELECT status FROM schedule_fires WHERE id=$1")
        .bind(fire)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        state, "failed",
        "demoted admin cannot retain scheduled administrative execution"
    );
    owner.write().unwrap().role = "admin".into();
    let response = http
        .patch(format!("{origin}/api/v1/schedules/{schedule}"))
        .bearer_auth(token)
        .json(&schedule_input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query("UPDATE schedules SET next_fire_at=now()-interval '20 seconds' WHERE id=$1")
        .bind(schedule)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        cicd::outbox::fire_due_schedules_with_git_root(&pool, bare.parent().unwrap()).await,
        1
    );
    let scheduled_pipeline: Uuid = sqlx::query_scalar(
        "SELECT pipeline_id FROM schedule_fires WHERE schedule_id=$1 AND status='triggered'",
    )
    .bind(schedule)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (stored, admin): (Uuid, bool) = sqlx::query_as(
        "SELECT owner_subject,requires_admin FROM pipeline_human_owners WHERE pipeline_id=$1",
    )
    .bind(scheduled_pipeline)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, subject);
    assert!(admin);
    let legacy = Uuid::new_v4();
    sqlx::query("INSERT INTO schedules(id,project_id,cron,git_ref,enabled,next_fire_at) VALUES($1,$2,'* * * * *','source',true,now()-interval '40 seconds')").bind(legacy).bind(project).execute(&pool).await.unwrap();
    assert_eq!(
        cicd::outbox::fire_due_schedules_with_git_root(&pool, bare.parent().unwrap()).await,
        0
    );
    let legacy_owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_subject FROM schedule_fires WHERE schedule_id=$1")
            .bind(legacy)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        legacy_owner, None,
        "historical automation never selects the first active administrator"
    );
    owner.write().unwrap().role = "user".into();
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    sqlx::query("UPDATE repository_push_configs SET configuration_id=NULL WHERE repository_id=$1")
        .bind(repo.id)
        .execute(&pool)
        .await
        .unwrap();
    let collection = format!("{origin}/api/v1/catalog/repositories/{}/pulls", repo.id);
    let input = json!({"title":"Application clock fixture","source_branch":"source","target_branch":"fixture"});
    let response = http
        .post(&collection)
        .bearer_auth(token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let review: serde_json::Value = response.json().await.unwrap();
    let action = format!("{collection}/{}/action", review["number"].as_i64().unwrap());
    let hook = bare.join("hooks/reference-transaction");
    let original_hook = tokio::fs::read(&hook).await.ok();
    let hook_script = format!(
        "#!/bin/sh\nset -eu\ncat >/dev/null\nif test \"$1\" = prepared; then\ntouch '{}'\nsleep 4\ntouch '{}'\nfi\n",
        root.join("prepared").display(),
        root.join("late-effect").display()
    );
    tokio::fs::write(&hook, hook_script).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
    }
    owner.write().unwrap().expires_ms = 2500;
    let result = http
        .post(&action)
        .bearer_auth(token)
        .json(&json!({"action":"merge"}))
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        root.join("prepared").exists(),
        "real ref preparation reached with app policy alone"
    );
    assert_eq!(
        git(
            &root,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                "refs/heads/fixture"
            ]
        )
        .await
        .stdout,
        original_ref
    );
    tokio::time::sleep(std::time::Duration::from_millis(4300)).await;
    assert!(
        !root.join("late-effect").exists(),
        "native hook cannot outlive original application clock"
    );
    owner.write().unwrap().expires_ms = 30000;
    assert_eq!(
        http.post(&action)
            .bearer_auth(token)
            .json(&json!({"action":"merge"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "unknown original preparation is reconciled, never blindly republished"
    );
    if let Some(bytes) = original_hook {
        tokio::fs::write(&hook, bytes).await.unwrap();
    } else {
        tokio::fs::remove_file(&hook).await.unwrap();
    }
    // A separate legitimate review exercises the real deferred application COMMIT fence.
    let response = http
        .post(&collection)
        .bearer_auth(token)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let next: serde_json::Value = response.json().await.unwrap();
    let next_action = format!("{collection}/{}/action", next["number"].as_i64().unwrap());
    sqlx::raw_sql(
        "CREATE SEQUENCE qa_commit_seen;
        CREATE FUNCTION qa_commit_delay() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM nextval('qa_commit_seen'); PERFORM pg_sleep(2); RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER a_qa_commit_delay AFTER INSERT ON forge_request_commit_fences
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_commit_delay();",
    )
    .execute(&pool)
    .await
    .unwrap();
    owner.write().unwrap().expires_ms = 1500;
    assert_eq!(
        http.post(&next_action)
            .bearer_auth(token)
            .json(&json!({"action":"merge"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let seen: bool = sqlx::query_scalar("SELECT is_called FROM qa_commit_seen")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(seen, "real deferred COMMIT was entered");
    assert_eq!(
        git(
            &root,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                "refs/heads/fixture"
            ]
        )
        .await
        .stdout,
        original_ref
    );
    let status: String = sqlx::query_scalar("SELECT status FROM pull_requests WHERE id=$1")
        .bind(Uuid::parse_str(next["id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        status, "open",
        "expired application clock cannot commit a merged PR"
    );
    sqlx::raw_sql(
        "DROP TRIGGER a_qa_commit_delay ON forge_request_commit_fences;
        DROP FUNCTION qa_commit_delay(); DROP SEQUENCE qa_commit_seen;",
    )
    .execute(&pool)
    .await
    .unwrap();
    owner.write().unwrap().expires_ms = 30000;
    // A project-pinned token remains pinned even for a current central admin.
    let global_token = "cicd_native_global_fixture";
    sqlx::query(
        "INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,scopes)
        VALUES($1,'global-fixture',$2,'fixture',$3,ARRAY['api:read','api:write'])",
    )
    .bind(Uuid::new_v4())
    .bind(cicd::auth::hash_token(global_token))
    .bind(local)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        http.get(format!("{origin}/api/v1/users"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "old local admin cannot manage users under current owner=user"
    );
    // Reading build evidence never grants the right to manufacture runner output.
    for resource in ["logs", "test-report", "artifacts"] {
        assert_eq!(
            http.post(format!(
                "{origin}/api/v1/jobs/{}/{resource}",
                Uuid::new_v4()
            ))
            .bearer_auth(global_token)
            .json(&json!({"message":"untrusted evidence"}))
            .send()
            .await
            .unwrap()
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    owner.write().unwrap().role = "admin".into();
    assert_eq!(
        http.get(format!("{origin}/api/v1/runners"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "current admin does not remove the original project pin"
    );
    assert_eq!(
        http.get(format!("{origin}/api/v1/runners"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        http.get(format!("{origin}/api/v1/users"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "admin role does not change a product credential audience"
    );
    owner.write().unwrap().role = "user".into();
    assert_eq!(
        http.get(format!("{origin}/api/v1/runners"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http.get(format!("{origin}/api/v1/users"))
            .bearer_auth(global_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "a product credential is not forwarded to the Auth directory"
    );
    // Compose independent owner-app and space policies using the same stored PAT.
    // These are owned fixture projections, never writes to an installed catalog.
    use pdlc_shared::space_access::SpaceRef;
    let visible = SpaceRef {
        registry_instance_id: registry,
        space_id: Uuid::new_v4(),
    };
    let hidden = SpaceRef {
        registry_instance_id: registry,
        space_id: Uuid::new_v4(),
    };
    owner.write().unwrap().spaces = vec![visible.clone()];
    let hidden_repo = cicd::git_host::create_repository_core(
        &pool,
        &cicd::git_host::GitConfig {
            root: root.join("repositories"),
            token: None,
            internal_token: None,
        },
        "hidden",
        Some("private"),
    )
    .await
    .unwrap();
    for (repository, space) in [(repo.id, visible), (hidden_repo.id, hidden)] {
        let group = Uuid::new_v4();
        sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Owned fixture space')")
            .bind(group)
            .bind(format!("fixture-{}", group.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE repository_catalog SET group_id=$1 WHERE id=$2")
            .bind(group)
            .bind(repository)
            .execute(&pool)
            .await
            .unwrap();
        let binding = json!({"schema_version":1,"operation_id":Uuid::new_v4(),
            "generation":1,"state":"active",
            "namespace":{"registry_instance_id":registry,"namespace_id":space.space_id},
            "resource":{"resource_id":group,"instance_id":instance,"kind":"git_group"}});
        sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
            .bind(group).bind(registry).bind(space.space_id).bind(binding).execute(&pool).await.unwrap();
        let operation = Uuid::new_v4();
        let command =
            json!({"schema_version":2,"space":space,"operation_id":operation,"revision":1});
        sqlx::query("INSERT INTO forge_space_catalog(registry_instance_id,space_id,group_id,revision,state,operation_id,command) VALUES($1,$2,$3,1,'active',$4,$5)")
            .bind(registry).bind(space.space_id).bind(group).bind(operation).bind(command).execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE forge_space_admission_state SET active=true WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['api:read','api:write','git:read','git:write'] WHERE token_hash=$1")
        .bind(cicd::auth::hash_token(global_token)).execute(&pool).await.unwrap();
    for (path, credential, expected) in [
        (
            "/git/fixture/info/refs?service=git-upload-pack".to_string(),
            token,
            StatusCode::OK,
        ),
        (
            "/git/hidden/info/refs?service=git-upload-pack".to_string(),
            global_token,
            StatusCode::FORBIDDEN,
        ),
        (
            format!("/api/v1/catalog/repositories/{}/pulls", repo.id),
            token,
            StatusCode::OK,
        ),
        (
            format!("/api/v1/catalog/repositories/{}/pulls", hidden_repo.id),
            global_token,
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = http
            .get(format!("{origin}{path}"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert_eq!(status, expected, "product owner +spaces {path}: {body}");
    }
    let response = http
        .get(format!("{origin}/api/v1/catalog/repositories?limit=1"))
        .bearer_auth(global_token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: serde_json::Value = response.json().await.unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], repo.id.to_string());
    git_ok(
        &work,
        &[
            "-c",
            &header,
            "push",
            "origin",
            "HEAD:refs/heads/product-with-spaces",
        ],
    )
    .await;
    let response = http
        .post(&collection)
        .bearer_auth(token)
        .json(&input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "API-scoped PAT also retains space write fence: {body}"
    );
    sqlx::query("UPDATE api_tokens SET scopes=ARRAY['git:read','git:write'] WHERE id=$1")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    owner.write().unwrap().applications.clear();
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(
        !git(&work, &["-c", &header, "fetch", "origin"])
            .await
            .status
            .success()
    );
    assert!(
        !git(
            &work,
            &["-c", &header, "push", "origin", "HEAD:refs/heads/fixture"]
        )
        .await
        .status
        .success(),
        "an application revoke also rejects a write-scoped product PAT"
    );
    let current_ref = git(
        &root,
        &[
            "--git-dir",
            bare.to_str().unwrap(),
            "rev-parse",
            "refs/heads/fixture",
        ],
    )
    .await
    .stdout;
    assert_eq!(
        current_ref, original_ref,
        "denied pushes cannot change the native ref"
    );
    owner.write().unwrap().applications = vec!["ci-cd".into()];
    owner.write().unwrap().unavailable = true;
    assert_eq!(
        http.get(&endpoint)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    app_task.abort();
    reader_task.abort();
    pool.close().await;
}
