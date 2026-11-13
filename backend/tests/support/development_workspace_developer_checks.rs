//! Published-recipe and runner protocol qualification, not an image execution gate.
use super::*;
use serde_json::Value;

pub(super) async fn publish(pool: &PgPool, root: &std::path::Path, id: Uuid) {
    let workspace = store::workspace(pool, id).await.unwrap();
    let profile = json!({"apiVersion":"pdlc.dev/workspace/v1", "repositories":[{"key":"source"}],
        "tools":[], "preparationChecks":["unit"], "requiredDeveloperChecks":["unit"],
        "checks":[{"key":"unit","executor":"forge","requiresRuntime":false,"commands":["test -f README.md"]},
            {"key":"release","executor":"forge-delivery","requiresRuntime":false},
            {"key":"base-release","kind":"delivery","executor":"forge-delivery","requiresRuntime":false},
            {"key":"publish","kind":"build","executor":"forge-build","requiresRuntime":false}]});
    let profile_hash = store::request_hash(&profile).unwrap();
    let environment: Value = sqlx::query_scalar("SELECT jsonb_build_object('adapter','compose','environmentId',id,'projectId',project_id,'url',url) FROM environments WHERE id=$1")
        .bind(workspace.environment_id).fetch_one(pool).await.unwrap();
    let environment_hash = store::request_hash(&environment).unwrap();
    let public = json!({"id":workspace.catalog_target_id,"repository_id":workspace.repository_id,
        "configuration_id":workspace.project_id,"profile_id":workspace.profile_id,
        "environment_id":workspace.environment_id,"environment_hash":environment_hash,
        "name":"Developer fixture","kind":"existing","public_origin":null,
        "profile_hash":profile_hash,"profile":profile,
        "repositories":[{"key":"source","repository_id":workspace.repository_id,"clone_url":"https://fixture.invalid/source.git"}]});
    let directory = root
        .join("targets")
        .join(workspace.catalog_target_id.to_string());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("public.json"),
        serde_json::to_vec(&public).unwrap(),
    )
    .unwrap();
    let pipeline = json!({"version":1,"jobs":{"unit":{"image":format!("fixture/tools@sha256:{}","a".repeat(64)),"commands":["test -f README.md"]}}});
    let delivery=json!({"version":1,"jobs":{"deploy":{"helm_deploy":{"environment_id":workspace.environment_id,
        "images":{"image":{"job":"publish","artifact":"out/image.json"}},"receipt_artifact":"out/release.json"}}}});
    let publication = json!({"version":1,"jobs":{"publish":{"image_publish":{
        "context":".","dockerfile":"Dockerfile","repository":"registry.example.test/team/application",
        "descriptor_artifact":"out/build.json","published_artifact":"out/image.json"}}}});
    std::fs::write(
        directory.join("recipes.json"),
        serde_json::to_vec(
            &json!({"profileHash":profile_hash,"recipes":{"prepare":pipeline,"unit":pipeline,"release":delivery,"publish":publication,"base-release":{"version":1,"base_resource_rollout":base_target()}}}),
        )
        .unwrap(),
    )
    .unwrap();
    sqlx::query("UPDATE development_workspaces SET profile=$2,profile_hash=$3,catalog_hash=$4,environment_hash=$5 WHERE id=$1")
        .bind(id).bind(profile).bind(profile_hash).bind(store::request_hash(&public).unwrap()).bind(environment_hash).execute(pool).await.unwrap();
}

pub(super) async fn exercise(app: &Router, pool: &PgPool, workspace: Uuid, data: &Value) {
    let (status, registered) = http(app,"POST","/api/v1/runner/register",None,
        json!({"protocolVersion":1,"registrationToken":"workspace-runner-fixture","name":"workspace-checks","capabilities":{"executorKinds":["shell"],"pipelineVariables":true}})).await;
    assert_eq!(status, StatusCode::CREATED, "{registered}");
    let runner = registered["credential"].as_str().unwrap();
    let (status, response) = http(app,"POST","/api/v1/runner/heartbeat",Some(runner),
        json!({"protocolVersion":1,"status":"online","capacity":{"totalSlots":1,"busySlots":0},"capabilities":{"executorKinds":["shell"],"pipelineVariables":true}})).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{response}");
    let current = store::workspace(pool, workspace).await.unwrap();
    let checkout = std::path::Path::new(data["checkout"].as_str().unwrap());
    let commit = git(checkout, None, &["rev-parse", "HEAD"]).await;
    let prepare = json!({"action":"prepare","sources":[{"key":"source","repository_id":current.repository_id,"commit":commit}],
        "local":{"machine_id":Uuid::new_v4(),"profile_hash":current.profile_hash,
        "repositories":[{"key":"source","commit":commit,"dirty":false}],"tools":[]}});
    for command in [
        prepare,
        json!({"action":"check","recipe":"unit","source_commit":commit}),
    ] {
        let operation = Uuid::new_v4();
        let version: i64 =
            sqlx::query_scalar("SELECT version FROM development_workspaces WHERE id=$1")
                .bind(workspace)
                .fetch_one(pool)
                .await
                .unwrap();
        let (status, result) = http(
            app,
            "POST",
            &format!("/api/v1/development-workspaces/{workspace}/operations"),
            Some(TOKEN),
            json!({"operation_id":operation,"expected_version":version,"command":command}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let pipeline = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let row = store::operation(pool, workspace, operation).await.unwrap();
                assert!(
                    !matches!(row.state.as_str(), "failed" | "unknown"),
                    "{:?}",
                    row.result
                );
                if let Some(pipeline) = row.pipeline_id {
                    break pipeline;
                }
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        })
        .await
        .unwrap();
        let (status, result) = http(
            app,
            "GET",
            &format!("/api/v1/pipelines/{pipeline}"),
            Some(TOKEN),
            json!(null),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let (status, offer) = http(
            app,
            "POST",
            "/api/v1/runner/work:poll",
            Some(runner),
            json!({"protocolVersion":1,"capacity":{"freeSlots":1},"waitSeconds":0}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{offer}");
        assert_eq!(offer["attempt"]["pipelineId"], pipeline.to_string());
        assert_eq!(offer["attempt"]["commitSha"], commit);
        assert_eq!(
            offer["attempt"]["environment"]["CICD_VAR_DEVELOPMENT_WORKSPACE_ID"],
            workspace.to_string()
        );
        assert!(!offer.to_string().contains(TOKEN));
        let control = json!({"protocolVersion":1,"leaseToken":offer["leaseToken"],"fencingToken":offer["fencingToken"]});
        let lease = offer["leaseId"].as_str().unwrap();
        let (status, result) = http(
            app,
            "POST",
            &format!("/api/v1/runner/leases/{lease}/ack"),
            Some(runner),
            control.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        if command["action"] == "check" {
            let client = reqwest::Client::new();
            let endpoint = format!("{}/fixture", data["origin"].as_str().unwrap());
            client
                .post(&endpoint)
                .json("stale")
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
            assert_eq!(
                http(
                    app,
                    "POST",
                    &format!("/api/v1/runner/leases/{lease}/renew"),
                    Some(runner),
                    control.clone()
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
            client
                .post(&endpoint)
                .json("restore")
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
        let (status, result) = http(
            app,
            "POST",
            &format!("/api/v1/runner/leases/{lease}/renew"),
            Some(runner),
            control,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let (status, result) = http(app,"POST",&format!("/api/v1/runner/leases/{lease}/complete"),Some(runner),
            json!({"protocolVersion":1,"leaseToken":offer["leaseToken"],"fencingToken":offer["fencingToken"],"attemptId":offer["attempt"]["id"],"outcome":"success","exitCode":0,"finishedAt":chrono::Utc::now()})).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let row = store::operation(pool, workspace, operation).await.unwrap();
                assert!(
                    !matches!(row.state.as_str(), "failed" | "unknown"),
                    "{:?}",
                    row.result
                );
                if row.state == "succeeded" {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(
        store::workspace(pool, workspace).await.unwrap().preparation,
        "prepared"
    );
}

// This protocol fixture creates/claims a real typed publication pipeline, then
// reports a deliberate failed runner outcome. It never invents a built image.
pub(super) async fn exercise_publication(app:&Router,pool:&PgPool,workspace:Uuid,data:&Value,catalog:&std::path::Path) {
    let current=store::workspace(pool,workspace).await.unwrap();
    let commit=git(std::path::Path::new(data["checkout"].as_str().unwrap()),None,&["rev-parse","HEAD"]).await;
    let path=format!("/api/v1/development-workspaces/{workspace}/operations");
    let command=json!({"action":"check","recipe":"publish","source_commit":commit});
    let version=store::workspace(pool,workspace).await.unwrap().version;
    let request=|command:Value|json!({"operation_id":Uuid::new_v4(),"expected_version":version,"command":command});
    let control_url=format!("{}/fixture",data["origin"].as_str().unwrap());
    let client=reqwest::Client::new();
    for mode in ["stale","foreign_repository"] {
        client.post(&control_url).json(mode).send().await.unwrap().error_for_status().unwrap();
        assert_eq!(http(app,"POST",&path,Some(TOKEN),request(command.clone())).await.0,StatusCode::FORBIDDEN);
        client.post(&control_url).json(if mode=="stale"{"restore"}else{"restore_repository"})
            .send().await.unwrap().error_for_status().unwrap();
    }
    let (status,account)=http(app,"POST","/api/v1/admin/service-accounts",Some(PROJECT_OWNER),
        json!({"name":format!("publication-neighbor-{}",Uuid::new_v4().simple())})).await;
    assert_eq!(status,StatusCode::CREATED,"{account}");
    let (status,native)=http(app,"POST",&format!("/api/v1/admin/service-accounts/{}/tokens",account["id"].as_str().unwrap()),Some(PROJECT_OWNER),
        json!({"name":"native-neighbor","project_id":current.project_id,"scopes":["api:read","api:write"]})).await;
    assert_eq!(status,StatusCode::CREATED);
    let native_token=native["token"].as_str().unwrap();
    let session=store::session(pool,workspace).await.unwrap().unwrap();
    let pipelines=sqlx::query_scalar::<_,i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(current.project_id).fetch_one(pool).await.unwrap();
    for (name,token,expected) in [("anonymous",None,StatusCode::UNAUTHORIZED),
        ("readonly machine",Some(SOURCE_READER),StatusCode::FORBIDDEN),
        ("neighbor human",Some(OTHER_HUMAN),StatusCode::CONFLICT),
        ("native SAT",Some(native_token),StatusCode::FORBIDDEN)] {
        let attempted=request(command.clone());
        let attempted_id=attempted["operation_id"].as_str().unwrap().parse::<Uuid>().unwrap();
        let (status,body)=http(app,"POST",&path,token,attempted).await;
        assert_eq!(status,expected,"{name}: {body}");
        if name=="neighbor human" {
            // A legitimate human writer retains its normal route, but cannot
            // borrow the current machine's session or create its publication.
            assert_eq!(body["error"]["message"],"workspace_session_owned_by_another_actor");
        }
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM development_workspace_operations WHERE id=$1")
            .bind(attempted_id).fetch_one(pool).await.unwrap(),0,"{name} created an operation");
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM pipelines WHERE project_id=$1")
            .bind(current.project_id).fetch_one(pool).await.unwrap(),pipelines,"{name} created a pipeline");
        let held=store::session(pool,workspace).await.unwrap().unwrap();
        assert_eq!((held.id,held.actor_id,held.owner_subject),(session.id,session.actor_id,session.owner_subject));
        assert_eq!(held.machine_assignment,session.machine_assignment);
    }
    let mut foreign=command.clone(); foreign["recipe"]=json!("foreign-publish");
    assert_eq!(http(app,"POST",&path,Some(TOKEN),request(foreign)).await.0,StatusCode::FORBIDDEN);
    let recipes_path=catalog.join("targets").join(current.catalog_target_id.to_string()).join("recipes.json");
    let original=std::fs::read(&recipes_path).unwrap();
    let recipes:Value=serde_json::from_slice(&original).unwrap();
    let publication=recipes["recipes"]["publish"].clone();
    for jobs in [
        json!({"publish":publication["jobs"]["publish"],"shell":{"commands":["false"]}}),
        json!({"deploy":{"helm_deploy":{"environment_id":current.environment_id}}}),
        json!({"publish":{"image_publish":publication["jobs"]["publish"]["image_publish"],"commands":["false"]}}),
    ] {
        let mut changed=recipes.clone(); changed["recipes"]["publish"]["jobs"]=jobs;
        std::fs::write(&recipes_path,serde_json::to_vec(&changed).unwrap()).unwrap();
        let (status,body)=http(app,"POST",&path,Some(TOKEN),request(command.clone())).await;
        assert_eq!(status,StatusCode::CONFLICT,"{body}");
    }
    std::fs::write(&recipes_path,original).unwrap();
    // An old prepared commit is not the current candidate publication source.
    let base=data["base_commit"].as_str().unwrap();
    assert_ne!(base,commit);
    let mut stale_sources=current.sources.clone(); stale_sources[0]["commit"]=json!(base);
    sqlx::query("UPDATE development_workspaces SET sources=$2 WHERE id=$1").bind(workspace).bind(stale_sources).execute(pool).await.unwrap();
    let stale=Uuid::new_v4();
    let mut stale_command=command.clone(); stale_command["source_commit"]=json!(base);
    assert_eq!(http(app,"POST",&path,Some(TOKEN),json!({"operation_id":stale,"expected_version":version,"command":stale_command})).await.0,StatusCode::OK);
    tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,stale).await.unwrap();
        if row.state=="failed" {
            assert!(row.pipeline_id.is_none());
            assert_eq!(row.result.unwrap()["reason"],"workspace_candidate_commit_changed"); break;
        }
        assert_ne!(row.state,"unknown","{:?}",row.result);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    sqlx::query("UPDATE development_workspaces SET sources=$2 WHERE id=$1").bind(workspace).bind(&current.sources).execute(pool).await.unwrap();
    let version=store::workspace(pool,workspace).await.unwrap().version;
    let operation=Uuid::new_v4();
    let body=json!({"operation_id":operation,"expected_version":version,"command":command});
    let (status,accepted)=http(app,"POST",&path,Some(TOKEN),body.clone()).await;
    assert_eq!(status,StatusCode::OK,"{accepted}");
    assert!(!accepted.to_string().contains("sealed"));
    let pipeline=tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,operation).await.unwrap();
        assert!(!matches!(row.state.as_str(),"failed"|"unknown"),"{:?}",row.result);
        if let Some(pipeline)=row.pipeline_id {break pipeline;}
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    let (status,detail)=http(app,"GET",&format!("/api/v1/pipelines/{pipeline}"),Some(TOKEN),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{detail}");
    assert_eq!(detail["pipeline"]["project_id"],current.project_id.to_string());
    assert_eq!(detail["plan"]["resolved_commit_sha"],commit);
    assert_eq!(detail["plan"]["config_source"],"development_workspace");
    assert_eq!(detail["plan"]["plan"]["jobs"][0]["image_publish"],publication["jobs"]["publish"]["image_publish"]);
    assert_eq!(detail["plan"]["plan"]["jobs"].as_array().unwrap().len(),1);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM pipeline_human_owners WHERE pipeline_id=$1").bind(pipeline).fetch_one(pool).await.unwrap(),0);
    let capabilities=json!({"executorKinds":["kubernetes"],"jobKinds":["image-publish-v1"],"pipelineVariables":true});
    let (status,registered)=http(app,"POST","/api/v1/runner/register",None,json!({"protocolVersion":1,"registrationToken":"workspace-runner-fixture","name":"publication-check","capabilities":capabilities})).await;
    assert_eq!(status,StatusCode::CREATED,"{registered}");
    let runner=registered["credential"].as_str().unwrap();
    assert_eq!(http(app,"POST","/api/v1/runner/heartbeat",Some(runner),json!({"protocolVersion":1,"status":"online","capacity":{"totalSlots":1,"busySlots":0},"capabilities":capabilities,"activeLeaseIds":[]})).await.0,StatusCode::NO_CONTENT);
    let poll=json!({"protocolVersion":1,"capacity":{"freeSlots":1},"waitSeconds":0});
    client.post(&control_url).json("stale").send().await.unwrap().error_for_status().unwrap();
    assert_eq!(http(app,"POST","/api/v1/runner/work:poll",Some(runner),poll.clone()).await.0,StatusCode::FORBIDDEN);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1").bind(pipeline).fetch_one(pool).await.unwrap(),0);
    client.post(&control_url).json("restore").send().await.unwrap().error_for_status().unwrap();
    let (status,offer)=http(app,"POST","/api/v1/runner/work:poll",Some(runner),poll).await;
    assert_eq!(status,StatusCode::OK,"{offer}");
    assert_eq!(offer["attempt"]["pipelineId"],pipeline.to_string());
    assert_eq!(offer["attempt"]["commitSha"],commit);
    assert_eq!(offer["attempt"]["imagePublish"],publication["jobs"]["publish"]["image_publish"]);
    assert!(!offer.to_string().contains(TOKEN));
    let lease=offer["leaseId"].as_str().unwrap();
    let control=json!({"protocolVersion":1,"leaseToken":offer["leaseToken"],"fencingToken":offer["fencingToken"]});
    assert_eq!(http(app,"POST",&format!("/api/v1/runner/leases/{lease}/ack"),Some(runner),control.clone()).await.0,StatusCode::OK);
    client.post(&control_url).json("stale").send().await.unwrap().error_for_status().unwrap();
    assert_eq!(http(app,"POST",&format!("/api/v1/runner/leases/{lease}/renew"),Some(runner),control.clone()).await.0,StatusCode::FORBIDDEN);
    client.post(&control_url).json("restore").send().await.unwrap().error_for_status().unwrap();
    assert_eq!(http(app,"POST",&format!("/api/v1/runner/leases/{lease}/renew"),Some(runner),control.clone()).await.0,StatusCode::OK);
    let (status,result)=http(app,"POST",&format!("/api/v1/runner/leases/{lease}/complete"),Some(runner),json!({"protocolVersion":1,"leaseToken":offer["leaseToken"],"fencingToken":offer["fencingToken"],"attemptId":offer["attempt"]["id"],"outcome":"failed","exitCode":1,"finishedAt":chrono::Utc::now()})).await;
    assert_eq!(status,StatusCode::OK,"{result}");
    tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,operation).await.unwrap();
        if row.state=="failed" {assert_eq!(row.result.unwrap()["pipelineStatus"],"failed");break;}
        assert_ne!(row.state,"unknown","{:?}",row.result);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    assert_eq!(http(app,"POST",&path,Some(TOKEN),body).await.1["id"],operation.to_string());
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM pipelines WHERE variables->>'development_workspace_operation_id'=$1").bind(operation.to_string()).fetch_one(pool).await.unwrap(),1);
    eprintln!("FORGE_DEVELOPER_PUBLICATION_CHECK=verified");
}

// Current Central PAT and public HTTP coverage. This proves admission, original
// read access and assignment revocation before a runner effect, not Helm execution.
pub(super) async fn exercise_devops(app:&Router,pool:&PgPool,workspace:Uuid,data:&Value,catalog:&std::path::Path,artifacts:&std::path::Path,developer_operation:Uuid) {
    let endpoint=format!("{}/fixture",data["origin"].as_str().unwrap());
    let client=reqwest::Client::new();
    let current=store::workspace(pool,workspace).await.unwrap();
    let target=json!({"schema":"forge/kubernetes-environment/v1","cluster":"fixture","namespace":"product","release":"product","chart":"chart","health_url":"https://product.example.test/health"});
    sqlx::query("UPDATE environments SET kubernetes_target=$2 WHERE id=$1").bind(current.environment_id).bind(&target).execute(pool).await.unwrap();
    let environment_hash=store::request_hash(&target).unwrap();
    sqlx::query("UPDATE development_workspaces SET environment_hash=$2,runtime='running',desired_runtime='running' WHERE id=$1").bind(workspace).bind(&environment_hash).execute(pool).await.unwrap();
    let public_path=catalog.join("targets").join(current.catalog_target_id.to_string()).join("public.json");
    let mut public:Value=serde_json::from_slice(&std::fs::read(&public_path).unwrap()).unwrap();
    public["environment_hash"]=json!(environment_hash);
    std::fs::write(public_path,serde_json::to_vec(&public).unwrap()).unwrap();
    sqlx::query("UPDATE development_workspaces SET catalog_hash=$2 WHERE id=$1").bind(workspace).bind(store::request_hash(&public).unwrap()).execute(pool).await.unwrap();
    let (status,principal)=http(app,"GET","/api/v1/auth/principal",Some(DEVOPS_TOKEN),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{principal}");
    let developer_actor:Uuid=sqlx::query_scalar("SELECT id FROM users WHERE central_sub=$1").bind(data["subject"].as_str().unwrap()).fetch_one(pool).await.unwrap();
    assert_ne!(principal["sub"],developer_actor.to_string());
    assert_eq!(http(app,"POST",&format!("/api/v1/projects/{}/memberships",current.project_id),Some(PROJECT_OWNER),json!({"user_id":principal["sub"],"role":"developer"})).await.0,StatusCode::OK);
    let path=format!("/api/v1/development-workspaces/{workspace}/operations");
    // A different machine reads original Developer history using its own PAT.
    assert_eq!(http(app,"GET",&format!("{path}/{developer_operation}"),Some(DEVOPS_TOKEN),json!(null)).await.0,StatusCode::OK);
    client.post(&endpoint).json("deployment_ready").send().await.unwrap().error_for_status().unwrap();
    let begin=Uuid::new_v4();
    let version=store::workspace(pool,workspace).await.unwrap().version;
    let request=json!({"operation_id":begin,"expected_version":version,"command":{"action":"begin_session","task":data["task"]}});
    assert_eq!(http(app,"POST",&path,Some(TOKEN),request.clone()).await.0,StatusCode::FORBIDDEN);
    let (status,response)=http(app,"POST",&path,Some(DEVOPS_TOKEN),request).await;
    assert_eq!(status,StatusCode::OK,"{response}");
    tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,begin).await.unwrap();
        if row.state=="succeeded" { assert!(row.result.unwrap()["developerCheckPolicy"].is_null()); break; }
        assert_ne!(row.state,"failed","{:?}",row.result);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    let session=store::session(pool,workspace).await.unwrap().unwrap();
    assert_eq!(session.machine_assignment.unwrap()["stage"],"Deployment");
    let version=store::workspace(pool,workspace).await.unwrap().version;
    let commit=current.sources[0]["commit"].as_str().unwrap();
    let publication=Uuid::new_v4();
    let release=json!({"action":"check","recipe":"release","source_commit":commit,"source_pipeline_id":publication});
    assert_eq!(http(app,"POST",&path,Some(DEVOPS_TOKEN),json!({"operation_id":Uuid::new_v4(),"expected_version":version,"command":release})).await.0,StatusCode::FORBIDDEN);
    client.post(&endpoint).json("deployment_running").send().await.unwrap().error_for_status().unwrap();
    for command in [json!({"action":"check","recipe":"unit","source_commit":commit}),
        json!({"action":"check","recipe":"release","source_commit":commit}),
        json!({"action":"prepare","sources":[],"local":{"machine_id":Uuid::new_v4(),"profile_hash":current.profile_hash,"repositories":[],"tools":[]}})] {
        assert_eq!(http(app,"POST",&path,Some(DEVOPS_TOKEN),json!({"operation_id":Uuid::new_v4(),"expected_version":version,"command":command})).await.0,StatusCode::FORBIDDEN);
    }
    // A prepared historical source is insufficient once the candidate moved.
    let base=data["base_commit"].as_str().unwrap();
    assert_ne!(base,commit);
    let mut stale_sources=current.sources.clone(); stale_sources[0]["commit"]=json!(base);
    sqlx::query("UPDATE development_workspaces SET sources=$2 WHERE id=$1").bind(workspace).bind(stale_sources).execute(pool).await.unwrap();
    let stale=Uuid::new_v4();
    let mut stale_check=release.clone(); stale_check["source_commit"]=json!(base);
    assert_eq!(http(app,"POST",&path,Some(DEVOPS_TOKEN),json!({"operation_id":stale,"expected_version":version,"command":stale_check})).await.0,StatusCode::OK);
    tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,stale).await.unwrap();
        if row.state=="failed" {
            assert!(row.pipeline_id.is_none());
            assert_eq!(row.result.unwrap()["reason"],"workspace_candidate_commit_changed");
            break;
        }
        assert_ne!(row.state,"unknown","{:?}",row.result);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    sqlx::query("UPDATE development_workspaces SET sources=$2 WHERE id=$1").bind(workspace).bind(&current.sources).execute(pool).await.unwrap();
    exercise_base_delivery(app,pool,workspace,catalog,artifacts,commit).await;
    let version=store::workspace(pool,workspace).await.unwrap().version;
    let operation=Uuid::new_v4();
    let (status,response)=http(app,"POST",&path,Some(DEVOPS_TOKEN),json!({"operation_id":operation,"expected_version":version,"command":release})).await;
    assert_eq!(status,StatusCode::OK,"{response}");
    let pipeline=tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,operation).await.unwrap();
        assert!(!matches!(row.state.as_str(),"failed"|"unknown"),"{:?}",row.result);
        if let Some(pipeline)=row.pipeline_id { break pipeline; }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    let (status,proof)=http(app,"GET",&format!("/api/v1/pipelines/{pipeline}"),Some(DEVOPS_TOKEN),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{proof}");
    assert_eq!(proof["plan"]["plan"]["jobs"][0]["helm_deploy"]["source_pipeline_id"],publication.to_string());
    client.post(&endpoint).json("stale").send().await.unwrap().error_for_status().unwrap();
    let capabilities=json!({"executorKinds":["kubernetes"],"jobKinds":["helm-deploy-v1"],"helmTargets":[current.environment_id],"pipelineVariables":true});
    let (status,registered)=http(app,"POST","/api/v1/runner/register",None,json!({"protocolVersion":1,"registrationToken":"workspace-runner-fixture","name":"devops-fence","capabilities":capabilities})).await;
    assert_eq!(status,StatusCode::CREATED,"{registered}");
    let token=registered["credential"].as_str().unwrap();
    assert_eq!(http(app,"POST","/api/v1/runner/heartbeat",Some(token),json!({"protocolVersion":1,"status":"online","capacity":{"totalSlots":1,"busySlots":0},"capabilities":capabilities,"activeLeaseIds":[]})).await.0,StatusCode::NO_CONTENT);
    let (status,blocked)=http(app,"POST","/api/v1/runner/work:poll",Some(token),json!({"protocolVersion":1,"capacity":{"freeSlots":1},"waitSeconds":0})).await;
    assert_eq!(status,StatusCode::FORBIDDEN,"{blocked}");
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1").bind(pipeline).fetch_one(pool).await.unwrap(),0);
    assert_eq!(http(app,"GET",&format!("{path}/{operation}"),Some(DEVOPS_TOKEN),json!(null)).await.0,StatusCode::OK);
}

fn base_target() -> Value {
    json!({"schema":"pdlc/base-resource-target/v1","installationId":"fixture-installation",
        "slot":"pdlc","component":"task-backend","namespace":"pdlc",
        "namespaceUid":"11111111-1111-4111-8111-111111111111","deployment":"task-backend",
        "deploymentUid":"22222222-2222-4222-8222-222222222222","container":"backend",
        "publicationJob":"publish","publicationArtifact":"out/image.json"})
}

async fn exercise_base_delivery(app:&Router,pool:&PgPool,workspace:Uuid,catalog:&std::path::Path,
    artifacts:&std::path::Path,commit:&str) {
    use sha2::{Digest,Sha256};
    let current=store::workspace(pool,workspace).await.unwrap();
    let (source,stage,job,attempt)=(Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4());
    let spec=json!({"context":".","dockerfile":"Dockerfile","repository":"registry.example.test/team/application",
        "descriptor_artifact":"out/build.json","published_artifact":"out/image.json"});
    let raw=serde_json::to_string(&json!({"version":1,"jobs":{"publish":{"image_publish":spec}}})).unwrap();
    let plan=json!({"format":"v1-dag","version":1,"config_source":"development_workspace","resolved_commit_sha":commit,
        "jobs":[{"key":"publish","image_publish":spec}]});
    let hash=|bytes:&[u8]|format!("{:x}",Sha256::digest(bytes));
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,$3,$3,'success')")
        .bind(source).bind(current.project_id).bind(commit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,'development_workspace','v1',$2,$2,$3,$4,$5,$6)")
        .bind(source).bind(commit).bind(hash(raw.as_bytes())).bind(hash(&serde_json::to_vec(&plan).unwrap())).bind(&raw).bind(&plan).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'publish',0,'success')")
        .bind(stage).bind(source).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'publish','forge:runner-image-publish','forge:image-publish',0,'success')")
        .bind(job).bind(stage).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger) VALUES($1,$2,1,'success','initial')")
        .bind(attempt).bind(job).execute(pool).await.unwrap();
    let descriptor=b"original-publication-descriptor".to_vec();
    let published=cicd_domain::published_image::PublishedImage::new(commit.into(),
        format!("registry.example.test/team/application@sha256:{}","b".repeat(64)),hash(&descriptor)).unwrap();
    for (name,bytes) in [("out/build.json",descriptor),("out/image.json",serde_json::to_vec(&published).unwrap())] {
        let path=artifacts.join(Uuid::new_v4().to_string());std::fs::write(&path,&bytes).unwrap();
        sqlx::query("INSERT INTO artifacts(id,job_id,attempt_id,name,storage_path,size_bytes,sha256,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+interval '1 hour')")
            .bind(Uuid::new_v4()).bind(job).bind(attempt).bind(cicd_domain::artifact_names::artifact_name_from_declared_path(name))
            .bind(path.to_str().unwrap()).bind(bytes.len() as i64).bind(hash(&bytes)).execute(pool).await.unwrap();
    }
    let path=format!("/api/v1/development-workspaces/{workspace}/operations");
    let operation=Uuid::new_v4();
    let body=json!({"operation_id":operation,"expected_version":current.version,
        "command":{"action":"check","recipe":"base-release","source_commit":commit,"source_pipeline_id":source}});
    assert_eq!(http(app,"POST",&path,Some(TOKEN),body.clone()).await.0,StatusCode::FORBIDDEN);
    let (status,queued)=http(app,"POST",&path,Some(DEVOPS_TOKEN),body.clone()).await;
    assert_eq!(status,StatusCode::OK,"{queued}");
    tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,operation).await.unwrap();
        if row.state=="unknown" {
            assert!(row.pipeline_id.is_none());
            assert_eq!(row.result.as_ref().unwrap()["reason"],"workspace_base_delivery_receipt_missing");break;
        }
        assert_ne!(row.state,"failed","{:?}",row.result);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    let target=base_target();
    let receipt=json!({"schema":"pdlc/resource-rollout-delivery/v1","phase":"complete","operationId":Uuid::new_v4(),
        "installationId":target["installationId"],"slot":target["slot"],
        "planSha256":"a".repeat(64),"sourceConfigurationSha256":"b".repeat(64),"configurationSha256":"c".repeat(64),
        "sourceCatalogSha256":"d".repeat(64),"normalReceiptSha256":"e".repeat(64),
        "component":{"key":target["component"],"namespace":target["namespace"],"namespaceUid":target["namespaceUid"],
            "deployment":target["deployment"],"deploymentUid":target["deploymentUid"],"container":target["container"]},
        "images":{"backend":published.image_ref},"generation":3,"observedGeneration":3,
        "replicas":1,"readyReplicas":1,"ready":true,"guard":"READY"});
    let envelope=json!({"sha256":store::request_hash(&receipt).unwrap(),"receipt":receipt});
    let directory=catalog.join("targets").join(current.catalog_target_id.to_string()).join("deliveries");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join(format!("{source}.json")),serde_json::to_vec(&envelope).unwrap()).unwrap();
    let (status,response)=http(app,"POST",&format!("{path}/{operation}/reconcile"),Some(PROJECT_OWNER),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{response}");
    tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
        let row=store::operation(pool,workspace,operation).await.unwrap();
        if row.state=="succeeded" {
            assert!(row.pipeline_id.is_none());let result=row.result.unwrap();
            assert_eq!(result["baseOperationObservation"]["target"],target);
            assert_eq!(result["baseOperationObservation"]["deliveryReceipt"],envelope);
            assert_eq!(result["baseOperationObservation"]["sourcePipelineId"],source.to_string());break;
        }
        assert!(!matches!(row.state.as_str(),"failed"|"unknown"),"{:?}",row.result);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }}).await.unwrap();
    let (status,original)=http(app,"GET",&format!("{path}/{operation}"),Some(DEVOPS_TOKEN),json!(null)).await;
    assert_eq!(status,StatusCode::OK,"{original}");
    assert_eq!(http(app,"POST",&path,Some(DEVOPS_TOKEN),body).await.1,original);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM pipelines WHERE variables->>'development_workspace_operation_id'=$1")
        .bind(operation.to_string()).fetch_one(pool).await.unwrap(),0);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM deployments WHERE pipeline_id=$1")
        .bind(source).fetch_one(pool).await.unwrap(),0);
    eprintln!("FORGE_BASE_NATIVE_DELIVERY=verified");
}
