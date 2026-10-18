use super::*;
use crate::domain::sdlc_workspace::WorkspaceRole;
use axum::http::StatusCode;

pub(super) fn fixture() -> (
    StoredCredential,
    AuthContext,
    SdlcWorkspaceConfig,
    SecretsConfig,
    Value,
) {
    let (intent, live) = super::super::tests::fixture(WorkspaceRole::Developer, "Development");
    let context = TaskCredentialContext {
        version: 1,
        project_id: Uuid::new_v4(),
        intent,
        run_id: serde_json::from_value(live["receipt"]["binding"]["run_id"].clone()).unwrap(),
        chat_id: serde_json::from_value(live["receipt"]["binding"]["chat_id"].clone()).unwrap(),
        machine_subject: "role-agent".into(),
        trigger_id: Uuid::new_v4(),
        variables_sha256: "a".repeat(64),
    };
    let subject = Uuid::new_v4();
    let config = SdlcWorkspaceConfig {
        delegated_subject: Some(subject),
        ..Default::default()
    };
    let secrets = SecretsConfig {
        key: Some([29; 32]),
    };
    let central = AuthContext {
        user_id: subject.to_string(),
        email: Some("machine@example.invalid".into()),
        scopes: [
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            context.grant().unwrap(),
        ]
        .into(),
        role: None,
        session_id: None,
        token: format!("sdlc_pat_{}", "a".repeat(43)),
    };
    let stored = Identity::verified(&central, &config)
        .unwrap()
        .store(context, &secrets)
        .unwrap();
    (stored, central, config, secrets, live)
}

fn request(stored: &StoredCredential) -> RenewalRequest {
    RenewalRequest {
        renewal_key: "credential:successor:1".into(),
        predecessor_fingerprint: stored.fingerprint.clone(),
        credential_context: stored.context.clone(),
        replacement_token: format!("sdlc_pat_{}", "b".repeat(43)),
    }
}

fn root(child: &AuthContext) -> AuthContext {
    let mut root = child.clone();
    root.token = format!("sdlc_pat_{}", "r".repeat(43));
    root.scopes = ["ci-cd:read".into(), "ci-cd:write".into()].into();
    root
}

#[test]
fn renewal_owner_is_exact_machine_p_not_child_human_or_readonly() {
    let (_, child, config, _, _) = fixture();
    let parent = root(&child);
    assert_eq!(
        owner_identity(&parent, &config).unwrap(),
        config.delegated_subject.unwrap()
    );
    assert!(owner_identity(&child, &config).is_err());
    for case in 0..6 {
        let mut bad = parent.clone();
        match case {
            0 => bad.user_id = Uuid::new_v4().to_string(),
            1 => bad.role = Some("admin".into()),
            2 => bad.session_id = Some(Uuid::new_v4().to_string()),
            3 => {
                bad.scopes.remove("ci-cd:write");
            }
            4 => {
                bad.scopes.insert("git:write".into());
            }
            _ => bad.token = "legacy.jwt".into(),
        }
        assert!(owner_identity(&bad, &config).is_err(), "case {case}");
    }
    let mut collision = config;
    collision.operation_subject = collision.delegated_subject;
    assert!(owner_identity(&parent, &collision).is_err());
}

#[test]
fn renewal_predecessor_cas_and_payload_hash_do_not_rotate_business_identity() {
    let (stored, _, _, _, _) = fixture();
    let mut request = request(&stored);
    request.validate().unwrap();
    compare_predecessor(&stored, &request).unwrap();
    let pipeline = Uuid::new_v4();
    let hash = request
        .fingerprint(stored.context.project_id, pipeline, stored.subject)
        .unwrap();
    assert_eq!(
        hash,
        request
            .fingerprint(stored.context.project_id, pipeline, stored.subject)
            .unwrap()
    );
    let unchanged_context = serde_json::to_value(&stored.context).unwrap();
    request.predecessor_fingerprint = "f".repeat(64);
    assert_eq!(
        compare_predecessor(&stored, &request).unwrap_err().status,
        StatusCode::CONFLICT
    );
    assert_ne!(
        hash,
        request
            .fingerprint(stored.context.project_id, pipeline, stored.subject)
            .unwrap()
    );
    assert_eq!(
        serde_json::to_value(&stored.context).unwrap(),
        unchanged_context
    );
}

#[test]
fn renewal_replacement_requires_same_full_context_and_exact_child_scopes() {
    let (original, mut child, config, secrets, _) = fixture();
    let request = request(&original);
    child.token = request.replacement_token.clone();
    let next = replacement(
        &child,
        &config,
        &secrets,
        &original,
        request.credential_context.clone(),
    )
    .unwrap();
    assert_ne!(next.fingerprint, original.fingerprint);
    assert_eq!(next.context, original.context);
    assert_eq!(next.context_hash, original.context_hash);
    for (path, value) in [
        ("/runId", json!(Uuid::new_v4())),
        ("/chatId", json!(Uuid::new_v4())),
        ("/triggerId", json!(Uuid::new_v4())),
        ("/variablesSha256", json!("f".repeat(64))),
        ("/projectId", json!(Uuid::new_v4())),
        ("/intent/sourceCommit", json!("f".repeat(40))),
        ("/intent/operationKey", json!("different:business:key")),
        ("/intent/binding/fencingToken", json!(4)),
        ("/intent/binding/assignmentId", json!(Uuid::new_v4())),
    ] {
        let mut changed = serde_json::to_value(&original.context).unwrap();
        *changed.pointer_mut(path).unwrap() = value;
        let changed: TaskCredentialContext = serde_json::from_value(changed).unwrap();
        let mut issuer = child.clone();
        issuer.scopes = [
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            changed.grant().unwrap(),
        ]
        .into();
        assert!(
            replacement(&issuer, &config, &secrets, &original, changed).is_err(),
            "{path}"
        );
    }
    for extra in ["git:write", "ci-cd:admin", "tracker:write"] {
        let mut overbroad = child.clone();
        overbroad.scopes.insert(extra.into());
        assert!(
            replacement(
                &overbroad,
                &config,
                &secrets,
                &original,
                original.context.clone()
            )
            .is_err()
        );
    }
    assert!(
        replacement(
            &root(&child),
            &config,
            &secrets,
            &original,
            original.context.clone()
        )
        .is_err()
    );
}

#[test]
fn renewal_request_is_strict_bounded_and_does_not_accept_bearer_ambiguity() {
    let (stored, _, _, _, _) = fixture();
    let request = request(&stored);
    let mut value = json!({"renewalKey":request.renewal_key,"predecessorFingerprint":request.predecessor_fingerprint,
        "credentialContext":request.credential_context,"replacementToken":request.replacement_token});
    assert!(
        serde_json::from_value::<RenewalRequest>(value.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    value["authority"] = json!("caller");
    assert!(serde_json::from_value::<RenewalRequest>(value).is_err());
    for key in ["", "../escape", "has space", &"a".repeat(129)] {
        assert!(validate_key(key).is_err());
    }
    let mut headers = HeaderMap::new();
    assert!(bearer(&headers).is_err());
    headers.append(
        "authorization",
        "Bearer sdlc_pat_fixture_parent".parse().unwrap(),
    );
    assert!(bearer(&headers).is_ok());
    headers.append(
        "authorization",
        "Bearer sdlc_pat_other_parent".parse().unwrap(),
    );
    assert!(bearer(&headers).is_err());
}

#[tokio::test]
async fn renewal_tracker_binding_rejects_changed_run_fence_and_expired_lease() {
    use axum::routing::get;
    use std::sync::{Arc, RwLock};
    let (stored, _, _, _, value) = fixture();
    let peer = Arc::new(RwLock::new(value.clone()));
    let state = peer.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = SdlcWorkspaceConfig {
        tracker_read_url: Some(format!("http://{}", listener.local_addr().unwrap())),
        tracker_read_token: Some("synthetic-reader".into()),
        ..Default::default()
    };
    let path = format!(
        "/api/v1/issues/{}/sdlc/lifecycle",
        stored.context.intent.binding.task_id
    );
    let router = Router::new().route(
        &path,
        get(move || {
            let state = state.clone();
            async move {
                let body = state.read().unwrap().clone();
                Json(body)
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    assert!(
        tracker_bound(&config, &stored.context.intent, Some(&stored.context))
            .await
            .is_ok()
    );
    for (path, changed) in [
        ("/receipt/binding/run_id", json!(Uuid::new_v4())),
        ("/receipt/binding/chat_id", json!(Uuid::new_v4())),
        ("/receipt/assignment/fencing_token", json!(4)),
        ("/receipt/lease/expires_at", value["observed_at"].clone()),
    ] {
        let mut body = value.clone();
        *body.pointer_mut(path).unwrap() = changed;
        *peer.write().unwrap() = body;
        assert!(
            tracker_bound(&config, &stored.context.intent, Some(&stored.context))
                .await
                .is_err(),
            "{path}"
        );
    }
    server.abort();
}

#[test]
fn renewal_sql_preserves_original_intent_guard_and_is_append_only() {
    let sql = include_str!("../../../../../migrations/0095_task_credential_successors.sql");
    for required in [
        "TG_OP<>'INSERT'",
        "FOR UPDATE",
        "NEW.generation<>previous_generation+1",
        "NEW.predecessor_fingerprint IS DISTINCT FROM previous->>'fingerprint'",
        "original-ARRAY['fingerprint','encrypted_child']",
        "UNIQUE (pipeline_id,renewal_key)",
    ] {
        assert!(sql.contains(required), "{required}");
    }
    for forbidden in [
        "UPDATE task_pipeline_intents",
        "CREATE OR REPLACE",
        "DISABLE TRIGGER",
        "job_queue",
    ] {
        assert!(!sql.contains(forbidden), "{forbidden}");
    }
}

#[tokio::test]
#[ignore = "requires an explicitly migrated disposable CICD_TEST_DATABASE_URL including 0095"]
async fn renewal_pg_successors_are_cas_immutable_and_do_not_change_business_custody() {
    let url = std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable PG required");
    let parsed = reqwest::Url::parse(&url).expect("synthetic PG URL required");
    assert!(
        parsed
            .path()
            .trim_start_matches('/')
            .starts_with("forge_test_"),
        "disposable forge_test_ database required"
    );
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let (original, mut child, config, secrets, _) = fixture();
    let project = original.context.project_id;
    let pipeline = Uuid::new_v4();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://example.invalid/test.git')")
        .bind(project).bind(format!("renewal-{project}")).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,$3,$3,'queued')")
        .bind(pipeline).bind(project).bind(&original.context.intent.source_commit).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'prepare',0,'queued')")
        .bind(stage).bind(pipeline).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'prepare','unused','unused',0,'queued')")
        .bind(job).bind(stage).execute(&mut *tx).await.unwrap();
    ledger::append_delegated_task_pipeline(
        &mut tx,
        pipeline,
        project,
        &original.value().unwrap(),
        job,
        &original.context.intent,
    )
    .await
    .unwrap();
    let before: Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM task_pipeline_intents t WHERE pipeline_id=$1")
            .bind(pipeline)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let record = ledger::find_task_pipeline(&mut tx, pipeline, true)
        .await
        .unwrap()
        .unwrap();
    let request = request(&original);
    child.token = request.replacement_token.clone();
    let next = replacement(
        &child,
        &config,
        &secrets,
        &original,
        original.context.clone(),
    )
    .unwrap();
    let request_hash = request
        .fingerprint(project, pipeline, original.subject)
        .unwrap();
    let receipt = custody::append(
        &mut tx,
        pipeline,
        &request.renewal_key,
        &request_hash,
        &original.fingerprint,
        &next,
    )
    .await
    .unwrap();
    assert!(receipt.current);
    assert_eq!(receipt.generation, 1);
    assert_eq!(
        custody::current(&mut tx, &record)
            .await
            .unwrap()
            .fingerprint,
        next.fingerprint
    );
    assert_eq!(
        custody::find(&mut tx, pipeline, &request.renewal_key)
            .await
            .unwrap()
            .unwrap()
            .request_hash,
        request_hash
    );
    let wire = serde_json::to_string(&receipt).unwrap();
    assert!(!wire.contains(&child.token));
    assert!(!wire.contains("encrypted_child"));
    child.token = format!("sdlc_pat_{}", "c".repeat(43));
    let second = replacement(
        &child,
        &config,
        &secrets,
        &original,
        original.context.clone(),
    )
    .unwrap();
    let receipt2 = custody::append(
        &mut tx,
        pipeline,
        "credential:successor:2",
        &"d".repeat(64),
        &next.fingerprint,
        &second,
    )
    .await
    .unwrap();
    assert!(receipt2.current);
    assert_eq!(receipt2.generation, 2);
    assert!(
        !custody::find(&mut tx, pipeline, &request.renewal_key)
            .await
            .unwrap()
            .unwrap()
            .current
    );
    assert_eq!(
        custody::current(&mut tx, &record)
            .await
            .unwrap()
            .fingerprint,
        second.fingerprint
    );
    assert_eq!(
        custody::append(
            &mut tx,
            pipeline,
            "resurrection",
            &"e".repeat(64),
            &second.fingerprint,
            &next
        )
        .await
        .unwrap_err()
        .status,
        StatusCode::CONFLICT
    );
    for statement in [
        "UPDATE task_credential_successors SET request_hash=repeat('f',64) WHERE pipeline_id=$1",
        "DELETE FROM task_credential_successors WHERE pipeline_id=$1",
        "INSERT INTO task_credential_successors(pipeline_id,generation,renewal_key,request_hash,predecessor_fingerprint,credential) SELECT pipeline_id,2,'stale',request_hash,predecessor_fingerprint,credential FROM task_credential_successors WHERE pipeline_id=$1",
        "UPDATE task_pipeline_intents SET delegated_credential=jsonb_set(delegated_credential,'{fingerprint}',to_jsonb(repeat('f',64))) WHERE pipeline_id=$1",
    ] {
        sqlx::query("SAVEPOINT denied")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            sqlx::query(statement)
                .bind(pipeline)
                .execute(&mut *tx)
                .await
                .is_err()
        );
        sqlx::query("ROLLBACK TO SAVEPOINT denied")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let after: Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM task_pipeline_intents t WHERE pipeline_id=$1")
            .bind(pipeline)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(after, before);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_credential_successors WHERE pipeline_id=$1")
            .bind(pipeline)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 2);
    tx.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn renewal_actual_routes_reject_missing_or_nonplatform_owner_before_db() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let (stored, _, _, _, _) = fixture();
    let mut config = crate::config::RuntimeConfig::test_default();
    config.sdlc_workspace.delegated_subject = Some(stored.subject);
    let app = routes().with_state(Arc::new(AppState {
        pool: None,
        namespace_admission_pool: None,
        auth_secret: None,
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()),
    }));
    let request = request(&stored);
    let body = json!({"renewalKey":request.renewal_key,"predecessorFingerprint":request.predecessor_fingerprint,
        "credentialContext":request.credential_context,"replacementToken":request.replacement_token});
    let path = format!(
        "/api/v1/projects/{}/pipelines/{}/credential-renewals",
        stored.context.project_id,
        Uuid::new_v4()
    );
    for credential in [None, Some("Bearer legacy.jwt")] {
        let mut post = Request::builder()
            .method("POST")
            .uri(&path)
            .header("content-type", "application/json");
        let mut get = Request::builder().uri(format!("{path}/credential:successor:1"));
        if let Some(credential) = credential {
            post = post.header("authorization", credential);
            get = get.header("authorization", credential);
        }
        for request in [
            post.body(Body::from(body.to_string())).unwrap(),
            get.body(Body::empty()).unwrap(),
        ] {
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    }
}
