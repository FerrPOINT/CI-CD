//! Published-recipe and runner protocol qualification, not an image execution gate.
use super::*;
use serde_json::Value;

pub(super) async fn publish(pool: &PgPool, root: &std::path::Path, id: Uuid) {
    let workspace = store::workspace(pool, id).await.unwrap();
    let profile = json!({"apiVersion":"pdlc.dev/workspace/v1", "repositories":[{"key":"source"}],
        "tools":[], "preparationChecks":["unit"], "requiredDeveloperChecks":["unit"],
        "checks":[{"key":"unit","executor":"forge","requiresRuntime":false,"commands":["test -f README.md"]}]});
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
    std::fs::write(
        directory.join("recipes.json"),
        serde_json::to_vec(
            &json!({"profileHash":profile_hash,"recipes":{"prepare":pipeline,"unit":pipeline}}),
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
