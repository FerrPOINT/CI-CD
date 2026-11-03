//! Manual native regression gate. Only an owned, disposable forge_space_test DB.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    pool: PgPool,
    control: PgPool,
    app: Router,
    tokens: Vec<String>,
    users: Vec<Uuid>,
    schema: String,
}
impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("FORGE_SHARED_SPACE_TEST_DATABASE_URL")
            .expect("owned fixture URL required");
        let control = PgPool::connect(&url).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT current_database()")
                .fetch_one(&control)
                .await
                .unwrap(),
            "forge_space_test"
        );
        let schema = format!("audit_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let own = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .after_connect(move |connection, _| {
                let own = own.clone();
                Box::pin(async move {
                    sqlx::query("SELECT set_config('search_path',$1,false),set_config('application_name',$1,false)")
                    .bind(own)
                    .execute(connection)
                    .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        cicd::migrations_from_path(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"),
        )
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
        let mut config = cicd::config::RuntimeConfig::test_default();
        config.auth.secret = Some("rust-audit-owned-fixture-secret-only".into());
        config.secrets.key = Some([7; 32]);
        let app = cicd::api::app_with_git_and_config(
            Some(pool.clone()),
            cicd::git_host::GitConfig::default(),
            None,
            config,
        )
        .unwrap();
        let mut tokens = Vec::new();
        let mut users = Vec::new();
        for index in 0..2 {
            let user = Uuid::new_v4();
            let username = format!("audit{index}");
            sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'admin')")
                .bind(user)
                .bind(&username)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
                .bind(user)
                .bind(cicd::auth::hash_password("synthetic-audit-fixture-only").unwrap())
                .execute(&pool)
                .await
                .unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/v1/auth/login")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({"username":username,"password":"synthetic-audit-fixture-only"})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            tokens.push(
                body(response).await["access_token"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            users.push(user);
        }
        Self {
            pool,
            control,
            app,
            tokens,
            users,
            schema,
        }
    }
    async fn request(
        &self,
        actor: usize,
        method: &str,
        path: &str,
        value: Value,
    ) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", format!("Bearer {}", self.tokens[actor]))
                    .header("content-type", "application/json")
                    .body(if method == "GET" || method == "DELETE" {
                        Body::empty()
                    } else {
                        Body::from(value.to_string())
                    })
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn project(&self) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects(id,name,repository_url,default_branch) VALUES($1,$2,'','main')",
        )
        .bind(id)
        .bind(format!("audit-{id}"))
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }
    async fn pipeline(&self, project: Uuid) -> (Uuid, Uuid, Uuid) {
        let response = self
            .request(
                0,
                "POST",
                &format!("/api/v1/projects/{project}/pipelines"),
                json!({"git_ref":"main"}),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let pipeline: Uuid = body(response).await["pipeline"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let (job,attempt):(Uuid,Uuid)=sqlx::query_as("SELECT j.id,a.id FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN execution_attempts a ON a.job_id=j.id WHERE s.pipeline_id=$1 ORDER BY s.position,j.position LIMIT 1").bind(pipeline).fetch_one(&self.pool).await.unwrap();
        (pipeline, job, attempt)
    }
    async fn close(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
    }
}
async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "manual owned PostgreSQL regression"]
async fn sensitive_mutation_and_verified_actor_audit_are_atomic() {
    let f = Fixture::new().await;
    let project = f.project().await;
    sqlx::query("ALTER TABLE audit_log ADD CONSTRAINT audit_injected_failure CHECK(action<>'secret.upserted')").execute(&f.pool).await.unwrap();
    let path = format!("/api/v1/projects/{project}/secrets");
    assert_eq!(
        f.request(0, "POST", &path, json!({"key":"SYNTHETIC","value":"first"}))
            .await
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_secrets")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("ALTER TABLE audit_log DROP CONSTRAINT audit_injected_failure")
        .execute(&f.pool)
        .await
        .unwrap();
    let mut secret = None;
    for actor in 0..2 {
        let response = f
            .request(
                actor,
                "POST",
                &path,
                json!({"key":"SYNTHETIC","value":format!("value-{actor}")}),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        secret = Some(body(response).await["id"].as_str().unwrap().to_owned());
    }
    let actors: Vec<String> = sqlx::query_scalar(
        "SELECT actor FROM audit_log WHERE action='secret.upserted' ORDER BY created_at,id",
    )
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(actors.len(), 2);
    for user in &f.users {
        assert!(actors.contains(&user.to_string()));
    }
    sqlx::query("ALTER TABLE audit_log ADD CONSTRAINT audit_injected_failure CHECK(action<>'secret.deleted')").execute(&f.pool).await.unwrap();
    let path = format!("/api/v1/secrets/{}", secret.unwrap());
    assert_eq!(
        f.request(0, "DELETE", &path, Value::Null).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_secrets")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query("ALTER TABLE audit_log DROP CONSTRAINT audit_injected_failure")
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.request(1, "DELETE", &path, Value::Null).await.status(),
        StatusCode::OK
    );
    let runner = Uuid::new_v4();
    sqlx::query("INSERT INTO runners(id,name,tags) VALUES($1,'audit-runner','{}')")
        .bind(runner)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE audit_log ADD CONSTRAINT audit_injected_failure CHECK(action<>'runner.deleted')").execute(&f.pool).await.unwrap();
    assert_eq!(
        f.request(
            0,
            "DELETE",
            &format!("/api/v1/runners/{runner}"),
            Value::Null
        )
        .await
        .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM runners WHERE id=$1)")
            .bind(runner)
            .fetch_one(&f.pool)
            .await
            .unwrap()
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "manual owned PostgreSQL regression"]
async fn manual_job_transition_serializes_and_rolls_back_with_its_outbox() {
    let f = Fixture::new().await;
    let project = f.project().await;
    let (pipeline, job, attempt) = f.pipeline(project).await;
    let path = format!("/api/v1/jobs/{job}/status");
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"running"}))
            .await
            .status(),
        StatusCode::OK
    );
    let (left, right) = tokio::join!(
        f.request(0, "POST", &path, json!({"status":"success"})),
        f.request(1, "POST", &path, json!({"status":"failed"}))
    );
    assert_eq!(
        usize::from(left.status() == StatusCode::OK)
            + usize::from(right.status() == StatusCode::OK),
        1
    );
    assert!(left.status() == StatusCode::CONFLICT || right.status() == StatusCode::CONFLICT);
    let (current,recorded):(String,String)=sqlx::query_as("SELECT j.status,a.status FROM jobs j JOIN execution_attempts a ON a.job_id=j.id WHERE j.id=$1 AND a.id=$2").bind(job).bind(attempt).fetch_one(&f.pool).await.unwrap();
    assert_eq!(current, recorded);
    let (_, job, attempt) = f.pipeline(project).await;
    let path = format!("/api/v1/jobs/{job}/status");
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"running"}))
            .await
            .status(),
        StatusCode::OK
    );
    let lease = Uuid::new_v4();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,lease_status,generation,lease_expires_at) VALUES($1,$2,$3,'embedded','active',1,now()+interval '1 hour')").bind(lease).bind(job).bind(attempt).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"success"}))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT lease_status FROM job_leases WHERE id=$1")
            .bind(lease)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        "active"
    );
    sqlx::query("UPDATE job_leases SET lease_status='canceled',terminal_status='canceled',completed_at=now() WHERE id=$1").bind(lease).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"success"}))
            .await
            .status(),
        StatusCode::CONFLICT,
        "lease cancellation alone is not a physical completion receipt"
    );
    let (other, job, attempt) = f.pipeline(project).await;
    let path = format!("/api/v1/jobs/{job}/status");
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"running"}))
            .await
            .status(),
        StatusCode::OK
    );
    sqlx::query("UPDATE jobs SET status='success' WHERE id<>$1 AND stage_id IN(SELECT id FROM stages WHERE pipeline_id=$2)").bind(job).bind(other).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE stages SET status='success' WHERE pipeline_id=$1 AND id<>(SELECT stage_id FROM jobs WHERE id=$2)").bind(other).bind(job).execute(&f.pool).await.unwrap();
    sqlx::query(
        "ALTER TABLE domain_events ADD CONSTRAINT outbox_injected_failure CHECK(false) NOT VALID",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"success"}))
            .await
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let (current,recorded):(String,String)=sqlx::query_as("SELECT j.status,a.status FROM jobs j JOIN execution_attempts a ON a.job_id=j.id WHERE j.id=$1 AND a.id=$2").bind(job).bind(attempt).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        (current.as_str(), recorded.as_str()),
        ("running", "running")
    );
    sqlx::query("ALTER TABLE domain_events DROP CONSTRAINT outbox_injected_failure")
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.request(0, "POST", &path, json!({"status":"success"}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id=$1")
            .bind(other)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        "success"
    );
    assert_ne!(pipeline, other);
    for _ in 0..3 {
        let (parent, job, attempt) = f.pipeline(project).await;
        let path = format!("/api/v1/jobs/{job}/status");
        assert_eq!(
            f.request(0, "POST", &path, json!({"status":"running"}))
                .await
                .status(),
            StatusCode::OK
        );
        let cancel_path = format!("/api/v1/pipelines/{parent}/cancel");
        let (manual, cancel) = tokio::join!(
            f.request(0, "POST", &path, json!({"status":"success"})),
            f.request(1, "POST", &cancel_path, Value::Null)
        );
        assert!(matches!(
            manual.status(),
            StatusCode::OK | StatusCode::CONFLICT
        ));
        assert_eq!(cancel.status(), StatusCode::OK);
        let (job_status,attempt_status):(String,String)=sqlx::query_as("SELECT j.status,a.status FROM jobs j JOIN execution_attempts a ON a.id=$2 WHERE j.id=$1").bind(job).bind(attempt).fetch_one(&f.pool).await.unwrap();
        assert_eq!(job_status, attempt_status);
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id=$1")
                .bind(parent)
                .fetch_one(&f.pool)
                .await
                .unwrap(),
            "canceled"
        );
        let retry_path = format!("/api/v1/jobs/{job}/retry");
        let (manual, retry) = tokio::join!(
            f.request(0, "POST", &path, json!({"status":"running"})),
            f.request(1, "POST", &retry_path, Value::Null)
        );
        assert!(matches!(
            manual.status(),
            StatusCode::OK | StatusCode::CONFLICT
        ));
        assert!(matches!(
            retry.status(),
            StatusCode::OK | StatusCode::CONFLICT
        ));
        let (current,latest):(String,String)=sqlx::query_as("SELECT j.status,a.status FROM jobs j JOIN LATERAL (SELECT status FROM execution_attempts WHERE job_id=j.id ORDER BY attempt_no DESC LIMIT 1) a ON true WHERE j.id=$1").bind(job).fetch_one(&f.pool).await.unwrap();
        assert_eq!(current, latest);
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM execution_attempts WHERE id=$1")
                .bind(attempt)
                .fetch_one(&f.pool)
                .await
                .unwrap(),
            attempt_status,
            "retry never rewrites the old terminal attempt"
        );
    }
    f.close().await;
}

#[tokio::test]
#[ignore = "manual owned PostgreSQL regression, includes one polling interval"]
async fn log_stream_drains_terminal_tail_stops_on_disconnect_and_exposes_db_error() {
    let f = Fixture::new().await;
    let project = f.project().await;
    let (_, job, attempt) = f.pipeline(project).await;
    sqlx::query("INSERT INTO job_logs(job_id,attempt_id,sequence,message) SELECT $1,$2,n,'line-'||n::text FROM generate_series(1,501) n").bind(job).bind(attempt).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE execution_attempts SET status='success' WHERE id=$1")
        .bind(attempt)
        .execute(&f.pool)
        .await
        .unwrap();
    let response = f
        .request(
            0,
            "GET",
            &format!("/api/v1/jobs/{job}/logs/stream"),
            Value::Null,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let output = String::from_utf8(
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            to_bytes(response.into_body(), 1024 * 1024),
        )
        .await
        .unwrap()
        .unwrap()
        .to_vec(),
    )
    .unwrap();
    assert_eq!(output.matches("data: line-").count(), 501);
    assert!(output.find("data: line-501").unwrap() < output.find("event: done").unwrap());
    let (_, job, attempt) = f.pipeline(project).await;
    sqlx::query("INSERT INTO job_logs(job_id,attempt_id,sequence,message) VALUES($1,$2,1,'start')")
        .bind(job)
        .bind(attempt)
        .execute(&f.pool)
        .await
        .unwrap();
    let response = f
        .request(
            0,
            "GET",
            &format!("/api/v1/jobs/{job}/logs/stream"),
            Value::Null,
        )
        .await;
    let mut stream = response.into_body().into_data_stream();
    assert!(stream.next().await.unwrap().is_ok());
    drop(stream);
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let before: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT max(query_start) FROM pg_stat_activity WHERE application_name=$1",
    )
    .bind(&f.schema)
    .fetch_one(&f.control)
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let after: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT max(query_start) FROM pg_stat_activity WHERE application_name=$1",
    )
    .bind(&f.schema)
    .fetch_one(&f.control)
    .await
    .unwrap();
    assert_eq!(before, after, "disconnected response left a polling task");
    let response = f
        .request(
            0,
            "GET",
            &format!("/api/v1/jobs/{job}/logs/stream"),
            Value::Null,
        )
        .await;
    let mut stream = response.into_body().into_data_stream();
    assert!(stream.next().await.unwrap().is_ok());
    f.pool.close().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .is_err(),
        "database failure must not become a healthy empty stream"
    );
    drop(stream);
    f.close().await;
}

#[tokio::test]
#[ignore = "manual owned PostgreSQL cancellation contract regression"]
async fn already_failed_deployment_does_not_confirm_a_new_cancel_request() {
    let f = Fixture::new().await;
    let project = f.project().await;
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/projects/{project}/environments"),
            json!({"name":"failed-before-cancel","protected":true,"required_approvals":1}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let environment = body(response).await["id"].as_str().unwrap().to_owned();
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/environments/{environment}/deployments"),
            json!({"git_ref":"main"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let id = Uuid::parse_str(body(response).await["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE deployments SET status='failed' WHERE id=$1")
        .bind(id)
        .execute(&f.pool)
        .await
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            f.request(
                0,
                "POST",
                &format!("/api/v1/deployments/{id}/cancel"),
                Value::Null
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
    }
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT cancel_requested_at IS NULL FROM deployments WHERE id=$1"
        )
        .bind(id)
        .fetch_one(&f.pool)
        .await
        .unwrap()
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_log WHERE action='deployment.cancel.requested' AND resource_id=$1")
        .bind(id).fetch_one(&f.pool).await.unwrap(),0);
    f.close().await;
}

#[tokio::test]
#[ignore = "manual owned PostgreSQL cancellation/reconciliation regression"]
async fn deployment_cancel_is_idempotent_and_waits_for_confirmed_execution() {
    let f = Fixture::new().await;
    let project = f.project().await;
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/projects/{project}/environments"),
            json!({"name":"audit-protected","protected":true,"required_approvals":1}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let environment = body(response).await["id"].as_str().unwrap().to_owned();
    let create = format!("/api/v1/environments/{environment}/deployments");
    let response = f
        .request(0, "POST", &create, json!({"git_ref":"main"}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let first = body(response).await["id"].as_str().unwrap().to_owned();
    let cancel = format!("/api/v1/deployments/{first}/cancel");
    let response = f.request(0, "POST", &cancel, json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = body(response).await;
    assert_eq!(value["status"], "failed");
    assert!(value["pipeline_id"].is_null());
    assert_eq!(
        f.request(1, "POST", &cancel, json!({})).await.status(),
        StatusCode::OK
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_log WHERE action='deployment.cancel.requested' AND resource_id=$1").bind(first.parse::<Uuid>().unwrap()).fetch_one(&f.pool).await.unwrap(),1);
    let response = f
        .request(0, "POST", &create, json!({"git_ref":"main"}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let id = body(response).await["id"].as_str().unwrap().to_owned();
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/deployments/{id}/approvals"),
            json!({"decision":"approved"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = f
        .request(0, "GET", &format!("/api/v1/deployments/{id}"), Value::Null)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let pipeline = body(response).await["pipeline_id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let (job,attempt):(Uuid,Uuid)=sqlx::query_as("SELECT j.id,a.id FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN execution_attempts a ON a.job_id=j.id WHERE s.pipeline_id=$1 ORDER BY s.position,j.position LIMIT 1").bind(pipeline).fetch_one(&f.pool).await.unwrap();
    let runner = Uuid::new_v4();
    let lease = Uuid::new_v4();
    sqlx::query("INSERT INTO runners(id,name,tags) VALUES($1,'audit-external','{}')")
        .bind(runner)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET status='running' WHERE id=$1")
        .bind(job)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE execution_attempts SET status='running' WHERE id=$1")
        .bind(attempt)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,runner_id,lease_token_hash,lease_status,generation,lease_expires_at,acknowledged_at) VALUES($1,$2,$3,'audit-external',$4,$5,'active',1,now()+interval '1 hour',now())").bind(lease).bind(job).bind(attempt).bind(runner).bind("f".repeat(64)).execute(&f.pool).await.unwrap();
    let cancel = format!("/api/v1/deployments/{id}/cancel");
    let accepted = f.request(0, "POST", &cancel, json!({})).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    drop(accepted); // body/ack lost after server commit
    assert_eq!(
        f.request(0, "POST", &cancel, json!({})).await.status(),
        StatusCode::ACCEPTED
    );
    let mut config = cicd::config::RuntimeConfig::test_default();
    config.runner.mode = cicd::config::RunnerMode::Kubernetes;
    config.runner.embedded_enabled = false;
    let pool = f.pool.clone();
    let task = tokio::spawn(cicd::runner::maintenance_loop_with_config(
        pool,
        cicd::runner::RuntimeRunnerConfig::from_config(&config),
    ));
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            let requested: bool = sqlx::query_scalar(
                "SELECT cancel_requested_at IS NOT NULL FROM job_leases WHERE id=$1",
            )
            .bind(lease)
            .fetch_one(&f.pool)
            .await
            .unwrap();
            if requested {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pending = body(
        f.request(0, "GET", &format!("/api/v1/deployments/{id}"), Value::Null)
            .await,
    )
    .await;
    assert!(
        matches!(pending["status"].as_str(), Some("pending" | "running")),
        "request/DB cancellation is not an exit receipt"
    );
    // This fixture explicitly supplies a worker completion; live process/runner
    // cancellation is a separate integrated acceptance, not claimed by SQL setup.
    sqlx::query("UPDATE job_leases SET lease_status='canceled',terminal_status='canceled',completed_at=now(),completion_received_at=now() WHERE id=$1").bind(lease).execute(&f.pool).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            let status: String = sqlx::query_scalar("SELECT status FROM deployments WHERE id=$1")
                .bind(id.parse::<Uuid>().unwrap())
                .fetch_one(&f.pool)
                .await
                .unwrap();
            if status == "failed" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(
        f.request(0, "POST", &cancel, json!({})).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        f.request(
            0,
            "POST",
            &format!("/api/v1/pipelines/{pipeline}/retry"),
            json!({})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_log WHERE action='deployment.cancel.requested' AND resource_id=$1").bind(id.parse::<Uuid>().unwrap()).fetch_one(&f.pool).await.unwrap(),1);
    // A manually linked/shared pipeline is not owned by this deployment.
    let (shared, _, _) = f.pipeline(project).await;
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/projects/{project}/environments"),
            json!({"name":"manual-link","protected":false}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let environment = body(response).await["id"].as_str().unwrap().to_owned();
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/environments/{environment}/deployments"),
            json!({"git_ref":"main","pipeline_id":shared,"status":"pending"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let manual = body(response).await["id"].as_str().unwrap().to_owned();
    assert_eq!(
        f.request(
            0,
            "POST",
            &format!("/api/v1/deployments/{manual}/cancel"),
            Value::Null
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM pipelines WHERE id=$1")
            .bind(shared)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        "queued"
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT cancel_requested_at IS NULL FROM deployments WHERE id=$1"
        )
        .bind(Uuid::parse_str(&manual).unwrap())
        .fetch_one(&f.pool)
        .await
        .unwrap()
    );
    f.close().await;
}

#[tokio::test]
#[ignore = "manual owned PostgreSQL regression"]
async fn deployment_request_key_replays_original_identity_and_rejects_payload_reuse() {
    let f = Fixture::new().await;
    let project = f.project().await;
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/projects/{project}/environments"),
            json!({"name":"idempotent","protected":true,"required_approvals":1}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let environment = body(response).await["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/environments/{environment}/deployments");
    let key = Uuid::new_v4();
    let payload = json!({"git_ref":"main","request_key":key});
    let (first, second) = tokio::join!(
        f.request(0, "POST", &path, payload.clone()),
        f.request(0, "POST", &path, payload.clone())
    );
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    let first = body(first).await;
    let second = body(second).await;
    assert_eq!(first["id"], second["id"]);
    let id = first["id"].as_str().unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deployments WHERE request_key=$1")
            .bind(key)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action='deployment.created'"
        )
        .fetch_one(&f.pool)
        .await
        .unwrap(),
        1
    );
    // Loss of the HTTP acknowledgement does not require a new deployment.
    drop(f.request(0, "POST", &path, payload.clone()).await);
    let readback = f
        .request(
            0,
            "GET",
            &format!("/api/v1/environments/{environment}/deployment-requests/{key}"),
            Value::Null,
        )
        .await;
    assert_eq!(readback.status(), StatusCode::OK);
    assert_eq!(body(readback).await["id"], id);
    assert_eq!(
        f.request(
            0,
            "POST",
            &path,
            json!({"git_ref":"other","request_key":key})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.request(1, "POST", &path, payload).await.status(),
        StatusCode::CONFLICT
    );
    let missing = Uuid::new_v4();
    assert_eq!(
        f.request(
            0,
            "GET",
            &format!("/api/v1/environments/{environment}/deployment-requests/{missing}"),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.request(
            1,
            "GET",
            &format!("/api/v1/environments/{environment}/deployment-requests/{key}"),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    // Existing records without an input/actor proof must not be silently adopted.
    let legacy = Uuid::new_v4();
    sqlx::query("INSERT INTO deployments(id,environment_id,git_ref,status,request_key) VALUES($1,$2,'main','pending',$3)").bind(Uuid::new_v4()).bind(Uuid::parse_str(&environment).unwrap()).bind(legacy).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.request(
            0,
            "POST",
            &path,
            json!({"git_ref":"main","request_key":legacy})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    // A creation audit failure rolls the durable key back together with its row.
    let failed = Uuid::new_v4();
    sqlx::query("ALTER TABLE audit_log ADD CONSTRAINT deployment_audit_failure CHECK(action<>'deployment.created') NOT VALID").execute(&f.pool).await.unwrap();
    assert_eq!(
        f.request(
            0,
            "POST",
            &path,
            json!({"git_ref":"main","request_key":failed})
        )
        .await
        .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deployments WHERE request_key=$1")
            .bind(failed)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("ALTER TABLE audit_log DROP CONSTRAINT deployment_audit_failure")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE deployments SET status='success' WHERE id=$1")
        .bind(Uuid::parse_str(id).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let rollback_key = Uuid::new_v4();
    let rollback_path = format!("/api/v1/deployments/{id}/rollback");
    let one = f
        .request(
            0,
            "POST",
            &rollback_path,
            json!({"request_key":rollback_key}),
        )
        .await;
    assert_eq!(one.status(), StatusCode::OK);
    let one = body(one).await;
    let two = f
        .request(
            0,
            "POST",
            &rollback_path,
            json!({"request_key":rollback_key}),
        )
        .await;
    assert_eq!(two.status(), StatusCode::OK);
    assert_eq!(body(two).await["id"], one["id"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action='deployment.rollback.created'"
        )
        .fetch_one(&f.pool)
        .await
        .unwrap(),
        1
    );
    // Durable creation can outlive a failed pipeline publication. Retry the
    // same owner operation after the transient fault, not a new deployment.
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/projects/{project}/environments"),
            json!({"name":"resumable","protected":false}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let env = body(response).await["id"].as_str().unwrap().to_owned();
    let response = f
        .request(
            0,
            "POST",
            &format!("/api/v1/environments/{env}/deployments"),
            json!({"git_ref":"main"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let source = body(response).await["id"].as_str().unwrap().to_owned();
    let key = Uuid::new_v4();
    let path = format!("/api/v1/deployments/{source}/rollback");
    sqlx::query(
        "ALTER TABLE stages ADD CONSTRAINT pipeline_publication_fault CHECK(false) NOT VALID",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        f.request(0, "POST", &path, json!({"request_key":key}))
            .await
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let original: Uuid = sqlx::query_scalar("SELECT id FROM deployments WHERE request_key=$1")
        .bind(key)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE stages DROP CONSTRAINT pipeline_publication_fault")
        .execute(&f.pool)
        .await
        .unwrap();
    let response = f
        .request(0, "POST", &path, json!({"request_key":key}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let resumed = body(response).await;
    assert_eq!(resumed["id"], original.to_string());
    assert!(resumed["pipeline_id"].is_string());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deployments WHERE request_key=$1")
            .bind(key)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        1
    );
    f.close().await;
}

#[path = "support/early_commit_loss.rs"]
mod early_commit_loss;

#[tokio::test]
#[ignore = "manual owned PostgreSQL wire-fault regression"]
async fn artifact_negative_readback_waits_for_the_inflight_commit() {
    use std::sync::atomic::Ordering;
    let f = Fixture::new().await;
    let project = f.project().await;
    let (_, job, _) = f.pipeline(project).await;
    sqlx::raw_sql("CREATE SEQUENCE qa_artifact_commit_witness;
        CREATE FUNCTION qa_artifact_commit_pause() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM nextval('qa_artifact_commit_witness'); PERFORM pg_sleep(1); RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER qa_artifact_commit_pause AFTER INSERT ON artifacts
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION qa_artifact_commit_pause();")
        .execute(&f.pool).await.unwrap();
    let socket = std::env::var("FORGE_TEST_POSTGRES_SOCKET").expect("owned fixture socket")
        + "/.s.PGSQL.5432";
    let proxy = early_commit_loss::EarlyCommitLoss::start(socket).await;
    let url = format!(
        "postgres://qa@127.0.0.1:{}/forge_space_test?sslmode=disable",
        proxy.port
    );
    let own = f.schema.clone();
    let proxied = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .after_connect(move |connection, _| {
            let own = own.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('search_path',$1,false),set_config('default_transaction_isolation','repeatable read',false)")
                    .bind(own)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("forge-artifact-{}", Uuid::new_v4().simple()));
    let mut config = cicd::config::RuntimeConfig::test_default();
    config.auth.secret = Some("rust-audit-owned-fixture-secret-only".into());
    config.secrets.key = Some([7; 32]);
    config.artifacts.root = root.clone();
    let app = cicd::api::app_with_git_and_config(
        Some(proxied.clone()),
        cicd::git_host::GitConfig::default(),
        None,
        config,
    )
    .unwrap();
    proxy.armed.store(true, Ordering::SeqCst);
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.oneshot(
            Request::post(format!("/api/v1/jobs/{job}/artifacts"))
                .header("authorization", format!("Bearer {}", f.tokens[0]))
                .header("x-artifact-name", "fixture.txt")
                .header("content-type", "text/plain")
                .body(Body::from("committed artifact bytes"))
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        proxy.lost.load(Ordering::SeqCst),
        "the real COMMIT must be interrupted before its acknowledgement"
    );
    assert!(
        response.status().is_server_error(),
        "lost publication reply keeps its existing error contract"
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM artifacts WHERE job_id=$1")
                .bind(job)
                .fetch_one(&f.pool)
                .await
                .unwrap()
                == 1
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the original transaction must really COMMIT");
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT is_called FROM qa_artifact_commit_witness")
            .fetch_one(&f.pool)
            .await
            .unwrap()
    );
    let path: String = sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE job_id=$1")
        .bind(job)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(&path).await.unwrap(),
        b"committed artifact bytes",
        "a negative snapshot before COMMIT is not permission to delete the file"
    );
    proxied.close().await;
    drop(proxy);
    // Neighboring success/checksum and confirmed rollback retain the same HTTP contract.
    let mut config = cicd::config::RuntimeConfig::test_default();
    config.auth.secret = Some("rust-audit-owned-fixture-secret-only".into());
    config.secrets.key = Some([7; 32]);
    config.artifacts.root = root.clone();
    let app = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        cicd::git_host::GitConfig::default(),
        None,
        config,
    )
    .unwrap();
    let request = || {
        Request::post(format!("/api/v1/jobs/{job}/artifacts"))
            .header("authorization", format!("Bearer {}", f.tokens[0]))
            .header("x-artifact-name", "normal.txt")
            .header("content-type", "text/plain")
            .body(Body::from("normal artifact"))
            .unwrap()
    };
    let response = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let id = body(response).await["id"].as_str().unwrap().to_owned();
    let download = || {
        Request::get(format!("/api/v1/artifacts/{id}/download"))
            .header("authorization", format!("Bearer {}", f.tokens[0]))
            .body(Body::empty())
            .unwrap()
    };
    let response = app.clone().oneshot(download()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "normal artifact"
    );
    let path: String = sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE id=$1")
        .bind(id.parse::<Uuid>().unwrap())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    tokio::fs::write(path, b"tampered").await.unwrap();
    assert_eq!(
        app.clone().oneshot(download()).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    sqlx::query("ALTER TABLE audit_log ADD CONSTRAINT qa_reject_artifact_audit CHECK(action<>'artifact.uploaded') NOT VALID")
        .execute(&f.pool).await.unwrap();
    let count = std::fs::read_dir(&root).unwrap().count();
    assert!(
        app.oneshot(request())
            .await
            .unwrap()
            .status()
            .is_server_error()
    );
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        count,
        "confirmed pre-COMMIT failure removes only its owned staged file"
    );
    std::fs::remove_dir_all(root).unwrap();
    f.close().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "manual Unix FIFO/owned PostgreSQL async-executor boundary"]
async fn slow_artifact_read_does_not_stall_neighboring_async_work() {
    use std::{
        io::Write,
        time::{Duration, Instant},
    };
    let f = Fixture::new().await;
    let project = f.project().await;
    let (_, job, _) = f.pipeline(project).await;
    let root = std::env::temp_dir().join(format!("forge-slow-io-{}", Uuid::new_v4()));
    let mut config = cicd::config::RuntimeConfig::test_default();
    config.auth.secret = Some("rust-audit-owned-fixture-secret-only".into());
    config.secrets.key = Some([7; 32]);
    config.artifacts.root = root.clone();
    let app = cicd::api::app_with_git_and_config(
        Some(f.pool.clone()),
        cicd::git_host::GitConfig::default(),
        None,
        config,
    )
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/v1/jobs/{job}/artifacts"))
                .header("authorization", format!("Bearer {}", f.tokens[0]))
                .header("x-artifact-name", "slow.txt")
                .header("content-type", "text/plain")
                .body(Body::from("slow fixture bytes"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let id = Uuid::parse_str(body(response).await["id"].as_str().unwrap()).unwrap();
    let path: String = sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE id=$1")
        .bind(id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        let _ = ready_tx.send(());
        // This thread always releases the read, even on a failed assertion.
        let _ = release_rx.recv_timeout(Duration::from_secs(2));
        file.write_all(b"slow fixture bytes").unwrap();
    });
    let request = Request::get(format!("/api/v1/artifacts/{id}/download"))
        .header("authorization", format!("Bearer {}", f.tokens[0]))
        .body(Body::empty())
        .unwrap();
    let start = Instant::now();
    let download = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    ready_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let heartbeat_elapsed = start.elapsed();
    let read_was_pending = !download.is_finished();
    let _ = release_tx.send(());
    let response = download.await.unwrap();
    writer.join().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "slow fixture bytes"
    );
    std::fs::remove_dir_all(root).unwrap();
    f.close().await;
    assert!(
        read_was_pending,
        "heartbeat must run while the real filesystem read is blocked"
    );
    assert!(
        heartbeat_elapsed < Duration::from_millis(500),
        "single-thread async executor stalled for {heartbeat_elapsed:?}"
    );
}
