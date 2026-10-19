//! Production observer SQL over connection-private projection tables, not a substitute for R auth.
use super::*;
use sqlx::Connection;

async fn observation(conn: &mut PgConnection, pipeline: Uuid) -> CandidateJobObservation {
    let mut rows = candidate_jobs(conn, pipeline).await.unwrap();
    assert_eq!(rows.len(), 1);
    rows.pop().unwrap()
}

#[tokio::test]
#[ignore = "requires the existing disposable CICD_TEST_DATABASE_URL PostgreSQL"]
async fn candidate_jobs_nested_names_stale_rows_duplicates_and_aliases_fail_closed() {
    let url =
        std::env::var("CICD_TEST_DATABASE_URL").expect("explicit owned test database required");
    let mut connection = PgConnection::connect(&url).await.unwrap();
    let mut tx = connection.begin().await.unwrap();
    sqlx::raw_sql(
        "SET LOCAL search_path=pg_temp; \
         CREATE TEMP TABLE stages(id uuid,pipeline_id uuid,status text) ON COMMIT DROP; \
         CREATE TEMP TABLE jobs(id uuid,stage_id uuid,status text,finished_at timestamptz,artifact_paths text[]) ON COMMIT DROP; \
         CREATE TEMP TABLE execution_attempts(id uuid,job_id uuid,attempt_no int,status text,finished_at timestamptz) ON COMMIT DROP; \
         CREATE TEMP TABLE job_leases(job_id uuid,attempt_id uuid,generation int,lease_status text,terminal_status text,completed_at timestamptz,acknowledged_at timestamptz,completion_received_at timestamptz) ON COMMIT DROP; \
         CREATE TEMP TABLE artifacts(id uuid,job_id uuid,attempt_id uuid,name text,sha256 text,size_bytes bigint,purged_at timestamptz,expires_at timestamptz) ON COMMIT DROP;",
    ).execute(&mut *tx).await.unwrap();
    let pipeline = Uuid::new_v4();
    let stage = Uuid::new_v4();
    let job = Uuid::new_v4();
    let old_attempt = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    sqlx::query("INSERT INTO stages VALUES($1,$2,'success')")
        .bind(stage)
        .bind(pipeline)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO jobs VALUES($1,$2,'success',clock_timestamp(),ARRAY['dist/report.txt'])",
    )
    .bind(job)
    .bind(stage)
    .execute(&mut *tx)
    .await
    .unwrap();
    for (id, number) in [(old_attempt, 1), (attempt, 2)] {
        sqlx::query("INSERT INTO execution_attempts VALUES($1,$2,$3,'success',clock_timestamp())")
            .bind(id)
            .bind(job)
            .bind(number)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO job_leases VALUES($1,$2,2,'completed','success',clock_timestamp(),clock_timestamp(),clock_timestamp())")
        .bind(job).bind(attempt).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO artifacts VALUES($1,$2,$3,'dist__report.txt',$4,12,NULL,clock_timestamp()+interval '1 hour')")
        .bind(Uuid::new_v4()).bind(job).bind(old_attempt).bind("a".repeat(64))
        .execute(&mut *tx).await.unwrap();
    let missing = observation(&mut tx, pipeline).await;
    assert!(missing.terminal_acknowledged);
    assert_eq!(missing.attempt_id, Some(attempt));
    assert!(!missing.artifacts_complete); // Correct name from an old attempt is not current evidence.
    sqlx::query("INSERT INTO artifacts VALUES($1,$2,$3,'dist__report.txt',$4,12,NULL,clock_timestamp()+interval '1 hour')")
        .bind(artifact).bind(job).bind(attempt).bind("b".repeat(64))
        .execute(&mut *tx).await.unwrap();
    assert!(observation(&mut tx, pipeline).await.artifacts_complete);
    sqlx::query("UPDATE artifacts SET name='dist/report.txt' WHERE id=$1")
        .bind(artifact)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(!observation(&mut tx, pipeline).await.artifacts_complete);
    sqlx::query("UPDATE artifacts SET name='dist__report.txt' WHERE id=$1")
        .bind(artifact)
        .execute(&mut *tx)
        .await
        .unwrap();
    let duplicate = Uuid::new_v4();
    sqlx::query("INSERT INTO artifacts VALUES($1,$2,$3,'dist__report.txt',$4,12,NULL,clock_timestamp()+interval '1 hour')")
        .bind(duplicate).bind(job).bind(attempt).bind("b".repeat(64))
        .execute(&mut *tx).await.unwrap();
    assert!(!observation(&mut tx, pipeline).await.artifacts_complete);
    sqlx::query("DELETE FROM artifacts WHERE id=$1")
        .bind(duplicate)
        .execute(&mut *tx)
        .await
        .unwrap();
    for alias in ["dist__report.txt", "dist\\report.txt", "dist/report.txt"] {
        sqlx::query("UPDATE jobs SET artifact_paths=$1")
            .bind(vec!["dist/report.txt", alias])
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(!observation(&mut tx, pipeline).await.artifacts_complete);
    }
    sqlx::query("UPDATE jobs SET artifact_paths=ARRAY['dist/report.txt']")
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "UPDATE artifacts SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        "UPDATE artifacts SET purged_at=clock_timestamp() WHERE id=$1",
        "UPDATE artifacts SET sha256=NULL WHERE id=$1",
        "UPDATE artifacts SET size_bytes=0 WHERE id=$1",
    ] {
        sqlx::query(statement)
            .bind(artifact)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(!observation(&mut tx, pipeline).await.artifacts_complete);
        sqlx::query("UPDATE artifacts SET expires_at=clock_timestamp()+interval '1 hour',purged_at=NULL,sha256=$2,size_bytes=12 WHERE id=$1")
            .bind(artifact).bind("b".repeat(64)).execute(&mut *tx).await.unwrap();
    }
    assert!(observation(&mut tx, pipeline).await.artifacts_complete);
    let mut extras = Vec::new();
    for n in 0..32 {
        let id = Uuid::new_v4();
        extras.push(id);
        sqlx::query("INSERT INTO artifacts VALUES($1,$2,$3,$4,$5,12,NULL,clock_timestamp()+interval '1 hour')")
            .bind(id).bind(job).bind(attempt).bind(format!("extra-{n}"))
            .bind("b".repeat(64)).execute(&mut *tx).await.unwrap();
    }
    assert!(!observation(&mut tx, pipeline).await.artifacts_complete);
    sqlx::query("DELETE FROM artifacts WHERE id=ANY($1)")
        .bind(extras)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(observation(&mut tx, pipeline).await.artifacts_complete);
    let next_attempt = Uuid::new_v4();
    sqlx::query("INSERT INTO execution_attempts VALUES($1,$2,3,'success',clock_timestamp())")
        .bind(next_attempt)
        .bind(job)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(!observation(&mut tx, pipeline).await.artifacts_complete);
    tx.rollback().await.unwrap();
}
