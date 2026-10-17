use super::*;
use crate::domain::sdlc_workspace::{WorkspaceRole, WorkspaceTaskBinding};
use std::collections::HashSet;

fn fixture() -> (
    TaskCredentialContext,
    AuthContext,
    SdlcWorkspaceConfig,
    SecretsConfig,
) {
    let task = Uuid::new_v4();
    let context = TaskCredentialContext {
        version: 1,
        project_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        chat_id: Uuid::new_v4(),
        machine_subject: "retained-native-machine".into(),
        trigger_id: Uuid::new_v4(),
        variables_sha256: digest(&json!({})).unwrap(),
        intent: TaskWorkspaceIntent {
            operation_key: "task:original".into(),
            repository_id: Uuid::new_v4(),
            source_commit: "a".repeat(40),
            role: WorkspaceRole::Developer,
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
        },
    };
    let subject = Uuid::new_v4();
    let central = AuthContext {
        user_id: subject.to_string(),
        email: Some("machine@example.test".into()),
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
    (
        context,
        central,
        config,
        SecretsConfig {
            key: Some([17; 32]),
        },
    )
}

#[test]
fn producer_reuses_base_delegate_exact_service_scopes_and_bounded_ttl() {
    let (context, _, _, _) = fixture();
    let request = delegation_request(&context, "original:credential", 900).unwrap();
    assert_eq!(request["service"], "ci-cd");
    assert_eq!(
        request["scopes"],
        json!(["ci-cd:read", "ci-cd:write", context.grant().unwrap()])
    );
    assert_eq!(request["idempotency_key"], "original:credential");
    assert_eq!(request.as_object().unwrap().len(), 5);
    for ttl in [0, -1, 1801, i64::MAX] {
        assert!(delegation_request(&context, "original", ttl).is_err());
    }
    for key in ["", "space key", "\n", &"x".repeat(129)] {
        assert!(delegation_request(&context, key, 900).is_err());
    }
}

#[test]
fn grant_binds_every_context_field_including_original_trigger_and_native_run() {
    let (context, central, config, _) = fixture();
    let identity = Identity::verified(&central, &config).unwrap();
    identity
        .bind(&context, context.project_id, &context.intent)
        .unwrap();
    let original = serde_json::to_value(&context).unwrap();
    for (path, replacement) in [
        ("/projectId", json!(Uuid::new_v4())),
        ("/runId", json!(Uuid::new_v4())),
        ("/chatId", json!(Uuid::new_v4())),
        ("/machineSubject", json!("another-machine")),
        ("/triggerId", json!(Uuid::new_v4())),
        ("/variablesSha256", json!("c".repeat(64))),
        ("/intent/repositoryId", json!(Uuid::new_v4())),
        ("/intent/sourceCommit", json!("c".repeat(40))),
        ("/intent/role", json!("devops")),
        ("/intent/agentId", json!(Uuid::new_v4())),
        ("/intent/trackerLeaseId", json!(Uuid::new_v4())),
        ("/intent/holderSubject", json!("another")),
        ("/intent/expectedLeaseVersion", json!(3)),
        ("/intent/operationKey", json!("other:key")),
        ("/intent/binding/assignmentId", json!(Uuid::new_v4())),
        ("/intent/binding/executionId", json!(Uuid::new_v4())),
        ("/intent/binding/fencingToken", json!(4)),
        ("/intent/binding/assignmentHash", json!("c".repeat(64))),
        ("/intent/binding/trackerInstanceId", json!(Uuid::new_v4())),
        ("/intent/binding/trackerProjectId", json!(Uuid::new_v4())),
        ("/intent/binding/routingSnapshotId", json!(Uuid::new_v4())),
        ("/intent/binding/requirementRevision", json!(2)),
        ("/intent/binding/workflowTaskRef", json!("SDLC-28")),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        let changed: TaskCredentialContext = serde_json::from_value(changed).unwrap();
        assert!(
            identity
                .bind(&changed, changed.project_id, &changed.intent)
                .is_err(),
            "{path}"
        );
    }
    let mut changed = context;
    changed.intent.binding.task_id = Uuid::new_v4();
    changed.intent.binding.root_task_id = changed.intent.binding.task_id;
    assert!(
        identity
            .bind(&changed, changed.project_id, &changed.intent)
            .is_err()
    );
}

#[test]
fn root_human_session_foreign_subject_malformed_and_ambiguous_grants_fail_closed() {
    let (context, central, config, _) = fixture();
    for scopes in [
        ["ci-cd:read".into(), "ci-cd:write".into()]
            .into_iter()
            .collect(),
        [context.grant().unwrap(), "ci-cd:write".into()]
            .into_iter()
            .collect(),
        [
            "ci-cd:sdlc:task:v1:not-a-hash".into(),
            "ci-cd:read".into(),
            "ci-cd:write".into(),
        ]
        .into_iter()
        .collect(),
        [
            context.grant().unwrap(),
            format!("{GRANT_PREFIX}{}", "c".repeat(64)),
            "ci-cd:read".into(),
            "ci-cd:write".into(),
        ]
        .into_iter()
        .collect(),
    ] {
        let mut changed = central.clone();
        changed.scopes = scopes;
        assert!(Identity::verified(&changed, &config).is_err());
    }
    for extra in [
        "git:write",
        "api:write",
        "ci-cd:admin",
        "task-tracker:read",
        "ci-cd:sdlc:unknown:abc",
    ] {
        let mut changed = central.clone();
        changed.scopes.insert(extra.into());
        assert!(Identity::verified(&changed, &config).is_err());
    }
    let mut changed = central.clone();
    changed.user_id = Uuid::new_v4().to_string();
    assert!(Identity::verified(&changed, &config).is_err());
    let mut changed = central.clone();
    changed.session_id = Some(Uuid::new_v4().to_string());
    assert!(Identity::verified(&changed, &config).is_err());
    let mut changed = central.clone();
    changed.role = Some("admin".into());
    assert!(Identity::verified(&changed, &config).is_err());
    let mut changed = central.clone();
    changed.token = "human-jwt".into();
    assert!(Identity::verified(&changed, &config).is_err());
    let mut changed = config.clone();
    changed.operation_subject = changed.delegated_subject;
    assert!(Identity::verified(&central, &changed).is_err());
}

#[test]
fn only_task_trigger_and_exact_task_readback_routes_no_git_admin_or_delivery() {
    let (_, central, config, _) = fixture();
    let identity = Identity::verified(&central, &config).unwrap();
    let p = Uuid::new_v4();
    let pipeline = Uuid::new_v4();
    identity
        .route("POST", &format!("/api/v1/projects/{p}/pipelines"))
        .unwrap();
    identity
        .route(
            "GET",
            &format!("/api/v1/projects/{p}/pipelines/{pipeline}/task-workspace"),
        )
        .unwrap();
    for (method, path) in [
        ("GET", "/api/v1/projects".into()),
        ("GET", format!("/api/v1/projects/{p}")),
        ("GET", format!("/api/v1/pipelines/{pipeline}")),
        ("POST", format!("/api/v1/pipelines/{pipeline}/retry")),
        (
            "POST",
            format!("/api/v1/projects/{p}/sdlc/workspace-operations"),
        ),
        ("POST", "/api/v1/admin/service-accounts".into()),
        ("POST", "/git/example/git-receive-pack".into()),
        ("POST", "/api/v1/projects/../pipelines".into()),
        ("DELETE", format!("/api/v1/projects/{p}/pipelines")),
    ] {
        assert!(identity.route(method, &path).is_err(), "{method} {path}");
    }
}

#[test]
fn encrypted_custody_is_restart_readable_and_cannot_splice_context_subject_or_child() {
    let (context, central, config, secrets) = fixture();
    let identity = Identity::verified(&central, &config).unwrap();
    let stored = identity.store(context.clone(), &secrets).unwrap();
    assert_eq!(stored.open(&secrets).unwrap(), central.token);
    let serialized = stored.value().unwrap();
    assert!(!serialized.to_string().contains(&central.token));
    assert_eq!(
        StoredCredential::from_value(&serialized)
            .unwrap()
            .open(&secrets)
            .unwrap(),
        central.token
    );
    for (field, value) in [
        ("subject", json!(Uuid::new_v4())),
        ("fingerprint", json!("c".repeat(64))),
        ("context_hash", json!("c".repeat(64))),
        ("encrypted_child", json!("v1:YQ==:YQ==")),
    ] {
        let mut changed = serialized.clone();
        changed[field] = value;
        assert!(
            StoredCredential::from_value(&changed)
                .unwrap()
                .open(&secrets)
                .is_err()
        );
    }
    let mut changed = stored.clone();
    changed.context.run_id = Uuid::new_v4();
    assert!(changed.open(&secrets).is_err());
    assert!(
        stored
            .open(&SecretsConfig {
                key: Some([18; 32])
            })
            .is_err()
    );
    assert!(
        identity
            .store(context, &SecretsConfig { key: None })
            .is_err()
    );
}

#[tokio::test]
async fn async_verifier_reintrospects_same_child_each_time_and_never_uses_cached_grant() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let (context, central, config, secrets) = fixture();
    let stored = Identity::verified(&central, &config)
        .unwrap()
        .store(context.clone(), &secrets)
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let calls = calls.clone();
        let central = central.clone();
        stored
            .verify_with(
                &config,
                &secrets,
                context.project_id,
                &context.intent,
                move |token| async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(token, central.token);
                    Ok(Some(central))
                },
            )
            .await
            .unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    for status in [401, 503] {
        assert!(
            stored
                .verify_with(
                    &config,
                    &secrets,
                    context.project_id,
                    &context.intent,
                    move |_| async move {
                        if status == 401 {
                            Err(ApiError::unauthorized())
                        } else {
                            Err(ApiError::service_unavailable("Base unavailable"))
                        }
                    }
                )
                .await
                .is_err()
        );
    }
    let mut changed = central.clone();
    changed.scopes = HashSet::from(["ci-cd:read".into(), "ci-cd:write".into()]);
    assert!(
        stored
            .verify_with(
                &config,
                &secrets,
                context.project_id,
                &context.intent,
                move |_| async move { Ok(Some(changed)) }
            )
            .await
            .is_err()
    );
    assert!(
        stored
            .verify_with(
                &config,
                &secrets,
                context.project_id,
                &context.intent,
                |_| async { Ok(None) }
            )
            .await
            .is_err()
    );
}

#[test]
fn native_context_requires_original_run_and_chat_not_only_well_formed_ids() {
    let (context, _, _, _) = fixture();
    let value = json!({"receipt":{"binding":{"run_id":context.run_id,"chat_id":context.chat_id,"machine_subject":context.machine_subject}}});
    assert!(context.native_matches(&value));
    for field in ["run_id", "chat_id", "machine_subject"] {
        let mut changed = value.clone();
        changed["receipt"]["binding"][field] = json!(Uuid::new_v4());
        assert!(!context.native_matches(&changed));
    }
}

#[test]
fn canonical_digest_is_key_order_independent_and_unknown_context_fields_rejected() {
    assert_eq!(
        digest(&json!({"z":1,"a":{"d":2,"b":3}})).unwrap(),
        digest(&json!({"a":{"b":3,"d":2},"z":1})).unwrap()
    );
    let (context, _, _, _) = fixture();
    let mut value = serde_json::to_value(context).unwrap();
    value["admin"] = json!(true);
    assert!(serde_json::from_value::<TaskCredentialContext>(value).is_err());
}

#[test]
fn trigger_binds_original_uuid_exact_source_and_all_variables() {
    let (context, _, _, _) = fixture();
    context
        .bind_trigger(
            &context.trigger_id.to_string(),
            Some(&context.intent.source_commit),
            &json!({}),
        )
        .unwrap();
    assert!(
        context
            .bind_trigger(
                &Uuid::new_v4().to_string(),
                Some(&context.intent.source_commit),
                &json!({})
            )
            .is_err()
    );
    assert!(
        context
            .bind_trigger(&context.trigger_id.to_string(), Some("main"), &json!({}))
            .is_err()
    );
    assert!(
        context
            .bind_trigger(&context.trigger_id.to_string(), None, &json!({}))
            .is_err()
    );
    assert!(
        context
            .bind_trigger(
                &context.trigger_id.to_string(),
                Some(&context.intent.source_commit),
                &json!({"PRIVILEGED":"yes"})
            )
            .is_err()
    );
}

#[test]
fn replay_fingerprint_preserves_legacy_and_binds_child_not_random_cipher_nonce() {
    let (context, central, config, secrets) = fixture();
    let identity = Identity::verified(&central, &config).unwrap();
    let a = identity.store(context.clone(), &secrets).unwrap();
    let b = identity.store(context.clone(), &secrets).unwrap();
    assert_ne!(a.encrypted_child, b.encrypted_child);
    assert_eq!(
        trigger_fingerprint("legacy-exact".into(), None).unwrap(),
        "legacy-exact"
    );
    let original = trigger_fingerprint("trigger".into(), Some(&a)).unwrap();
    assert_eq!(
        original,
        trigger_fingerprint("trigger".into(), Some(&b)).unwrap()
    );
    let mut foreign = central;
    foreign.token = "sdlc_pat_other_child".into();
    let foreign = Identity::verified(&foreign, &config)
        .unwrap()
        .store(context, &secrets)
        .unwrap();
    assert_ne!(
        original,
        trigger_fingerprint("trigger".into(), Some(&foreign)).unwrap()
    );
    let mut changed = a;
    changed.context_hash = "c".repeat(64);
    assert_ne!(
        original,
        trigger_fingerprint("trigger".into(), Some(&changed)).unwrap()
    );
}

#[tokio::test]
async fn expired_revoked_or_replaced_child_cannot_refresh_saved_command_authority() {
    let (context, central, config, secrets) = fixture();
    let stored = Identity::verified(&central, &config)
        .unwrap()
        .store(context.clone(), &secrets)
        .unwrap();
    for phase in ["queued-spawn", "preparation", "artifact-publication"] {
        for reason in ["expired-child", "revoked-child", "revoked-parent"] {
            let result = stored
                .verify_with(
                    &config,
                    &secrets,
                    context.project_id,
                    &context.intent,
                    |_| async { Err(ApiError::unauthorized()) },
                )
                .await;
            assert!(
                result.is_err(),
                "{phase}: {reason} cannot authorize a new effect"
            );
        }
    }
    // Even a live replacement with identical context is not the original child.
    let mut replacement = central;
    replacement.token = "sdlc_pat_renewed_fixture".into();
    assert!(
        stored
            .verify_with(
                &config,
                &secrets,
                context.project_id,
                &context.intent,
                move |_| async { Ok(Some(replacement)) }
            )
            .await
            .is_err()
    );
    assert!(stored.context == context);
}
