//! Real HTTP/PG admission only. No runner, image, Docker or lifecycle effects.
use super::*;
use serde_json::{Value, json};

const YAML: &str = "version: 1\njobs:\n  build_app:\n    image_build:\n      context: .\n      dockerfile: Dockerfile\n      descriptor_artifact: candidate.json\n      readable_schema_versions: [1]\n  validate:\n    needs: [build_app]\n    commands: [echo repository-owned-acceptance]\n    secrets: [APP_DB_URL]\n    timeout: 20s\n    workspace_deploy:\n      compose: task.compose.json\n      images_manifest: reports/workspace-deployment.json\n";

fn recipe() -> Vec<u8> {
    serde_json::to_vec(&json!({"services":{
        "app":{"x-forge-image-build":"build_app","user":"10001:10001",
            "mem_limit":16777216,"cpus":0.5,"pids_limit":32,
            "environment":{"DATABASE_URL":"${APP_DB_URL}"}},
        "db":{"image":format!("sha256:{}","a".repeat(64)),"user":"999:999",
            "mem_limit":16777216,"cpus":0.5,"pids_limit":32,
            "volumes":[{"type":"volume","source":"data","target":"/var/lib/database"}]}},
        "volumes":{"data":{}}}))
    .unwrap()
}

async fn devops_fixture() -> TaskPipeline {
    let mut f = TaskPipeline::new_with_source(
        Some(YAML.into()),
        &[
            ("task.compose.json", &recipe()),
            ("Dockerfile", b"FROM scratch\n"),
        ],
        None,
    )
    .await;
    f.intent.role = WorkspaceRole::Devops;
    {
        let mut peer = f.tracker.lock().await;
        let mut routing = peer["receipt"]["task"]["routing"].clone();
        routing["policy"]["routes"]["devops"] = json!({"agent_id":f.intent.agent_id});
        let mut assignment = peer["receipt"]["assignment"].clone();
        assignment["role"] = json!("devops");
        assignment["stage"] = json!("Deployment");
        assignment["routing"] = routing.clone();
        assignment
            .as_object_mut()
            .unwrap()
            .remove("assignment_hash");
        f.intent.binding.assignment_hash =
            sha(&serde_json::to_vec(&canonical(assignment.clone())).unwrap());
        assignment["assignment_hash"] = json!(f.intent.binding.assignment_hash);
        peer["receipt"]["assignment"] = assignment;
        peer["receipt"]["task"]["stage"] = json!("Deployment");
        peer["receipt"]["task"]["routing"] = routing;
    }
    f.intent.validate().unwrap();
    f
}

async fn custody(f: &TaskPipeline) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
        'pipelines',(SELECT count(*) FROM pipelines),
        'plans',(SELECT count(*) FROM pipeline_plans),
        'triggers',(SELECT count(*) FROM pipeline_triggers),
        'jobs',(SELECT count(*) FROM jobs),
        'queue',(SELECT count(*) FROM job_queue),
        'attempts',(SELECT count(*) FROM execution_attempts),
        'leases',(SELECT count(*) FROM job_leases),
        'intents',(SELECT count(*) FROM task_pipeline_intents),
        'operations',(SELECT count(*) FROM sdlc_workspace_operations),
        'artifacts',(SELECT count(*) FROM artifacts))",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

async fn no_execution(f: &TaskPipeline) {
    assert!(f.running.lock().await.is_empty());
    assert!(f.worker.lock().await.is_none());
    let state: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object(
        'leases',(SELECT count(*) FROM job_leases),
        'operations',(SELECT count(*) FROM sdlc_workspace_operations),
        'artifacts',(SELECT count(*) FROM artifacts),
        'claimed',(SELECT count(*) FROM task_pipeline_intents
            WHERE preparation_request IS NOT NULL OR cardinality(submitted_attempts)>0))",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        state,
        json!({"leases":0,"operations":0,"artifacts":0,"claimed":0})
    );
}

async fn finish(f: Arc<TaskPipeline>, result: Result<(), tokio::task::JoinError>) {
    let mut f = Arc::try_unwrap(f).unwrap_or_else(|_| panic!("admission fixture still borrowed"));
    let cleanup = f.cleanup().await;
    if let Err(error) = result {
        if cleanup.is_err() {
            eprintln!("owned admission fixture cleanup failed");
        }
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
        panic!("admission case unexpectedly canceled");
    }
    cleanup.expect("owned admission fixture cleanup");
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL, configured QA namespace IDs and Git; no runner/Docker"]
async fn task_http_workspace_deploy_requires_task_and_devops_before_enqueue() {
    // The existing fixture starts as a genuinely authorized Developer, not a forged role string.
    let f = Arc::new(
        TaskPipeline::new_with_source(Some(YAML.into()), &[("task.compose.json", &recipe())], None)
            .await,
    );
    let case = f.clone();
    let result = tokio::spawn(async move {
        let before = custody(&case).await;
        let path = format!("/api/v1/projects/{}/pipelines", case.project);
        let (status, _) = case
            .request("POST", &path, json!({"git_ref":case.intent.source_commit}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            custody(&case).await,
            before,
            "generic trigger must not create executable records"
        );
        let (status, _) = case.trigger(&case.intent).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "Developer deployment must not enqueue a later-held job"
        );
        assert_eq!(
            custody(&case).await,
            before,
            "role mismatch is rejected before effects"
        );
        no_execution(&case).await;
    })
    .await;
    finish(f, result).await;
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL, configured QA namespace IDs and Git; no runner/Docker"]
async fn task_http_workspace_deploy_seals_existing_plan_and_upstream_without_claiming_execution() {
    let f = Arc::new(devops_fixture().await);
    let case = f.clone();
    let result = tokio::spawn(async move {
        let (status,created) = case.trigger(&case.intent).await;
        assert_eq!(status,StatusCode::OK,"{created}");
        let pipeline: Uuid = serde_json::from_value(created["pipeline"]["id"].clone()).unwrap();
        let plan: Value = sqlx::query_scalar("SELECT plan FROM pipeline_plans WHERE pipeline_id=$1")
            .bind(pipeline).fetch_one(&case.pool).await.unwrap();
        let jobs = plan["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(),2);
        let build = jobs.iter().find(|job|job["key"]=="build_app").unwrap();
        let deploy = jobs.iter().find(|job|job["key"]=="validate").unwrap();
        assert!(build["image_build"].is_object());
        assert!(build.get("workspace_deploy").is_none());
        assert!(deploy.get("image_build").is_none());
        assert_eq!(deploy["needs"],json!(["build_app"]));
        assert_eq!(deploy["commands"],json!(["echo repository-owned-acceptance"]));
        assert_eq!(deploy["required_secrets"],json!(["APP_DB_URL"]));
        assert_eq!(deploy["artifact_paths"],json!(["reports/workspace-deployment.json"]));
        assert_eq!(deploy["workspace_deploy"],json!({"compose":"task.compose.json","images_manifest":"reports/workspace-deployment.json"}));
        let saved: Value = sqlx::query_scalar("SELECT intent FROM task_pipeline_intents WHERE pipeline_id=$1")
            .bind(pipeline).fetch_one(&case.pool).await.unwrap();
        assert_eq!(saved,serde_json::to_value(&case.intent).unwrap());
        let counts = custody(&case).await;
        assert_eq!(counts["jobs"],2);
        assert_eq!(counts["queue"],2);
        assert_eq!(counts["intents"],1);
        no_execution(&case).await;
        let recipe_path=case.temp.join("source/task.compose.json");
        assert_eq!(std::fs::read(&recipe_path).unwrap(),recipe(),"admission never rewrites Git input");
        case.tracker.lock().await["runtime_state"] = json!("unknown");
        let (status,replayed) = case.trigger(&case.intent).await;
        assert_eq!(status,StatusCode::OK,"{replayed}");
        assert_eq!(replayed["pipeline"]["id"],json!(pipeline));
        assert_eq!(custody(&case).await,counts,"same key must retain original immutable plan");
        let replayed_plan: Value = sqlx::query_scalar("SELECT plan FROM pipeline_plans WHERE pipeline_id=$1")
            .bind(pipeline).fetch_one(&case.pool).await.unwrap();
        let replayed_intent: Value = sqlx::query_scalar("SELECT intent FROM task_pipeline_intents WHERE pipeline_id=$1")
            .bind(pipeline).fetch_one(&case.pool).await.unwrap();
        assert_eq!(replayed_plan,plan);
        assert_eq!(replayed_intent,saved);
        let mut foreign=case.intent.clone();
        foreign.operation_key=format!("changed:{}",Uuid::new_v4());
        assert_eq!(case.trigger(&foreign).await.0,StatusCode::CONFLICT);
        assert_eq!(custody(&case).await,counts,"same key cannot substitute another intent");
        no_execution(&case).await;
        assert!(!created.to_string().contains(&case.token));
        assert!(!created.to_string().contains("qa-tracker-reader"));
    }).await;
    finish(f, result).await;
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL, configured QA namespace IDs and Git; no runner/Docker"]
async fn task_http_workspace_deploy_changed_or_expired_tracker_admission_has_no_effects() {
    let f = Arc::new(devops_fixture().await);
    let case = f.clone();
    let result = tokio::spawn(async move {
        let original = case.tracker.lock().await.clone();
        let before = custody(&case).await;
        for field in [
            "version",
            "holder_subject",
            "expires_at",
            "agent_id",
            "routing",
            "runtime_state",
        ] {
            let mut changed = original.clone();
            match field {
                "version" => changed["receipt"]["lease"][field] = json!(1),
                "holder_subject" => changed["receipt"]["lease"][field] = json!("foreign-scheduler"),
                "expires_at" => {
                    changed["receipt"]["lease"][field] = json!(Utc::now() - Duration::seconds(1))
                }
                "agent_id" => changed["receipt"]["assignment"][field] = json!(Uuid::new_v4()),
                "routing" => {
                    changed["receipt"]["task"][field]["snapshot_id"] = json!(Uuid::new_v4())
                }
                "runtime_state" => changed[field] = json!("unknown"),
                _ => unreachable!(),
            }
            *case.tracker.lock().await = changed;
            assert_eq!(
                case.trigger(&case.intent).await.0,
                StatusCode::CONFLICT,
                "{field}"
            );
            assert_eq!(
                custody(&case).await,
                before,
                "{field} must not enqueue/claim/publish"
            );
        }
        *case.tracker.lock().await = original;
        no_execution(&case).await;
    })
    .await;
    finish(f, result).await;
}
