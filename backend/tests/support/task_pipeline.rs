use super::*;
use cicd::store::sdlc_workspace as ledger;
use serde_json::json;

/// Journal/HTTP locator fixture, not a claimed Tracker-admitted production runner execution.
#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL and MAIN task_pipeline_intents migration"]
async fn task_pipeline_original_custody_cas_receipt_locator_and_cleanup_are_retained() {
    let mut physical = PhysicalFixture::new().await;
    let f = &mut physical.f;
    f.request.binding.tracker_instance_id = Uuid::new_v4().to_string();
    f.request.binding.root_task_id = f.request.binding.task_id;
    let intent = TaskWorkspaceIntent {
        operation_key: f.request.operation_key.clone(),
        binding: f.request.binding.clone(),
        repository_id: f.request.repository_id,
        source_commit: f.request.source_commit.clone(),
        role: f.request.role,
        agent_id: Uuid::new_v4(),
        tracker_lease_id: Uuid::new_v4(),
        holder_subject: "fixture-scheduler".into(),
        expected_lease_version: 1,
    };
    intent.validate().unwrap();
    let (job,pipeline):(Uuid,Uuid)=sqlx::query_as("SELECT j.id,s.pipeline_id FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN stages s ON s.id=j.stage_id WHERE l.id=$1")
        .bind(f.request.lease_id).fetch_one(&f.pool).await.unwrap();
    sqlx::query("UPDATE job_leases SET runner_name='embedded' WHERE id=$1")
        .bind(f.request.lease_id)
        .execute(&f.pool)
        .await
        .unwrap();
    // Fresh INSERTs exercise the closed JSON CHECKs, not retained-row immutability.
    for (label, field, value) in [
        ("null execution ID", "executionId", serde_json::Value::Null),
        (
            "nil routing snapshot",
            "routingSnapshotId",
            json!(Uuid::nil()),
        ),
        ("unknown binding key", "unexpected", json!(true)),
    ] {
        let mut malformed = serde_json::to_value(&intent).unwrap();
        malformed["binding"][field] = value;
        let mut tx = f.pool.begin().await.unwrap();
        let error = sqlx::query("INSERT INTO task_pipeline_intents(pipeline_id,project_id,service_account_id,api_token_id,preparation_job_id,operation_key,intent) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(pipeline).bind(f.project).bind(f.subject).bind(f.token_id).bind(job)
            .bind(&intent.operation_key).bind(malformed).execute(&mut *tx).await.unwrap_err();
        let code = error.as_database_error().and_then(|error| error.code());
        tx.rollback().await.unwrap();
        assert_eq!(
            code.as_deref(),
            Some("23514"),
            "{label}: JSON CHECK must reject INSERT"
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_pipeline_intents WHERE pipeline_id=$1")
                .bind(pipeline)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "{label}: failed INSERT must leave no custody row");
    }
    let mut conn = f.pool.acquire().await.unwrap();
    ledger::append_task_pipeline(
        &mut conn, pipeline, f.project, f.subject, f.token_id, job, &intent,
    )
    .await
    .unwrap();
    drop(conn);
    let url = format!(
        "/api/v1/projects/{}/pipelines/{pipeline}/task-workspace",
        f.project
    );
    let (status, pending) = call(&f.app, "GET", &url, &f.read_token, String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending, serde_json::Value::Null);
    for (label, field, value) in [
        ("string contract version", "contractVersion", json!("1")),
        ("unknown preparation key", "unexpected", json!(true)),
    ] {
        let mut malformed = serde_json::to_value(&f.request).unwrap();
        malformed[field] = value;
        let mut tx = f.pool.begin().await.unwrap();
        let error = sqlx::query(
            "UPDATE task_pipeline_intents SET preparation_request=$2 WHERE pipeline_id=$1",
        )
        .bind(pipeline)
        .bind(malformed)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        let code = error.as_database_error().and_then(|error| error.code());
        tx.rollback().await.unwrap();
        assert_eq!(
            code.as_deref(),
            Some("23514"),
            "{label}: JSON CHECK must reject initial preparation"
        );
        let unchanged: bool = sqlx::query_scalar("SELECT preparation_request IS NULL AND intent=$2 AND cardinality(submitted_attempts)=0 FROM task_pipeline_intents WHERE pipeline_id=$1")
            .bind(pipeline).bind(serde_json::to_value(&intent).unwrap())
            .fetch_one(&f.pool).await.unwrap();
        assert!(
            unchanged,
            "{label}: failed UPDATE must retain untouched custody"
        );
    }
    let claim = |pool: sqlx::PgPool, request: WorkspaceOperationRequest| async move {
        let mut conn = pool.acquire().await.unwrap();
        ledger::claim_task_preparation(&mut conn, pipeline, &request)
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(
        claim(f.pool.clone(), f.request.clone()),
        claim(f.pool.clone(), f.request.clone())
    );
    assert_ne!(a, b, "one durable preparation custody claim only");
    // A new connection sees exactly the original identities; no reset or new generated key.
    let mut conn = f.pool.acquire().await.unwrap();
    let saved = ledger::find_task_pipeline(&mut conn, pipeline, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.intent.0, intent);
    assert_eq!(saved.preparation_request.as_ref().unwrap().0, f.request);
    assert!(
        !ledger::claim_task_preparation(&mut conn, pipeline, &f.request)
            .await
            .unwrap()
    );
    drop(conn);
    let (status, receipt) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["physicalSourceObserved"], true);
    assert_eq!(receipt["dispatchAllowed"], false);
    let (status, replay) = f.post(&f.request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt, replay);
    let attempt = f.request.attempt_id;
    let submit = |pool: sqlx::PgPool| async move {
        let mut conn = pool.acquire().await.unwrap();
        ledger::claim_task_submission(&mut conn, pipeline, attempt)
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(submit(f.pool.clone()), submit(f.pool.clone()));
    assert_ne!(a, b, "one original job submission only");
    let mut conn = f.pool.acquire().await.unwrap();
    assert!(
        ledger::task_attempt_submitted(&mut conn, pipeline, attempt)
            .await
            .unwrap()
    );
    drop(conn);
    let other = Fixture::new().await;
    let (status, _) = call(&f.app, "GET", &url, &other.read_token, String::new()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    physical.owned.record_completion("success").unwrap();
    physical.owned.acknowledge_completion("success").unwrap();
    cicd::runner_workspace::OwnedWorkspace::reopen(
        &physical.temp.join("workspaces"),
        &f.request.workspace_id,
    )
    .unwrap()
    .cleanup_after_ack()
    .await
    .unwrap();
    let (status, after) = call(&f.app, "GET", &url, &f.read_token, String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        after, receipt,
        "cleanup cannot erase or replace the original receipt"
    );
    let original: WorkspaceOperationReceipt = serde_json::from_value(after.clone()).unwrap();
    let mut conn = f.pool.acquire().await.unwrap();
    let candidate = ledger::candidate_pipeline(&mut conn, &original)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(candidate.pipeline_id, pipeline);
    assert_eq!(
        candidate.commit_sha.as_deref(),
        Some(original.request.source_commit.as_str())
    );
    assert!(!after.to_string().contains("leaseToken"));
    assert!(!after.to_string().contains(&f.token));
    assert!(!after.to_string().contains(&f.read_token));
    drop(conn);
    // Mutation attempts run in independent rolled-back transactions; retained rows stay intact.
    for sql in [
        "UPDATE task_pipeline_intents SET preparation_request=NULL WHERE pipeline_id=$1",
        "UPDATE task_pipeline_intents SET submitted_attempts='{}' WHERE pipeline_id=$1",
        "UPDATE task_pipeline_intents SET intent=jsonb_set(intent,'{sourceCommit}',to_jsonb(repeat('c',40))) WHERE pipeline_id=$1",
        "DELETE FROM task_pipeline_intents WHERE pipeline_id=$1",
    ] {
        let mut tx = f.pool.begin().await.unwrap();
        let refused = sqlx::query(sql)
            .bind(pipeline)
            .execute(&mut *tx)
            .await
            .is_err();
        tx.rollback().await.unwrap();
        assert!(refused, "immutable custody mutation must be rejected");
    }
}
