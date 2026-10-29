//! Real owner storage qualification in a dedicated PostgreSQL schema.
#![cfg(feature = "integration")]
use cicd::store::identity_deletion::{IdentityDeletionStore, OwnerError};
use pdlc_shared::{identity_lifecycle::DeletionRequest, space_access::CentralIdentity};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;
const ISSUER: &str = "https://auth.example.test";
fn window() -> cicd::store::identity_deletion::IdentityDeletionWindow {
    cicd::store::identity_deletion::IdentityDeletionWindow::restricted(
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        chrono::Utc::now().timestamp() + 60,
    )
    .unwrap()
}
fn expired_window() -> cicd::store::identity_deletion::IdentityDeletionWindow {
    cicd::store::identity_deletion::IdentityDeletionWindow::restricted(
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        chrono::Utc::now().timestamp() - 1,
    )
    .unwrap()
}

fn request() -> DeletionRequest {
    DeletionRequest {
        schema_version: 1,
        operation_id: Uuid::new_v4(),
        inventory_revision: Uuid::new_v4(),
        identity: CentralIdentity {
            issuer: ISSUER.into(),
            subject: Uuid::new_v4().to_string(),
        },
    }
}
async fn linked_user(pool: &PgPool, subject: &str, username: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,$2,'developer',$3)")
        .bind(id)
        .bind(username)
        .bind(subject)
        .execute(pool)
        .await
        .unwrap();
    id
}
async fn verify_canonical_human_owner_sources(pool: &PgPool, store: &IdentityDeletionStore) {
    let target = request();
    let subject = Uuid::parse_str(&target.identity.subject).unwrap();
    let project = Uuid::new_v4();
    let pipeline = Uuid::new_v4();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let schedule = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://example.test/synthetic.git')")
        .bind(project).bind(format!("owner-fixture-{project}")).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','queued')",
    )
    .bind(pipeline)
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'fixture',0,'queued')")
        .bind(stage).bind(pipeline).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'fixture','example.invalid/test','true',0,'queued')")
        .bind(job).bind(stage).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'queued')",
    )
    .bind(attempt)
    .bind(job)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO pipeline_human_owners(pipeline_id,owner_subject) VALUES($1,$2)")
        .bind(pipeline)
        .bind(subject)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO execution_attempt_human_owners(attempt_id,owner_subject,requires_admin) VALUES($1,$2,false)")
        .bind(attempt).bind(subject).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO schedules(id,project_id,cron,git_ref,owner_subject) VALUES($1,$2,'0 0 * * *','main',$3)")
        .bind(schedule).bind(project).bind(subject).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO schedule_fires(id,schedule_id,project_id,scheduled_for,owner_subject) VALUES($1,$2,$3,clock_timestamp(),$4)")
        .bind(Uuid::new_v4()).bind(schedule).bind(project).bind(subject).execute(pool).await.unwrap();
    let repository = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("sync-owner-fixture-{repository}"))
        .execute(pool)
        .await
        .unwrap();
    let settings = serde_json::json!({"provider":"github","instance_url":"https://github.com","repository_path":"fixture/owner","direction":"pull","automatic":false});
    sqlx::query("INSERT INTO repository_sync_connections(repository_id,settings,version,encrypted_credential,owner_subject) VALUES($1,$2,1,'synthetic-ciphertext',$3)").bind(repository).bind(&settings).bind(subject).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state,owner_subject) VALUES($1,$2,1,$3,'manual','queued',$4)").bind(Uuid::new_v4()).bind(repository).bind(settings).bind(subject).execute(pool).await.unwrap();
    assert_eq!(
        store
            .check(&target, &window())
            .await
            .unwrap()
            .reference_count,
        6
    );
    assert_eq!(
        store
            .freeze(&target, &window())
            .await
            .unwrap()
            .reference_count,
        6
    );
    for table in [
        "pipeline_human_owners",
        "execution_attempt_human_owners",
        "schedules",
        "schedule_fires",
        "repository_sync_connections",
        "repository_sync_runs",
    ] {
        let error = sqlx::query(&format!(
            "UPDATE {table} SET owner_subject=owner_subject WHERE owner_subject=$1"
        ))
        .bind(subject)
        .execute(pool)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_deletion_fenced",
            "{table}"
        );
    }
    // NULL is the released non-human scheduler contract, not an inferred UUID.
    sqlx::query("INSERT INTO schedules(id,project_id,cron,git_ref,owner_subject) VALUES($1,$2,'0 0 * * *','main',NULL)")
        .bind(Uuid::new_v4()).bind(project).execute(pool).await.unwrap();
    store.release(&target, &window()).await.unwrap();
    // Released configuration history remains protected independently of an
    // identity fence. Releasing the fence permits writers, never history loss.
    let error = sqlx::query("DELETE FROM projects WHERE id=$1")
        .bind(project)
        .execute(pool)
        .await
        .unwrap_err();
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("42501"));
    assert_eq!(error.message(), "delivery_configuration_history_protected");
    for table in [
        "pipeline_human_owners",
        "execution_attempt_human_owners",
        "schedules",
        "schedule_fires",
        "repository_sync_connections",
        "repository_sync_runs",
    ] {
        let updated = sqlx::query(&format!(
            "UPDATE {table} SET owner_subject=owner_subject WHERE owner_subject=$1"
        ))
        .bind(subject)
        .execute(pool)
        .await
        .unwrap()
        .rows_affected();
        assert_eq!(updated, 1, "{table}");
    }
    assert_eq!(
        store
            .check(&target, &window())
            .await
            .unwrap()
            .reference_count,
        6
    );
}

async fn verify(pool: &PgPool) {
    let store = IdentityDeletionStore::new(pool.clone());
    let target = request();
    assert!(matches!(
        store.check(&target, &window()).await,
        Err(OwnerError::Unbound)
    ));
    store.bind_legacy_issuer(ISSUER).await.unwrap();
    store.bind_legacy_issuer(ISSUER).await.unwrap();
    assert!(matches!(
        store.bind_legacy_issuer("https://other.example.test").await,
        Err(OwnerError::Conflict)
    ));
    verify_malformed_refs(pool, &store).await;
    verify_canonical_human_owner_sources(pool, &store).await;
    // Old freeform provenance is explicitly unknown; an operator assertion is durable.
    sqlx::query("INSERT INTO audit_log(action,resource_type,actor) VALUES('historical','fixture','unclassified-old-actor')")
        .execute(pool).await.unwrap();
    assert!(matches!(
        store.check(&target, &window()).await,
        Err(OwnerError::Unresolved)
    ));
    assert!(matches!(
        store.freeze(&target, &window()).await,
        Err(OwnerError::Unresolved)
    ));
    sqlx::query("INSERT INTO identity_legacy_aliases(kind,alias,disposition,assertion_reference) VALUES('legacy_actor','unclassified-old-actor','non_identity','fixture-explicit-operator-assertion')")
        .execute(pool).await.unwrap();
    assert_eq!(
        store
            .check(&target, &window())
            .await
            .unwrap()
            .reference_count,
        0
    );
    let first = store.freeze(&target, &window()).await.unwrap();
    assert_eq!(first.reference_count, 0);
    assert_eq!(first, store.freeze(&target, &window()).await.unwrap());
    let error =
        sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,$2,'developer',$3)")
            .bind(Uuid::new_v4())
            .bind(format!("blocked-{}", Uuid::new_v4().simple()))
            .bind(&target.identity.subject)
            .execute(pool)
            .await
            .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_deletion_fenced"
    );
    // A finalized identity keeps its fence. Other supported human/machine writes continue.
    let active = request();
    let user = linked_user(pool, &active.identity.subject, "known-current-user").await;
    let refs = serde_json::json!([{"kind":"local_user_id","alias":user}]);
    sqlx::query("INSERT INTO audit_log(action,resource_type,actor,identity_refs) VALUES('current','fixture','display-metadata',$1)")
        .bind(&refs).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('worker','fixture','[]')",
    )
    .execute(pool)
    .await
    .unwrap();
    let error=sqlx::query("INSERT INTO audit_log(action,resource_type,actor) VALUES('raw','fixture','new-untyped-actor')")
        .execute(pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_legacy_reference_unresolved"
    );
    assert!(sqlx::query("UPDATE identity_legacy_aliases SET disposition='non_identity',issuer=NULL,subject=NULL WHERE kind='local_user_id' AND alias=$1")
        .bind(user.to_string()).execute(pool).await.is_err());

    // Exact username aliases survive rename/deletion, without email or UUID-shaped actor guesses.
    let repo = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,'identity-fixture')")
        .bind(repo)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,created_by) VALUES($1,'identity-fixture',1,'Fixture','feature','main','known-current-user')")
        .bind(Uuid::new_v4()).execute(pool).await.unwrap();
    sqlx::query("UPDATE users SET username='renamed-current-user' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    let error = sqlx::query("UPDATE users SET central_sub=$2 WHERE id=$1")
        .bind(user)
        .bind(Uuid::new_v4().to_string())
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_alias_conflict"
    );
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    let used = store.freeze(&active, &window()).await.unwrap();
    assert_eq!(
        used.reference_count, 2,
        "PR username and typed audit retain the old identity after local deletion"
    );
    store.bind_legacy_issuer(ISSUER).await.unwrap();
    let error=sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,created_by) VALUES($1,'identity-fixture',2,'Blocked','another','main','known-current-user')")
        .bind(Uuid::new_v4()).execute(pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_deletion_fenced"
    );
    verify_sources(pool, &active, user).await;

    // A worker lease UUID is not a user identity. Machine principals are explicitly classified.
    let machine = Uuid::new_v4();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'fixture-machine')")
        .bind(machine)
        .execute(pool)
        .await
        .unwrap();
    let unknown:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM forge_identity_alias('local_principal_id',$1) WHERE unresolved)")
        .bind(machine.to_string()).fetch_one(pool).await.unwrap();
    assert!(!unknown);
    sqlx::query("INSERT INTO audit_log(action,resource_type,actor,identity_refs) VALUES('machine','fixture','machine-display',$1)")
        .bind(serde_json::json!([{"kind":"local_principal_id","alias":machine}])).execute(pool).await.unwrap();
    verify_machine_denied_audit(pool, &store, machine).await;

    // Cancellation leaves a tombstone before any delayed freeze can acquire its lock.
    let cancelled = request();
    store.release(&cancelled, &window()).await.unwrap();
    store.release(&cancelled, &window()).await.unwrap();
    assert!(matches!(
        store.freeze(&cancelled, &window()).await,
        Err(OwnerError::Conflict)
    ));
    let mut conflict = target.clone();
    conflict.inventory_revision = Uuid::new_v4();
    assert!(matches!(
        store.release(&conflict, &window()).await,
        Err(OwnerError::Conflict)
    ));
    let mut foreign = target.clone();
    foreign.identity.issuer = "https://other.example.test".into();
    assert!(matches!(
        store.check(&foreign, &window()).await,
        Err(OwnerError::Unbound)
    ));
    assert!(matches!(
        store.freeze(&target, &expired_window()).await,
        Err(OwnerError::Expired)
    ));
    store.release(&target, &window()).await.unwrap();
    assert!(matches!(
        store.freeze(&target, &window()).await,
        Err(OwnerError::Conflict)
    ));
    verify_commit_deadlines(pool, &store).await;
    verify_races(pool, &store).await;
    let mut blocking = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock_shared(714402216002::bigint)")
        .execute(&mut *blocking)
        .await
        .unwrap();
    let bounded = tokio::time::timeout(
        std::time::Duration::from_secs(12),
        store.check(&request(), &window()),
    )
    .await
    .expect("owner lock wait is bounded");
    assert!(
        matches!(bounded, Err(OwnerError::Storage)),
        "timeout must never become a zero-reference receipt"
    );
    blocking.rollback().await.unwrap();
    sqlx::query("DELETE FROM identity_fence_epoch WHERE singleton")
        .execute(pool)
        .await
        .unwrap();
    assert!(matches!(
        store.check(&request(), &window()).await,
        Err(OwnerError::Storage)
    ));
    let error = sqlx::query(
        "INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('worker','fixture','[]')",
    )
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_fence_epoch_missing"
    );
}
async fn verify_malformed_refs(pool: &PgPool, store: &IdentityDeletionStore) {
    let legacy: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE username='identity-legacy-blank-subject'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        legacy, 1,
        "required scalar malformed before migration must remain visible"
    );
    let target = request();
    assert!(matches!(
        store.check(&target, &window()).await,
        Err(OwnerError::Unresolved)
    ));
    assert!(matches!(
        store.freeze(&target, &window()).await,
        Err(OwnerError::Unresolved)
    ));
    sqlx::query("DELETE FROM users WHERE username='identity-legacy-blank-subject'")
        .execute(pool)
        .await
        .unwrap();
    let cases = [
        serde_json::json!([{"kind":"central","issuer":17,"subject":target.identity.subject}]),
        serde_json::json!([{"kind":"central","issuer":null,"subject":target.identity.subject}]),
        serde_json::json!([{"kind":"central","issuer":"  ","subject":target.identity.subject}]),
        serde_json::json!([{"kind":"central","issuer":ISSUER,"subject":17}]),
        serde_json::json!([{"kind":"central","issuer":ISSUER,"subject":null}]),
        serde_json::json!([{"kind":"central","issuer":ISSUER,"subject":" "}]),
        serde_json::json!([{"kind":"local_user_id","alias":" "}]),
        serde_json::json!([{"kind":"local_user_id","alias":17}]),
    ];
    for refs in &cases {
        let id: i64 = sqlx::query_scalar("INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('fixture.malformed','fixture',$1) RETURNING id")
            .bind(refs).fetch_one(pool).await.unwrap();
        assert!(matches!(
            store.check(&target, &window()).await,
            Err(OwnerError::Unresolved)
        ));
        assert!(matches!(
            store.freeze(&target, &window()).await,
            Err(OwnerError::Unresolved)
        ));
        sqlx::query("DELETE FROM audit_log WHERE id=$1")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    let nonarray = serde_json::json!({"kind":"central","subject":target.identity.subject});
    let unknown: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM forge_identity_typed_refs($1) WHERE unresolved)",
    )
    .bind(&nonarray)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(unknown);
    let schema_rejects = sqlx::query("INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('fixture.invalid_shape','fixture',$1)")
        .bind(&nonarray).execute(pool).await.unwrap_err();
    assert_eq!(
        schema_rejects
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    assert_eq!(
        store
            .freeze(&target, &window())
            .await
            .unwrap()
            .reference_count,
        0
    );
    let guarded = sqlx::query("INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('fixture.denied_shape','fixture',$1)")
        .bind(&nonarray).execute(pool).await.unwrap_err();
    assert_eq!(
        guarded.as_database_error().unwrap().message(),
        "identity_legacy_reference_unresolved"
    );
    for refs in &cases {
        let error = sqlx::query("INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('fixture.denied_malformed','fixture',$1)")
            .bind(refs).execute(pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_legacy_reference_unresolved"
        );
    }
    for subject in ["", "  "] {
        let error = sqlx::query(
            "INSERT INTO users(id,username,role,central_sub) VALUES($1,$2,'developer',$3)",
        )
        .bind(Uuid::new_v4())
        .bind(format!("malformed-subject-{}", Uuid::new_v4().simple()))
        .bind(subject)
        .execute(pool)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_legacy_reference_unresolved"
        );
    }
    // SQL NULL on the optional actor remains a supported system event.
    sqlx::query("INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('fixture.worker','fixture','[]')")
        .execute(pool).await.unwrap();
    let error = sqlx::query("INSERT INTO audit_log(action,resource_type,actor) VALUES('fixture.blank_actor','fixture','  ')")
        .execute(pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_legacy_reference_unresolved"
    );
    // Missing central issuer uses the explicit deployment binding; explicit null did not.
    let exact: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM forge_identity_typed_refs($1) WHERE NOT unresolved AND issuer=$2 AND subject=$3)")
        .bind(serde_json::json!([{"kind":"central","subject":target.identity.subject}]))
        .bind(ISSUER).bind(&target.identity.subject).fetch_one(pool).await.unwrap();
    assert!(exact);
    store.release(&target, &window()).await.unwrap();
}

async fn verify_machine_denied_audit(pool: &PgPool, store: &IdentityDeletionStore, machine: Uuid) {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let token = "forge_sat_fixture_identity_denied";
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,scopes) VALUES($1,'fixture-token',$2,'fixture','service_account',$3,$4)")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(token)).bind(machine).bind(vec!["api:read".to_owned()]).execute(pool).await.unwrap();
    let app = cicd::api::app_with_auth_secret(
        Some(pool.clone()),
        Some("fixture-owner-local-hmac".into()),
    );
    // Authenticate the same native machine first, then deny its insufficient
    // role on an existing administration route rather than an unknown path.
    let accepted = app
        .clone()
        .oneshot(
            Request::get("/api/v1/auth/principal")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    let response = app
        .oneshot(
            Request::get("/api/v1/users")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let refs: serde_json::Value = sqlx::query_scalar(
        "SELECT identity_refs FROM audit_log WHERE action='auth.denied' AND resource_type='route' AND resource_id=$1 AND actor='GET /api/v1/users' ORDER BY id DESC LIMIT 1",
    )
    .bind(machine)
    .fetch_one(pool)
    .await
    .expect("denied machine request must not lose its audit under a permanent fence");
    assert_eq!(
        refs,
        serde_json::json!([{"kind":"local_principal_id","alias":machine}])
    );
    assert_eq!(
        store
            .check(&request(), &window())
            .await
            .unwrap()
            .reference_count,
        0
    );
}
async fn verify_sources(pool: &PgPool, target: &DeletionRequest, user: Uuid) {
    for (table, column) in [
        ("user_credentials", "user_id"),
        ("sessions", "user_id"),
        ("api_tokens", "user_id"),
        ("project_memberships", "user_id"),
        ("tenant_memberships", "user_id"),
        ("notification_preferences", "user_id"),
        ("forge_confirmed_user_roles", "user_id"),
        ("tenants", "created_by"),
        ("pull_task_links", "created_by_user_id"),
        ("repository_attach_operations", "actor_id"),
        ("delivery_configuration_operations", "actor_id"),
        ("repository_sync_operations", "actor_id"),
    ] {
        let error = sqlx::query(&format!("INSERT INTO {table}({column}) VALUES($1)"))
            .bind(user)
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_deletion_fenced",
            "{table}"
        );
    }
    let error = sqlx::query(
        "INSERT INTO service_accounts(id,name,created_by) VALUES($1,'blocked-machine-owner',$2)",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_deletion_fenced"
    );
    for table in ["audit_log", "releases", "deployment_approvals"] {
        let error = sqlx::query(&format!("INSERT INTO {table}(identity_refs) VALUES($1)"))
            .bind(serde_json::json!([{"kind":"local_user_id","alias":user}]))
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_deletion_fenced",
            "{table}"
        );
    }
    let error = sqlx::query(
        "INSERT INTO forge_space_owner_operations(owner_issuer,owner_subject) VALUES($1,$2)",
    )
    .bind(ISSUER)
    .bind(&target.identity.subject)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_deletion_fenced"
    );
    for (table, column, value) in [
        (
            "task_pipeline_intents",
            "delegated_credential",
            serde_json::json!({"subject":target.identity.subject}),
        ),
        (
            "task_credential_successors",
            "credential",
            serde_json::json!({"subject":target.identity.subject}),
        ),
    ] {
        let error = sqlx::query(&format!("INSERT INTO {table}({column}) VALUES($1)"))
            .bind(value)
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "identity_deletion_fenced",
            "{table}"
        );
    }
    let error = sqlx::query("INSERT INTO sdlc_workspace_operations(delegated_subject) VALUES($1)")
        .bind(Uuid::parse_str(&target.identity.subject).unwrap())
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "identity_deletion_fenced"
    );
}
async fn verify_races(pool: &PgPool, store: &IdentityDeletionStore) {
    let target = request();
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,$2,'developer',$3)")
        .bind(Uuid::new_v4())
        .bind(format!("racing-{}", Uuid::new_v4().simple()))
        .bind(&target.identity.subject)
        .execute(&mut *writer)
        .await
        .unwrap();
    let freeze = {
        let store = store.clone();
        let target = target.clone();
        tokio::spawn(async move { store.freeze(&target, &window()).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event='advisory' AND query LIKE '%714402216002%')")
            .fetch_one(pool).await.unwrap();if waiting {break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
    }).await.unwrap();
    writer.commit().await.unwrap();
    assert_eq!(freeze.await.unwrap().unwrap().reference_count, 1);
    let stale = request();
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query("SELECT * FROM identity_fence_epoch")
        .fetch_all(&mut *writer)
        .await
        .unwrap();
    store.freeze(&stale, &window()).await.unwrap();
    let error = sqlx::query(
        "INSERT INTO audit_log(action,resource_type,identity_refs) VALUES('worker','fixture','[]')",
    )
    .execute(&mut *writer)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("40001")
    );
    writer.rollback().await.unwrap();
}
#[tokio::test]
async fn owner_guards_legacy_aliases_all_sources_and_concurrent_writers() {
    let url = std::env::var("CICD_TEST_DATABASE_URL").expect("dedicated database URL");
    let admin = PgPool::connect(&url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(database, "forge_test_cicd", "refuse working DB");
    let schema = format!("identity_owner_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let configured = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .after_connect(move |conn, _| {
            let schema = configured.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {schema}"))
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let result = tokio::spawn({
        let pool = pool.clone();
        async move {
    let mut predecessor = cicd::migrations_from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"),
    )
    .await
    .unwrap();
    predecessor
        .migrations
        .to_mut()
        .retain(|migration| migration.version < 105);
    predecessor.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,'identity-legacy-blank-subject','developer','  ')")
        .bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    cicd::migrations_from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"),
    )
    .await
    .unwrap()
    .run(&pool)
    .await
    .unwrap();
            verify(&pool).await
        }
    })
    .await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn verify_commit_deadlines(pool: &PgPool, store: &IdentityDeletionStore) {
    // The barrier is reached only by a deferred trigger during COMMIT, after
    // every Rust pre-commit check and the durable journal write have run.
    for releasing in [false, true] {
        let command = request();
        let original = if releasing {
            Some(store.freeze(&command, &window()).await.unwrap())
        } else {
            None
        };
        sqlx::raw_sql("CREATE SEQUENCE qa_owner_commit_seen;
            CREATE SEQUENCE qa_owner_commit_until; CREATE SEQUENCE qa_owner_commit_pid; CREATE SEQUENCE qa_owner_commit_valid;
            CREATE FUNCTION qa_owner_commit_delay() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN PERFORM nextval('qa_owner_commit_seen');
            PERFORM setval('qa_owner_commit_until',floor(extract(epoch FROM NEW.valid_until)*1000000)::bigint,true);
            PERFORM setval('qa_owner_commit_pid',pg_backend_pid()::bigint,true);
            PERFORM setval('qa_owner_commit_valid',CASE WHEN clock_timestamp()<NEW.valid_until THEN 1 ELSE 2 END,true);
            PERFORM pg_advisory_xact_lock(714402216102::bigint); RETURN NEW; END $$;
            CREATE CONSTRAINT TRIGGER a_qa_owner_commit_delay AFTER INSERT ON forge_request_commit_fences
            DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_owner_commit_delay();")
            .execute(pool).await.unwrap();
        let mut barrier = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *barrier)
            .await
            .unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(714402216102::bigint)")
            .execute(&mut *barrier)
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let fixed = cicd::store::identity_deletion::IdentityDeletionWindow::restricted(
            deadline,
            chrono::Utc::now().timestamp() + 30,
        )
        .unwrap();
        let snapshot_sql = "SELECT jsonb_build_object('journal',(SELECT COALESCE(jsonb_agg(to_jsonb(o)),'[]'::jsonb) FROM identity_deletion_operations o WHERE operation_id=$1),'epoch',(SELECT to_jsonb(e) FROM identity_fence_epoch e WHERE singleton))";
        let before: serde_json::Value = sqlx::query_scalar(snapshot_sql)
            .bind(command.operation_id)
            .fetch_one(pool)
            .await
            .unwrap();
        let task = {
            let store = store.clone();
            let command = command.clone();
            tokio::spawn(async move {
                if releasing {
                    store.release(&command, &fixed).await.map(|_| ())
                } else {
                    store.freeze(&command, &fixed).await.map(|_| ())
                }
            })
        };
        let _abort = OwnerCommitAbort(task.abort_handle());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT is_called AND $1=ANY(pg_blocking_pids(last_value::int)) FROM qa_owner_commit_pid")
                    .bind(pid).fetch_one(pool).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("the exact owner backend must reach the real deferred COMMIT barrier");
        let entered: bool = sqlx::query_scalar("SELECT is_called FROM qa_owner_commit_seen")
            .fetch_one(pool)
            .await
            .unwrap();
        assert!(
            entered,
            "the test must not settle for a pre-commit rejection"
        );
        let valid_at_entry: i64 =
            sqlx::query_scalar("SELECT last_value FROM qa_owner_commit_valid")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(
            valid_at_entry, 1,
            "the actual DB commit cap must still be valid on deferred trigger entry"
        );
        let still_valid:bool=sqlx::query_scalar("SELECT clock_timestamp()<to_timestamp(last_value::double precision/1000000) FROM qa_owner_commit_until").fetch_one(pool).await.unwrap();
        assert!(
            still_valid,
            "the original DB deadline must be unexpired at the actual blocked COMMIT barrier"
        );
        tokio::time::timeout(std::time::Duration::from_secs(5),async {
            loop {
                let expired:bool=sqlx::query_scalar("SELECT clock_timestamp()>=to_timestamp(last_value::double precision/1000000) FROM qa_owner_commit_until").fetch_one(pool).await.unwrap();
                if expired { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("the original fixed DB cap must expire without renewal");
        barrier.commit().await.unwrap();
        assert!(
            matches!(task.await.unwrap(), Err(OwnerError::Expired)),
            "the original monotonic deadline must abort COMMIT"
        );
        let after: serde_json::Value = sqlx::query_scalar(snapshot_sql)
            .bind(command.operation_id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(
            after, before,
            "all operation fields and the epoch must roll back exactly"
        );
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM identity_deletion_operations WHERE operation_id=$1",
        )
        .bind(command.operation_id)
        .fetch_optional(pool)
        .await
        .unwrap();
        assert_eq!(
            state.as_deref(),
            if releasing { Some("frozen") } else { None },
            "expired COMMIT must roll back its journal/fence transition"
        );
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM forge_request_commit_fences")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "transient clock rows never accumulate");
        sqlx::raw_sql("DROP TRIGGER a_qa_owner_commit_delay ON forge_request_commit_fences; DROP FUNCTION qa_owner_commit_delay(); DROP SEQUENCE qa_owner_commit_seen; DROP SEQUENCE qa_owner_commit_until; DROP SEQUENCE qa_owner_commit_pid; DROP SEQUENCE qa_owner_commit_valid;")
            .execute(pool).await.unwrap();
        let fresh = store.freeze(&command, &window()).await.unwrap();
        if let Some(original) = original {
            assert_eq!(
                fresh, original,
                "expired release must retain the exact original freeze"
            );
        }
        store.release(&command, &window()).await.unwrap();
    }
}

struct OwnerCommitAbort(tokio::task::AbortHandle);
impl Drop for OwnerCommitAbort {
    fn drop(&mut self) {
        self.0.abort();
    }
}
