//! Actual registered-reader middleware and PG route; retained producer fixtures, no runner effects.
use super::*;
use crate::{
    delegated_credentials::{StoredCredential, digest as context_digest},
    registered_devops_evidence::{EvidenceQuery, tests::fixture},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    routing::get as get_handler,
};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{collections::BTreeMap, str::FromStr, time::Duration};
use tower::ServiceExt;

const CHILD: &str = "FORGE_NAMESPACE_EVIDENCE_PG_CHILD";
const READER: &str = "00000000-0000-0000-0000-000000000007";
const INSTANCE: &str = "00000000-0000-0000-0000-000000000009";
const REGISTRY: &str = "00000000-0000-0000-0000-000000000010";
const POSITIVE: &str = "task_delivery::namespace_evidence_pg_tests::namespace_devops_evidence_pg_registered_reader_retained_candidate_and_empty_target";
const NEGATIVE: &str = "task_delivery::namespace_evidence_pg_tests::namespace_devops_evidence_pg_foreign_project_and_assignment_mismatch_are_read_only";

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "owned Git fixture failed");
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, (u32, String)> {
    use std::os::unix::fs::PermissionsExt;
    let mut result = BTreeMap::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        let hash = if metadata.is_dir() {
            for (name, value) in inventory(&path) {
                result.insert(PathBuf::from(path.file_name().unwrap()).join(name), value);
            }
            String::new()
        } else {
            assert!(metadata.is_file());
            sha(&std::fs::read(&path).unwrap())
        };
        result.insert(
            path.file_name().unwrap().into(),
            (metadata.permissions().mode(), hash),
        );
    }
    result
}

async fn rows(pool: &sqlx::PgPool) -> BTreeMap<String, Value> {
    let names: Vec<String> = sqlx::query_scalar("SELECT table_name FROM information_schema.tables WHERE table_schema=current_schema() AND table_type='BASE TABLE' ORDER BY table_name")
        .fetch_all(pool).await.unwrap();
    let mut result = BTreeMap::new();
    for name in names {
        // PostgreSQL owns these identifiers; never use any HTTP locator as SQL text.
        let identifier = name.replace('"', "\"\"");
        let value: Value = sqlx::query_scalar(&format!(
            "SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]'::jsonb) FROM \"{identifier}\" t"
        )).fetch_one(pool).await.unwrap();
        result.insert(name, value);
    }
    result
}

fn url(repository: Uuid, query: &EvidenceQuery) -> String {
    let mut uri = reqwest::Url::parse(&format!(
        "http://fixture.invalid/api/v1/namespace-repositories/{repository}/devops-evidence"
    ))
    .unwrap();
    {
        let mut pairs = uri.query_pairs_mut();
        for (name, value) in [
            (
                "registry_instance_id",
                query.registry_instance_id.to_string(),
            ),
            ("namespace_id", query.namespace_id.to_string()),
            ("base_ref", query.base_ref.clone()),
            ("base_commit", query.base_commit.clone()),
            (
                "delivery_configuration_name",
                query.delivery_configuration_name.clone(),
            ),
            ("project_id", query.project_id.to_string()),
            ("source_pipeline_id", query.source_pipeline_id.to_string()),
            ("operation_key", query.operation_key.clone()),
            ("source_commit", query.source_commit.clone()),
            ("request_hash", query.request_hash.clone()),
            ("task_id", query.task_id.to_string()),
            ("root_task_id", query.root_task_id.to_string()),
            ("assignment_id", query.assignment_id.to_string()),
            ("execution_id", query.execution_id.to_string()),
            ("fencing_token", query.fencing_token.to_string()),
        ] {
            pairs.append_pair(name, &value);
        }
    }
    format!("{}?{}", uri.path(), uri.query().unwrap())
}

async fn get(app: &Router, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::get(uri);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        app.clone().oneshot(request.body(Body::empty()).unwrap()),
    )
    .await
    .expect("one-connection readback must not acquire a second pool slot")
    .unwrap();
    let status = response.status();
    if status == StatusCode::OK {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn exercise(pool: sqlx::PgPool, negative: bool) {
    let root = std::env::temp_dir().join(format!("forge-namespace-pg-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let root = Temp(root.canonicalize().unwrap());
    let git_root = root.0.join("git");
    let source = root.0.join("source");
    let artifacts = root.0.join("artifacts");
    for path in [&git_root, &source, &artifacts] {
        std::fs::create_dir(path).unwrap();
    }
    let yaml = "version: 1\njobs:\n  build:\n    commands: [\"printf fixture\"]\n    artifacts:\n      paths: [product.txt]\n";
    std::fs::write(source.join(".forge-ci.yml"), yaml).unwrap();
    git(&source, &["init", "--initial-branch=main"]);
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
            "retained candidate fixture",
        ],
    );
    let commit = git(&source, &["rev-parse", "HEAD"]);
    let (mut query, record, mut receipt) = fixture();
    let mut credential = record.delegated_credential.unwrap().0;
    // Match 0094's envelope shape, but keep an invalid nonce/ciphertext: R must never decrypt it.
    credential["encrypted_child"] = json!("v1:AA==:AA==");
    let mut stored = StoredCredential::from_value(&credential).unwrap();
    assert_ne!(stored.subject.to_string(), READER);
    stored.context.intent.source_commit = commit.clone();
    stored.context_hash = context_digest(&stored.context).unwrap();
    receipt.request.source_commit = commit.clone();
    receipt.request_hash = sha(&serde_json::to_vec(&receipt.request).unwrap());
    query.source_commit = commit.clone();
    query.base_commit = commit.clone();
    query.request_hash = receipt.request_hash.clone();
    query.registry_instance_id = Uuid::parse_str(REGISTRY).unwrap();
    let repository = receipt.request.repository_id;
    let pipeline = query.source_pipeline_id;
    crate::api::sdlc_workspace::task_pipeline::renewal::pg_tests::seed(&pool, &stored, pipeline)
        .await;
    let (group, namespace, mut command): (Uuid, Uuid, Value) = sqlx::query_as("SELECT r.group_id,b.namespace_id,b.command FROM repository_catalog r JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=$1")
        .bind(repository).fetch_one(&pool).await.unwrap();
    command["namespace"]["registry_instance_id"] = json!(REGISTRY);
    command["resource"]["instance_id"] = json!(INSTANCE);
    sqlx::query(
        "UPDATE forge_workspace_projects SET registry_instance_id=$1 WHERE namespace_id=$2",
    )
    .bind(query.registry_instance_id)
    .bind(namespace)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE forge_namespace_bindings SET registry_instance_id=$1,command=$2 WHERE resource_id=$3")
        .bind(query.registry_instance_id).bind(command).bind(group).execute(&pool).await.unwrap();
    query.namespace_id = namespace;
    sqlx::query("UPDATE projects SET name=$2 WHERE id=$1")
        .bind(query.project_id)
        .bind(&query.delivery_configuration_name)
        .execute(&pool)
        .await
        .unwrap();
    let storage: String =
        sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1")
            .bind(repository)
            .fetch_one(&pool)
            .await
            .unwrap();
    let name: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id=$1")
        .bind(repository)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        storage, name,
        "candidate and catalog must address the same actual bare repository"
    );
    let bare = git_root.join(format!("{storage}.git"));
    git(
        &root.0,
        &[
            "clone",
            "--bare",
            source.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let job: Uuid = sqlx::query_scalar(
        "SELECT preparation_job_id FROM task_pipeline_intents WHERE pipeline_id=$1",
    )
    .bind(pipeline)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE pipelines SET status='success',finished_at=now() WHERE id=$1")
        .bind(pipeline)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE stages SET name='build',status='success' WHERE pipeline_id=$1")
        .bind(pipeline)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET name='build',status='success',finished_at=now(),artifact_paths=ARRAY['product.txt'] WHERE id=$1")
        .bind(job).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,started_at,finished_at,exit_code) VALUES($1,$2,1,'success',now(),now(),0)")
        .bind(receipt.request.attempt_id).bind(job).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_status,lease_expires_at,acknowledged_at,completed_at,completion_received_at,terminal_status) VALUES($1,$2,$3,'retained-fixture',1,'completed',now(),now(),now(),now(),'success')")
        .bind(receipt.request.lease_id).bind(job).bind(receipt.request.attempt_id).execute(&pool).await.unwrap();
    let plan = json!({"stages":[{"name":"build","jobs":[{"name":"build","artifact_paths":["product.txt"]}]}]});
    let plan_hash = sha(&serde_json::to_vec(&plan).unwrap());
    sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,'repository','fixture/1',$2,$2,$3,$4,$5,$6)")
        .bind(pipeline).bind(&commit).bind(sha(yaml.as_bytes())).bind(&plan_hash).bind(yaml).bind(plan).execute(&pool).await.unwrap();
    let artifact = Uuid::new_v4();
    let bytes = b"retained Developer artifact\n";
    let artifact_path = artifacts.join("product.txt");
    std::fs::write(&artifact_path, bytes).unwrap();
    sqlx::query("INSERT INTO artifacts(id,job_id,attempt_id,name,storage_path,sha256,size_bytes,expires_at) VALUES($1,$2,$3,'product.txt',$4,$5,$6,now()+interval '1 hour')")
        .bind(artifact).bind(job).bind(receipt.request.attempt_id).bind(artifact_path.to_str().unwrap()).bind(sha(bytes)).bind(bytes.len() as i64).execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(
        ledger::claim_task_preparation(&mut tx, pipeline, &receipt.request)
            .await
            .unwrap()
    );
    ledger::append_delegated_receipt(&mut tx, stored.subject, &stored.context_hash, &receipt)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let environment = Uuid::new_v4();
    sqlx::query("INSERT INTO environments(id,project_id,name,protected,required_approvals) VALUES($1,$2,'fixture',false,0)")
        .bind(environment).bind(query.project_id).execute(&pool).await.unwrap();
    let policy = DeliveryPolicy {
        origin: "http://127.0.0.1:1".into(),
        health_path: "/health".into(),
        health_body_sha256: "1".repeat(64),
        acceptance_path: "/acceptance".into(),
        acceptance_body_sha256: "2".repeat(64),
        task_execution: Some(probes::TaskExecutionPolicy {
            environment: "nonproduction".into(),
            environment_id: environment,
            repository_id: repository,
            tracker_instance_id: receipt.request.binding.tracker_instance_id.clone(),
            tracker_project_id: receipt.request.binding.tracker_project_id,
            holder_subject: "fixture-fleet".into(),
        }),
        delivery_selection: if negative {
            None
        } else {
            Some(super::owner_selection::Policy {
                version: 1,
                action: super::owner_selection::Action::Deploy,
                stage_name: "build".into(),
                job_name: "build".into(),
                artifact_name: "product.txt".into(),
            })
        },
    };
    let target = root.0.join("target");
    Target::open(&target, query.project_id, policy.clone(), true).unwrap();
    let policy_path = root.0.join("policy.json");
    std::fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let mut config = crate::config::RuntimeConfig::test_default();
    config.git.root = git_root.clone();
    config.artifacts.root = artifacts;
    config.sdlc_workspace.local_delivery_root = Some(target);
    config.sdlc_workspace.local_delivery_policy = Some(policy_path);
    let state = Arc::new(AppState {
        pool: Some(pool.clone()),
        namespace_admission_pool: Some(pool.clone()),
        auth_secret: None,
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()),
    });
    let app = crate::registered_devops_evidence::routes().with_state(state);
    let foreign_project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://fixture.invalid/foreign.git')")
        .bind(foreign_project).bind(format!("foreign-{foreign_project}")).execute(&pool).await.unwrap();
    let before_rows = rows(&pool).await;
    let before_files = inventory(&root.0);
    let uri = url(repository, &query);
    assert_eq!(get(&app, &uri, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        get(&app, &uri, Some("sdlc_pat_foreign")).await.0,
        StatusCode::FORBIDDEN
    );
    if negative {
        let mut mismatch = query.clone();
        mismatch.project_id = foreign_project;
        let (status, body) = get(&app, &url(repository, &mismatch), Some("sdlc_pat_reader")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.to_string().contains("devops_project_mapping_mismatch"));
        let mut mismatch = query.clone();
        mismatch.assignment_id = Uuid::new_v4();
        let (status, body) = get(&app, &url(repository, &mismatch), Some("sdlc_pat_reader")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body.to_string()
                .contains("original operation binding or hash mismatch")
        );
    }
    for _ in 0..2 {
        let (status, body) = get(&app, &uri, Some("sdlc_pat_reader")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["schema"], "forge/namespace-devops-evidence/v1");
        assert_eq!(body["source"]["repository_id"], repository.to_string());
        assert_eq!(
            body["source"]["delivery_configuration"]["id"],
            query.project_id.to_string()
        );
        assert_eq!(
            body["candidate"]["operationReceipt"],
            serde_json::to_value(&receipt).unwrap()
        );
        assert_eq!(body["candidate"]["pipelineId"], pipeline.to_string());
        assert_eq!(body["candidate"]["sourceCommit"], commit);
        assert_eq!(body["candidate"]["candidateObserved"], true);
        assert_eq!(
            body["candidate"]["artifacts"][0]["artifactId"],
            artifact.to_string()
        );
        assert_eq!(body["candidate"]["artifacts"][0]["sha256"], sha(bytes));
        assert_eq!(body["target"]["policySha256"], policy.sha256().unwrap());
        assert_eq!(body["target"]["current"], json!({"state":"absent"}));
        assert_eq!(body["target"]["confirmed"], json!({"state":"absent"}));
        if negative {
            assert!(body["target"].get("selection").is_none());
        } else {
            assert_eq!(
                body["target"]["selection"],
                json!({
                    "schema":"forge/owner-delivery-selection/v1",
                    "action":"deploy", "sourcePipelineId":pipeline,
                    "configSha256":sha(yaml.as_bytes()), "planSha256":plan_hash,
                    "artifact": {"artifactId":artifact, "jobId":job,
                        "attemptId":receipt.request.attempt_id,
                        "sha256":sha(bytes), "sizeBytes":bytes.len() as i64}
                })
            );
        }
        assert_eq!(body["dispatchAllowed"], false);
        for name in [
            "dispatchAllowed",
            "deploymentVerified",
            "acceptanceVerified",
        ] {
            assert_eq!(body["candidate"][name], false);
        }
        let text = body.to_string();
        for private in [
            "encrypted_child",
            "sdlc_pat_",
            "fixture-private-ciphertext",
            "v1:AA==:AA==",
            root.0.to_str().unwrap(),
        ] {
            assert!(
                !text.contains(private),
                "private owner material crossed readback boundary"
            );
        }
    }
    assert!(
        rows(&pool).await == before_rows,
        "GET/replay/denial changed persisted custody or dispatch state"
    );
    assert_eq!(
        inventory(&root.0),
        before_files,
        "reader changed source, artifacts or enrolled target"
    );
}

async fn child(negative: bool) {
    let options = PgConnectOptions::from_str(
        &std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable PG required"),
    )
    .unwrap();
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("forge_test_"))
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let schema = format!("it_namespace_evidence_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let search = search.clone();
            Box::pin(async move {
                sqlx::query(&search).execute(connection).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let case_pool = pool.clone();
    let case = tokio::spawn(async move {
        crate::migrations()
            .await
            .unwrap()
            .run(&case_pool)
            .await
            .unwrap();
        exercise(case_pool, negative).await;
    })
    .await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    if let Err(error) = case {
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
        panic!("owned namespace PG fixture cancelled");
    }
}

async fn isolated(selector: &str, negative: bool) {
    if std::env::var(CHILD).as_deref() == Ok(selector) {
        child(negative).await;
        return;
    }
    // Same controlled Auth introspection peer/child-process isolation as the reader middleware fixture.
    let auth = Router::new().route("/auth/tokens/introspect", get_handler(|headers: HeaderMap| async move {
        let subject = match headers.get("authorization").and_then(|h| h.to_str().ok()) {
            Some("Bearer sdlc_pat_reader") => READER,
            Some("Bearer sdlc_pat_foreign") => "00000000-0000-0000-0000-000000000008",
            _ => return (StatusCode::UNAUTHORIZED, Json(json!({}))),
        };
        (StatusCode::OK, Json(json!({"sub":subject,"email":"fixture@example.invalid","scopes":["ci-cd:read"],"display_name":"Fixture"})))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}/oidc/jwks", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, auth).await.unwrap() });
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", selector, "--ignored", "--show-output"])
        .env(CHILD, selector)
        .env("CICD_AUTH__CENTRAL_JWKS_URI", uri)
        .env("CICD_AUTH__CENTRAL_ISSUER", "http://namespace-fixture.test")
        .env("CICD_AUTH__CENTRAL_TIMEOUT_SECS", "5")
        .env("CICD_NAMESPACE__READER_SUBJECTS", READER)
        .env("CICD_NAMESPACE__OWNER_SUBJECTS", "")
        .env("CICD_NAMESPACE__INSTANCE_ID", INSTANCE)
        .env("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", REGISTRY)
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output()).await;
    server.abort();
    let _ = server.await;
    let output = output
        .expect("isolated namespace evidence fixture timed out")
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored;"));
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; actual reader HTTP/PG, no runner effects"]
async fn namespace_devops_evidence_pg_registered_reader_retained_candidate_and_empty_target() {
    isolated(POSITIVE, false).await;
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; actual reader HTTP/PG, no runner effects"]
async fn namespace_devops_evidence_pg_foreign_project_and_assignment_mismatch_are_read_only() {
    isolated(NEGATIVE, true).await;
}
