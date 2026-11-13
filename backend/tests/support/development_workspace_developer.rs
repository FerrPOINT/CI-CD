//! Real public HTTP, Central PAT verification, PostgreSQL and workspace worker.
use super::*;
use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use std::sync::{Arc, RwLock};

const TOKEN: &str = "sdlc_pat_workspace_developer_fixture";
const DEVOPS_TOKEN: &str = "sdlc_pat_workspace_devops_fixture";
const SOURCE_READER: &str = "sdlc_pat_workspace_source_reader_fixture";
const PROJECT_OWNER: &str = "sdlc_pat_workspace_project_owner_fixture";
const OWNER_READER: &str = "workspace-current-owner-reader-fixture";
const OTHER_HUMAN: &str = "sdlc_pat_workspace_other_human_fixture";
const CHILD: &str = "FORGE_WORKSPACE_DEVELOPER_TEST";
const SELECTOR: &str =
    "developer::configured_developer_is_fenced_by_assignment_and_keeps_humans_separate";

#[path = "development_workspace_developer_checks.rs"]
mod checks;

#[derive(Clone)]
struct Owner {
    context: serde_json::Value,
    subject: Uuid,
    devops_subject: Uuid,
    source_reader: Uuid,
    repository: Uuid,
    project_owner: Uuid,
    other_human: Uuid,
    revoked: bool,
    persistent: bool,
}

fn current_application_access(subject: Uuid, role: &str, headers: &HeaderMap) -> Value {
    let now=chrono::Utc::now();
    json!({"schema_version":1,"issuer":format!("http://{}",headers["host"].to_str().unwrap()),
        "subject":subject,"role":role,"applications":["ci-cd"],"revision":1,"enforced":true,
        "observed_at":now,"expires_at":now+chrono::Duration::seconds(30)})
}

#[tokio::test]
async fn configured_developer_is_fenced_by_assignment_and_keeps_humans_separate() {
    if let Ok(raw) = std::env::var(CHILD) {
        child(serde_json::from_str(&raw).unwrap()).await;
        return;
    }
    let f = Fixture::new().await;
    let workspace = store::workspace(&f.pool, f.workspace).await.unwrap();
    let (registry,namespace):(Uuid,Uuid)=sqlx::query_as("SELECT registry_instance_id,namespace_id FROM forge_workspace_projects WHERE tracker_instance_id=$1 AND tracker_project_id=$2")
        .bind(workspace.tracker_instance_id).bind(workspace.tracker_project_id).fetch_one(&f.pool).await.unwrap();
    let subject = Uuid::new_v4();
    let forge_instance = Uuid::new_v4();
    let source_reader = Uuid::new_v4();
    let task = Uuid::new_v4();
    let namespace = json!({"registry_instance_id":registry,"namespace_id":namespace});
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO git_groups(id,slug,name,namespace_managed) VALUES($1,$2,'Workspace Developer fixture',true)")
        .bind(group).bind(format!("workspace-{}",group.simple())).execute(&f.pool).await.unwrap();
    let command = json!({"schema_version":1,"namespace":namespace,
        "resource":{"kind":"git_group","instance_id":forge_instance,"resource_id":group},
        "operation_id":Uuid::new_v4(),"generation":1,"state":"active"});
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
        .bind(group).bind(registry).bind(namespace["namespace_id"].as_str().unwrap().parse::<Uuid>().unwrap()).bind(command).execute(&f.pool).await.unwrap();
    sqlx::query(
        "UPDATE repository_catalog SET group_id=$2,ready=true WHERE id=$1 AND group_id IS NULL",
    )
    .bind(workspace.repository_id)
    .bind(group)
    .execute(&f.pool)
    .await
    .unwrap();
    // Hosted creation also publishes the public alias; storage names alone do
    // not become canonical Git URLs when a repository joins a namespace.
    let public_name: String = sqlx::query_scalar(
        "SELECT g.slug||'/'||r.slug FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id WHERE r.id=$1",
    )
    .bind(workspace.repository_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES($1,$2)")
        .bind(&public_name)
        .bind(workspace.repository_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let native = tempfile::tempdir().unwrap();
    let checkout = native.path().join("checkout");
    let git_root = native.path().join("repositories");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(&git_root).unwrap();
    for args in [
        vec!["init", "--initial-branch=main"],
        vec!["config", "user.name", "Workspace fixture"],
        vec!["config", "user.email", "fixture@invalid.test"],
    ] {
        git(&checkout, None, &args).await;
    }
    std::fs::write(checkout.join("README.md"), "first\n").unwrap();
    git(&checkout, None, &["add", "README.md"]).await;
    git(&checkout, None, &["commit", "-m", "base"]).await;
    let base = git(&checkout, None, &["rev-parse", "HEAD"]).await;
    let storage: String =
        sqlx::query_scalar("SELECT storage_name FROM repository_catalog WHERE id=$1")
            .bind(workspace.repository_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let bare = git_root.join(format!("{storage}.git"));
    git(
        &checkout,
        None,
        &["clone", "--bare", ".", bare.to_str().unwrap()],
    )
    .await;
    std::fs::write(checkout.join("README.md"), "candidate\n").unwrap();
    git(&checkout, None, &["commit", "-am", "candidate"]).await;
    let source = json!({"contract_version":1,"tracker_instance_id":workspace.tracker_instance_id,"development_workspace_id":workspace.id,
        "project_id":workspace.tracker_project_id,"task_id":task,"root_task_id":task,
        "confirmation_id":Uuid::new_v4(),"requirement_revision":1,"content_hash":"a".repeat(64),
        "routing_snapshot_id":Uuid::new_v4(),"namespace":namespace,
        "forge_instance_id":forge_instance,
        "repository_id":workspace.repository_id,"base_ref":"refs/heads/main",
        "base_commit":base,"candidate_ref":format!("refs/heads/sdlc/{task}"),
        "source_revision":1,"policy_hash":"c".repeat(64),"source_hash":"d".repeat(64),
        "owner_subject":Uuid::new_v4(),"created_at":chrono::Utc::now()});
    let owner = Arc::new(RwLock::new(Owner {
        subject,
        devops_subject:Uuid::new_v4(),
        source_reader,
        repository: workspace.repository_id,
        project_owner: Uuid::new_v4(),
        other_human: Uuid::new_v4(),
        revoked: false,
        persistent: true,
        context: json!({
        "schema_version":1,"namespace":namespace,"tracker_instance_id":workspace.tracker_instance_id,
        "project_id":workspace.tracker_project_id,"task_id":task,"task_key":"DEV-1","state":"active",
        "execution":{"interaction_protocol":"configured-cli-v1","root_task_id":task,
            "stage":"Development","status":"InProgress","requirement_revision":1,"source_binding":source,
            "assignment":{"assignment_id":Uuid::new_v4(),"execution_id":Uuid::new_v4(),
                "agent_id":Uuid::new_v4(),"version":1,"machine_subject":subject}}}),
    }));
    let router=Router::new()
        .route("/oidc/jwks",get(||async{Json(json!({"keys":[]}))}))
        .route("/auth/tokens/introspect",get(|State(owner):State<Arc<RwLock<Owner>>>, headers:HeaderMap|async move {
            let owner=owner.read().unwrap().clone();
            let (subject,role,email,reader)=match headers["authorization"].to_str().unwrap() {
                value if value==format!("Bearer {SOURCE_READER}")=>(owner.source_reader,"service_account","reader@fixture.invalid",true),
                value if value==format!("Bearer {TOKEN}")=>(owner.subject,"service_account","developer@fixture.invalid",false),
                value if value==format!("Bearer {DEVOPS_TOKEN}")=>(owner.devops_subject,"service_account","devops@fixture.invalid",false),
                value if value==format!("Bearer {PROJECT_OWNER}")=>(owner.project_owner,"admin","owner@fixture.invalid",false),
                value if value==format!("Bearer {OTHER_HUMAN}")=>(owner.other_human,"user","human@fixture.invalid",false),
                _=>panic!("unexpected fixture bearer"),
            };
            if owner.revoked {return (StatusCode::UNAUTHORIZED,Json(json!({})));}
            let mut profile=json!({"sub":subject,"email":email,
                "role":role,"subject_role":role,"scopes":if reader{vec!["ci-cd:read"]}else{vec!["ci-cd:read","ci-cd:write"]},
                "observed_at":chrono::Utc::now(),"expires_at":if role != "service_account" || reader || !owner.persistent {
                    json!(chrono::Utc::now()+chrono::Duration::seconds(300))
                } else {Value::Null}});
            if role!="service_account" {
                profile["application_access"]=current_application_access(subject,role,&headers);
            }
            (StatusCode::OK,Json(profile))
        }))
        // The asynchronous human reconcile reads current Auth authority, never
        // the deliberately unprivileged JIT users.role projection.
        .route("/auth/access/decisions",post(|State(owner):State<Arc<RwLock<Owner>>>,headers:HeaderMap,Json(request):Json<Value>|async move {
            assert_eq!(headers["authorization"],format!("Bearer {OWNER_READER}"));
            assert_eq!(request["schema_version"],1);
            let owner=owner.read().unwrap();
            let decisions=request["subjects"].as_array().unwrap().iter().map(|subject| {
                let role=if *subject==json!(owner.project_owner) {"admin"}
                    else {assert_eq!(*subject,json!(owner.other_human));"user"};
                current_application_access(subject.as_str().unwrap().parse().unwrap(),role,&headers)
            }).collect::<Vec<_>>();
            Json(json!({"schema_version":1,"decisions":decisions}))
        }))
        .route("/auth/directory",get(|State(owner):State<Arc<RwLock<Owner>>>,headers:HeaderMap|async move {
            assert_eq!(headers["authorization"],format!("Bearer {PROJECT_OWNER}"));
            let owner=owner.read().unwrap();
            // Auth's real directory contract contains humans only, with no role field.
            Json(json!([
                {"status":"active","id":owner.project_owner,"email":"owner@fixture.invalid","username":"owner","display_name":"Owner"},
                {"status":"active","id":owner.other_human,"email":"human@fixture.invalid","username":"human","display_name":"Human"}
            ]))
        }))
        .route("/api/v1/namespace-tasks/{id}",get(|State(owner):State<Arc<RwLock<Owner>>>, headers:HeaderMap, axum::extract::Path(id):axum::extract::Path<Uuid>, uri:axum::http::Uri|async move {
            assert_eq!(headers["authorization"],"Bearer workspace-reader-fixture");
            let mut context=owner.read().unwrap().context.clone();
            assert_eq!(context["task_id"],id.to_string());
            let query=reqwest::Url::parse(&format!("http://fixture{uri}")).unwrap();
            let params:std::collections::BTreeMap<_,_>=query.query_pairs().into_owned().collect();
            assert_eq!(params["registry_instance_id"],context["namespace"]["registry_instance_id"].as_str().unwrap());
            assert_eq!(params["namespace_id"],context["namespace"]["namespace_id"].as_str().unwrap());
            if params.get("include_execution").map(String::as_str)!=Some("true") {context.as_object_mut().unwrap().remove("execution");}
            Json(context)
        }))
        .route("/fixture",post(|State(owner):State<Arc<RwLock<Owner>>>,Json(mode):Json<String>|async move {
            let mut owner=owner.write().unwrap();
            match mode.as_str() {
                "foreign_repository"=>owner.context["execution"]["source_binding"]["repository_id"]=json!(Uuid::new_v4()),
                "restore_repository"=>{ let repository=owner.repository; owner.context["execution"]["source_binding"]["repository_id"]=json!(repository); },
                "stale"=>owner.context["execution"]["assignment"]["version"]=json!(2),
                "restore"=>owner.context["execution"]["assignment"]["version"]=json!(1),
                "deployment_ready"=>{
                    owner.context["execution"]["stage"]=json!("Deployment");
                    owner.context["execution"]["status"]=json!("Ready");
                    let subject=owner.devops_subject;
                    owner.context["execution"]["assignment"]["machine_subject"]=json!(subject);
                    owner.context["execution"]["assignment"]["assignment_id"]=json!(Uuid::new_v4());
                    owner.context["execution"]["assignment"]["execution_id"]=json!(Uuid::new_v4());
                },
                "deployment_running"=>owner.context["execution"]["status"]=json!("InProgress"),
                "revoked"=>owner.revoked=true,
                "dated"=>owner.persistent=false,
                "persistent"=>owner.persistent=true,
                _=>panic!("unknown fixture mode"),
            }
            StatusCode::NO_CONTENT
        })).with_state(owner);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut reader = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    reader.write_all(b"workspace-reader-fixture").unwrap();
    let data = json!({"database":f.database,"workspace":f.workspace,"other_workspace":f.other_workspace,"project":f.project,"subject":subject,"task":task,"origin":origin,
        "git_root":git_root,"checkout":checkout,"repository":public_name,
        "registry":registry,"namespace":namespace["namespace_id"],"repository_id":workspace.repository_id,
        "tracker_instance_id":workspace.tracker_instance_id,"base_commit":base});
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SELECTOR, "--nocapture"])
            .env(CHILD, data.to_string())
            .env("CICD_AUTH__CENTRAL_JWKS_URI", format!("{origin}/oidc/jwks"))
            .env("CICD_AUTH__CENTRAL_ISSUER", &origin)
            .env("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_REQUIRED", "true")
            .env("CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN", OWNER_READER)
            .env("CICD_NAMESPACE__TRACKER_URL", &origin)
            .env(
                "CICD_NAMESPACE__TRACKER_INSTANCE_ID",
                workspace.tracker_instance_id.to_string(),
            )
            .env("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string())
            .env("CICD_NAMESPACE__TRACKER_TOKEN_FILE", reader.path())
            .env("CICD_NAMESPACE__READER_SUBJECTS", source_reader.to_string())
            .env("CICD_NAMESPACE__INSTANCE_ID", forge_instance.to_string())
            .env(
                "CICD_SECRETS_KEY",
                "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
            )
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .kill_on_drop(true)
            .output(),
    )
    .await;
    server.abort();
    let _ = server.await;
    f.close().await;
    let output = output
        .expect("isolated Developer HTTP case timed out")
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let marker = "FORGE_DEVELOPER_PUBLICATION_CHECK=verified";
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line == marker),
        "isolated Developer case did not confirm publication coverage"
    );
    let base_marker = "FORGE_BASE_NATIVE_DELIVERY=verified";
    assert!(String::from_utf8_lossy(&output.stderr).lines().any(|line| line == base_marker),
        "isolated DevOps case did not confirm Base-native delivery coverage");
    let adoption_marker="FORGE_PROFILE_ADOPTION=verified";
    assert!(String::from_utf8_lossy(&output.stderr).lines().any(|line|line==adoption_marker),
        "isolated case did not confirm profile adoption coverage");
    eprintln!("{marker}");
    eprintln!("{base_marker}");
    eprintln!("{adoption_marker}");
}

async fn child(data: serde_json::Value) {
    let id = |key: &str| data[key].as_str().unwrap().parse::<Uuid>().unwrap();
    let workspace = id("workspace");
    let project = id("project");
    let task = id("task");
    let subject = id("subject");
    let options = PgConnectOptions::from_str(&std::env::var("CICD_TEST_DATABASE_URL").unwrap())
        .unwrap()
        .database(data["database"].as_str().unwrap());
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    let catalog = tempfile::tempdir().unwrap();
    checks::publish(&pool, catalog.path(), workspace, true).await;
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some("workspace-developer-fixture-only".into()));
    let artifacts = tempfile::tempdir().unwrap();
    config.artifacts.root = artifacts.path().into();
    config.secrets.key = Some([7; 32]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    config.http.bind = listener.local_addr().unwrap().to_string();
    // This isolated child owns the Git HTTP server and its public origin.
    unsafe {
        std::env::set_var(
            "CICD_NAMESPACE__PUBLIC_GIT_ORIGIN",
            format!("http://{}", config.http.bind),
        );
    }
    config.git.root = data["git_root"].as_str().unwrap().into();
    config.git.internal_token = Some("workspace-fixture-internal".into());
    config.runner.registration_token = Some("workspace-runner-fixture".into());
    let git_url = format!(
        "http://{}/git/{}.git",
        config.http.bind,
        data["repository"].as_str().unwrap()
    );
    config.development_workspaces.catalog_root = Some(catalog.path().into());
    let app = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config.clone(),
    )
    .unwrap();
    let served = app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    let path = format!("/api/v1/development-workspaces/{workspace}");
    let (status, principal) = http(
        &app,
        "GET",
        "/api/v1/auth/principal",
        Some(TOKEN),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{principal}");
    assert_eq!(principal["principal_type"], "service_account");
    assert!(principal["project_id"].is_null());
    // An ordinary machine profile grants nothing without explicit membership.
    assert_eq!(
        http(&app, "GET", &path, Some(TOKEN), json!(null)).await.0,
        StatusCode::FORBIDDEN
    );
    let actor: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE central_sub=$1 AND role='service_account'")
            .bind(subject.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(principal["sub"], actor.to_string());
    let memberships = format!("/api/v1/projects/{project}/memberships");
    let grant = json!({"user_id":actor,"role":"developer"});
    assert_eq!(http(&app,"POST",&memberships,Some(TOKEN),grant.clone()).await.0,StatusCode::FORBIDDEN);
    let (status, users) = http(&app,"GET","/api/v1/users",Some(PROJECT_OWNER),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{users}");
    assert!(users.as_array().unwrap().iter().all(|user|user["id"]!=actor.to_string()));
    let (status, human) = http(&app,"GET","/api/v1/auth/principal",Some(OTHER_HUMAN),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{human}");
    assert_eq!(http(&app,"POST",&memberships,Some(PROJECT_OWNER),json!({"user_id":human["sub"],"role":"developer"})).await.0,StatusCode::FORBIDDEN);
    assert_eq!(http(&app,"DELETE",&format!("{memberships}/{}",human["sub"].as_str().unwrap()),Some(PROJECT_OWNER),json!(null)).await.0,StatusCode::FORBIDDEN);
    sqlx::query("UPDATE users SET enabled=false WHERE id=$1")
        .bind(actor).execute(&pool).await.unwrap();
    assert_eq!(http(&app,"POST",&memberships,Some(PROJECT_OWNER),grant.clone()).await.0,StatusCode::FORBIDDEN);
    sqlx::query("UPDATE users SET enabled=true WHERE id=$1")
        .bind(actor).execute(&pool).await.unwrap();
    // Membership and its audit must either both commit or both roll back.
    sqlx::raw_sql("CREATE FUNCTION reject_membership_fixture_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='project_membership.upserted' THEN RAISE EXCEPTION 'fixture audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_membership_fixture_audit BEFORE INSERT ON audit_log FOR EACH ROW EXECUTE FUNCTION reject_membership_fixture_audit();")
        .execute(&pool).await.unwrap();
    assert_eq!(http(&app,"POST",&memberships,Some(PROJECT_OWNER),grant.clone()).await.0,StatusCode::INTERNAL_SERVER_ERROR);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_memberships WHERE project_id=$1 AND user_id=$2")
        .bind(project).bind(actor).fetch_one(&pool).await.unwrap();
    assert_eq!(count,0);
    sqlx::raw_sql("DROP TRIGGER reject_membership_fixture_audit ON audit_log; DROP FUNCTION reject_membership_fixture_audit();")
        .execute(&pool).await.unwrap();
    let (status, membership) = http(&app,"POST",&memberships,Some(PROJECT_OWNER),grant).await;
    assert_eq!(status,StatusCode::OK,"{membership}");
    assert_eq!(membership["user_id"],actor.to_string());
    let (status, members) = http(&app,"GET",&memberships,Some(PROJECT_OWNER),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{members}");
    assert!(members.as_array().unwrap().iter().any(|member| member["user_id"]==actor.to_string() && member["role"]=="developer"));
    let (status, owner) = http(&app,"GET","/api/v1/auth/principal",Some(PROJECT_OWNER),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{owner}");
    assert_eq!(sqlx::query_scalar::<_,String>("SELECT role FROM users WHERE id=$1")
        .bind(owner["sub"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(&pool).await.unwrap(),
        "developer", "current Auth admin must not promote the local profile");
    let audit_actor: String = sqlx::query_scalar("SELECT actor FROM audit_log WHERE resource_id=$1 AND action='project_membership.upserted'")
        .bind(project).fetch_one(&pool).await.unwrap();
    assert_eq!(audit_actor,format!("{}:{actor}:developer",owner["sub"].as_str().unwrap()));
    let machine_role: String = sqlx::query_scalar("SELECT role FROM users WHERE id=$1")
        .bind(actor).fetch_one(&pool).await.unwrap();
    assert_eq!(machine_role,"service_account");
    let (status, card) = http(&app, "GET", &path, Some(TOKEN), json!(null)).await;
    assert_eq!(status, StatusCode::OK, "persistent root ingress: {card}");
    let client = reqwest::Client::new();
    let control = format!("{}/fixture", data["origin"].as_str().unwrap());
    // Same real bridge, router and DB principal: a dated root still works;
    // switching back to explicit null must preserve current project rights.
    for mode in ["dated", "persistent"] {
        client
            .post(&control)
            .json(mode)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        let (status, principal) = http(
            &app,
            "GET",
            "/api/v1/auth/principal",
            Some(TOKEN),
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{mode}: {principal}");
        assert_eq!(principal["sub"], actor.to_string());
        assert_eq!(principal["principal_type"], "service_account");
        let (status, current) = http(&app, "GET", &path, Some(TOKEN), json!(null)).await;
        assert_eq!(status, StatusCode::OK, "{mode}: {current}");
        assert_eq!(current, card);
    }
    let mut observation = reqwest::Url::parse(&format!(
        "http://fixture/api/v1/namespace-repositories/{}/source",
        id("repository_id")
    ))
    .unwrap();
    observation
        .query_pairs_mut()
        .append_pair("registry_instance_id", data["registry"].as_str().unwrap())
        .append_pair("namespace_id", data["namespace"].as_str().unwrap())
        .append_pair("base_ref", "refs/heads/main")
        .append_pair("base_commit", data["base_commit"].as_str().unwrap())
        .append_pair("development_workspace_id", &workspace.to_string());
    let endpoint = format!("{}?{}", observation.path(), observation.query().unwrap());
    let (status, proof) = http(&app, "GET", &endpoint, Some(SOURCE_READER), json!(null)).await;
    assert_eq!(status, StatusCode::OK, "{proof}");
    assert_eq!(proof["development_workspace_id"], workspace.to_string());
    assert_eq!(
        http(
            &app,
            "GET",
            &endpoint.replace(&workspace.to_string(), &Uuid::new_v4().to_string()),
            Some(SOURCE_READER),
            json!(null)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(&app, "GET", &endpoint, Some(TOKEN), json!(null))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for action in [
        "start",
        "stop",
        "observe",
        "recover_session",
        "reconcile",
        "build",
    ] {
        assert_eq!(
            card["actions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["action"] == action)
                .unwrap()["allowed"],
            false
        );
    }
    assert_eq!(
        http(
            &app,
            "GET",
            &format!("/api/v1/projects/{}/development-workspaces", Uuid::new_v4()),
            Some(TOKEN),
            json!(null)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for command in [
        json!({"action":"start"}),
        json!({"action":"stop"}),
        json!({"action":"build","source_commit":"a".repeat(40)}),
        json!({"action":"recover_session","session_id":Uuid::new_v4()}),
    ] {
        assert_eq!(
            http(
                &app,
                "POST",
                &format!("{path}/operations"),
                Some(TOKEN),
                json!({"operation_id":Uuid::new_v4(),"expected_version":1,"command":command})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    for denied in [
        format!("/api/v1/projects/{project}/development-workspaces"),
        format!("/api/v1/projects/{project}/pipelines"),
    ] {
        assert_eq!(
            http(&app, "POST", &denied, Some(TOKEN), json!({})).await.0,
            StatusCode::FORBIDDEN
        );
    }
    checks::publish(&pool, catalog.path(), workspace, false).await;
    let unsupported = Uuid::new_v4();
    let (status, denied) = http(
        &app, "POST", &format!("{path}/operations"), Some(TOKEN),
        json!({"operation_id":unsupported,"expected_version":1,"command":{"action":"begin_session","task":task.to_string()}}),
    ).await;
    assert_eq!(status, StatusCode::CONFLICT, "{denied}");
    assert!(
        denied
            .to_string()
            .contains("workspace_required_developer_checks_missing"),
        "{denied}"
    );
    assert!(store::session(&pool, workspace).await.unwrap().is_none());
    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM development_workspace_operations WHERE id=$1")
            .bind(unsupported)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(recorded, 0);
    checks::publish(&pool, catalog.path(), workspace, true).await;
    let begin = Uuid::new_v4();
    assert_eq!(http(&app,"POST",&format!("/api/v1/development-workspaces/{}/operations",id("other_workspace")),Some(TOKEN),json!({"operation_id":Uuid::new_v4(),"expected_version":1,"command":{"action":"begin_session","task":task.to_string()}})).await.0,StatusCode::FORBIDDEN);
    let request = json!({"operation_id":begin,"expected_version":1,"command":{"action":"begin_session","task":task.to_string()}});
    let (status, result) = http(
        &app,
        "POST",
        &format!("{path}/operations"),
        Some(TOKEN),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(!result.to_string().contains(TOKEN));
    assert!(!result.to_string().contains("sealed"));
    checks::publish(&pool, catalog.path(), workspace, false).await;
    let (status, replay) = http(
        &app,
        "POST",
        &format!("{path}/operations"),
        Some(TOKEN),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, result);
    checks::publish(&pool, catalog.path(), workspace, true).await;
    let plan: serde_json::Value = sqlx::query_scalar(
        "SELECT execution_plan FROM development_workspace_operations WHERE id=$1",
    )
    .bind(begin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(plan["developerAuthority"]["sealed"].is_string());
    assert!(!plan.to_string().contains(TOKEN));
    let worker = tokio::spawn(cicd::development_workspaces::worker::supervisor(
        pool.clone(),
        config.clone(),
    ));
    let wait = |operation| {
        let pool = pool.clone();
        async move {
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                loop {
                    let operation = store::operation(&pool, workspace, operation).await.unwrap();
                    if matches!(operation.state.as_str(), "succeeded" | "failed" | "unknown") {
                        return operation;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .unwrap()
        }
    };
    let accepted = wait(begin).await;
    assert_eq!(accepted.state, "succeeded", "{:?}", accepted.result);
    let check_policy = accepted.result.as_ref().unwrap()["developerCheckPolicy"].clone();
    assert_eq!(
        check_policy,
        json!({"profile_hash":store::workspace(&pool, workspace).await.unwrap().profile_hash,
               "recipes":["unit"]})
    );
    let session = store::session(&pool, workspace).await.unwrap().unwrap();
    assert_eq!(session.id, begin);
    assert_eq!(session.owner_subject, subject);
    assert_eq!(session.task_id, task);
    assert_eq!(
        session.machine_assignment.as_ref().unwrap()["machine_subject"],
        subject.to_string()
    );
    assert!(
        sqlx::query(
            "UPDATE development_workspace_sessions SET machine_assignment='{}' WHERE id=$1"
        )
        .bind(begin)
        .execute(&pool)
        .await
        .is_err()
    );
    let checkout = std::path::Path::new(data["checkout"].as_str().unwrap());
    git(checkout, None, &["remote", "add", "origin", &git_url]).await;
    let candidate = format!("refs/heads/sdlc/{task}");
    git(
        checkout,
        Some(workspace),
        &["push", "origin", &format!("HEAD:{candidate}")],
    )
    .await;
    let remote = git(
        checkout,
        Some(workspace),
        &["ls-remote", "origin", &candidate],
    )
    .await;
    assert!(remote.starts_with(&git(checkout, None, &["rev-parse", "HEAD"]).await));
    let denied = git_output(
        checkout,
        Some(workspace),
        &["push", "origin", "HEAD:refs/heads/foreign"],
    )
    .await;
    assert!(!denied.status.success(), "foreign branch was accepted");
    let pulls = format!("/api/v1/catalog/repositories/{}/pulls", id("repository_id"));
    let (status, pull) = http(
        &app,
        "POST",
        &pulls,
        Some(TOKEN),
        json!({"title":"Developer fixture","source_branch":candidate,"target_branch":"main",
               "primary_task":{"tracker_instance_id":id("tracker_instance_id"),"task_id":task}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pull}");
    assert_eq!(
        pull["primary_tracker_instance_id"],
        data["tracker_instance_id"]
    );
    assert_eq!(pull["primary_task_id"], task.to_string());
    assert_eq!(pull["repository_id"], data["repository_id"]);
    let (status, readback) = http(
        &app,
        "GET",
        &format!("{pulls}/{}", pull["number"]),
        Some(TOKEN),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{readback}");
    assert_eq!(readback, pull);
    let pipelines: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(project)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        pipelines, 0,
        "Developer Git must not invoke ordinary delivery"
    );
    checks::exercise(&app, &pool, workspace, &data,"unit").await;
    checks::exercise_publication(&app, &pool, workspace, &data, catalog.path()).await;
    let version: i64 = sqlx::query_scalar("SELECT version FROM development_workspaces WHERE id=$1")
        .bind(workspace)
        .fetch_one(&pool)
        .await
        .unwrap();
    client
        .post(&control)
        .json("stale")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let denied = git_output(
        checkout,
        Some(workspace),
        &["ls-remote", "origin", &candidate],
    )
    .await;
    assert!(
        !denied.status.success(),
        "stale assignment kept native Git access"
    );
    let end = Uuid::new_v4();
    let end_request = json!({"operation_id":end,"expected_version":version,"command":{"action":"end_session","session_id":begin}});
    assert_eq!(
        http(
            &app,
            "POST",
            &format!("{path}/operations"),
            Some(TOKEN),
            end_request.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // A lost response can read the original receipt even after its assignment ends.
    let (status, replay) = http(
        &app,
        "POST",
        &format!("{path}/operations"),
        Some(TOKEN),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["id"], begin.to_string());
    assert_eq!(replay["result"]["developerCheckPolicy"], check_policy);
    let mut conflict = request;
    conflict["command"]["task"] = json!(Uuid::new_v4());
    assert_eq!(
        http(
            &app,
            "POST",
            &format!("{path}/operations"),
            Some(TOKEN),
            conflict
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    worker.abort();
    let _ = worker.await;
    let worker = tokio::spawn(cicd::development_workspaces::worker::supervisor(
        pool.clone(),
        config,
    ));
    assert_eq!(
        http(
            &app,
            "GET",
            &format!("{path}/operations/{begin}"),
            Some(TOKEN),
            json!(null)
        )
        .await
        .1["state"],
        "succeeded"
    );
    client
        .post(&control)
        .json("restore")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let (status, result) = http(
        &app,
        "POST",
        &format!("{path}/operations"),
        Some(TOKEN),
        end_request,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(wait(end).await.state, "succeeded");
    assert!(store::session(&pool, workspace).await.unwrap().is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM development_workspace_sessions WHERE workspace_id=$1"
        )
        .bind(workspace)
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    checks::exercise_profile_adoption(&app,&pool,workspace,&data,catalog.path(),begin).await;
    checks::exercise_devops(&app,&pool,workspace,&data,catalog.path(),artifacts.path(),begin).await;
    assert_eq!(http(&app,"DELETE",&format!("{memberships}/{actor}"),Some(TOKEN),json!(null)).await.0,StatusCode::FORBIDDEN);
    let (status, removed) = http(&app,"DELETE",&format!("{memberships}/{actor}"),Some(PROJECT_OWNER),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{removed}");
    assert_eq!(
        http(&app, "GET", &path, Some(TOKEN), json!(null)).await.0,
        StatusCode::FORBIDDEN
    );
    client
        .post(&control)
        .json("revoked")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        http(
            &app,
            "GET",
            "/api/v1/auth/principal",
            Some(TOKEN),
            json!(null),
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
        "explicit revocation rejects the same persistent root before handler dispatch",
    );
    assert!(
        http(&app, "GET", &path, Some(TOKEN), json!(null))
            .await
            .0
            .is_client_error()
    );
    worker.abort();
    let _ = worker.await;
    server.abort();
    let _ = server.await;
    pool.close().await;
}

async fn git_output(
    directory: &std::path::Path,
    workspace: Option<Uuid>,
    args: &[&str],
) -> std::process::Output {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("CICD_API_TOKEN", TOKEN)
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "credential.helper")
        .env(
            "GIT_CONFIG_VALUE_0",
            "!f() { test \"$1\" = get || exit 0; printf 'password=%s\\n' \"$CICD_API_TOKEN\"; }; f",
        )
        .env("GIT_CONFIG_KEY_1", "http.followRedirects")
        .env("GIT_CONFIG_VALUE_1", "false")
        .kill_on_drop(true);
    if let Some(workspace) = workspace {
        command
            .arg("-c")
            .arg(format!("credential.username=workspace-{workspace}"));
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        command.args(args).output(),
    )
    .await
    .unwrap()
    .unwrap()
}
async fn git(directory: &std::path::Path, workspace: Option<Uuid>, args: &[&str]) -> String {
    let output = git_output(directory, workspace, args).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
