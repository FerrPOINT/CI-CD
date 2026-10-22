//! Additive failure-only ACK after lease expiry. No success or task/queue reactivation.
use super::*;
use crate::execution::kubernetes::{Execution, Scope, Terminal};
use std::path::Path;
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct KubernetesTerminationProof {
    pub cluster_ca_sha256: String,
    pub namespace_uid: String,
    pub workspace_claim_uid: String,
    pub pod_uid: String,
}

pub(super) async fn complete(
    db: &PgPool,
    runner: &AuthenticatedRunner,
    lease: Uuid,
    input: &RunnerCompleteRequest,
) -> Result<Json<RunnerCompleteResponse>, ApiError> {
    if !matches!(input.outcome.as_str(), "failed" | "failure" | "canceled") {
        return Err(ApiError::conflict(
            "Expired execution cannot report success",
        ));
    }
    let proof = input
        .termination_proof
        .as_ref()
        .ok_or_else(|| ApiError::conflict("Stopped proof required"))?;
    let hash = crate::auth::hash_token(input.lease_token.trim());
    let row=sqlx::query_as::<_,(Uuid,bool)>("SELECT l.job_id,l.completion_received_at IS NOT NULL FROM job_leases l WHERE l.id=$1 AND l.runner_id=$2 AND l.lease_token_hash=$3 AND l.attempt_id=$4 AND l.generation=$5 AND l.lease_expires_at<=now() AND l.acknowledged_at IS NOT NULL").bind(lease).bind(runner.id).bind(&hash).bind(input.attempt_id).bind(input.fencing_token).fetch_optional(db).await.map_err(ApiError::internal)?.ok_or_else(||ApiError::conflict("Original expired lease unavailable"))?;
    // No caller boolean is accepted: read the exact shared protected custody and actual API object.
    let execution = Execution::reopen(
        Path::new("/workspaces"),
        input.attempt_id,
        input.fencing_token,
    )
    .map_err(|_| ApiError::conflict("Original Kubernetes custody unavailable"))?;
    let scope = execution.scope();
    let config = execution.config();
    if scope.lease_id != lease
        || scope.job_id != row.0
        || config.cluster_ca_sha256 != proof.cluster_ca_sha256
        || config.namespace_uid != proof.namespace_uid
        || config.workspace_claim_uid != proof.workspace_claim_uid
    {
        return Err(ApiError::conflict("Termination proof identity mismatch"));
    }
    let terminal = if row.1 {
        execution
            .terminal_record()
            .map_err(|_| ApiError::conflict("Prior stop receipt unavailable"))?
    } else {
        execution
            .reconcile()
            .await
            .map_err(|_| ApiError::conflict("Actual Kubernetes termination unavailable"))?
            .ok_or_else(|| ApiError::conflict("Kubernetes execution remains active"))?
    };
    if terminal.pod_uid != proof.pod_uid || Some(terminal.exit_code) != input.exit_code {
        return Err(ApiError::conflict("Termination observation mismatch"));
    }
    let terminal_status = terminal_status_for_outcome(&input.outcome)?;
    if !acknowledge(
        db,
        runner.id,
        lease,
        &hash,
        scope,
        &terminal,
        terminal_status,
    )
    .await
    .map_err(ApiError::internal)?
    {
        return Err(ApiError::conflict(
            "Expired termination no longer owns this attempt",
        ));
    }
    Ok(Json(RunnerCompleteResponse {
        protocol_version: PROTOCOL_VERSION,
        accepted: true,
        terminal_status: terminal_status.into(),
    }))
}

/// Caller supplies only a stop observation verified from exact retained custody/API identity.
/// Lock the job BEFORE evaluating generation so the normal retry/claim producers serialize here.
async fn acknowledge(
    db: &PgPool,
    runner: Uuid,
    lease: Uuid,
    token_hash: &str,
    scope: &Scope,
    _terminal: &Terminal,
    outcome: &str,
) -> Result<bool, sqlx::Error> {
    if !matches!(outcome, "failed" | "canceled") {
        return Ok(false);
    }
    let mut tx = db.begin().await?;
    let exists = sqlx::query_scalar::<_, Uuid>("SELECT id FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(scope.job_id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        return Ok(false);
    }
    let row=sqlx::query_as::<_,(Uuid,bool)>("SELECT l.id,l.completion_received_at IS NOT NULL FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN execution_attempts a ON a.id=l.attempt_id JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id WHERE l.id=$1 AND l.runner_id=$2 AND l.lease_token_hash=$3 AND l.job_id=$4 AND l.attempt_id=$5 AND l.generation=$6 AND l.acknowledged_at IS NOT NULL AND l.lease_expires_at<=now() AND l.lease_status IN ('expired','completed','canceled') AND l.terminal_status=$7 AND j.status=$7 AND a.status=$7 AND a.finished_at IS NOT NULL AND p.commit_sha=$8 AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id) AND NOT EXISTS(SELECT 1 FROM job_leases other WHERE other.job_id=j.id AND other.id<>l.id AND other.lease_status='active') FOR UPDATE OF l,a").bind(lease).bind(runner).bind(token_hash).bind(scope.job_id).bind(scope.attempt_id).bind(scope.generation).bind(outcome).bind(&scope.source_commit).fetch_optional(&mut *tx).await?;
    let Some((_, accepted)) = row else {
        return Ok(false);
    };
    if !accepted {
        sqlx::query("UPDATE job_leases SET lease_status=$2,completion_received_at=now() WHERE id=$1 AND completion_received_at IS NULL").bind(lease).bind(lease_status_for_terminal(outcome)).execute(&mut *tx).await?;
    }
    // Preserve projection, original finished_at, queue state, diagnostic and source binding.
    tx.commit().await?;
    Ok(true)
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    async fn pool() -> (PgPool, PgPool, String) {
        let url = std::env::var("CICD_TEST_DATABASE_URL").expect("CICD_TEST_DATABASE_URL required");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("it_kube_term_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        crate::migrations().await.unwrap().run(&pool).await.unwrap();
        (pool, admin, schema)
    }
    async fn fixture(db: &PgPool) -> (Uuid, Uuid, Scope, Terminal) {
        let project = Uuid::new_v4();
        let pipeline = Uuid::new_v4();
        let stage = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let lease = Uuid::new_v4();
        let runner = Uuid::new_v4();
        sqlx::query("INSERT INTO runners(id,name) VALUES($1,$2)")
            .bind(runner)
            .bind(format!("kube-term-{runner}"))
            .execute(db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://example.invalid/repo.git')").bind(project).bind(format!("kube-term-{project}")).execute(db).await.unwrap();
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,'main',$3,'failed')").bind(pipeline).bind(project).bind("a".repeat(40)).execute(db).await.unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'test',0,'failed')").bind(stage).bind(pipeline).execute(db).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'test','unused','true',0,'failed')").bind(job).bind(stage).execute(db).await.unwrap();
        sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger,finished_at) VALUES($1,$2,1,'failed','external_runner',now())").bind(attempt).bind(job).execute(db).await.unwrap();
        sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_id,runner_name,lease_status,generation,lease_expires_at,completed_at,terminal_status,acknowledged_at,lease_token_hash) VALUES($1,$2,$3,$4,'fixture','expired',1,now()-interval '1 minute',now(),'failed',now(),$5)").bind(lease).bind(job).bind(attempt).bind(runner).bind(crate::auth::hash_token("original-lease-token")).execute(db).await.unwrap();
        (
            runner,
            lease,
            Scope {
                job_id: job,
                attempt_id: attempt,
                lease_id: lease,
                generation: 1,
                source_commit: "a".repeat(40),
            },
            Terminal {
                pod_uid: Uuid::new_v4().to_string(),
                exit_code: 143,
                reason: "Terminated".into(),
            },
        )
    }
    #[tokio::test]
    async fn expired_termination_ack_is_owned_failure_only_latest_generation_and_idempotent() {
        let (db, admin, schema) = pool().await;
        let (runner, lease, scope, terminal) = fixture(&db).await;
        let token = crate::auth::hash_token("original-lease-token");
        assert!(
            !acknowledge(
                &db,
                runner,
                lease,
                "wrong-token",
                &scope,
                &terminal,
                "failed"
            )
            .await
            .unwrap()
        );
        assert!(
            !acknowledge(
                &db,
                Uuid::new_v4(),
                lease,
                &token,
                &scope,
                &terminal,
                "failed"
            )
            .await
            .unwrap()
        );
        assert!(
            !acknowledge(&db, runner, lease, &token, &scope, &terminal, "success")
                .await
                .unwrap()
        );
        let mut stale = scope.clone();
        stale.generation = 2;
        assert!(
            !acknowledge(&db, runner, lease, &token, &stale, &terminal, "failed")
                .await
                .unwrap()
        );
        assert!(
            acknowledge(&db, runner, lease, &token, &scope, &terminal, "failed")
                .await
                .unwrap()
        );
        assert!(
            acknowledge(&db, runner, lease, &token, &scope, &terminal, "failed")
                .await
                .unwrap()
        );
        let status:(String,String)=sqlx::query_as("SELECT j.status,a.status FROM jobs j JOIN execution_attempts a ON a.job_id=j.id WHERE j.id=$1").bind(scope.job_id).fetch_one(&db).await.unwrap();
        assert_eq!(status, ("failed".into(), "failed".into()));
        let (runner, lease, scope, terminal) = fixture(&db).await;
        let mut tx = db.begin().await.unwrap();
        sqlx::query("SELECT id FROM jobs WHERE id=$1 FOR UPDATE")
            .bind(scope.job_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let db2 = db.clone();
        let scope2 = scope.clone();
        let token2 = token.clone();
        let task = tokio::spawn(async move {
            acknowledge(&db2, runner, lease, &token2, &scope2, &terminal, "failed").await
        });
        sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_expires_at) VALUES($1,$2,$3,'later-owner',2,now()+interval '1 minute')").bind(Uuid::new_v4()).bind(scope.job_id).bind(scope.attempt_id).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(!task.await.unwrap().unwrap());
        db.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}
