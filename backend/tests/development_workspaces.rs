//! PostgreSQL instance/session/original-key invariants on disposable databases.
#![cfg(feature = "integration")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use cicd_domain::development_workspace::WorkspaceAction;
use cicd_infra::development_workspace as store;
use serde_json::json;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::str::FromStr;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    pool: PgPool,
    admin: PgPool,
    database: String,
    workspace: Uuid,
    other_workspace: Uuid,
    project: Uuid,
    environment: Uuid,
    actor: Uuid,
    other_actor: Uuid,
}

async fn http(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn public_api_preserves_original_commands_across_restart_and_revokes_project_access() {
    let f = Fixture::new().await;
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'developer'),($1,$3,'viewer')")
        .bind(f.project).bind(f.actor).bind(f.other_actor).execute(&f.pool).await.unwrap();
    for actor in [f.actor, f.other_actor] {
        sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
            .bind(actor)
            .bind(cicd::auth::hash_password("WorkspaceFixturePass1!").unwrap())
            .execute(&f.pool)
            .await
            .unwrap();
    }
    let config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some(format!("workspace-fixture-{}", f.workspace)));
    let app = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        config.git.to_git_config(),
        None,
        config.clone(),
    )
    .unwrap();
    let mut tokens = Vec::new();
    for actor in [f.actor, f.other_actor] {
        let (status, result) = http(
            &app,
            "POST",
            "/api/v1/auth/login",
            None,
            json!({"username":actor.to_string(),"password":"WorkspaceFixturePass1!"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        tokens.push(result["access_token"].as_str().unwrap().to_owned());
    }
    let path = format!("/api/v1/development-workspaces/{}", f.workspace);
    sqlx::query("UPDATE users SET username='workspace-developer' WHERE id=$1")
        .bind(f.actor)
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, card) = http(&app, "GET", &path, Some(&tokens[1]), json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["workspace"]["owner_name"], "workspace-developer");
    for token in &tokens {
        let (status, _) = http(&app,"POST",&format!("/api/v1/projects/{}/development-workspaces",f.project),Some(token),
            json!({"id":Uuid::new_v4(),"operation_id":Uuid::new_v4(),"catalog_target_id":Uuid::new_v4(),"name":"No creator permission","create":false})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,'workspace-fixture-machine')")
        .bind(account)
        .execute(&f.pool)
        .await
        .unwrap();
    let machine = format!(
        "forge_sat_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes,expires_at) VALUES($1,'workspace-fixture',$2,'synthetic-only','service_account',$3,$4,$5,now()+interval '1 day')")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&machine)).bind(account).bind(f.project).bind(vec!["api:read","api:write"]).execute(&f.pool).await.unwrap();
    assert_eq!(
        http(
            &app,
            "GET",
            &format!("/api/v1/projects/{}", f.project),
            Some(&machine),
            json!(null)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        http(&app, "GET", &path, Some(&machine), json!(null))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(&app, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .0,
        StatusCode::OK
    );
    let operation = Uuid::new_v4();
    let request = json!({"operation_id":operation,"expected_version":1,"command":{"action":"begin_session","task":"DEV-1"}});
    assert_eq!(
        http(
            &app,
            "POST",
            &format!("{path}/operations"),
            Some(&tokens[1]),
            request.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, original) = http(
        &app,
        "POST",
        &format!("{path}/operations"),
        Some(&tokens[0]),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{original}");
    assert_eq!(original["id"], operation.to_string());
    assert_eq!(original["state"], "queued");
    assert!(original.get("execution_plan").is_none());
    assert_eq!(f.version().await, 2);
    let restarted = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    let (status, replay) = http(
        &restarted,
        "POST",
        &format!("{path}/operations"),
        Some(&tokens[0]),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, original);
    assert_eq!(f.version().await, 2);
    let mut changed = request;
    changed["command"]["task"] = json!("DEV-2");
    assert_eq!(
        http(
            &restarted,
            "POST",
            &format!("{path}/operations"),
            Some(&tokens[0]),
            changed
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, readback) = http(
        &restarted,
        "GET",
        &format!("{path}/operations/{operation}"),
        Some(&tokens[0]),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(readback, original);
    assert_eq!(http(&restarted,"POST",&format!("{path}/operations"),Some(&tokens[0]),
        json!({"operation_id":Uuid::new_v4(),"expected_version":2,"command":{"action":"stop","force":true}})).await.0,StatusCode::UNPROCESSABLE_ENTITY);
    sqlx::query("DELETE FROM project_memberships WHERE project_id=$1 AND user_id=$2")
        .bind(f.project)
        .bind(f.actor)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        http(&restarted, "GET", &path, Some(&tokens[0]), json!(null))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(&restarted, "GET", &path, Some(&tokens[1]), json!(null))
            .await
            .0,
        StatusCode::OK
    );
    drop(app);
    drop(restarted);
    f.close().await;
}
impl Fixture {
    async fn new() -> Self {
        let url =
            std::env::var("CICD_TEST_DATABASE_URL").expect("explicit disposable database required");
        let options = PgConnectOptions::from_str(&url).unwrap();
        assert!(
            options
                .get_database()
                .is_some_and(|name| name.starts_with("forge_test_"))
        );
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let database = format!("forge_test_development_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await
            .unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options.database(&database))
            .await
            .unwrap();
        cicd::migrations().await.unwrap().run(&pool).await.unwrap();
        let actor = Uuid::new_v4();
        let other_actor = Uuid::new_v4();
        for id in [actor, other_actor] {
            sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'developer')")
                .bind(id)
                .bind(id.to_string())
                .execute(&pool)
                .await
                .unwrap();
        }
        let repository = Uuid::new_v4();
        sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
            .bind(repository)
            .bind(format!("source-{}", repository.simple()))
            .execute(&pool)
            .await
            .unwrap();
        let project = Uuid::new_v4();
        let tracker_instance = Uuid::new_v4();
        let tracker_project = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Instance fixture','https://example.invalid/source.git',$2)").bind(project).bind(repository).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,1,'Product','DEV','active',clock_timestamp())")
            .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(tracker_instance).bind(tracker_project).execute(&pool).await.unwrap();
        let workspace = Uuid::new_v4();
        let other_workspace = Uuid::new_v4();
        let environment = Uuid::new_v4();
        for (id, environment) in [(workspace, environment), (other_workspace, Uuid::new_v4())] {
            sqlx::query("INSERT INTO environments(id,project_id,name) VALUES($1,$2,$3)")
                .bind(environment)
                .bind(project)
                .bind(id.to_string())
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO development_workspaces(id,project_id,repository_id,environment_id,environment_hash,tracker_instance_id,tracker_project_id,name,profile_id,target_id,catalog_target_id,profile_hash,catalog_hash,profile,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$5,$5,'{}',$12)")
                .bind(id).bind(project).bind(repository).bind(environment).bind("a".repeat(64)).bind(tracker_instance).bind(tracker_project).bind(id.to_string()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(actor).execute(&pool).await.unwrap();
        }
        Self {
            pool,
            admin,
            database,
            workspace,
            other_workspace,
            project,
            environment,
            actor,
            other_actor,
        }
    }
    async fn version(&self) -> i64 {
        sqlx::query_scalar("SELECT version FROM development_workspaces WHERE id=$1")
            .bind(self.workspace)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
    async fn enqueue(
        &self,
        id: Uuid,
        actor: Uuid,
        version: i64,
        action: WorkspaceAction,
    ) -> Result<store::Operation, store::Error> {
        let mut tx = self.pool.begin().await?;
        let operation = store::enqueue(
            &mut tx,
            self.workspace,
            id,
            actor,
            actor,
            version,
            &action,
            &json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(operation)
    }
    async fn close(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

#[tokio::test]
async fn concurrent_commands_keep_one_original_key_and_immutable_result() {
    let f = Fixture::new().await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (a, b) = tokio::join!(
        f.enqueue(first, f.actor, 1, WorkspaceAction::Start {}),
        f.enqueue(second, f.actor, 1, WorkspaceAction::Stop {})
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let original = a.or(b).unwrap();
    let command: WorkspaceAction = serde_json::from_value(original.request.clone()).unwrap();
    let replay = f
        .enqueue(original.id, f.actor, 1, command.clone())
        .await
        .unwrap();
    assert_eq!(replay.id, original.id);
    assert_eq!(f.version().await, 2);
    assert!(matches!(
        f.enqueue(original.id, f.other_actor, 1, command).await,
        Err(store::Error::Conflict("operation_key_payload_conflict"))
    ));
    assert!(matches!(
        f.enqueue(original.id, f.actor, 1, WorkspaceAction::Observe {})
            .await,
        Err(store::Error::Conflict("operation_key_payload_conflict"))
    ));
    sqlx::query(
        "UPDATE development_workspace_operations SET state='unknown',result=$2 WHERE id=$1",
    )
    .bind(original.id)
    .bind(json!({"state":"unknown"}))
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), f.actor, 2, WorkspaceAction::Stop {})
            .await,
        Err(store::Error::Conflict("workspace_operation_in_progress"))
    ));
    sqlx::query("UPDATE development_workspace_operations SET state='succeeded',finished_at=clock_timestamp(),result=$2 WHERE id=$1").bind(original.id).bind(json!({"state":"succeeded"})).execute(&f.pool).await.unwrap();
    assert!(
        sqlx::query("UPDATE development_workspace_operations SET result='{}' WHERE id=$1")
            .bind(original.id)
            .execute(&f.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM development_workspace_operations WHERE id=$1")
            .bind(original.id)
            .execute(&f.pool)
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn session_writer_is_transactional_and_stop_never_ends_the_session() {
    let f = Fixture::new().await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let insert = |id, actor| {
        sqlx::query("INSERT INTO development_workspace_sessions(id,workspace_id,actor_id,owner_subject,task_id,task_ref) VALUES($1,$2,$3,$3,$4,'DEV-1')").bind(id).bind(f.workspace).bind(actor).bind(Uuid::new_v4()).execute(&f.pool)
    };
    let (a, b) = tokio::join!(insert(first, f.actor), insert(second, f.other_actor));
    assert_ne!(a.is_ok(), b.is_ok());
    let session = store::session(&f.pool, f.workspace).await.unwrap().unwrap();
    assert!(matches!(
        f.enqueue(
            Uuid::new_v4(),
            session.actor_id,
            1,
            WorkspaceAction::Stop {}
        )
        .await,
        Err(store::Error::Conflict("workspace_has_active_session"))
    ));
    let other = if session.actor_id == f.actor {
        f.other_actor
    } else {
        f.actor
    };
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), other, 1, WorkspaceAction::Start {})
            .await,
        Err(store::Error::Conflict(
            "workspace_session_owned_by_another_actor"
        ))
    ));
    assert!(
        store::session(&f.pool, f.workspace)
            .await
            .unwrap()
            .unwrap()
            .ended_at
            .is_none()
    );
    assert!(
        f.enqueue(
            Uuid::new_v4(),
            session.actor_id,
            1,
            WorkspaceAction::Start {}
        )
        .await
        .is_ok()
    );
    f.close().await;
}

#[tokio::test]
async fn native_delivery_hold_blocks_only_its_instance_until_confirmed() {
    let f = Fixture::new().await;
    let pipeline = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','failed')",
    )
    .bind(pipeline)
    .bind(f.project)
    .execute(&f.pool)
    .await
    .unwrap();
    let deployment = Uuid::new_v4();
    sqlx::query("INSERT INTO deployments(id,environment_id,pipeline_id,git_ref,status,runtime_phase) VALUES($1,$2,$3,'main','failed','held')").bind(deployment).bind(f.environment).bind(pipeline).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    assert!(store::workspace_busy(&mut tx, f.workspace).await.unwrap());
    assert!(
        !store::workspace_busy(&mut tx, f.other_workspace)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), f.actor, 1, WorkspaceAction::Stop {})
            .await,
        Err(store::Error::Conflict("workspace_has_active_pipeline"))
    ));
    sqlx::query("UPDATE deployments SET runtime_phase='finished' WHERE id=$1")
        .bind(deployment)
        .execute(&f.pool)
        .await
        .unwrap();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let lease = Uuid::new_v4();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'native',0,'success')").bind(stage).bind(pipeline).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'native','fixture','true',0,'success')").bind(job).bind(stage).execute(&f.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'success')",
    )
    .bind(attempt)
    .bind(job)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO runners(id,name) VALUES($1,'owned-runner')")
        .bind(runner)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_id,runner_name,lease_status,generation,lease_expires_at,completed_at,terminal_status,acknowledged_at) VALUES($1,$2,$3,$4,'owned-runner','completed',1,clock_timestamp(),clock_timestamp(),'success',clock_timestamp())")
        .bind(lease).bind(job).bind(attempt).bind(runner).execute(&f.pool).await.unwrap();
    assert!(matches!(
        f.enqueue(Uuid::new_v4(), f.actor, 1, WorkspaceAction::Stop {})
            .await,
        Err(store::Error::Conflict("workspace_has_active_pipeline"))
    ));
    sqlx::query("UPDATE job_leases SET completion_received_at=clock_timestamp() WHERE id=$1")
        .bind(lease)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        f.enqueue(Uuid::new_v4(), f.actor, 1, WorkspaceAction::Stop {})
            .await
            .is_ok()
    );
    f.close().await;
}
