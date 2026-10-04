use chrono::{DateTime, Utc};
use cicd_domain::sdlc_workspace::WorkspaceOperationReceipt;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub async fn find_receipt(
    conn: &mut PgConnection,
    project: Uuid,
    subject: Uuid,
    key: &str,
) -> Result<Option<WorkspaceOperationReceipt>, sqlx::Error> {
    sqlx::query_scalar::<_, sqlx::types::Json<WorkspaceOperationReceipt>>(
        "SELECT receipt FROM sdlc_workspace_operations WHERE project_id=$1 AND service_account_id=$2 AND operation_key=$3")
        .bind(project).bind(subject).bind(key).fetch_optional(conn).await.map(|r| r.map(|r| r.0))
}

pub async fn append_receipt(
    conn: &mut PgConnection,
    subject: Uuid,
    receipt: &WorkspaceOperationReceipt,
) -> Result<(), sqlx::Error> {
    let r = &receipt.request;
    let b = &r.binding;
    sqlx::query("INSERT INTO sdlc_workspace_operations (id,project_id,service_account_id,operation_key,request_hash,task_id,root_task_id,assignment_id,execution_id,fencing_token,lease_id,attempt_id,workspace_generation,repository_id,source_commit,receipt,recorded_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)")
        .bind(receipt.operation_id).bind(receipt.project_id).bind(subject).bind(&r.operation_key)
        .bind(&receipt.request_hash).bind(b.task_id).bind(b.root_task_id).bind(b.assignment_id)
        .bind(b.execution_id).bind(b.fencing_token).bind(r.lease_id).bind(r.attempt_id)
        .bind(r.workspace_generation).bind(r.repository_id).bind(&r.source_commit)
        .bind(sqlx::types::Json(receipt)).bind(receipt.recorded_at).execute(conn).await?;
    Ok(())
}

pub struct LeaseObservation {
    pub attempt_id: Uuid,
    pub generation: i64,
    pub current_generation: i64,
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub expired: bool,
    pub acknowledged: bool,
    pub completion_received: bool,
    pub source_commit: Option<String>,
}

pub async fn lease_observation(
    conn: &mut PgConnection,
    project: Uuid,
    lease: Uuid,
    lock: bool,
) -> Result<Option<LeaseObservation>, sqlx::Error> {
    let mut query = "SELECT l.attempt_id,l.generation,l.lease_status,l.lease_expires_at,
        l.lease_expires_at <= clock_timestamp() AS expired,l.acknowledged_at IS NOT NULL AS acknowledged,
        l.completion_received_at IS NOT NULL AS completion_received,p.commit_sha,
        (SELECT max(generation) FROM job_leases WHERE job_id=l.job_id) AS current_generation
        FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id AND a.job_id=l.job_id
        JOIN jobs j ON j.id=l.job_id JOIN stages s ON s.id=j.stage_id
        JOIN pipelines p ON p.id=s.pipeline_id WHERE p.project_id=$1 AND l.id=$2".to_string();
    if lock {
        query.push_str(" FOR UPDATE OF l,j FOR SHARE OF a,s,p");
    }
    sqlx::query(&query)
        .bind(project)
        .bind(lease)
        .fetch_optional(conn)
        .await?
        .map(|r| {
            Ok(LeaseObservation {
                attempt_id: r.try_get("attempt_id")?,
                generation: r.try_get("generation")?,
                current_generation: r.try_get("current_generation")?,
                status: r.try_get("lease_status")?,
                expires_at: r.try_get("lease_expires_at")?,
                expired: r.try_get("expired")?,
                acknowledged: r.try_get("acknowledged")?,
                completion_received: r.try_get("completion_received")?,
                source_commit: r.try_get("commit_sha")?,
            })
        })
        .transpose()
}
