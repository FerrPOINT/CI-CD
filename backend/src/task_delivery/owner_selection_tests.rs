use super::*;
use serde_json::json;

fn policy() -> serde_json::Value {
    json!({"version":1,"action":"deploy","stageName":"build","jobName":"package", "artifactName":"dist__bundle.tar"})
}

#[test]
fn owner_selection_requires_explicit_closed_policy_without_rollback_default() {
    let parsed: Policy = serde_json::from_value(policy()).unwrap();
    parsed.validate().unwrap();
    for (field, value) in [
        ("version", json!(2)),
        ("jobName", json!("")),
        ("artifactName", json!("dist/bundle.tar")),
        ("stageName", json!(" build")),
    ] {
        let mut p = policy();
        p[field] = value;
        let parsed: Policy = serde_json::from_value(p).unwrap();
        assert!(parsed.validate().is_err());
    }
    for value in [json!({}), json!({"version":1,"action":"rollback"}), {
        let mut p = policy();
        p["artifactId"] = json!(Uuid::new_v4());
        p
    }] {
        assert!(serde_json::from_value::<Policy>(value).is_err());
    }
}

fn candidate() -> CandidateEvidenceReadback {
    // Reuse the existing native-shaped receipt fixture instead of inventing request authority.
    let (_, _, mut original) = crate::registered_devops_evidence::tests::fixture();
    original.physical_source_observed = true;
    CandidateEvidenceReadback {
        schema: "forge/candidate-evidence-readback/v1".into(),
        operation_receipt: original,
        pipeline_id: Uuid::new_v4(),
        observed_at: chrono::Utc::now(),
        source_commit: "a".repeat(40),
        config_sha256: Some("b".repeat(64)),
        plan_sha256: Some("c".repeat(64)),
        candidate_observed: true,
        artifacts: vec![CandidateArtifactEvidence {
            artifact_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            sha256: "d".repeat(64),
            size_bytes: 12,
        }],
        blockers: vec![],
        dispatch_allowed: false,
        deployment_verified: false,
        acceptance_verified: false,
    }
}

#[test]
fn owner_selection_uses_exact_id_and_retains_candidate_hashes_not_first_artifact() {
    let mut c = candidate();
    let desired = c.artifacts[0].clone();
    let mut other = desired.clone();
    other.artifact_id = Uuid::new_v4();
    c.artifacts.insert(0, other);
    let selection = selected(&c, &[desired.artifact_id]).unwrap();
    assert_eq!(selection.artifact.artifact_id, desired.artifact_id);
    assert_eq!(selection.source_pipeline_id, c.pipeline_id);
    assert_eq!(selection.config_sha256, c.config_sha256.unwrap());
    assert_eq!(selection.plan_sha256, c.plan_sha256.unwrap());
}

#[test]
fn owner_selection_missing_duplicate_stale_or_foreign_rows_never_select() {
    let mut c = candidate();
    let id = c.artifacts[0].artifact_id;
    assert!(selected(&c, &[]).is_err());
    assert!(selected(&c, &[id, id]).is_err());
    assert!(selected(&c, &[Uuid::new_v4()]).is_err());
    c.artifacts.push(c.artifacts[0].clone());
    assert!(selected(&c, &[id]).is_err());
    c.artifacts.pop();
    c.artifacts[0].size_bytes = 0;
    assert!(selected(&c, &[id]).is_err());
    c.artifacts[0].size_bytes = 12;
    c.candidate_observed = false;
    assert!(selected(&c, &[id]).is_err());
}

#[test]
fn absent_owner_selection_preserves_original_policy_serialized_bytes() {
    let raw = format!(
        "{{\"origin\":\"http://127.0.0.1:3456/\",\"healthPath\":\"/health\",\"healthBodySha256\":\"{}\",\"acceptancePath\":\"/acceptance\",\"acceptanceBodySha256\":\"{}\"}}",
        "a".repeat(64),
        "b".repeat(64),
    );
    let parsed: crate::task_delivery::DeliveryPolicy = serde_json::from_str(&raw).unwrap();
    parsed.validate().unwrap();
    assert!(parsed.delivery_selection.is_none());
    assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
}

#[tokio::test]
#[ignore = "requires the existing disposable CICD_TEST_DATABASE_URL PostgreSQL"]
async fn owner_selection_pg_exact_latest_attempt_duplicates_and_path_aliases() {
    use sqlx::Connection;
    let url =
        std::env::var("CICD_TEST_DATABASE_URL").expect("explicit owned test database required");
    let mut connection = sqlx::PgConnection::connect(&url).await.unwrap();
    let mut tx = connection.begin().await.unwrap();
    // Connection-private SQL projection fixture; real ledger/auth custody remains covered by
    // the existing R HTTP/PG cases. No product tables or migration constraints are replaced.
    sqlx::raw_sql(
        "SET LOCAL search_path=pg_temp; \
         CREATE TEMP TABLE stages(id uuid,pipeline_id uuid,name text) ON COMMIT DROP; \
         CREATE TEMP TABLE jobs(id uuid,stage_id uuid,name text,artifact_paths text[]) ON COMMIT DROP; \
         CREATE TEMP TABLE execution_attempts(id uuid,job_id uuid,attempt_no int) ON COMMIT DROP; \
         CREATE TEMP TABLE artifacts(id uuid,job_id uuid,attempt_id uuid,name text,purged_at timestamptz,expires_at timestamptz) ON COMMIT DROP;",
    ).execute(&mut *tx).await.unwrap();
    let c = candidate();
    let a = &c.artifacts[0];
    let stage = Uuid::new_v4();
    sqlx::query("INSERT INTO stages VALUES($1,$2,'build')")
        .bind(stage)
        .bind(c.pipeline_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO jobs VALUES($1,$2,'package',ARRAY['dist/bundle.tar'])")
        .bind(a.job_id)
        .bind(stage)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO execution_attempts VALUES($1,$2,1)")
        .bind(a.attempt_id)
        .bind(a.job_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO artifacts VALUES($1,$2,$3,'dist__bundle.tar',NULL,clock_timestamp()+interval '1 hour')")
        .bind(a.artifact_id).bind(a.job_id).bind(a.attempt_id).execute(&mut *tx).await.unwrap();
    let p: Policy = serde_json::from_value(policy()).unwrap();
    assert!(resolve(&mut tx, None, &c).await.unwrap().is_none());
    assert_eq!(
        resolve(&mut tx, Some(&p), &c)
            .await
            .unwrap()
            .unwrap()
            .artifact
            .artifact_id,
        a.artifact_id
    );
    let mut wrong = p.clone();
    wrong.stage_name = "foreign".into();
    assert!(resolve(&mut tx, Some(&wrong), &c).await.is_err());
    sqlx::query("INSERT INTO artifacts VALUES($1,$2,$3,'dist__bundle.tar',NULL,clock_timestamp()+interval '1 hour')")
        .bind(Uuid::new_v4()).bind(a.job_id).bind(a.attempt_id).execute(&mut *tx).await.unwrap();
    assert!(resolve(&mut tx, Some(&p), &c).await.is_err());
    sqlx::query("DELETE FROM artifacts WHERE id<>$1")
        .bind(a.artifact_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    for alias in ["dist__bundle.tar", "dist\\bundle.tar", "dist/bundle.tar"] {
        sqlx::query("UPDATE jobs SET artifact_paths=$1")
            .bind(vec!["dist/bundle.tar", alias])
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(resolve(&mut tx, Some(&p), &c).await.is_err());
    }
    sqlx::query("UPDATE jobs SET artifact_paths=ARRAY['dist/bundle.tar']")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO execution_attempts VALUES($1,$2,2)")
        .bind(Uuid::new_v4())
        .bind(a.job_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(resolve(&mut tx, Some(&p), &c).await.is_err());
    tx.rollback().await.unwrap();
}
