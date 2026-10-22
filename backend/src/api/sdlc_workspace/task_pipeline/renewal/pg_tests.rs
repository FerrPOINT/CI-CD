//! Actual HTTP handlers and independent PG transactions; no runner/Git/Docker effects.
use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode, Uri},
    response::Response,
};
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{str::FromStr, sync::Arc};
use tower::ServiceExt;

const CHILD: &str = "FORGE_RENEWAL_HTTP_PG_CHILD";
const SELECTOR: &str = "api::sdlc_workspace::task_pipeline::renewal::pg_tests::renewal_pg_http_handoff_racing_keys_and_lost_ack";

fn token(letter: char) -> String {
    format!("sdlc_pat_{}", letter.to_string().repeat(43))
}

fn body(original: &StoredCredential, key: &str, letter: char) -> Value {
    json!({"renewalKey":key,"predecessorFingerprint":original.fingerprint,
        "credentialContext":original.context,"replacementToken":token(letter)})
}

async fn post(app: Router, path: &str, value: &Value, bearer: &str) -> Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(path)
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn wire(response: Response) -> Value {
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = axum::body::to_bytes(response.into_body(), 16_384)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

pub(crate) async fn seed(pool: &PgPool, original: &StoredCredential, pipeline: Uuid) {
    seed_with_owner(pool, original, pipeline, Uuid::new_v4(), Uuid::new_v4()).await;
}

pub(crate) async fn seed_with_owner(
    pool: &PgPool,
    original: &StoredCredential,
    pipeline: Uuid,
    registry: Uuid,
    instance: Uuid,
) {
    let context = &original.context;
    let project = context.project_id;
    let b = &context.intent.binding;
    let group = Uuid::new_v4();
    let namespace = Uuid::new_v4();
    let command = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: registry,
            namespace_id: namespace,
        },
        resource: ResourceRef {
            instance_id: instance,
            kind: ResourceKind::GitGroup,
            resource_id: group,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: None,
    };
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Renewal fixture')")
        .bind(group)
        .bind(format!("renewal-{}", group.simple()))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
        .bind(group).bind(registry).bind(namespace).bind(serde_json::to_value(command).unwrap()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(context.intent.repository_id)
        .bind(format!("renewal_{}", pipeline.simple()))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(context.intent.repository_id)
        .bind(group)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://example.invalid/test.git',$3)")
        .bind(project).bind(format!("renewal-{project}")).bind(context.intent.repository_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,1,'Renewal fixture','QA','active',now())")
        .bind(registry).bind(namespace).bind(Uuid::parse_str(&b.tracker_instance_id).unwrap()).bind(b.tracker_project_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,$3,$3,'queued')")
        .bind(pipeline).bind(project).bind(&context.intent.source_commit).execute(&mut *tx).await.unwrap();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
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
        &context.intent,
    )
    .await
    .unwrap();
    let fingerprint = crate::delegated_credentials::trigger_fingerprint(
        "synthetic-original-trigger".into(),
        Some(original),
    )
    .unwrap();
    sqlx::query("INSERT INTO pipeline_triggers(id,project_id,source,idempotency_key,request_fingerprint,pipeline_id) VALUES($1,$2,'api',$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(project).bind(context.trigger_id.to_string()).bind(fingerprint).bind(pipeline).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn business(pool: &PgPool, pipeline: Uuid) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object(\
        'intent',(SELECT to_jsonb(t) FROM task_pipeline_intents t WHERE pipeline_id=$1),\
        'pipeline',(SELECT to_jsonb(p) FROM pipelines p WHERE id=$1),\
        'triggers',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM pipeline_triggers t WHERE pipeline_id=$1),\
        'stages',(SELECT jsonb_agg(to_jsonb(s) ORDER BY id) FROM stages s WHERE pipeline_id=$1),\
        'jobs',(SELECT jsonb_agg(to_jsonb(j) ORDER BY j.id) FROM jobs j JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1),\
        'queue',(SELECT jsonb_agg(to_jsonb(q) ORDER BY q.job_id) FROM job_queue q JOIN jobs j ON j.id=q.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1),\
        'attempts',(SELECT jsonb_agg(to_jsonb(a) ORDER BY a.id) FROM execution_attempts a JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1),\
        'leases',(SELECT jsonb_agg(to_jsonb(l) ORDER BY l.id) FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN stages s ON s.id=j.stage_id WHERE s.pipeline_id=$1))")
        .bind(pipeline).fetch_one(pool).await.unwrap()
}

async fn pipeline_detail_readback_checks(
    state: Arc<AppState>,
    original: &StoredCredential,
    pipeline: Uuid,
    current_token: &str,
) {
    let pool = state.pool.as_ref().unwrap();
    let (foreign_stored, mut foreign_central, _, _, _) = super::tests::fixture();
    foreign_central.user_id = original.subject.to_string();
    foreign_central.token = token('d');
    foreign_central.scopes = [
        "ci-cd:read".into(),
        "ci-cd:write".into(),
        foreign_stored.context.grant().unwrap(),
    ]
    .into();
    let foreign = Identity::verified(&foreign_central, &state.config.sdlc_workspace)
        .unwrap()
        .store(foreign_stored.context, &state.config.secrets)
        .unwrap();
    let foreign_pipeline = Uuid::new_v4();
    seed(pool, &foreign, foreign_pipeline).await;
    let before = business(pool, pipeline).await;
    let expected =
        serde_json::to_value(crate::api::pipeline_detail(pool, pipeline).await.unwrap()).unwrap();
    let app = Router::new()
        .route(
            "/api/v1/pipelines/{pipeline_id}",
            get(crate::api::pipelines_routes::get_pipeline),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::api::authz_mw::require_auth,
        ))
        .with_state(state.clone());
    for (requested, bearer, expected_status) in [
        (pipeline, current_token.to_owned(), StatusCode::OK),
        (
            foreign_pipeline,
            current_token.to_owned(),
            StatusCode::FORBIDDEN,
        ),
        (pipeline, token('a'), StatusCode::UNAUTHORIZED),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/pipelines/{requested}"))
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected_status);
        if expected_status == StatusCode::OK {
            assert_eq!(wire(response).await, expected);
        }
    }
    assert_eq!(
        business(pool, pipeline).await,
        before,
        "GET must not change original business custody"
    );
}

async fn run_child() {
    let input: Value =
        serde_json::from_str(&std::env::var("FORGE_RENEWAL_FIXTURE").unwrap()).unwrap();
    let original = StoredCredential::from_value(&input["original"]).unwrap();
    let schema = std::env::var("FORGE_RENEWAL_SCHEMA").unwrap();
    assert!(
        schema.starts_with("it_renewal_")
            && schema
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    );
    let application = format!("renewal_{}", Uuid::new_v4().simple());
    let url = std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable PG required");
    let options = PgConnectOptions::from_str(&url).unwrap();
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("forge_test_"))
    );
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(
            options
                .options([("search_path", schema.as_str())])
                .application_name(&application),
        )
        .await
        .unwrap();
    crate::migrations().await.unwrap().run(&pool).await.unwrap();
    let pipeline = Uuid::new_v4();
    seed(&pool, &original, pipeline).await;
    let before = business(&pool, pipeline).await;
    assert_eq!(before["queue"], Value::Null);
    assert_eq!(before["attempts"], Value::Null);
    let mut config = crate::config::RuntimeConfig::test_default();
    config.secrets = SecretsConfig {
        key: Some([29; 32]),
    };
    config.sdlc_workspace = SdlcWorkspaceConfig {
        delegated_subject: Some(original.subject),
        tracker_read_url: Some(std::env::var("FORGE_RENEWAL_PEER").unwrap()),
        tracker_read_token: Some("synthetic-tracker-reader".into()),
        ..Default::default()
    };
    let state = Arc::new(AppState {
        pool: Some(pool.clone()),
        namespace_admission_pool: None,
        auth_secret: Some("synthetic-pipeline-read-signing-key".into()),
        git: config.git.to_git_config(),
        config,
        running_jobs: None,
        rate_limiter: Arc::new(crate::rate_limit::RateLimiter::default()),
    });
    let app = routes().with_state(state.clone());
    let path = format!(
        "/api/v1/projects/{}/pipelines/{pipeline}/credential-renewals",
        original.context.project_id
    );
    let left = body(&original, "race:left", 'b');
    let right = body(&original, "race:right", 'c');

    // Force both real handler transactions to wait on the same original row.
    // This witnesses concurrent CAS, not sequential calls labelled a race.
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pipeline_id FROM task_pipeline_intents WHERE pipeline_id=$1 FOR UPDATE")
        .bind(pipeline)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let a_app = app.clone();
    let a_path = path.clone();
    let a_body = left.clone();
    let a = tokio::spawn(async move { post(a_app, &a_path, &a_body, &token('r')).await });
    let b_app = app.clone();
    let b_path = path.clone();
    let b_body = right.clone();
    let b = tokio::spawn(async move { post(b_app, &b_path, &b_body, &token('r')).await });
    let waiting = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE '%task_pipeline_intents%'")
                .bind(&application).fetch_one(&pool).await.unwrap();
            if count == 2 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await;
    blocker.rollback().await.unwrap();
    let results = tokio::time::timeout(Duration::from_secs(20), async {
        (a.await.unwrap(), b.await.unwrap())
    })
    .await
    .unwrap();
    assert!(
        waiting.is_ok(),
        "both original-row lock waiters must be observed"
    );
    let statuses = [results.0.status(), results.1.status()];
    assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1);
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1
    );
    let winner = if results.0.status() == StatusCode::OK {
        &left
    } else {
        &right
    };
    // Deliberately lose the successful POST response. Recovery uses only the key.
    drop(results);
    let key = winner["renewalKey"].as_str().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("{path}/{key}"))
                .header("authorization", format!("Bearer {}", token('r')))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let ack = wire(response).await;
    assert_eq!(ack["generation"], 1);
    assert_eq!(ack["current"], true);
    assert_eq!(
        ack["fingerprint"],
        crate::auth::hash_token(winner["replacementToken"].as_str().unwrap())
    );
    let request: RenewalRequest = serde_json::from_value(winner.clone()).unwrap();
    assert_eq!(
        ack["requestHash"],
        request
            .fingerprint(original.context.project_id, pipeline, original.subject)
            .unwrap()
    );
    let replay = post(app.clone(), &path, winner, &token('r')).await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(wire(replay).await, ack);
    let mut changed = winner.clone();
    changed["replacementToken"] = json!(token('d'));
    assert_eq!(
        post(app.clone(), &path, &changed, &token('r'))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        post(app.clone(), &path, winner, &token('a')).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post(
            app.clone(),
            &path,
            winner,
            winner["replacementToken"].as_str().unwrap()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    // Exercise the real normal-operation resolver. The original token is expired
    // at the issuer; only the acknowledged successor can authorize this boundary.
    let mut tx = pool.begin().await.unwrap();
    let record = ledger::find_task_pipeline(&mut tx, pipeline, false)
        .await
        .unwrap()
        .unwrap();
    let until = super::super::authorize_saved(
        &mut tx,
        &state.config.sdlc_workspace,
        &record,
        &state.config.secrets,
    )
    .await
    .unwrap();
    assert!(Instant::now() < until);
    let current = custody::current(&mut tx, &record).await.unwrap();
    assert_eq!(current.fingerprint, ack["fingerprint"].as_str().unwrap());
    tx.commit().await.unwrap();
    pipeline_detail_readback_checks(
        state.clone(),
        &original,
        pipeline,
        winner["replacementToken"].as_str().unwrap(),
    )
    .await;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_credential_successors WHERE pipeline_id=$1")
            .bind(pipeline)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "race/replay/readback must publish exactly one successor"
    );
    assert_eq!(
        business(&pool, pipeline).await,
        before,
        "original intent/trigger/jobs/queue/attempts/leases remain byte-equivalent"
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; real HTTP/PG, no runner effects"]
async fn renewal_pg_http_handoff_racing_keys_and_lost_ack() {
    if std::env::var_os(CHILD).is_some() {
        run_child().await;
        return;
    }
    let url = std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable PG required");
    let options = PgConnectOptions::from_str(&url).unwrap();
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("forge_test_"))
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let schema = format!("it_renewal_{}", Uuid::new_v4().simple());
    let (original, _, _, _, live) = super::tests::fixture();
    let auth_original = original.clone();
    let tracker_path = format!(
        "/api/v1/issues/{}/sdlc/lifecycle",
        original.context.intent.binding.task_id
    );
    let router = Router::new()
        .route("/oidc/jwks", get(|| async { Json(json!({"keys":[]})) }))
        .route("/auth/tokens/introspect", get(move |uri: Uri, headers: HeaderMap| {
            let original = auth_original.clone(); async move {
                let bearer = headers.get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
                let scopes = if bearer == format!("Bearer {}", token('r')) {
                    json!(["ci-cd:read","ci-cd:write"])
                } else if ['b','c','d'].iter().any(|c| bearer == format!("Bearer {}", token(*c))) {
                    json!(["ci-cd:read","ci-cd:write",original.context.grant().unwrap()])
                } else { return Err(StatusCode::UNAUTHORIZED); };
                let mut metadata = json!({"sub":original.subject,"email":"fixture@example.invalid",
                    "display_name":"Synthetic renewal P","scopes":scopes});
                if uri.query() == Some("include_lifetime=true") {
                    let now = Utc::now(); metadata["observed_at"] = json!(now);
                    metadata["expires_at"] = json!(now + chrono::Duration::seconds(600));
                } else { assert!(uri.query().is_none()); }
                Ok(Json(metadata))
            }
        }))
        .route(&tracker_path, get(move |headers: HeaderMap| {
            let mut value = live.clone(); async move {
                assert_eq!(headers["authorization"], "Bearer synthetic-tracker-reader");
                let now = Utc::now(); value["observed_at"] = json!(now);
                for name in ["claimed_at","heartbeat_at"] { value["receipt"]["lease"][name] = json!(now); }
                value["receipt"]["lease"]["expires_at"] = json!(now + chrono::Duration::seconds(30));
                Json(value)
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    // Existing SDK OnceCell isolation pattern: only the child receives synthetic
    // Auth/Tracker origins. Parent/process-wide production config is never edited.
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", SELECTOR, "--ignored", "--nocapture"])
        .env(CHILD, "1")
        .env("FORGE_RENEWAL_SCHEMA", &schema)
        .env(
            "FORGE_RENEWAL_FIXTURE",
            json!({"original":original.value().unwrap()}).to_string(),
        )
        .env("FORGE_RENEWAL_PEER", &peer)
        .env("CICD_AUTH__CENTRAL_JWKS_URI", format!("{peer}/oidc/jwks"))
        .env("CICD_AUTH__CENTRAL_TIMEOUT_SECS", "5")
        .env_remove("CICD_NAMESPACE__OWNER_SUBJECTS")
        .env_remove("CICD_NAMESPACE__READER_SUBJECTS")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .kill_on_drop(true);
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(120), command.output()).await;
    server.abort();
    // The child owns every connection, migration and committed fixture row in
    // this schema. Drop it even when the child reports an assertion failure.
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let output = output.expect("bounded renewal PG child").unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
