//! Acknowledged successor overlay; never rewrite original intent or trigger custody.
use super::{StoredCredential, digest};
use crate::{api::ApiError, store::sdlc_workspace::TaskPipelineJournal};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenewalReadback {
    pub pipeline_id: Uuid,
    pub renewal_key: String,
    pub request_hash: String,
    pub generation: i64,
    pub predecessor_fingerprint: String,
    pub fingerprint: String,
    pub context_hash: String,
    pub recorded_at: DateTime<Utc>,
    /// A historical ACK is not permission to republish a superseded credential.
    pub current: bool,
}

/// Hold the existing intent row through caller commit; renewal uses FOR UPDATE.
pub(crate) async fn current(
    conn: &mut PgConnection,
    record: &TaskPipelineJournal,
) -> Result<StoredCredential, ApiError> {
    let original = StoredCredential::from_value(
        &record
            .delegated_credential
            .as_ref()
            .ok_or_else(ApiError::forbidden)?
            .0,
    )?;
    sqlx::query("SELECT pipeline_id FROM task_pipeline_intents WHERE pipeline_id=$1 FOR SHARE")
        .bind(record.pipeline_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(ApiError::internal)?;
    let value: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT credential FROM task_credential_successors WHERE pipeline_id=$1 ORDER BY generation DESC LIMIT 1",
    ).bind(record.pipeline_id).fetch_optional(conn).await.map_err(ApiError::internal)?;
    let current = match value {
        Some(value) => StoredCredential::from_value(&value)?,
        None => return Ok(original),
    };
    same_context(&original, &current)?;
    Ok(current)
}

pub(crate) fn same_context(
    original: &StoredCredential,
    replacement: &StoredCredential,
) -> Result<(), ApiError> {
    if original.subject != replacement.subject
        || original.context != replacement.context
        || original.context_hash != replacement.context_hash
        || replacement.context_hash != digest(&replacement.context)?
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

pub(crate) async fn find(
    conn: &mut PgConnection,
    pipeline: Uuid,
    key: &str,
) -> Result<Option<RenewalReadback>, ApiError> {
    let row = sqlx::query("SELECT s.renewal_key,s.generation,s.request_hash,s.predecessor_fingerprint,\
        s.credential->>'fingerprint' AS fingerprint,s.credential->>'context_hash' AS context_hash,s.recorded_at,\
        NOT EXISTS(SELECT 1 FROM task_credential_successors n WHERE n.pipeline_id=s.pipeline_id AND n.generation>s.generation) AS current \
        FROM task_credential_successors s WHERE s.pipeline_id=$1 AND s.renewal_key=$2")
        .bind(pipeline).bind(key).fetch_optional(conn).await.map_err(ApiError::internal)?;
    row.map(|row| -> Result<_, ApiError> {
        Ok(RenewalReadback {
            request_hash: row.try_get("request_hash").map_err(ApiError::internal)?,
            pipeline_id: pipeline,
            renewal_key: row.try_get("renewal_key").map_err(ApiError::internal)?,
            generation: row.try_get("generation").map_err(ApiError::internal)?,
            predecessor_fingerprint: row
                .try_get("predecessor_fingerprint")
                .map_err(ApiError::internal)?,
            fingerprint: row.try_get("fingerprint").map_err(ApiError::internal)?,
            context_hash: row.try_get("context_hash").map_err(ApiError::internal)?,
            recorded_at: row.try_get("recorded_at").map_err(ApiError::internal)?,
            current: row.try_get("current").map_err(ApiError::internal)?,
        })
    })
    .transpose()
}

/// Caller owns the original intent's FOR UPDATE lock and fresh authority permit.
pub(crate) async fn append(
    conn: &mut PgConnection,
    pipeline: Uuid,
    key: &str,
    request_hash: &str,
    predecessor: &str,
    replacement: &StoredCredential,
) -> Result<RenewalReadback, ApiError> {
    let seen: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM task_credential_successors WHERE pipeline_id=$1 AND credential->>'fingerprint'=$2)")
        .bind(pipeline).bind(&replacement.fingerprint).fetch_one(&mut *conn).await.map_err(ApiError::internal)?;
    if seen {
        return Err(ApiError::conflict(
            "A superseded credential cannot be reinstalled",
        ));
    }
    sqlx::query("INSERT INTO task_credential_successors(pipeline_id,generation,renewal_key,request_hash,predecessor_fingerprint,credential) \
        SELECT $1,coalesce(max(generation),0)+1,$2,$3,$4,$5 FROM task_credential_successors WHERE pipeline_id=$1")
        .bind(pipeline).bind(key).bind(request_hash).bind(predecessor).bind(replacement.value()?)
        .execute(&mut *conn).await.map_err(ApiError::internal)?;
    find(conn, pipeline, key)
        .await?
        .ok_or_else(ApiError::unavailable)
}
