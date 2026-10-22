//! Durable PR publication intent; Git refs remain the authority for an unknown effect.
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

#[derive(FromRow)]
pub struct Attempt {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub source_ref: String,
    pub source_oid: String,
    pub target_ref: String,
    pub target_oid: String,
    pub merge_oid: String,
    pub state: String,
}

pub async fn active(
    connection: &mut PgConnection,
    pull: Uuid,
) -> Result<Option<Attempt>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id,repository_id,source_ref,source_oid,target_ref,target_oid,merge_oid,state \
         FROM pull_request_merge_attempts WHERE pull_request_id=$1 AND state<>'rejected'",
    )
    .bind(pull)
    .fetch_optional(connection)
    .await
}

pub async fn prepare(
    connection: &mut PgConnection,
    pull: Uuid,
    attempt: &Attempt,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO pull_request_merge_attempts \
         (id,pull_request_id,repository_id,source_ref,source_oid,target_ref,target_oid,merge_oid) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(attempt.id)
    .bind(pull)
    .bind(attempt.repository_id)
    .bind(&attempt.source_ref)
    .bind(&attempt.source_oid)
    .bind(&attempt.target_ref)
    .bind(&attempt.target_oid)
    .bind(&attempt.merge_oid)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn finish(
    connection: &mut PgConnection,
    attempt: Uuid,
    published: bool,
) -> Result<(), sqlx::Error> {
    let changed = sqlx::query(
        "UPDATE pull_request_merge_attempts SET state=$2,completed_at=now() \
         WHERE id=$1 AND state='prepared'",
    )
    .bind(attempt)
    .bind(if published { "confirmed" } else { "rejected" })
    .execute(connection)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}
