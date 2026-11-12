//! PostgreSQL instance/session/original-key invariants on disposable databases.
#![cfg(feature = "integration")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use cicd_domain::development_workspace::WorkspaceAction;
use cicd_infra::development_workspace as store;
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::str::FromStr;
use tower::ServiceExt;
use uuid::Uuid;

#[path = "support/development_workspace_developer.rs"]
mod developer;
#[path = "support/pull_fixture.rs"]
mod pull_fixture;

struct Fixture {
    pool: PgPool,
    admin: PgPool,
    database: String,
    workspace: Uuid,
    other_workspace: Uuid,
    project: Uuid,
    environment: Uuid,
    actor: Uuid,
    other_actor: Uuid,
}

async fn http(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn delivery_operation_retains_exact_typed_observation_and_never_borrows_neighbor_pipeline() {
    use sha2::{Digest, Sha256};
    const CHILD: &str = "FORGE_WORKSPACE_OBSERVATION_READER_TEST";
    const READER: &str = "sdlc_pat_workspace_observation_reader_fixture";
    // Like the configured Developer fixture, isolate the process-global Central
    // bridge and authenticate the real HTTP GET; no anonymous/local-auth bypass.
    let subject = if let Ok(subject) = std::env::var(CHILD) {
        subject.parse::<Uuid>().unwrap()
    } else {
        let subject=Uuid::new_v4();
        let auth=axum::Router::new()
            .route("/oidc/jwks",axum::routing::get(||async {axum::Json(json!({"keys":[]}))}))
            .route("/auth/tokens/introspect",axum::routing::get(move |headers:axum::http::HeaderMap|async move {
                assert_eq!(headers["authorization"],format!("Bearer {READER}"));
                axum::Json(json!({"sub":subject,"email":"observation-reader@fixture.invalid",
                    "subject_role":"user","scopes":["ci-cd:read"],
                    "observed_at":chrono::Utc::now(),"expires_at":chrono::Utc::now()+chrono::Duration::seconds(300)}))
            }));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri=format!("http://{}/oidc/jwks",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {axum::serve(listener,auth).await.unwrap()});
        let output=tokio::time::timeout(std::time::Duration::from_secs(60),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","delivery_operation_retains_exact_typed_observation_and_never_borrows_neighbor_pipeline","--nocapture"])
                .env(CHILD,subject.to_string()).env("CICD_AUTH__CENTRAL_JWKS_URI",uri)
                .env("CICD_AUTH__CENTRAL_ISSUER","http://observation-reader.example.invalid")
                .env("NO_PROXY","127.0.0.1,localhost").env("no_proxy","127.0.0.1,localhost")
                .kill_on_drop(true).output()).await;
        server.abort();let _=server.await;
        let output=output.expect("isolated observation HTTP reader timed out").unwrap();
        assert!(output.status.success(),"{}\n{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
        return;
    };
    let f = Fixture::new().await;
    sqlx::query("UPDATE users SET central_sub=$2 WHERE id=$1").bind(f.actor).bind(subject.to_string()).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'viewer')")
        .bind(f.project).bind(f.actor).execute(&f.pool).await.unwrap();
    let catalog=tempfile::tempdir().unwrap();
    let mut config=cicd::config::RuntimeConfig::from_env_source(|_|None,false).unwrap()
        .with_auth_secret(Some("workspace-observation-fixture-only".into()));
    config.development_workspaces.catalog_root=Some(catalog.path().into());
    let app=cicd::api::app_with_git_and_config(Some(f.pool.clone()),config.git.to_git_config(),None,config.clone()).unwrap();
    let worker=tokio::spawn(cicd::development_workspaces::worker::supervisor(f.pool.clone(),config));
    let commit="a".repeat(40);
    let publication=Uuid::new_v4();
    for (machine,neighbor) in [(false,true),(true,false),(true,true)] {
        let (operation,pipeline,stage,job,attempt,deployment,lease,namespace)=
            (Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4());
        let recipe=json!({"version":1,"jobs":{"deploy":{"helm_deploy":{"environment_id":f.environment,
            "source_pipeline_id":publication,"images":{"image":{"job":"publish","artifact":"out/image.json"}},"receipt_artifact":"out/release.json"}}}});
        let raw=serde_json::to_string(&recipe).unwrap();
        let hash=|bytes:&[u8]|format!("{:x}",Sha256::digest(bytes));
        let plan=json!({"format":"v1-dag","version":1,"config_source":"development_workspace","resolved_commit_sha":commit,"jobs":[{"key":"deploy","command":"forge:helm-deploy","image":"forge:runner-helm-deploy","commands":[],"required_secrets":[],"allow_failure":false,"artifact_paths":["out/release.json"],"needs":[],"helm_deploy":recipe["jobs"]["deploy"]["helm_deploy"]}]});
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status,variables) VALUES($1,$2,$3,$3,'success',$4)")
            .bind(pipeline).bind(f.project).bind(&commit).bind(json!({"development_workspace_id":f.workspace,"development_workspace_operation_id":operation})).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,'development_workspace','v1',$2,$2,$3,$4,$5,$6)")
            .bind(pipeline).bind(&commit).bind(hash(raw.as_bytes())).bind(hash(&serde_json::to_vec(&plan).unwrap())).bind(raw).bind(plan).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'deploy',0,'success')").bind(stage).bind(pipeline).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'deploy','forge:runner-helm-deploy','forge:helm-deploy',0,'success')").bind(job).bind(stage).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger) VALUES($1,$2,1,'success','initial')").bind(attempt).bind(job).execute(&f.pool).await.unwrap();
        let image=format!("registry.example.test/product@sha256:{}","b".repeat(64));
        let receipt=json!({"schema":"forge/kubernetes-deployment/v1","deploymentId":deployment,"pipelineId":pipeline,
            "sourcePipelineId":publication,"attemptId":attempt,"leaseId":lease,"generation":1,"sourceCommit":commit,
            "cluster":"fixture","namespace":"product","namespaceUid":namespace,"release":"product","releaseRevision":2,
            "images":{"image":image},"healthStatus":200,"healthTransport":"public-https"});
        let observed=json!({"schema":"forge/kubernetes-deployment-observation/v1","phase":"deployed","deploymentId":deployment,
            "pipelineId":pipeline,"attemptId":attempt,"leaseId":lease,"generation":1,"sourceCommit":commit,
            "namespaceUid":namespace,"releaseRevision":2,"receipt":receipt});
        let offer=json!({"environmentId":f.environment,"deploymentId":deployment,"pipelineId":pipeline,"sourcePipelineId":publication,
            "jobId":job,"attemptId":attempt,"leaseId":lease,"generation":1,"sourceCommit":commit,"pipelineOrder":1,
            "target":{"schema":"forge/kubernetes-environment/v1","cluster":"fixture","namespace":"product","release":"product","chart":"chart","health_url":"https://product.example.test/health"},
            "images":{"image":{"schema":"forge/published-image/v1","sourceCommit":commit,"imageRef":image,"descriptorSha256":"c".repeat(64)}},"receiptArtifact":"out/release.json"});
        if !neighbor {
            sqlx::query("INSERT INTO deployments(id,environment_id,pipeline_id,git_ref,status,execution_attempt_id,runtime_phase,runtime_input,runtime_receipt) VALUES($1,$2,$3,$4,'success',$5,'finished',$6,$7)")
                .bind(deployment).bind(f.environment).bind(pipeline).bind(&commit).bind(attempt).bind(offer).bind(&observed).execute(&f.pool).await.unwrap();
        }
        let version=store::workspace(&f.pool,f.workspace).await.unwrap().version;
        let request=json!({"action":"check","recipe":"release","source_commit":commit,"source_pipeline_id":publication});
        // Projection reads retained execution evidence, never decrypts a historical
        // bearer or authorizes an effect. Real machine admission is exercised by
        // the separate Central PAT fixture below.
        let mut execution_plan=json!({"pipeline":recipe});
        if machine { execution_plan["developerAuthority"]=json!({"sealed":"fixture-readback-only"}); }
        sqlx::query("INSERT INTO development_workspace_operations(id,workspace_id,actor_id,owner_subject,request_hash,request,expected_version,state,execution_plan,pipeline_id,started_at) VALUES($1,$2,$3,$3,$4,$5,$6,'running',$7,$8,now())")
            .bind(operation).bind(f.workspace).bind(f.actor).bind(hash(operation.as_bytes())).bind(&request).bind(version).bind(execution_plan).bind(pipeline).execute(&f.pool).await.unwrap();
        let result=tokio::time::timeout(std::time::Duration::from_secs(10),async {
            loop { let result=store::operation(&f.pool,f.workspace,operation).await.unwrap();
                if result.state!="running" {break result;}
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        }).await.unwrap();
        let path=format!("/api/v1/development-workspaces/{}/operations/{operation}",f.workspace);
        assert_eq!(http(&app,"GET",&path,None,json!(null)).await.0,StatusCode::UNAUTHORIZED);
        let (status,public)=http(&app,"GET",&path,Some(READER),json!(null)).await;
        assert_eq!(status,StatusCode::OK,"{public}");
        assert_eq!(public["request"],request);
        if machine && neighbor {
            assert_eq!(result.state,"unknown");
            assert_eq!(public["result"]["requiresReconciliation"],true);
            assert!(public["result"]["deploymentObservation"].is_null());
        } else {
            assert_eq!(result.state,"succeeded","{:?}",result.result);
            if machine { assert_eq!(public["result"]["deploymentObservation"],observed); }
            else { assert!(public["result"]["deploymentObservation"].is_null()); }
            assert_eq!(http(&app,"GET",&path,Some(READER),json!(null)).await.1,public);
        }
    }
    worker.abort(); let _=worker.await;
    f.close().await;
}

#[tokio::test]
async fn typed_publication_claims_frozen_workspace_and_repository_plans_only() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new().await;
    let registration = format!("fixture-{}", Uuid::new_v4());
    let config = cicd::config::RuntimeConfig::from_env_source(
        |name| (name == "CICD_RUNNER_REGISTRATION_TOKEN").then(|| registration.clone()),
        false,
    )
    .unwrap();
    let app = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    let (status, registered) = http(&app, "POST", "/api/v1/runner/register", None,
        json!({"protocolVersion":1,"registrationToken":registration,"name":"typed-fixture",
            "tags":["linux"],"capabilities":{"executorKinds":["kubernetes"],"jobKinds":["image-publish-v1"]}})).await;
    assert_eq!(status, StatusCode::CREATED, "{registered}");
    let credential = registered["credential"].as_str().unwrap();
    let (status, _) = http(&app,"POST","/api/v1/runner/heartbeat",Some(credential),
        json!({"protocolVersion":1,"status":"online","capacity":{"totalSlots":1,"busySlots":0},
            "capabilities":{"executorKinds":["kubernetes"],"jobKinds":["image-publish-v1"]},"activeLeaseIds":[]})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let commit = "a".repeat(40);
    let raw_config = "published owner recipe";
    let config_hash = format!("{:x}", Sha256::digest(raw_config.as_bytes()));
    for (stored_source, plan_source, allowed) in [
        ("repository", "repository", true),
        ("development_workspace", "development_workspace", true),
        ("repository", "development_workspace", false),
        ("development_workspace", "repository", false),
        ("legacy_template", "legacy_template", false),
    ] {
        let pipeline = Uuid::new_v4();
        let stage = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let paths = vec!["out/build.json", "out/image.json"];
        let plan = json!({"format":"v1-dag","version":1,"config_source":plan_source,
            "resolved_commit_sha":commit,"jobs":[{"key":"publish","command":"forge:image-publish",
                "image":"forge:runner-image-publish","commands":[],"required_secrets":[],
                "allow_failure":false,"artifact_paths":paths,
                "image_publish":{"context":".","dockerfile":"Dockerfile",
                    "repository":"registry.example.test/team/application",
                    "descriptor_artifact":paths[0],"published_artifact":paths[1]}}]});
        let plan_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&plan).unwrap()));
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'main',$3,'queued')")
            .bind(pipeline).bind(f.project).bind(&commit).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,$2,'v1','main',$3,$4,$5,$6,$7)")
            .bind(pipeline).bind(stored_source).bind(&commit).bind(&config_hash).bind(&plan_hash).bind(raw_config).bind(&plan).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'build',0,'queued')")
            .bind(stage).bind(pipeline).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,required_tags,artifact_paths,position,status) VALUES($1,$2,'publish','forge:runner-image-publish','forge:image-publish',ARRAY['linux'],$3,0,'queued')")
            .bind(job).bind(stage).bind(&paths).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger) VALUES($1,$2,1,'queued','initial')")
            .bind(attempt).bind(job).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO job_queue(id,job_id,attempt_id,pipeline_id,stage_id,state,priority,required_tags) VALUES($1,$2,$3,$4,$5,'queued',100,ARRAY['linux'])")
            .bind(Uuid::new_v4()).bind(job).bind(attempt).bind(pipeline).bind(stage).execute(&f.pool).await.unwrap();
        let (status, offer) = http(&app,"POST","/api/v1/runner/work:poll",Some(credential),
            json!({"protocolVersion":1,"capacity":{"freeSlots":1},"tags":["linux"],"waitSeconds":0})).await;
        assert_eq!(
            status,
            if allowed {
                StatusCode::OK
            } else {
                StatusCode::CONFLICT
            },
            "stored={stored_source}, plan={plan_source}: {offer}"
        );
        let leases: i64 = sqlx::query_scalar("SELECT count(*) FROM job_leases WHERE attempt_id=$1")
            .bind(attempt)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert_eq!(leases, i64::from(allowed));
        if allowed {
            assert_eq!(offer["attempt"]["commitSha"], commit);
            assert_eq!(
                offer["attempt"]["imagePublish"],
                plan["jobs"][0]["image_publish"]
            );
        }
        sqlx::query("DELETE FROM pipelines WHERE id=$1")
            .bind(pipeline)
            .execute(&f.pool)
            .await
            .unwrap();
    }
    f.close().await;
}

#[tokio::test]
async fn typed_delivery_uses_the_frozen_workspace_operation_owner() {
    use sha2::{Digest, Sha256};
    #[derive(Clone, Copy)]
    struct Case {
        name: &'static str,
        source: &'static str,
        original_owner: bool,
        original_subject: bool,
        pipeline_owner: bool,
        enabled: bool,
        retry: bool,
        retry_owner: Option<bool>,
        stale_session: bool,
        stale_source: bool,
        unknown: bool,
        machine: bool,
        publication: Option<(&'static str, bool, bool)>,
        tampered_artifact: bool,
        allowed: bool,
    }
    let current = Case {
        name: "initial workspace delivery",
        source: "development_workspace",
        original_owner: true,
        original_subject: true,
        pipeline_owner: false,
        enabled: true,
        retry: false,
        retry_owner: None,
        stale_session: false,
        stale_source: false,
        unknown: false,
        machine: false,
        publication: None,
        tampered_artifact: false,
        allowed: true,
    };
    let retry = Case {
        name: "same session retry",
        retry: true,
        retry_owner: Some(true),
        ..current
    };
    for case in [
        current,
        Case {
            name: "completed publication from a separate pipeline",
            publication: Some(("success", false, false)),
            ..current
        },
        Case {
            name: "publication pipeline still running",
            publication: Some(("running", false, false)),
            allowed: false,
            ..current
        },
        Case {
            name: "publication pipeline for another commit",
            publication: Some(("success", true, false)),
            allowed: false,
            ..current
        },
        Case {
            name: "publication pipeline for another project",
            publication: Some(("success", false, true)),
            allowed: false,
            ..current
        },
        Case {
            name: "publication artifact changed after upload",
            publication: Some(("success", false, false)),
            tampered_artifact: true,
            allowed: false,
            ..current
        },
        Case {
            name: "foreign central subject",
            original_subject: false,
            allowed: false,
            ..current
        },
        Case {
            name: "machine session cannot release",
            machine: true,
            allowed: false,
            ..current
        },
        Case {
            name: "foreign original owner",
            original_owner: false,
            pipeline_owner: true,
            allowed: false,
            ..current
        },
        Case {
            name: "repository without pipeline owner",
            source: "repository",
            allowed: false,
            ..current
        },
        Case {
            name: "repository pipeline owner",
            source: "repository",
            original_owner: false,
            pipeline_owner: true,
            ..current
        },
        Case {
            name: "disabled owner",
            enabled: false,
            allowed: false,
            ..current
        },
        retry,
        Case {
            name: "foreign retry owner",
            retry_owner: Some(false),
            allowed: false,
            ..retry
        },
        Case {
            name: "missing retry owner",
            retry_owner: None,
            allowed: false,
            ..retry
        },
        Case {
            name: "old session",
            stale_session: true,
            allowed: false,
            ..retry
        },
        Case {
            name: "source changed",
            stale_source: true,
            allowed: false,
            ..retry
        },
        Case {
            name: "unknown original outcome",
            unknown: true,
            allowed: false,
            ..retry
        },
    ] {
        let source = case.source;
        let f = Fixture::new().await;
        let subject = Uuid::new_v4();
        let other_subject = Uuid::new_v4();
        let session = Uuid::new_v4();
        sqlx::query("UPDATE users SET enabled=$2 WHERE id=$1")
            .bind(f.actor)
            .bind(case.enabled)
            .execute(&f.pool)
            .await
            .unwrap();
        let files = tempfile::tempdir().unwrap();
        let registration = Uuid::new_v4().to_string();
        let mut config = cicd::config::RuntimeConfig::from_env_source(
            |name| (name == "CICD_RUNNER_REGISTRATION_TOKEN").then(|| registration.clone()),
            false,
        )
        .unwrap();
        config.artifacts.root = files.path().to_path_buf();
        let app = cicd::api::app_with_git_and_config(
            Some(f.pool.clone()),
            config.git.to_git_config(),
            None,
            config,
        )
        .unwrap();
        let capabilities = json!({"executorKinds":["kubernetes"],"jobKinds":["helm-deploy-v1"],"helmTargets":[f.environment]});
        let (status, registered) = http(&app,"POST","/api/v1/runner/register",None,
            json!({"protocolVersion":1,"registrationToken":registration,"name":"delivery-owner-fixture","tags":["linux"],"capabilities":capabilities})).await;
        assert_eq!(status, StatusCode::CREATED, "{registered}");
        let credential = registered["credential"].as_str().unwrap();
        assert_eq!(http(&app,"POST","/api/v1/runner/heartbeat",Some(credential),
            json!({"protocolVersion":1,"status":"online","capacity":{"totalSlots":1,"busySlots":0},"capabilities":capabilities,"activeLeaseIds":[]})).await.0,StatusCode::NO_CONTENT);
        let target = json!({"schema":"forge/kubernetes-environment/v1","cluster":"fixture","namespace":"product","release":"product","chart":"chart","health_url":"https://product.example.test/health"});
        sqlx::query("UPDATE environments SET kubernetes_target=$2 WHERE id=$1")
            .bind(f.environment)
            .bind(&target)
            .execute(&f.pool)
            .await
            .unwrap();
        let commit = "a".repeat(40);
        let selected_commit = if case.stale_source {
            "b".repeat(40)
        } else {
            commit.clone()
        };
        let sources = json!([{"key":"source","repository_id":
            sqlx::query_scalar::<_,Uuid>("SELECT repository_id FROM development_workspaces WHERE id=$1")
                .bind(f.workspace).fetch_one(&f.pool).await.unwrap(),"commit":selected_commit}]);
        sqlx::query("UPDATE development_workspaces SET desired_runtime='running',runtime='running',environment_hash=$2,preparation='prepared',sources=$3 WHERE id=$1")
            .bind(f.workspace).bind(store::request_hash(&target).unwrap()).bind(&sources).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO development_workspace_sessions(id,workspace_id,actor_id,owner_subject,task_id,task_ref,sources,machine_assignment) VALUES($1,$2,$3,$4,$5,'DEV-1',$6,$7)")
            .bind(session).bind(f.workspace).bind(f.actor).bind(subject).bind(Uuid::new_v4()).bind(&sources)
            .bind(if case.machine { Some(json!({"fixture":true})) } else { None }).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO development_workspace_operations(id,workspace_id,actor_id,owner_subject,request_hash,request,expected_version,state,finished_at,result) VALUES($1,$2,$3,$4,$5,'{\"action\":\"begin_session\"}',$6,'succeeded',now(),'{}')")
            .bind(session).bind(f.workspace).bind(f.actor).bind(subject).bind("a".repeat(64))
            .bind(if case.stale_session { 3_i64 } else { 1_i64 }).execute(&f.pool).await.unwrap();
        let pipeline = Uuid::new_v4();
        let stage = Uuid::new_v4();
        let publication_pipeline = case.publication.map(|_| Uuid::new_v4());
        let publication_stage = publication_pipeline
            .map(|_| Uuid::new_v4())
            .unwrap_or(stage);
        let producer = Uuid::new_v4();
        let producer_attempt = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let raw = "frozen owner recipe";
        let mut plan = json!({"format":"v1-dag","version":1,"config_source":source,"resolved_commit_sha":commit,"jobs":[
            {"key":"publish","image_publish":{"context":".","dockerfile":"Dockerfile","repository":"registry.example.test/team/application","descriptor_artifact":"out/build.json","published_artifact":"out/image.json"}},
            {"key":"deploy","command":"forge:helm-deploy","image":"forge:runner-helm-deploy","commands":[],"required_secrets":[],"allow_failure":false,"artifact_paths":["out/release.json"],"needs":["publish"],"helm_deploy":{"environment_id":f.environment,"images":{"image":{"job":"publish","artifact":"out/image.json"}},"receipt_artifact":"out/release.json"}}
        ]});
        let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,$3,$3,'queued')")
            .bind(pipeline).bind(f.project).bind(&commit).execute(&f.pool).await.unwrap();
        if let (Some(publication_id), Some((status, wrong_commit, wrong_project))) =
            (publication_pipeline, case.publication)
        {
            let publication_commit = if wrong_commit {
                "b".repeat(40)
            } else {
                commit.clone()
            };
            let mut publication_plan = plan.clone();
            publication_plan["resolved_commit_sha"] = json!(publication_commit);
            publication_plan["jobs"].as_array_mut().unwrap().pop();
            let publication_project = if wrong_project {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,'Foreign publication','https://example.invalid/foreign.git')")
                    .bind(id).execute(&f.pool).await.unwrap();
                id
            } else {
                f.project
            };
            sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,$3,$3,$4)")
                .bind(publication_id).bind(publication_project).bind(&publication_commit).bind(status).execute(&f.pool).await.unwrap();
            sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,$2,'v1',$3,$3,$4,$5,$6,$7)")
                .bind(publication_id).bind(source).bind(&publication_commit).bind(hash(raw.as_bytes())).bind(hash(&serde_json::to_vec(&publication_plan).unwrap())).bind(raw).bind(&publication_plan).execute(&f.pool).await.unwrap();
            sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'publication',0,$3)")
                .bind(publication_stage).bind(publication_id).bind(status).execute(&f.pool).await.unwrap();
            plan["jobs"].as_array_mut().unwrap().remove(0);
            plan["jobs"][0]["needs"] = json!([]);
            plan["jobs"][0]["helm_deploy"]["source_pipeline_id"] = json!(publication_id);
        }
        sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,$2,'v1',$3,$3,$4,$5,$6,$7)")
            .bind(pipeline).bind(source).bind(&commit).bind(hash(raw.as_bytes())).bind(hash(&serde_json::to_vec(&plan).unwrap())).bind(raw).bind(&plan).execute(&f.pool).await.unwrap();
        if case.pipeline_owner {
            sqlx::query(
                "INSERT INTO pipeline_human_owners(pipeline_id,owner_subject) VALUES($1,$2)",
            )
            .bind(pipeline)
            .bind(subject)
            .execute(&f.pool)
            .await
            .unwrap();
        }
        let actor = if case.original_owner {
            f.actor
        } else {
            f.other_actor
        };
        let operation = Uuid::new_v4();
        let operation_state = if case.unknown {
            "unknown"
        } else if case.retry {
            "failed"
        } else {
            "running"
        };
        sqlx::query("INSERT INTO development_workspace_operations(id,workspace_id,actor_id,owner_subject,request_hash,request,expected_version,state,pipeline_id,finished_at) VALUES($1,$2,$3,$4,$5,$6,2,$7,$8,CASE WHEN $7='failed' THEN now() END)")
            .bind(operation).bind(f.workspace).bind(actor).bind(if case.original_subject { subject } else { other_subject })
            .bind("b".repeat(64)).bind(json!({"action":"check","recipe":"dev-delivery","source_commit":commit}))
            .bind(operation_state).bind(pipeline).execute(&f.pool).await.unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'delivery',0,'queued')")
            .bind(stage).bind(pipeline).execute(&f.pool).await.unwrap();
        for (id, name, image, command, position, status, attempt_id) in [
            (
                producer,
                "publish",
                "forge:runner-image-publish",
                "forge:image-publish",
                0,
                "success",
                producer_attempt,
            ),
            (
                job,
                "deploy",
                "forge:runner-helm-deploy",
                "forge:helm-deploy",
                1,
                "queued",
                attempt,
            ),
        ] {
            sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,required_tags,artifact_paths,position,status) VALUES($1,$2,$3,$4,$5,ARRAY['linux'],ARRAY['out/release.json'],$6,$7)")
                .bind(id).bind(if id == producer { publication_stage } else { stage }).bind(name).bind(image).bind(command).bind(position).bind(status).execute(&f.pool).await.unwrap();
            let retrying = id == job && case.retry;
            if retrying {
                sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger) VALUES($1,$2,1,'canceled','initial')")
                    .bind(Uuid::new_v4()).bind(job).execute(&f.pool).await.unwrap();
            }
            sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger) VALUES($1,$2,$3,$4,$5)")
                .bind(attempt_id).bind(id).bind(if retrying { 2 } else { 1 }).bind(status)
                .bind(if retrying { "job_retry" } else { "initial" }).execute(&f.pool).await.unwrap();
        }
        if let Some(matches) = case.retry_owner {
            sqlx::query("INSERT INTO execution_attempt_human_owners(attempt_id,owner_subject,requires_admin) VALUES($1,$2,false)")
                .bind(attempt).bind(if matches { subject } else { other_subject }).execute(&f.pool).await.unwrap();
        }
        let descriptor = b"fixture retained build descriptor".to_vec();
        let published = cicd_domain::published_image::PublishedImage::new(
            commit.clone(),
            format!(
                "registry.example.test/team/application@sha256:{}",
                "c".repeat(64)
            ),
            hash(&descriptor),
        )
        .unwrap();
        for (name, bytes) in [
            ("out/build.json", descriptor),
            ("out/image.json", serde_json::to_vec(&published).unwrap()),
        ] {
            let path = files.path().join(Uuid::new_v4().to_string());
            std::fs::write(&path, &bytes).unwrap();
            sqlx::query("INSERT INTO artifacts(id,job_id,attempt_id,name,storage_path,size_bytes,sha256,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+interval '1 hour')")
                .bind(Uuid::new_v4()).bind(producer).bind(producer_attempt).bind(cicd_domain::artifact_names::artifact_name_from_declared_path(name)).bind(path.to_str().unwrap()).bind(bytes.len() as i64).bind(hash(&bytes)).execute(&f.pool).await.unwrap();
            if case.tampered_artifact && name == "out/image.json" {
                std::fs::write(&path, vec![b' '; bytes.len()]).unwrap();
            }
        }
        sqlx::query("INSERT INTO job_queue(id,job_id,attempt_id,pipeline_id,stage_id,state,priority,required_tags) VALUES($1,$2,$3,$4,$5,'queued',100,ARRAY['linux'])")
            .bind(Uuid::new_v4()).bind(job).bind(attempt).bind(pipeline).bind(stage).execute(&f.pool).await.unwrap();
        let (status,offer)=http(&app,"POST","/api/v1/runner/work:poll",Some(credential),
            json!({"protocolVersion":1,"capacity":{"freeSlots":1},"tags":["linux"],"waitSeconds":0})).await;
        assert_eq!(
            status,
            if case.allowed {
                StatusCode::OK
            } else {
                StatusCode::CONFLICT
            },
            "{}: {offer}",
            case.name
        );
        let leases: i64 = sqlx::query_scalar("SELECT count(*) FROM job_leases WHERE attempt_id=$1")
            .bind(attempt)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert_eq!(leases, i64::from(case.allowed), "{}", case.name);
        if case.allowed
            && let Some(publication_id) = publication_pipeline
        {
            let input: Value =
                sqlx::query_scalar("SELECT runtime_input FROM deployments WHERE pipeline_id=$1")
                    .bind(pipeline)
                    .fetch_one(&f.pool)
                    .await
                    .unwrap();
            assert_eq!(input["sourcePipelineId"], json!(publication_id));
            assert_eq!(
                input["images"]["image"]["imageRef"],
                json!(published.image_ref)
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM jobs WHERE id=$1")
                .bind(producer)
                .fetch_one(&f.pool)
                .await
                .unwrap(),
            "success"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM development_workspace_operations WHERE id=$1"
            )
            .bind(operation)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
            operation_state
        );
        if case.allowed {
            assert_eq!(offer["attempt"]["commitSha"], commit);
        }
        drop(app);
        f.close().await;
    }
}

#[tokio::test]
async fn public_api_preserves_original_commands_across_restart_and_revokes_project_access() {
    let f = Fixture::new().await;
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer'),($1,$3,'viewer')")
        .bind(f.project).bind(f.actor).bind(f.other_actor).execute(&f.pool).await.unwrap();
    for actor in [f.actor, f.other_actor] {
        sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
            .bind(actor)
            .bind(cicd::auth::hash_password("WorkspaceFixturePass1!").unwrap())
            .execute(&f.pool)
            .await
            .unwrap();
    }
    let config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some(format!("workspace-fixture-{}", f.workspace)));
    let app = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        config.git.to_git_config(),
        None,
        config.clone(),
    )
    .unwrap();
    let mut tokens = Vec::new();
    for actor in [f.actor, f.other_actor] {
        let (status, result) = http(
            &app,
            "POST",
            "/api/v1/auth/login",
            None,
            json!({"username":actor.to_string(),"password":"WorkspaceFixturePass1!"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        tokens.push(result["access_token"].as_str().unwrap().to_owned());
    }
    let path = format!("/api/v1/development-workspaces/{}", f.workspace);
    sqlx::query("UPDATE users SET username='workspace-developer' WHERE id=$1")
        .bind(f.actor)
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, card) = http(&app, "GET", &path, Some(&tokens[1]), json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["workspace"]["owner_name"], "workspace-developer");
    assert_eq!(card["busy"], false);
    let session = Uuid::new_v4();
    sqlx::query("INSERT INTO development_workspace_sessions(id,workspace_id,actor_id,owner_subject,task_id,task_ref) VALUES($1,$2,$3,$3,$4,'DEV-1')")
        .bind(session)
        .bind(f.workspace)
        .bind(f.actor)
        .bind(Uuid::new_v4())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .1["busy"],
        true
    );
    sqlx::query("UPDATE development_workspace_sessions SET ended_at=clock_timestamp() WHERE id=$1")
        .bind(session)
        .execute(&f.pool)
        .await
        .unwrap();
    let pipeline = Uuid::new_v4();
    let deployment = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','failed')",
    )
    .bind(pipeline)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO deployments(id,environment_id,pipeline_id,git_ref,status) VALUES($1,$2,$3,'main','running')")
        .bind(deployment)
        .bind(f.environment)
        .bind(pipeline)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .1["busy"],
        true
    );
    sqlx::query("UPDATE deployments SET status='failed',runtime_phase='held' WHERE id=$1")
        .bind(deployment)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .1["busy"],
        true
    );
    sqlx::query("UPDATE deployments SET runtime_phase='finished' WHERE id=$1")
        .bind(deployment)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .1["busy"],
        false
    );
    for token in &tokens {
        let (status, _) = http(&app,"POST",&format!("/api/v1/projects/{}/development-workspaces",f.project),Some(token),
            json!({"id":Uuid::new_v4(),"operation_id":Uuid::new_v4(),"catalog_target_id":Uuid::new_v4(),"name":"No creator permission","create":false})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'workspace-fixture-machine')")
        .bind(account)
        .execute(&f.pool)
        .await
        .unwrap();
    let machine = format!(
        "forge_sat_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes,expires_at) VALUES($1,'workspace-fixture',$2,'synthetic-only','service_account',$3,$4,$5,now()+interval '1 day')")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&machine)).bind(account).bind(f.project).bind(vec!["api:read","pull:create"]).execute(&f.pool).await.unwrap();
    assert_eq!(
        http(
            &app,
            "GET",
            &format!("/api/v1/projects/{}", f.project),
            Some(&machine),
            json!(null)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        http(&app, "GET", &path, Some(&machine), json!(null))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .0,
        StatusCode::OK
    );
    let operation = Uuid::new_v4();
    let request = json!({"operation_id":operation,"expected_version":1,"command":{"action":"begin_session","task":"DEV-1"}});
    assert_eq!(
        http(
            &app,
            "POST",
            &format!("{path}/operations"),
            Some(&tokens[1]),
            request.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, original) = http(
        &app,
        "POST",
        &format!("{path}/operations"),
        Some(&tokens[0]),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{original}");
    assert_eq!(original["id"], operation.to_string());
    assert_eq!(original["state"], "queued");
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .1["busy"],
        true
    );
    assert!(original.get("execution_plan").is_none());
    assert_eq!(f.version().await, 2);
    let restarted = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    let (status, replay) = http(
        &restarted,
        "POST",
        &format!("{path}/operations"),
        Some(&tokens[0]),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, original);
    assert_eq!(f.version().await, 2);
    let mut changed = request;
    changed["command"]["task"] = json!("DEV-2");
    assert_eq!(
        http(
            &restarted,
            "POST",
            &format!("{path}/operations"),
            Some(&tokens[0]),
            changed
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, readback) = http(
        &restarted,
        "GET",
        &format!("{path}/operations/{operation}"),
        Some(&tokens[0]),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(readback, original);
    assert_eq!(http(&restarted,"POST",&format!("{path}/operations"),Some(&tokens[0]),
        json!({"operation_id":Uuid::new_v4(),"expected_version":2,"command":{"action":"stop","force":true}})).await.0,StatusCode::UNPROCESSABLE_ENTITY);
    sqlx::query("DELETE FROM project_memberships WHERE project_id=$1 AND user_id=$2")
        .bind(f.project)
        .bind(f.actor)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        http(&restarted, "GET", &path, Some(&tokens[0]), json!(null))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(&restarted, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .0,
        StatusCode::OK
    );
    // The PR fixture attaches a namespace to the repository. Keep the legacy
    // workspace role assertions above independent of that projection.
    let assigned_repo: Uuid = sqlx::query_scalar("SELECT repository_id FROM projects WHERE id=$1")
        .bind(f.project)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let foreign_repo = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(foreign_repo)
        .bind(format!("source-{}", foreign_repo.simple()))
        .execute(&f.pool)
        .await
        .unwrap();
    for repository in [assigned_repo, foreign_repo] {
        let storage = cicd::repository_catalog::storage_by_id(&f.pool, repository)
            .await
            .unwrap();
        pull_fixture::insert(
            &f.pool,
            &storage,
            1,
            "Workspace candidate",
            ("feature", "main"),
            "open",
            "fixture",
        )
        .await;
    }
    let pulls = format!("/api/v1/catalog/repositories/{assigned_repo}/pulls");
    let (status, pull) = http(
        &app,
        "GET",
        &format!("{pulls}/1"),
        Some(&machine),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pull}");
    assert_eq!(pull["repository_id"], assigned_repo.to_string());
    let foreign_pull = format!("/api/v1/catalog/repositories/{foreign_repo}/pulls/1");
    let (status, foreign) = http(&app, "GET", &foreign_pull, Some(&machine), json!(null)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{foreign}");
    drop(app);
    drop(restarted);
    f.close().await;
}
impl Fixture {
    async fn new() -> Self {
        let url =
            std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable database required");
        let options = PgConnectOptions::from_str(&url).unwrap();
        assert!(
            options
                .get_database()
                .is_some_and(|name| name.starts_with("forge_test_"))
        );
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let database = format!("forge_test_development_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await
            .unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options.database(&database))
            .await
            .unwrap();
        cicd::migrations().await.unwrap().run(&pool).await.unwrap();
        let actor = Uuid::new_v4();
        let other_actor = Uuid::new_v4();
        for id in [actor, other_actor] {
            sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'developer')")
                .bind(id)
                .bind(id.to_string())
                .execute(&pool)
                .await
                .unwrap();
        }
        let repository = Uuid::new_v4();
        sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
            .bind(repository)
            .bind(format!("source-{}", repository.simple()))
            .execute(&pool)
            .await
            .unwrap();
        let project = Uuid::new_v4();
        let tracker_instance = Uuid::new_v4();
        let tracker_project = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Instance fixture','https://example.invalid/source.git',$2)").bind(project).bind(repository).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,1,'Product','DEV','active',clock_timestamp())")
            .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(tracker_instance).bind(tracker_project).execute(&pool).await.unwrap();
        let workspace = Uuid::new_v4();
        let other_workspace = Uuid::new_v4();
        let environment = Uuid::new_v4();
        for (id, environment) in [(workspace, environment), (other_workspace, Uuid::new_v4())] {
            sqlx::query("INSERT INTO environments(id,project_id,name,purpose) VALUES($1,$2,$3,'workspace_target')")
                .bind(environment)
                .bind(project)
                .bind(id.to_string())
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO development_workspaces(id,project_id,repository_id,environment_id,environment_hash,tracker_instance_id,tracker_project_id,name,profile_id,target_id,catalog_target_id,profile_hash,catalog_hash,profile,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$5,$5,'{}',$12)")
                .bind(id).bind(project).bind(repository).bind(environment).bind("a".repeat(64)).bind(tracker_instance).bind(tracker_project).bind(id.to_string()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(actor).execute(&pool).await.unwrap();
        }
        Self {
            pool,
            admin,
            database,
            workspace,
            other_workspace,
            project,
            environment,
            actor,
            other_actor,
        }
    }
    async fn version(&self) -> i64 {
        sqlx::query_scalar("SELECT version FROM development_workspaces WHERE id=$1")
            .bind(self.workspace)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
    async fn enqueue(
        &self,
        id: Uuid,
        actor: Uuid,
        version: i64,
        action: WorkspaceAction,
    ) -> Result<store::Operation, store::Error> {
        let mut tx = self.pool.begin().await?;
        let operation = store::enqueue(
            &mut tx,
            self.workspace,
            id,
            actor,
            actor,
            version,
            &action,
            &json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(operation)
    }
    async fn close(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

#[tokio::test]
async fn workspace_retirement_preserves_the_target_and_original_operation() {
    for (action, runtime, expected_purpose) in [
        (WorkspaceAction::Close {}, "stopped", "workspace_target"),
        (WorkspaceAction::ConvertToStand {}, "running", "stand"),
    ] {
        let f = Fixture::new().await;
        let id = Uuid::new_v4();
        let version = f.version().await;
        assert!(matches!(
            f.enqueue(id, f.actor, version, action.clone()).await,
            Err(store::Error::Conflict(
                "workspace_retirement_runtime_unconfirmed"
            ))
        ));
        sqlx::query("UPDATE development_workspaces SET runtime=$2,desired_runtime=$2,observed_at=now() WHERE id=$1")
            .bind(f.workspace).bind(runtime).execute(&f.pool).await.unwrap();
        let first = f
            .enqueue(id, f.actor, version, action.clone())
            .await
            .unwrap();
        let replay = f.enqueue(id, f.actor, version, action).await.unwrap();
        assert_eq!(first.state, "succeeded");
        assert_eq!(first.result, replay.result);
        assert_eq!(f.version().await, version + 1);
        let purpose: String = sqlx::query_scalar("SELECT purpose FROM environments WHERE id=$1")
            .bind(f.environment)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert_eq!(purpose, expected_purpose);
        assert!(matches!(
            f.enqueue(
                Uuid::new_v4(),
                f.actor,
                version + 1,
                WorkspaceAction::Start {}
            )
            .await,
            Err(store::Error::Conflict("workspace_retired"))
        ));
        f.close().await;
    }
}

#[tokio::test]
async fn concurrent_commands_keep_one_original_key_and_immutable_result() {
    let f = Fixture::new().await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (a, b) = tokio::join!(
        f.enqueue(first, f.actor, 1, WorkspaceAction::Start {}),
        f.enqueue(second, f.actor, 1, WorkspaceAction::Stop {})
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let original = a.or(b).unwrap();
    let command: WorkspaceAction = serde_json::from_value(original.request.clone()).unwrap();
    let replay = f
        .enqueue(original.id, f.actor, 1, command.clone())
        .await
        .unwrap();
    assert_eq!(replay.id, original.id);
    assert_eq!(f.version().await, 2);
    assert!(matches!(
        f.enqueue(original.id, f.other_actor, 1, command).await,
        Err(store::Error::Conflict("operation_key_payload_conflict"))
    ));
    assert!(matches!(
        f.enqueue(original.id, f.actor, 1, WorkspaceAction::Observe {})
            .await,
        Err(store::Error::Conflict("operation_key_payload_conflict"))
    ));
    sqlx::query(
        "UPDATE development_workspace_operations SET state='unknown',result=$2 WHERE id=$1",
    )
    .bind(original.id)
    .bind(json!({"state":"unknown"}))
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), f.actor, 2, WorkspaceAction::Stop {})
            .await,
        Err(store::Error::Conflict("workspace_operation_in_progress"))
    ));
    sqlx::query("UPDATE development_workspace_operations SET state='succeeded',finished_at=clock_timestamp(),result=$2 WHERE id=$1").bind(original.id).bind(json!({"state":"succeeded"})).execute(&f.pool).await.unwrap();
    assert!(
        sqlx::query("UPDATE development_workspace_operations SET result='{}' WHERE id=$1")
            .bind(original.id)
            .execute(&f.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM development_workspace_operations WHERE id=$1")
            .bind(original.id)
            .execute(&f.pool)
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn session_writer_is_transactional_and_stop_never_ends_the_session() {
    let f = Fixture::new().await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let insert = |id, actor| {
        sqlx::query("INSERT INTO development_workspace_sessions(id,workspace_id,actor_id,owner_subject,task_id,task_ref) VALUES($1,$2,$3,$3,$4,'DEV-1')").bind(id).bind(f.workspace).bind(actor).bind(Uuid::new_v4()).execute(&f.pool)
    };
    let (a, b) = tokio::join!(insert(first, f.actor), insert(second, f.other_actor));
    assert_ne!(a.is_ok(), b.is_ok());
    let session = store::session(&f.pool, f.workspace).await.unwrap().unwrap();
    assert!(matches!(
        f.enqueue(
            Uuid::new_v4(),
            session.actor_id,
            1,
            WorkspaceAction::Stop {}
        )
        .await,
        Err(store::Error::Conflict("workspace_has_active_session"))
    ));
    let other = if session.actor_id == f.actor {
        f.other_actor
    } else {
        f.actor
    };
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), other, 1, WorkspaceAction::Start {})
            .await,
        Err(store::Error::Conflict(
            "workspace_session_owned_by_another_actor"
        ))
    ));
    assert!(
        store::session(&f.pool, f.workspace)
            .await
            .unwrap()
            .unwrap()
            .ended_at
            .is_none()
    );
    assert!(
        f.enqueue(
            Uuid::new_v4(),
            session.actor_id,
            1,
            WorkspaceAction::Start {}
        )
        .await
        .is_ok()
    );
    f.close().await;
}

#[tokio::test]
async fn native_delivery_hold_blocks_only_its_instance_until_confirmed() {
    let f = Fixture::new().await;
    let pipeline = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','failed')",
    )
    .bind(pipeline)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    let deployment = Uuid::new_v4();
    sqlx::query("INSERT INTO deployments(id,environment_id,pipeline_id,git_ref,status,runtime_phase) VALUES($1,$2,$3,'main','failed','held')").bind(deployment).bind(f.environment).bind(pipeline).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    assert!(store::workspace_busy(&mut tx, f.workspace).await.unwrap());
    assert!(
        !store::workspace_busy(&mut tx, f.other_workspace)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), f.actor, 1, WorkspaceAction::Stop {})
            .await,
        Err(store::Error::Conflict("workspace_has_active_pipeline"))
    ));
    sqlx::query("UPDATE deployments SET runtime_phase='finished' WHERE id=$1")
        .bind(deployment)
        .execute(&f.pool)
        .await
        .unwrap();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let lease = Uuid::new_v4();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'native',0,'success')").bind(stage).bind(pipeline).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'native','fixture','true',0,'success')").bind(job).bind(stage).execute(&f.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'success')",
    )
    .bind(attempt)
    .bind(job)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO runners(id,name) VALUES($1,'owned-runner')")
        .bind(runner)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_id,runner_name,lease_status,generation,lease_expires_at,completed_at,terminal_status,acknowledged_at) VALUES($1,$2,$3,$4,'owned-runner','completed',1,clock_timestamp(),clock_timestamp(),'success',clock_timestamp())")
        .bind(lease).bind(job).bind(attempt).bind(runner).execute(&f.pool).await.unwrap();
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), f.actor, 1, WorkspaceAction::Stop {})
            .await,
        Err(store::Error::Conflict("workspace_has_active_pipeline"))
    ));
    sqlx::query("UPDATE job_leases SET completion_received_at=clock_timestamp() WHERE id=$1")
        .bind(lease)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        f.enqueue(Uuid::new_v4(), f.actor, 1, WorkspaceAction::Stop {})
            .await
            .is_ok()
    );
    f.close().await;
}
