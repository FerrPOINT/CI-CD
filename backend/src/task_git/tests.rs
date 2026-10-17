use super::*;
use crate::domain::sdlc_workspace::WorkspaceTaskBinding;

pub(super) fn seal(source: &mut SourceBinding) {
    let mut value = serde_json::to_value(&*source).unwrap();
    value.as_object_mut().unwrap().remove("source_hash");
    source.source_hash = digest(&value).unwrap();
}

pub(super) fn fixture() -> (GitContext, Value, AuthContext, SdlcWorkspaceConfig) {
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
        "stage":"Development","cycle":0,"attempt":1,"owner_subject":"fixture-owner",
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
        owner_subject: "fixture-owner".into(),
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
            "owner_subject":"fixture-owner","cycle":0,"waiting_reason":null,"routing":routing},
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

#[test]
fn confirmed_source_current_developer_and_exact_child_are_required() {
    let (c, r, auth, config) = fixture();
    c.validate().unwrap();
    assert_eq!(c.current(&r).unwrap(), std::time::Duration::from_secs(30));
    assert!(identity(&auth, &config, &c).is_ok());
}

#[test]
fn grants_are_disjoint_and_no_admin_root_or_pipeline_fallback() {
    let (c, _, auth, config) = fixture();
    for scopes in [
        vec!["ci-cd:read".into(), "ci-cd:write".into()],
        vec![
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            format!("ci-cd:sdlc:task:v1:{}", "a".repeat(64)),
        ],
        vec![
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            c.grant().unwrap(),
            "git:write".into(),
        ],
        vec![
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            c.grant().unwrap(),
            "ci-cd:admin".into(),
        ],
    ] {
        let mut changed = auth.clone();
        changed.scopes = scopes.into_iter().collect();
        assert!(identity(&changed, &config, &c).is_err());
    }
    let mut changed = auth.clone();
    changed.user_id = Uuid::new_v4().to_string();
    assert!(identity(&changed, &config, &c).is_err());
    let mut changed = auth.clone();
    changed.role = Some("admin".into());
    assert!(identity(&changed, &config, &c).is_err());
    let mut changed = auth;
    changed.session_id = Some(Uuid::new_v4().to_string());
    assert!(identity(&changed, &config, &c).is_err());
}

#[test]
fn context_cannot_change_source_ref_cas_fence_run_or_role_under_same_child() {
    let (c, _, auth, config) = fixture();
    let original = serde_json::to_value(&c).unwrap();
    for (path, value) in [
        ("/newOid", json!("f".repeat(40))),
        ("/oldOid", json!("a".repeat(40))),
        ("/runId", json!(Uuid::new_v4())),
        ("/projectId", json!(Uuid::new_v4())),
        ("/intent/binding/fencingToken", json!(4)),
        ("/source/candidate_ref", json!("refs/heads/main")),
        ("/intent/role", json!("devops")),
    ] {
        let mut value_c = original.clone();
        *value_c.pointer_mut(path).unwrap() = value;
        let changed: GitContext = serde_json::from_value(value_c).unwrap();
        assert!(identity(&auth, &config, &changed).is_err(), "{path}");
    }
}

#[test]
fn absent_held_stale_foreign_or_uncertain_tracker_readback_denies() {
    let (c, r, _, _) = fixture();
    for (path, value) in [
        ("/state", json!("blocked")),
        ("/binding", Value::Null),
        ("/current/runtime_state", json!("stopped")),
        (
            "/current/receipt/task/waiting_reason",
            json!("awaiting_owner"),
        ),
        ("/current/receipt/task/stage", json!("Draft")),
        ("/current/receipt/assignment/fencing_token", json!(4)),
        ("/current/receipt/lease/version", json!(1)),
        (
            "/current/receipt/lease/expires_at",
            json!(Utc::now() - chrono::Duration::seconds(1)),
        ),
        ("/current/receipt/binding/run_id", json!(Uuid::new_v4())),
        ("/current/receipt/binding/machine_subject", json!("other")),
    ] {
        let mut changed = r.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert!(c.current(&changed).is_err(), "{path}");
    }
}

#[test]
fn native_tuple_and_source_revision_are_separately_pinned() {
    let (c, mut r, _, _) = fixture();
    r["current"]["receipt"]["lease"]["version"] = json!(3);
    assert!(c.current(&r).is_ok());
    r["binding"]["source_revision"] = json!(2);
    assert!(c.current(&r).is_err());
    let mut changed = c;
    changed.source.source_revision = 2;
    assert!(changed.validate().is_err());
}

#[test]
fn base_request_is_existing_scoped_delegation_not_new_issuer() {
    let (c, _, _, _) = fixture();
    let value = delegation_request(&c, "fixture:original", 900).unwrap();
    assert_eq!(
        value["scopes"],
        json!(["ci-cd:read", "ci-cd:write", c.grant().unwrap()])
    );
    assert_eq!(value["expires_in_seconds"], 900);
    assert_eq!(value.as_object().unwrap().len(), 5);
    for ttl in [0, 1801] {
        assert!(delegation_request(&c, "fixture", ttl).is_err());
    }
}

#[test]
fn malformed_candidate_never_decodes_as_ordinary_pipeline_event() {
    let (_, _, _, _) = fixture();
    let mixed =
        json!({"task_candidate":{},"repository":"fixture/repo","ref_name":"refs/heads/main"});
    assert!(serde_json::from_value::<PushRequest>(mixed).is_err());
    assert!(matches!(
        serde_json::from_value::<PushRequest>(
            json!({"repository":"fixture/repo","ref_name":"refs/heads/main"})
        )
        .unwrap(),
        PushRequest::Legacy(_)
    ));
}

#[test]
fn duplicate_credential_or_context_headers_are_rejected() {
    let mut h = HeaderMap::new();
    h.append("authorization", "Bearer fixture-one".parse().unwrap());
    h.append("authorization", "Bearer fixture-two".parse().unwrap());
    assert!(one_header(&h, "authorization").is_err());
    assert!(one_header(&h, HEADER).is_err());
}

#[test]
fn repository_route_identity_has_only_one_optional_git_suffix() {
    for raw in ["fixture/repo", "fixture/repo.git"] {
        assert!(repository_matches(raw, "fixture/repo"));
    }
    for raw in [
        "fixture/repo.git.git",
        "repo",
        "other/repo.git",
        "fixture/repo.git/",
        " fixture/repo.git",
    ] {
        assert!(!repository_matches(raw, "fixture/repo"));
    }
}
