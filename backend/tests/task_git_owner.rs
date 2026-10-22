//! Native task child, real HTTP/Git/PG, current owner applications and fixed clocks.
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use cicd::{
    config::SdlcWorkspaceConfig,
    domain::sdlc_workspace::{TaskWorkspaceIntent, WorkspaceRole, WorkspaceTaskBinding},
    task_git::{GitContext, SourceBinding},
};
use pdlc_auth_core::AuthContext;
use pdlc_shared::resource_context::NamespaceRef;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, RwLock},
    time::Duration,
};
use uuid::Uuid;
fn digest(value: &impl serde::Serialize) -> Result<String, serde_json::Error> {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Object(v) => Value::Object(
                v.into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(v) => Value::Array(v.into_iter().map(canonical).collect()),
            other => other,
        }
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical(serde_json::to_value(
            value
        )?))?)
    ))
}
fn seal(source: &mut SourceBinding) {
    let mut value = serde_json::to_value(&*source).unwrap();
    value.as_object_mut().unwrap().remove("source_hash");
    source.source_hash = digest(&value).unwrap();
}

fn fixture() -> (GitContext, Value, AuthContext, SdlcWorkspaceConfig) {
    let now = Utc::now();
    let task = Uuid::new_v4();
    let mut intent = TaskWorkspaceIntent {
        operation_key: "git:fixture".into(),
        repository_id: Uuid::new_v4(),
        source_commit: "a".repeat(40),
        role: WorkspaceRole::Developer,
        agent_id: Uuid::new_v4(),
        tracker_lease_id: Uuid::new_v4(),
        holder_subject: "fixture-scheduler".into(),
        expected_lease_version: 2,
        binding: WorkspaceTaskBinding {
            tracker_instance_id: Uuid::new_v4().to_string(),
            tracker_project_id: Uuid::new_v4(),
            task_id: task,
            root_task_id: task,
            assignment_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            routing_snapshot_id: Uuid::new_v4(),
            requirement_revision: 1,
            fencing_token: 3,
            assignment_hash: "b".repeat(64),
            workflow_task_ref: "SDLC-27".into(),
        },
    };
    let routing = json!({"tracker_instance_id":intent.binding.tracker_instance_id,
        "project_id":intent.binding.tracker_project_id,"task_id":task,"root_task_id":task,
        "snapshot_id":intent.binding.routing_snapshot_id,"requirement_revision":1,
        "policy":{"tracker_instance_id":intent.binding.tracker_instance_id,
        "project_id":intent.binding.tracker_project_id,"routes":{"developer":{"agent_id":intent.agent_id}}}});
    let mut assignment = json!({"task_id":task,"assignment_id":intent.binding.assignment_id,
        "execution_id":intent.binding.execution_id,"execution":{"key":"SDLC-27","ordinal":"27"},
        "agent_id":intent.agent_id,"lease_id":intent.tracker_lease_id,"role":"developer",
        "stage":"Development","cycle":0,"attempt":1,"owner_subject":"11111111-1111-4111-8111-111111111111",
        "fencing_token":3,"routing":routing});
    intent.binding.assignment_hash = digest(&assignment).unwrap();
    assignment["assignment_hash"] = json!(intent.binding.assignment_hash);
    let mut source = SourceBinding {
        contract_version: 1,
        tracker_instance_id: intent.binding.tracker_instance_id.clone(),
        project_id: intent.binding.tracker_project_id,
        task_id: task,
        root_task_id: task,
        confirmation_id: Uuid::new_v4(),
        requirement_revision: 1,
        content_hash: "c".repeat(64),
        routing_snapshot_id: intent.binding.routing_snapshot_id,
        namespace: NamespaceRef {
            registry_instance_id: Uuid::new_v4(),
            namespace_id: Uuid::new_v4(),
        },
        forge_instance_id: Uuid::new_v4(),
        repository_id: intent.repository_id,
        base_ref: "refs/heads/main".into(),
        base_commit: intent.source_commit.clone(),
        candidate_ref: format!("refs/heads/sdlc/{task}"),
        source_revision: 1,
        policy_hash: "d".repeat(64),
        source_hash: String::new(),
        owner_subject: "11111111-1111-4111-8111-111111111111".into(),
        created_at: now,
    };
    seal(&mut source);
    let context = GitContext {
        version: 1,
        project_id: Uuid::new_v4(),
        repository: "fixture/repo".into(),
        source,
        intent,
        run_id: Uuid::new_v4(),
        chat_id: Uuid::new_v4(),
        machine_subject: "fixture-native-machine".into(),
        old_oid: "0".repeat(40),
        new_oid: "e".repeat(40),
    };
    let current = json!({"observed_at":now,"runtime_state":"running","receipt":{
        "task":{"task_id":task,"stage":"Development","status":"InProgress","runtime":"running",
            "owner_subject":"11111111-1111-4111-8111-111111111111","cycle":0,"waiting_reason":null,"routing":routing},
        "assignment":assignment,"lease":{"lease_id":context.intent.tracker_lease_id,"version":2,
            "holder_subject":"fixture-scheduler","claimed_at":now,"heartbeat_at":now,
            "expires_at":now+chrono::Duration::seconds(30)},
        "binding":{"run_id":context.run_id,"chat_id":context.chat_id,"machine_subject":context.machine_subject}}});
    let readback = json!({"state":"bound","binding":context.source,"current":current});
    let subject = Uuid::new_v4();
    let central = AuthContext {
        user_id: subject.to_string(),
        email: None,
        role: None,
        session_id: None,
        token: "sdlc_pat_fixture_child".into(),
        scopes: [
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            context.grant().unwrap(),
        ]
        .into(),
    };
    let config = SdlcWorkspaceConfig {
        delegated_subject: Some(subject),
        ..Default::default()
    };
    (context, readback, central, config)
}

#[derive(Clone)]
struct Peer {
    issuer: String,
    context: GitContext,
    readback: Value,
    child_subject: String,
    apps: Vec<&'static str>,
    expires_ms: i64,
    unavailable: bool,
    space_allowed: bool,
}
fn child_token(context: &GitContext) -> String {
    format!(
        "sdlc_pat_{}",
        context
            .grant()
            .unwrap()
            .strip_prefix("ci-cd:sdlc:git:v1:")
            .unwrap()
    )
}
async fn introspect(State(peer): State<Arc<RwLock<Peer>>>, headers: HeaderMap) -> Json<Value> {
    let p = peer.read().unwrap().clone();
    assert_eq!(
        headers["authorization"].to_str().unwrap(),
        format!("Bearer {}", child_token(&p.context))
    );
    let now = Utc::now();
    Json(
        json!({"sub":p.child_subject,"email":"native-child@example.test","scopes":["ci-cd:read","ci-cd:write",p.context.grant().unwrap()],"observed_at":now,"expires_at":now+chrono::Duration::seconds(30)}),
    )
}
async fn owner(
    State(peer): State<Arc<RwLock<Peer>>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let p = peer.read().unwrap().clone();
    assert_eq!(headers["authorization"], "Bearer task-owner-reader");
    assert_eq!(
        request,
        json!({"schema_version":1,"subjects":[p.context.source.owner_subject]})
    );
    if p.unavailable {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let now = Utc::now();
    Json(json!({"schema_version":1,"decisions":[{
 "schema_version":1,"issuer":p.issuer,"subject":p.context.source.owner_subject,"role":"user","applications":p.apps,
 "revision":1,"enforced":true,"observed_at":now,"expires_at":now+chrono::Duration::milliseconds(p.expires_ms)}]})).into_response()
}
async fn source(State(peer): State<Arc<RwLock<Peer>>>, headers: HeaderMap) -> Json<Value> {
    assert_eq!(headers["authorization"], "Bearer task-source-reader");
    let p = peer.read().unwrap();
    let mut reply = p.readback.clone();
    let now = Utc::now();
    reply["binding"] = json!(p.context.source);
    reply["current"]["observed_at"] = json!(now);
    reply["current"]["receipt"]["lease"]["heartbeat_at"] = json!(now);
    reply["current"]["receipt"]["lease"]["expires_at"] = json!(now + chrono::Duration::seconds(30));
    Json(reply)
}
async fn spaces(
    State(peer): State<Arc<RwLock<Peer>>>,
    headers: HeaderMap,
    Json(request): Json<pdlc_shared::space_access::SpaceAdmissionRequest>,
) -> Json<pdlc_shared::space_access::SpaceAdmission> {
    use pdlc_shared::space_access::*;
    assert_eq!(headers["authorization"], "Bearer task-space-reader");
    let p = peer.read().unwrap();
    assert_eq!(request.identity.subject, p.context.source.owner_subject);
    assert!(request.valid_for(&p.issuer));
    let now = Utc::now().timestamp();
    Json(SpaceAdmission {
        schema_version: 1,
        identity: request.identity,
        identity_active: true,
        policy_revision: 1,
        policy: SpaceAccessPolicy::Selected {
            spaces: if p.space_allowed {
                request.spaces.clone()
            } else {
                vec![]
            },
        },
        inheritance: None,
        validity: AdmissionValidity {
            observed_at: now,
            valid_until: now + 3,
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
}
async fn git(cwd: &Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new("git")
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .await
        .unwrap()
}
async fn git_ok(cwd: &Path, args: &[&str]) -> String {
    let out = git(cwd, args).await;
    assert!(
        out.status.success(),
        "fixture Git: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
#[tokio::test]
#[ignore = "requires isolated FORGE_PRODUCT_TRANSPORT_TEST_DATABASE_URL"]
async fn current_owner_admits_native_child_without_borrowing_human_or_unrestricted_git() {
    let pool =
        sqlx::PgPool::connect(&std::env::var("FORGE_PRODUCT_TRANSPORT_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "forge_product_transport_test"
    );
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    let root = std::env::temp_dir().join(format!("forge-native-owner-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let work = root.join("work");
    tokio::fs::create_dir(&work).await.unwrap();
    git_ok(&work, &["init", "--initial-branch=main"]).await;
    git_ok(&work, &["config", "user.name", "Fixture"]).await;
    git_ok(&work, &["config", "user.email", "fixture@example.test"]).await;
    tokio::fs::write(work.join("file"), b"first").await.unwrap();
    git_ok(&work, &["add", "file"]).await;
    git_ok(&work, &["commit", "-m", "base"]).await;
    let base = git_ok(&work, &["rev-parse", "HEAD"]).await;
    tokio::fs::write(work.join("file"), b"second")
        .await
        .unwrap();
    git_ok(&work, &["commit", "-am", "candidate"]).await;
    let candidate = git_ok(&work, &["rev-parse", "HEAD"]).await;
    let mut config = cicd::config::RuntimeConfig::test_default()
        .with_auth_secret(Some("native-fixture-secret".into()));
    config.git.root = root.join("repositories");
    config.git.internal_token = Some("native-fixture-internal".into());
    let git_config = cicd::git_host::GitConfig {
        root: config.git.root.clone(),
        token: config.git.token.clone(),
        internal_token: config.git.internal_token.clone(),
    };
    let repo =
        cicd::git_host::create_repository_core(&pool, &git_config, "fixture", Some("private"))
            .await
            .unwrap();
    let bare = config.git.root.join("fixture.git");
    git_ok(
        &work,
        &[
            "--git-dir",
            bare.to_str().unwrap(),
            "fetch",
            work.to_str().unwrap(),
            &format!("{base}:refs/heads/main"),
        ],
    )
    .await;
    let (mut context, readback, actor, mut native) = fixture();
    context.repository = "fixture/repo".into();
    context.intent.repository_id = repo.id;
    context.source.repository_id = repo.id;
    context.intent.source_commit = base.clone();
    context.source.base_commit = base;
    context.new_oid = candidate;
    seal(&mut context.source);
    let registry = context.source.namespace.registry_instance_id;
    let namespace = context.source.namespace.namespace_id;
    let instance = context.source.forge_instance_id;
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,'fixture','Native fixture')")
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$1,slug='repo' WHERE id=$2")
        .bind(group)
        .bind(repo.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES('fixture/repo',$1)")
        .bind(repo.id)
        .execute(&pool)
        .await
        .unwrap();
    let command = json!({"schema_version":1,"namespace":context.source.namespace,"resource":{"instance_id":instance,"resource_id":group,"kind":"git_group"},"operation_id":Uuid::new_v4(),"generation":1,"state":"active"});
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)").bind(group).bind(registry).bind(namespace).bind(command).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,1,'Fixture','FIX','active',now())")
 .bind(registry).bind(namespace).bind(context.intent.binding.tracker_instance_id.parse::<Uuid>().unwrap()).bind(context.intent.binding.tracker_project_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Native','https://fixture.invalid',$2)").bind(context.project_id).bind(repo.id).execute(&pool).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let peer = Arc::new(RwLock::new(Peer {
        issuer: issuer.clone(),
        context: context.clone(),
        readback,
        child_subject: actor.user_id,
        apps: vec!["ci-cd", "task-tracker", "fleet-control"],
        expires_ms: 30000,
        unavailable: false,
        space_allowed: false,
    }));
    let router = Router::new()
        .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
        .route("/auth/tokens/introspect", get(introspect))
        .route("/auth/access/decisions", post(owner))
        .route("/admission", post(spaces))
        .route(
            &format!(
                "/api/v1/issues/{}/sdlc/source-binding",
                context.source.task_id
            ),
            get(source),
        )
        .with_state(peer.clone());
    unsafe {
        std::env::set_var("CICD_AUTH__CENTRAL_ISSUER", &issuer);
        std::env::set_var("CICD_AUTH__CENTRAL_JWKS_URI", format!("{issuer}/oidc/jwks"));
        std::env::set_var("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_REQUIRED", "true");
        std::env::set_var(
            "CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN",
            "task-owner-reader",
        );
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
        std::env::set_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string());
        std::env::set_var("CICD_SPACES__ADMISSION_URL", format!("{issuer}/admission"));
        std::env::set_var("CICD_SPACES__ADMISSION_TOKEN", "task-space-reader");
    }
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    native.tracker_read_url = Some(issuer);
    native.tracker_read_token = Some("task-source-reader".into());
    config.sdlc_workspace = native;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    config.http.bind = listener.local_addr().unwrap().to_string();
    let router =
        cicd::api::app_with_git_and_config(Some(pool.clone()), git_config, None, config).unwrap();
    let app = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    let endpoint = format!("{origin}/git/fixture/repo.git/info/refs?service=git-receive-pack");
    async fn discover(http: &reqwest::Client, endpoint: &str, context: &GitContext) -> StatusCode {
        http.get(endpoint)
            .bearer_auth(child_token(context))
            .header(
                cicd::task_git::HEADER,
                serde_json::to_string(context).unwrap(),
            )
            .send()
            .await
            .unwrap()
            .status()
    }
    assert_eq!(discover(&http, &endpoint, &context).await, StatusCode::OK);
    peer.write().unwrap().apps = vec!["ci-cd", "task-tracker"];
    assert_eq!(
        discover(&http, &endpoint, &context).await,
        StatusCode::FORBIDDEN,
        "a saved child cannot retain revoked Fleet"
    );
    peer.write().unwrap().apps = vec!["ci-cd", "task-tracker", "fleet-control"];
    peer.write().unwrap().unavailable = true;
    assert_eq!(
        discover(&http, &endpoint, &context).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    peer.write().unwrap().unavailable = false;
    let bearer = format!(
        "http.extraHeader=Authorization: Bearer {}",
        child_token(&context)
    );
    let header = format!(
        "http.extraHeader=x-forge-task-git-context: {}",
        serde_json::to_string(&context).unwrap()
    );
    git_ok(
        &work,
        &[
            "-c",
            &bearer,
            "-c",
            &header,
            "push",
            &format!("{origin}/git/fixture/repo.git"),
            &format!("HEAD:{}", context.source.candidate_ref),
        ],
    )
    .await;
    assert_eq!(
        git_ok(
            &work,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                &context.source.candidate_ref
            ]
        )
        .await,
        context.new_oid
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pipelines")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0,
        "native task Git never triggers legacy CI"
    );
    // A new native request has an exact new OID/grant but may not restart its owner clock at a hook.
    tokio::fs::write(work.join("file"), b"third").await.unwrap();
    git_ok(&work, &["commit", "-am", "next candidate"]).await;
    context.old_oid = context.new_oid.clone();
    context.new_oid = git_ok(&work, &["rev-parse", "HEAD"]).await;
    peer.write().unwrap().context = context.clone();
    peer.write().unwrap().expires_ms = 2000;
    let hook = bare.join("hooks/reference-transaction");
    let late = root.join("late");
    tokio::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nif test \"$1\" = prepared; then sleep 4; touch '{}'; fi\n",
            late.display()
        ),
    )
    .await
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let bearer = format!(
        "http.extraHeader=Authorization: Bearer {}",
        child_token(&context)
    );
    let header = format!(
        "http.extraHeader=x-forge-task-git-context: {}",
        serde_json::to_string(&context).unwrap()
    );
    let denied = git(
        &work,
        &[
            "-c",
            &bearer,
            "-c",
            &header,
            "push",
            &format!("{origin}/git/fixture/repo.git"),
            &format!("HEAD:{}", context.source.candidate_ref),
        ],
    )
    .await;
    assert!(!denied.status.success());
    tokio::time::sleep(Duration::from_millis(4200)).await;
    assert!(
        !late.exists(),
        "expired native hook cannot outlive original owner authority"
    );
    assert_eq!(
        git_ok(
            &work,
            &[
                "--git-dir",
                bare.to_str().unwrap(),
                "rev-parse",
                &context.source.candidate_ref
            ]
        )
        .await,
        context.old_oid
    );
    tokio::fs::remove_file(hook).await.unwrap();
    peer.write().unwrap().expires_ms = 30000;
    // Space admission is independent and composes with the same verified task owner.
    let operation = Uuid::new_v4();
    let command = json!({"schema_version":2,"space":{"registry_instance_id":registry,"space_id":namespace},"operation_id":operation,"revision":1});
    sqlx::query("INSERT INTO forge_space_catalog(registry_instance_id,space_id,group_id,revision,state,operation_id,command) VALUES($1,$2,$3,1,'active',$4,$5)").bind(registry).bind(namespace).bind(group).bind(operation).bind(command).execute(&pool).await.unwrap();
    sqlx::query("UPDATE forge_space_admission_state SET active=true WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        discover(&http, &endpoint, &context).await,
        StatusCode::FORBIDDEN
    );
    peer.write().unwrap().space_allowed = true;
    tokio::time::sleep(Duration::from_millis(3100)).await;
    assert_eq!(discover(&http, &endpoint, &context).await, StatusCode::OK);
    peer.write().unwrap().apps.clear();
    assert_eq!(
        discover(&http, &endpoint, &context).await,
        StatusCode::FORBIDDEN,
        "warm space proof never restores a revoked app"
    );
    app.abort();
    server.abort();
    pool.close().await;
    tokio::fs::remove_dir_all(root).await.unwrap();
}
