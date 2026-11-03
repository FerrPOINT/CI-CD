//! Local projections for the verified central working directory.
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct DirectoryProjection {
    pub central_sub: String,
    pub id: Uuid,
    pub username: String,
    pub role: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

/// Enrollment is insert-only. A directory read never changes an existing local identity.
pub async fn project_directory_users(
    pool: &PgPool,
    subjects: &[String],
) -> Result<Vec<DirectoryProjection>, sqlx::Error> {
    if subjects.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "WITH missing AS (
            SELECT DISTINCT sub FROM unnest($1::text[]) AS wanted(sub)
            WHERE NOT EXISTS(SELECT 1 FROM users WHERE central_sub=sub)
        )
        INSERT INTO users(id,username,role,enabled,central_sub)
        SELECT id,'central-'||replace(id::text,'-',''),'developer',true,sub
        FROM (SELECT gen_random_uuid() AS id,sub FROM missing) created
        ON CONFLICT(central_sub) WHERE central_sub IS NOT NULL DO NOTHING",
    )
    .bind(subjects)
    .execute(&mut *tx)
    .await?;
    // The next READ COMMITTED snapshot includes a concurrent JIT winner.
    let result=sqlx::query_as(
        "SELECT central_sub,id,username,role,enabled,created_at FROM users WHERE central_sub=ANY($1)"
    ).bind(subjects).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(result)
}
