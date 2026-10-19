use super::*;
use crate::delegated_credentials::StoredCredential;
use crate::domain::sdlc_workspace::{
    WorkspaceOperationBlocker, WorkspaceOperationStatus, WorkspaceTaskBinding,
};
use pdlc_auth_core::AuthContext;
use serde_json::json;

fn fixture() -> (
    TaskCredentialContext,
    DeliveryVariables,
    WorkspaceOperationReceipt,
) {
    let task = Uuid::new_v4();
    let binding = WorkspaceTaskBinding {
        tracker_instance_id: Uuid::new_v4().to_string(),
        tracker_project_id: Uuid::new_v4(),
        task_id: task,
        root_task_id: task,
        assignment_id: Uuid::new_v4(),
        execution_id: Uuid::new_v4(),
        routing_snapshot_id: Uuid::new_v4(),
        requirement_revision: 1,
        fencing_token: 2,
        assignment_hash: "a".repeat(64),
        workflow_task_ref: "SDLC-27".into(),
    };
    let mut context = TaskCredentialContext {
        version: 1,
        project_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        chat_id: Uuid::new_v4(),
        machine_subject: "fixture-devops".into(),
        trigger_id: Uuid::new_v4(),
        variables_sha256: "b".repeat(64),
        intent: TaskWorkspaceIntent {
            operation_key: "devops:prepare".into(),
            binding,
            repository_id: Uuid::new_v4(),
            source_commit: "c".repeat(40),
            role: WorkspaceRole::Devops,
            agent_id: Uuid::new_v4(),
            tracker_lease_id: Uuid::new_v4(),
            holder_subject: "fixture-fleet".into(),
            expected_lease_version: 1,
        },
    };
    let mut developer = context.intent.clone();
    developer.role = WorkspaceRole::Developer;
    developer.operation_key = "developer:original".into();
    developer.binding.assignment_id = Uuid::new_v4();
    developer.binding.execution_id = Uuid::new_v4();
    developer.binding.fencing_token = 1;
    let attempt = Uuid::new_v4();
    let request = developer
        .request(
            Uuid::new_v4(),
            attempt,
            1,
            format!("attempt-{attempt}-1-{}", Uuid::new_v4().simple()),
        )
        .unwrap();
    let original = WorkspaceOperationReceipt {
        schema: "forge/workspace-operation-receipt/v1".into(),
        operation_id: Uuid::new_v4(),
        project_id: context.project_id,
        request_hash: sha(&serde_json::to_vec(&request).unwrap()),
        request,
        recorded_at: Utc::now(),
        status: WorkspaceOperationStatus::Blocked,
        blockers: vec![WorkspaceOperationBlocker::TrackerAdmissionUnavailable],
        physical_source_observed: false,
        dispatch_allowed: false,
    };
    let source = &original.request.binding;
    let current = &context.intent;
    let artifact = Artifact {
        id: Uuid::new_v4(),
        attempt_id: attempt,
        sha256: "d".repeat(64),
        size_bytes: 17,
        config_sha256: "e".repeat(64),
        plan_sha256: "f".repeat(64),
    };
    let variables = DeliveryVariables {
        forge_task_delivery: Binding {
            kind: KIND.into(),
            source_pipeline_id: Uuid::new_v4(),
            development_result_sha256: "1".repeat(64),
            target_policy_sha256: "2".repeat(64),
            target_kind: TargetKind::Static,
            command: DeliveryCommand {
                operation_key: "delivery:original".into(),
                workspace_operation_key: original.request.operation_key.clone(),
                original: WorkspaceOperationLookup {
                    request_hash: original.request_hash.clone(),
                    task_id: source.task_id,
                    root_task_id: source.root_task_id,
                    assignment_id: source.assignment_id,
                    execution_id: source.execution_id,
                    fencing_token: source.fencing_token,
                },
                action: DeliveryAction::Deploy,
                artifact_id: Some(artifact.id),
                expected_manifest_sha256: None,
                task_fence: Some(TaskDeliveryFence {
                    development_result_id: Uuid::new_v4(),
                    assignment_id: current.binding.assignment_id,
                    execution_id: current.binding.execution_id,
                    agent_id: current.agent_id,
                    lease_id: current.tracker_lease_id,
                    fencing_token: current.binding.fencing_token,
                    expected_lease_version: current.expected_lease_version,
                    assignment_hash: current.binding.assignment_hash.clone(),
                }),
            },
            artifact: Some(artifact),
            rollback_manifest_sha256: None,
        },
    };
    context.variables_sha256 = canonical_hash(&variables_value(&variables).unwrap()).unwrap();
    (context, variables, original)
}

fn identity(context: &TaskCredentialContext) -> (Identity, StoredCredential) {
    let subject = Uuid::new_v4();
    let config = crate::config::SdlcWorkspaceConfig {
        delegated_subject: Some(subject),
        ..Default::default()
    };
    let auth = AuthContext {
        user_id: subject.to_string(),
        email: Some("fixture@example.invalid".into()),
        role: None,
        session_id: None,
        token: format!("sdlc_pat_{}", "x".repeat(43)),
        scopes: [
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            context.grant().unwrap(),
        ]
        .into(),
    };
    let identity = Identity::verified(&auth, &config).unwrap();
    let stored = identity
        .store(
            context.clone(),
            &crate::config::SecretsConfig {
                key: Some([29; 32]),
            },
        )
        .unwrap();
    (identity, stored)
}

#[test]
fn producer_commits_closed_variables_without_new_scopes_or_context_schema() {
    let (context, variables, original) = fixture();
    let (context, value, request) =
        delegation_inputs(context, variables, "delivery:credential", 900).unwrap();
    assert_eq!(context.version, 1);
    assert_eq!(
        serde_json::to_value(&context)
            .unwrap()
            .as_object()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(
        request["scopes"],
        json!(["ci-cd:read", "ci-cd:write", context.grant().unwrap()])
    );
    let binding = retained(value.clone(), &context).unwrap();
    let payload = value["forgeTaskDelivery"].as_str().unwrap();
    let arg = format!("forgeTaskDelivery={payload}");
    let (key, text) = arg.split_once('=').unwrap();
    let cli = serde_json::to_value(std::collections::BTreeMap::from([(key, text)])).unwrap();
    assert_eq!(
        cli, value,
        "existing --variable string wire must preserve committed bytes"
    );
    binding.bind_source(&context, &original).unwrap();
    context
        .bind_trigger(
            &context.trigger_id.to_string(),
            Some(&context.intent.source_commit),
            &value,
        )
        .unwrap();
}

#[test]
fn old_unbound_variables_and_unknown_or_extra_fields_are_not_delivery_authority() {
    let (context, variables, _) = fixture();
    for value in [
        Value::Null,
        json!({}),
        json!({"role":"devops"}),
        json!({"forgeTaskDelivery":{}}),
    ] {
        let mut old = context.clone();
        old.variables_sha256 = canonical_hash(&value).unwrap();
        assert!(retained(value, &old).is_err());
    }
    let mut duplicate = variables_value(&variables).unwrap();
    let mut text = duplicate["forgeTaskDelivery"].as_str().unwrap().to_string();
    assert_eq!(text.pop(), Some('}'));
    text.push_str(",\"kind\":\"forge/task-delivery/v1\"}");
    duplicate["forgeTaskDelivery"] = json!(text);
    let mut signed_duplicate = context.clone();
    signed_duplicate.variables_sha256 = canonical_hash(&duplicate).unwrap();
    assert!(retained(duplicate, &signed_duplicate).is_err());
    let value = serde_json::to_value(variables).unwrap();
    for (path, extra) in [
        ("", "admin"),
        ("/forgeTaskDelivery", "url"),
        ("/forgeTaskDelivery/command", "accepted"),
    ] {
        let mut changed = value.clone();
        changed
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(extra.into(), json!(true));
        changed["forgeTaskDelivery"] = json!(changed["forgeTaskDelivery"].to_string());
        let mut same_hash = context.clone();
        same_hash.variables_sha256 = canonical_hash(&changed).unwrap();
        assert!(retained(changed, &same_hash).is_err());
    }
}

#[test]
fn every_delivery_command_source_artifact_policy_and_cas_field_is_hash_committed() {
    let (context, variables, _) = fixture();
    let value = serde_json::to_value(variables).unwrap();
    for (path, replacement) in [
        ("/forgeTaskDelivery/sourcePipelineId", json!(Uuid::new_v4())),
        (
            "/forgeTaskDelivery/developmentResultSha256",
            json!("3".repeat(64)),
        ),
        (
            "/forgeTaskDelivery/targetPolicySha256",
            json!("4".repeat(64)),
        ),
        ("/forgeTaskDelivery/targetKind", json!("oci")),
        ("/forgeTaskDelivery/command/operationKey", json!("other")),
        (
            "/forgeTaskDelivery/command/workspaceOperationKey",
            json!("other"),
        ),
        ("/forgeTaskDelivery/command/action", json!("rollback")),
        (
            "/forgeTaskDelivery/command/expectedManifestSha256",
            json!("5".repeat(64)),
        ),
        (
            "/forgeTaskDelivery/command/taskFence/developmentResultId",
            json!(Uuid::new_v4()),
        ),
        ("/forgeTaskDelivery/artifact/id", json!(Uuid::new_v4())),
        (
            "/forgeTaskDelivery/artifact/attemptId",
            json!(Uuid::new_v4()),
        ),
        ("/forgeTaskDelivery/artifact/sha256", json!("6".repeat(64))),
        ("/forgeTaskDelivery/artifact/sizeBytes", json!(18)),
        (
            "/forgeTaskDelivery/artifact/configSha256",
            json!("7".repeat(64)),
        ),
        (
            "/forgeTaskDelivery/artifact/planSha256",
            json!("8".repeat(64)),
        ),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        changed["forgeTaskDelivery"] = json!(changed["forgeTaskDelivery"].to_string());
        assert!(retained(changed, &context).is_err(), "{path}");
    }
}

#[test]
fn foreign_project_task_assignment_source_repository_and_fence_cannot_borrow_build() {
    let (context, variables, original) = fixture();
    let binding = variables.forge_task_delivery;
    for case in 0..8 {
        let mut changed = original.clone();
        match case {
            0 => changed.project_id = Uuid::new_v4(),
            1 => changed.request.binding.task_id = Uuid::new_v4(),
            2 => changed.request.binding.assignment_id = Uuid::new_v4(),
            3 => changed.request.source_commit = "9".repeat(40),
            4 => changed.request.repository_id = Uuid::new_v4(),
            5 => changed.request.binding.fencing_token += 1,
            6 => changed.request.binding.routing_snapshot_id = Uuid::new_v4(),
            _ => changed.request.binding.requirement_revision += 1,
        }
        assert!(
            binding.bind_source(&context, &changed).is_err(),
            "case {case}"
        );
    }
    let mut changed = context;
    changed.intent.binding.assignment_id = Uuid::new_v4();
    assert!(binding.bind_source(&changed, &original).is_err());
}

#[test]
fn rollback_requires_separate_exact_committed_last_confirmed_manifest() {
    let (mut context, mut variables, _) = fixture();
    let binding = &mut variables.forge_task_delivery;
    binding.command.action = DeliveryAction::Rollback;
    binding.command.artifact_id = None;
    binding.command.expected_manifest_sha256 = Some("3".repeat(64));
    binding.artifact = None;
    assert!(binding.validate(&context).is_err());
    binding.rollback_manifest_sha256 = Some("4".repeat(64));
    binding.validate(&context).unwrap();
    let value = variables_value(&variables).unwrap();
    context.variables_sha256 = canonical_hash(&value).unwrap();
    retained(value, &context).unwrap();
    variables.forge_task_delivery.rollback_manifest_sha256 = Some("5".repeat(64));
    assert!(retained(variables_value(&variables).unwrap(), &context).is_err());
}

#[test]
fn candidate_hash_and_exact_artifact_attempt_are_not_pipeline_success_aliases() {
    let (context, variables, original) = fixture();
    let binding = variables.forge_task_delivery;
    binding.bind_source(&context, &original).unwrap();
    let a = binding.artifact.as_ref().unwrap();
    let mut evidence = CandidateEvidenceReadback {
        schema: "forge/candidate-evidence-readback/v1".into(),
        operation_receipt: original,
        pipeline_id: binding.source_pipeline_id,
        observed_at: Utc::now(),
        source_commit: context.intent.source_commit,
        config_sha256: Some(a.config_sha256.clone()),
        plan_sha256: Some(a.plan_sha256.clone()),
        candidate_observed: true,
        artifacts: vec![CandidateArtifactEvidence {
            artifact_id: a.id,
            job_id: Uuid::new_v4(),
            attempt_id: a.attempt_id,
            sha256: a.sha256.clone(),
            size_bytes: a.size_bytes,
        }],
        blockers: vec![],
        dispatch_allowed: false,
        deployment_verified: false,
        acceptance_verified: false,
    };
    binding.candidate(&evidence).unwrap();
    evidence.artifacts[0].attempt_id = Uuid::new_v4();
    assert!(binding.candidate(&evidence).is_err());
    evidence.artifacts[0].attempt_id = a.attempt_id;
    evidence.candidate_observed = false;
    assert!(binding.candidate(&evidence).is_err());
}

#[test]
fn unknown_delivery_custody_cannot_be_rebound_to_new_run_or_unmarked_legacy_journal() {
    let (context, variables, original) = fixture();
    let (own_identity, _) = identity(&context);
    let session = Session {
        pipeline: Uuid::new_v4(),
        identity: own_identity,
        binding: variables.forge_task_delivery,
        original,
    };
    let dir = std::env::temp_dir().join(format!("forge-delivery-custody-{}", Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(session.check_journal(&dir).is_err());
        session.record_journal(&dir).unwrap();
        let before = std::fs::read(dir.join("task-child.json")).unwrap();
        session.check_journal(&dir).unwrap();
        let mut next = context.clone();
        next.run_id = Uuid::new_v4();
        let (next, _) = identity(&next);
        let next = Session {
            identity: next,
            ..session
        };
        assert!(next.check_journal(&dir).is_err());
        assert_eq!(std::fs::read(dir.join("task-child.json")).unwrap(), before);
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn expired_child_after_tracker_await_cannot_publish_delivery() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let effects = AtomicUsize::new(0);
    let until = std::time::Instant::now() + std::time::Duration::from_millis(20);
    let permit = tracker::Permit::test_until(until);
    tokio::time::sleep_until(tokio::time::Instant::from_std(until)).await;
    if permit.check().is_ok() {
        effects.fetch_add(1, Ordering::SeqCst);
    }
    assert_eq!(effects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; Axum/PG custody, no native delivery"]
async fn delivery_pg_retained_variables_current_child_lock_and_http_denial_without_source() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tower::ServiceExt;

    let options = PgConnectOptions::from_str(
        &std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable PG"),
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
    let schema = format!("it_delivery_child_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |conn, _| {
            let sql = search_path.clone();
            Box::pin(async move {
                sqlx::query(&sql).execute(conn).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let case_pool = pool.clone();
    let case = tokio::spawn(async move {
        let pool = case_pool;
        crate::migrations().await.unwrap().run(&pool).await.unwrap();
        let (context, variables, _) = fixture();
        let (identity, stored) = identity(&context);
        let pipeline = Uuid::new_v4();
        crate::api::sdlc_workspace::task_pipeline::renewal::pg_tests::seed(&pool, &stored, pipeline).await;
        sqlx::query("UPDATE pipelines SET variables=$2 WHERE id=$1").bind(pipeline).bind(variables_value(&variables).unwrap()).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let (_, actual) = current(&mut tx, &identity, pipeline).await.unwrap();
        assert_eq!(canonical_hash(&actual).unwrap(), canonical_hash(&variables.forge_task_delivery).unwrap());
        let mut competing = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='50ms'").execute(&mut *competing).await.unwrap();
        let denied = sqlx::query("SELECT pipeline_id FROM task_pipeline_intents WHERE pipeline_id=$1 FOR UPDATE").bind(pipeline).fetch_one(&mut *competing).await.unwrap_err();
        assert_eq!(denied.as_database_error().and_then(|e| e.code()).as_deref(), Some("55P03"));
        competing.rollback().await.unwrap();
        tx.rollback().await.unwrap();

        let mut config = crate::config::RuntimeConfig::test_default();
        config.secrets = crate::config::SecretsConfig { key: Some([29;32]) };
        config.sdlc_workspace.delegated_subject = Some(identity.subject);
        let state = Arc::new(AppState { pool: Some(pool.clone()), namespace_admission_pool: None,
            auth_secret: Some("synthetic-delivery-signing".into()), git: config.git.to_git_config(), config,
            running_jobs: None, rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()) });
        let app = sdlc_workspace::routes().with_state(state).layer(Extension(identity.clone()));
        let command = &variables.forge_task_delivery.command;
        let path = format!("/api/v1/projects/{}/sdlc/workspace-operations/{}/delivery-operations", context.project_id, command.workspace_operation_key);
        let request = || Request::post(&path).header("content-type", "application/json").body(Body::from(serde_json::to_string(command).unwrap())).unwrap();
        // A valid hash is not source authority. This fixture deliberately has no Developer source receipt.
        assert_eq!(app.clone().oneshot(request()).await.unwrap().status(), StatusCode::FORBIDDEN);
        sqlx::query("UPDATE pipelines SET variables='{}'::jsonb WHERE id=$1").bind(pipeline).execute(&pool).await.unwrap();
        assert_eq!(app.clone().oneshot(request()).await.unwrap().status(), StatusCode::FORBIDDEN);
        sqlx::query("UPDATE pipelines SET variables=$2 WHERE id=$1").bind(pipeline).bind(variables_value(&variables).unwrap()).execute(&pool).await.unwrap();
        let mut foreign = identity.clone(); foreign.grant_hash = "9".repeat(64);
        let mut tx = pool.begin().await.unwrap();
        assert!(current(&mut tx, &foreign, pipeline).await.is_err());
        tx.rollback().await.unwrap();

        let config = crate::config::SdlcWorkspaceConfig { delegated_subject: Some(identity.subject), ..Default::default() };
        let replacement = AuthContext { user_id: identity.subject.to_string(), email: Some("fixture@example.invalid".into()), role: None, session_id: None,
            token: format!("sdlc_pat_{}", "y".repeat(43)), scopes: ["ci-cd:read".into(), "ci-cd:write".into(), context.grant().unwrap()].into() };
        let successor = Identity::verified(&replacement, &config).unwrap();
        let next = successor.store(context.clone(), &crate::config::SecretsConfig { key: Some([29;32]) }).unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SELECT pipeline_id FROM task_pipeline_intents WHERE pipeline_id=$1 FOR UPDATE").bind(pipeline).fetch_one(&mut *tx).await.unwrap();
        crate::delegated_credentials::custody::append(&mut tx, pipeline, "delivery:renewal", &"1".repeat(64), &stored.fingerprint, &next).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(current(&mut tx, &identity, pipeline).await.is_err());
        current(&mut tx, &successor, pipeline).await.unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(app.oneshot(request()).await.unwrap().status(), StatusCode::FORBIDDEN);
        let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM job_queue),(SELECT count(*) FROM execution_attempts),(SELECT count(*) FROM job_leases)").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (0,0,0), "denied delivery must not enqueue, claim or start");
    }).await;
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
        panic!("delivery PG case cancelled");
    }
}
