use super::*;

fn fixture(role: WorkspaceRole, stage: &str) -> (TaskWorkspaceIntent, Value) {
    let task = Uuid::new_v4();
    let now = Utc::now();
    let mut intent = TaskWorkspaceIntent {
        operation_key: "workspace:original".into(),
        repository_id: Uuid::new_v4(),
        source_commit: "a".repeat(40),
        role,
        agent_id: Uuid::new_v4(),
        tracker_lease_id: Uuid::new_v4(),
        holder_subject: "fleet-scheduler".into(),
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
    let role = serde_json::to_value(role).unwrap();
    let routing = json!({"tracker_instance_id":intent.binding.tracker_instance_id,
        "project_id":intent.binding.tracker_project_id,"task_id":task,"root_task_id":task,
        "snapshot_id":intent.binding.routing_snapshot_id,"requirement_revision":1,
        "policy":{"tracker_instance_id":intent.binding.tracker_instance_id,
            "project_id":intent.binding.tracker_project_id,
            "routes":{(role.as_str().unwrap()):{"agent_id":intent.agent_id}}}});
    let mut assignment = json!({"task_id":task,"assignment_id":intent.binding.assignment_id,
        "execution_id":intent.binding.execution_id,"execution":{"ordinal":"27","key":"SDLC-27"},
        "agent_id":intent.agent_id,"lease_id":intent.tracker_lease_id,"role":role,
        "stage":stage,"cycle":0,"attempt":1,"owner_subject":"owner",
        "fencing_token":3,"routing":routing});
    intent.binding.assignment_hash = hash(&canonical(assignment.clone())).unwrap();
    assignment["assignment_hash"] = json!(intent.binding.assignment_hash);
    let value = json!({"observed_at":now,"runtime_state":"running","receipt":{
        "task":{"task_id":task,"stage":stage,"status":"InProgress","runtime":"running",
            "owner_subject":"owner","cycle":0,"waiting_reason":null,"routing":routing},
        "assignment":assignment,"lease":{"lease_id":intent.tracker_lease_id,"version":2,
            "holder_subject":"fleet-scheduler","claimed_at":now,"heartbeat_at":now,
            "expires_at":now+chrono::Duration::seconds(30)},
        "binding":{"run_id":Uuid::new_v4(),"chat_id":Uuid::new_v4(),"machine_subject":"role-agent"}}});
    (intent, value)
}

#[test]
fn six_actual_roles_and_developer_rework_use_current_assignment() {
    for (role, stage) in [
        (WorkspaceRole::Analyst, "Analysis"),
        (WorkspaceRole::Architect, "Architecture"),
        (WorkspaceRole::Developer, "Development"),
        (WorkspaceRole::Developer, "Rework"),
        (WorkspaceRole::Reviewer, "Review"),
        (WorkspaceRole::Tester, "Testing"),
        (WorkspaceRole::Devops, "Deployment"),
    ] {
        let (intent, value) = fixture(role, stage);
        assert_eq!(current(&value, &intent).unwrap(), Duration::from_secs(30));
    }
}

#[test]
fn task_intent_cannot_supply_runner_identity_or_receipt() {
    let (intent, _) = fixture(WorkspaceRole::Developer, "Development");
    for key in [
        "leaseId",
        "attemptId",
        "workspaceGeneration",
        "workspaceId",
        "receipt",
        "access",
    ] {
        let mut value = serde_json::to_value(&intent).unwrap();
        value[key] = json!("caller-controlled");
        assert!(
            serde_json::from_value::<TaskWorkspaceIntent>(value).is_err(),
            "{key}"
        );
    }
    let attempt = Uuid::new_v4();
    let lease = Uuid::new_v4();
    let workspace = format!("attempt-{attempt}-7-{}", Uuid::new_v4().simple());
    let request = intent
        .request(lease, attempt, 7, workspace.clone())
        .unwrap();
    assert_eq!(request.lease_id, lease);
    assert_eq!(request.attempt_id, attempt);
    assert_eq!(request.workspace_id, workspace);
    assert_eq!(request.access, WorkspaceAccess::ReadOnly);
    assert_eq!(request.binding, intent.binding);
    assert!(
        intent
            .request(lease, attempt, 8, request.workspace_id)
            .is_err()
    );
}

#[test]
fn heartbeat_advancement_does_not_change_original_identity() {
    let (intent, mut value) = fixture(WorkspaceRole::Developer, "Development");
    value["receipt"]["lease"]["version"] = json!(3);
    assert!(current(&value, &intent).is_ok());
    for (path, replacement) in [
        ("/receipt/lease/version", json!(1)),
        ("/receipt/lease/lease_id", json!(Uuid::new_v4())),
        ("/receipt/lease/holder_subject", json!("other")),
        ("/receipt/assignment/assignment_id", json!(Uuid::new_v4())),
        ("/receipt/assignment/fencing_token", json!(4)),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        assert!(current(&changed, &intent).is_err(), "{path}");
    }
}

#[test]
fn held_foreign_expired_and_tampered_context_cannot_prepare() {
    let (intent, value) = fixture(WorkspaceRole::Developer, "Development");
    for (path, replacement) in [
        ("/runtime_state", json!("unknown")),
        ("/receipt/task/runtime", json!("unknown")),
        ("/receipt/task/waiting_reason", json!("owner_stop")),
        ("/receipt/task/status", json!("Ready")),
        ("/receipt/task/stage", json!("")),
        ("/receipt/task/owner_subject", json!("foreign")),
        ("/receipt/assignment/agent_id", json!(Uuid::new_v4())),
        ("/receipt/assignment/execution_id", json!(Uuid::new_v4())),
        ("/receipt/assignment/execution/key", json!("SDLC-28")),
        ("/receipt/assignment/role", json!("devops")),
        ("/receipt/assignment/attempt", json!(2)),
        ("/receipt/task/routing/project_id", json!(Uuid::new_v4())),
        ("/receipt/task/routing/snapshot_id", json!(Uuid::new_v4())),
        ("/receipt/lease/expires_at", value["observed_at"].clone()),
        ("/receipt/binding/run_id", json!(Uuid::nil())),
        ("/receipt/binding/machine_subject", json!("fleet-scheduler")),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        assert!(current(&changed, &intent).is_err(), "{path}");
    }
    let mut child = intent;
    child.binding.root_task_id = Uuid::new_v4();
    assert!(current(&value, &child).is_err());
}

#[test]
fn malformed_bounds_and_unknown_role_fail_without_fallback() {
    let (intent, _) = fixture(WorkspaceRole::Analyst, "Analysis");
    for (field, value) in [
        ("expectedLeaseVersion", json!(0)),
        ("operationKey", json!("../x")),
        ("holderSubject", json!("\nprivate")),
        ("sourceCommit", json!("main")),
        ("agentId", json!(Uuid::nil())),
    ] {
        let mut body = serde_json::to_value(&intent).unwrap();
        body[field] = value;
        assert!(
            serde_json::from_value::<TaskWorkspaceIntent>(body)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let mut body = serde_json::to_value(intent).unwrap();
    body["role"] = json!("project_manager");
    assert!(serde_json::from_value::<TaskWorkspaceIntent>(body).is_err());
}

#[tokio::test]
async fn tracker_transport_is_readonly_bounded_and_spent_time_is_not_renewed() {
    use axum::{Router, routing::get};
    let (intent, mut value) = fixture(WorkspaceRole::Developer, "Development");
    value["receipt"]["lease"]["expires_at"] = json!(Utc::now() + chrono::Duration::milliseconds(5));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = SdlcWorkspaceConfig {
        tracker_read_url: Some(format!("http://{}", listener.local_addr().unwrap())),
        tracker_read_token: Some("fixture-reader".into()),
        ..Default::default()
    };
    let path = format!("/api/v1/issues/{}/sdlc/lifecycle", intent.binding.task_id);
    let app = Router::new().route(
        &path,
        get(move |headers: axum::http::HeaderMap| {
            let value = value.clone();
            async move {
                assert_eq!(headers["authorization"], "Bearer fixture-reader");
                tokio::time::sleep(Duration::from_millis(40)).await;
                Json(value)
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    assert!(tracker(&config, &intent).await.is_err());
    server.abort();
}
