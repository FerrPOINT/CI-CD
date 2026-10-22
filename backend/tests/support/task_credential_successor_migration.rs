//! Real populated 0094 -> 0095 upgrade; synthetic custody, never Auth/native admission.
use super::*;
use cicd::domain::sdlc_workspace::{
    TaskWorkspaceIntent, WorkspaceAccess, WorkspaceOperationBlocker, WorkspaceOperationReceipt,
    WorkspaceOperationRequest, WorkspaceOperationStatus, WorkspaceRole, WorkspaceTaskBinding,
};
use cicd::store::sdlc_workspace as ledger;
use cicd_app::repository_sync::Crypto;
use serde_json::{Value, json};

const BUSINESS: &str = "SELECT jsonb_build_object(
    'projects',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM projects r),
    'repositories',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM repositories r),
    'accounts',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM service_accounts r),
    'tokens',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM api_tokens r),
    'intents',(SELECT jsonb_agg(to_jsonb(r) ORDER BY pipeline_id) FROM task_pipeline_intents r),
    'receipts',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM sdlc_workspace_operations r),
    'pipelines',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM pipelines r),
    'plans',(SELECT jsonb_agg(to_jsonb(r) ORDER BY pipeline_id) FROM pipeline_plans r),
    'triggers',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM pipeline_triggers r),
    'stages',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM stages r),
    'jobs',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM jobs r),
    'queue',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM job_queue r),
    'attempts',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM execution_attempts r),
    'leases',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM job_leases r))";

const GUARDS: &str = "SELECT jsonb_object_agg(p.proname,
    jsonb_build_object('oid',p.oid,'definition',pg_get_functiondef(p.oid)))
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname=current_schema()
    AND p.proname IN ('guard_task_pipeline_intent','guard_delegated_workspace_receipt')";

async fn seed_0094(pool: &sqlx::PgPool, delegated: bool) {
    let (project, repository, subject, token_id, pipeline, stage, job, attempt, lease) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("history-{}", repository.simple()))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Credential history','https://example.invalid/history.git',$2)")
        .bind(project).bind(repository).execute(pool).await.unwrap();
    if !delegated {
        sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
            .bind(subject)
            .bind(format!("history-{subject}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,project_id,scopes,principal_type,service_account_id) VALUES($1,'history',$2,'fixture-only',$3,ARRAY['api:read','api:write'],'service_account',$4)")
            .bind(token_id).bind(cicd::auth::hash_token("history-not-an-issued-SAT"))
            .bind(project).bind(subject).execute(pool).await.unwrap();
    }
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status) VALUES($1,$2,$3,$3,'running')")
        .bind(pipeline).bind(project).bind("b".repeat(40)).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,'repository','history/1',$2,$2,$3,$4,'original config','{\"stages\":[]}'::jsonb)")
        .bind(pipeline).bind("b".repeat(40)).bind("c".repeat(64)).bind("d".repeat(64))
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_triggers(id,project_id,source,idempotency_key,request_fingerprint,pipeline_id) VALUES($1,$2,'api',$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(project).bind(format!("history:{pipeline}"))
        .bind("e".repeat(64)).bind(pipeline).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'prepare',0,'running')")
        .bind(stage).bind(pipeline).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'prepare','unused','unused',0,'running')")
        .bind(job).bind(stage).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'running')",
    )
    .bind(attempt)
    .bind(job)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_expires_at,acknowledged_at) VALUES($1,$2,$3,'embedded',1,now()+interval '1 hour',now())")
        .bind(lease).bind(job).bind(attempt).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO job_queue(id,job_id,attempt_id,pipeline_id,stage_id,state,leased_at,lease_id) VALUES($1,$2,$3,$4,$5,'leased',now(),$6)")
        .bind(Uuid::new_v4()).bind(job).bind(attempt).bind(pipeline).bind(stage).bind(lease)
        .execute(pool).await.unwrap();
    let queued_job = Uuid::new_v4();
    let queued_attempt = Uuid::new_v4();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'next','unused','unused',1,'queued')")
        .bind(queued_job).bind(stage).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO execution_attempts(id,job_id,attempt_no,status) VALUES($1,$2,1,'queued')",
    )
    .bind(queued_attempt)
    .bind(queued_job)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO job_queue(id,job_id,attempt_id,pipeline_id,stage_id) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(queued_job)
    .bind(queued_attempt)
    .bind(pipeline)
    .bind(stage)
    .execute(pool)
    .await
    .unwrap();

    let task = Uuid::new_v4();
    let intent = TaskWorkspaceIntent {
        operation_key: format!("history:{pipeline}"),
        binding: WorkspaceTaskBinding {
            tracker_instance_id: Uuid::new_v4().to_string(),
            tracker_project_id: Uuid::new_v4(),
            task_id: task,
            root_task_id: task,
            assignment_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            routing_snapshot_id: Uuid::new_v4(),
            requirement_revision: 3,
            fencing_token: 7,
            assignment_hash: "a".repeat(64),
            workflow_task_ref: "SDLC-17".into(),
        },
        repository_id: repository,
        source_commit: "b".repeat(40),
        role: WorkspaceRole::Analyst,
        agent_id: Uuid::new_v4(),
        tracker_lease_id: Uuid::new_v4(),
        holder_subject: "fixture-scheduler".into(),
        expected_lease_version: 1,
    };
    let mut conn = pool.acquire().await.unwrap();
    let context_hash = if delegated {
        let context = cicd::delegated_credentials::TaskCredentialContext {
            version: 1,
            project_id: project,
            intent: intent.clone(),
            run_id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            machine_subject: "fixture-native-machine".into(),
            trigger_id: Uuid::new_v4(),
            variables_sha256: "a".repeat(64),
        };
        let delegation =
            cicd::delegated_credentials::delegation_request(&context, "history:credential", 900)
                .unwrap();
        let hash = delegation["scopes"][2]
            .as_str()
            .unwrap()
            .strip_prefix("ci-cd:sdlc:task:v1:")
            .unwrap()
            .to_owned();
        // Real encryption of fixture-only bytes; no issued PAT or simulated Auth/native ACK.
        let token = "synthetic-history-not-an-issued-child";
        let plaintext =
            serde_json::to_string(&json!({"subject":subject,"context":context,"token":token}))
                .unwrap();
        let encrypted = cicd_infra::secrets::Secrets(Some([29; 32]))
            .seal(&plaintext)
            .unwrap();
        let credential = json!({"subject":subject,"fingerprint":cicd::auth::hash_token(token),
            "context_hash":hash,"context":context,"encrypted_child":encrypted});
        ledger::append_delegated_task_pipeline(
            &mut conn,
            pipeline,
            project,
            &credential,
            job,
            &intent,
        )
        .await
        .unwrap();
        Some(hash)
    } else {
        ledger::append_task_pipeline(
            &mut conn, pipeline, project, subject, token_id, job, &intent,
        )
        .await
        .unwrap();
        None
    };
    let request = WorkspaceOperationRequest {
        contract_version: 1,
        operation_key: intent.operation_key.clone(),
        binding: intent.binding.clone(),
        repository_id: repository,
        source_commit: intent.source_commit.clone(),
        lease_id: lease,
        attempt_id: attempt,
        workspace_generation: 1,
        workspace_id: format!("attempt-{attempt}-1-{}", Uuid::new_v4().simple()),
        role: intent.role,
        access: WorkspaceAccess::ReadOnly,
    };
    assert!(
        ledger::claim_task_preparation(&mut conn, pipeline, &request)
            .await
            .unwrap()
    );
    let receipt = WorkspaceOperationReceipt {
        schema: "forge/workspace-operation-receipt/v1".into(),
        operation_id: Uuid::new_v4(),
        project_id: project,
        request_hash: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&request).unwrap())
        ),
        request,
        recorded_at: Utc::now(),
        status: WorkspaceOperationStatus::Blocked,
        blockers: vec![
            WorkspaceOperationBlocker::TrackerAdmissionUnavailable,
            WorkspaceOperationBlocker::TrackerWorkspaceBindingUnavailable,
        ],
        physical_source_observed: false,
        dispatch_allowed: false,
    };
    if let Some(hash) = context_hash {
        ledger::append_delegated_receipt(&mut conn, subject, &hash, &receipt)
            .await
            .unwrap();
    } else {
        ledger::append_receipt(&mut conn, subject, &receipt)
            .await
            .unwrap();
    }
    assert!(
        ledger::claim_task_submission(&mut conn, pipeline, attempt)
            .await
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; populated 0094 -> 0095, no Auth/runner"]
async fn populated_0094_to_0095_preserves_original_custody_business_and_migration_history() {
    let (pool, admin, schema) = migration_catalog_empty_pool().await;
    let checked = pool.clone();
    // Join contains assertion panics so the existing schema cleanup also runs on failure.
    let result =
        tokio::spawn(async move {
            let full = cicd::migrations().await.unwrap();
            let prior = migration_catalog_subset(&full, 94);
            let successor = migration_catalog_subset(&full, 95);
            assert_eq!(
                prior.iter().map(|m| m.version).collect::<Vec<_>>(),
                (1..=40).chain([90, 91, 92, 93, 94]).collect::<Vec<_>>()
            );
            assert_eq!(
                successor.iter().map(|m| m.version).collect::<Vec<_>>(),
                (1..=40).chain([90, 91, 92, 93, 94, 95]).collect::<Vec<_>>()
            );
            prior.run(&checked).await.unwrap();
            assert!(
                sqlx::query_scalar::<_, bool>(
                    "SELECT to_regclass('task_credential_successors') IS NULL"
                )
                .fetch_one(&checked)
                .await
                .unwrap()
            );
            seed_0094(&checked, false).await;
            seed_0094(&checked, true).await;
            let before: Value = sqlx::query_scalar(BUSINESS)
                .fetch_one(&checked)
                .await
                .unwrap();
            for (table, count) in [
                ("intents", 2),
                ("receipts", 2),
                ("pipelines", 2),
                ("plans", 2),
                ("triggers", 2),
                ("jobs", 4),
                ("attempts", 4),
                ("queue", 4),
                ("leases", 2),
            ] {
                assert_eq!(before[table].as_array().unwrap().len(), count, "{table}");
            }
            let intents = before["intents"].as_array().unwrap();
            assert_eq!(
                intents
                    .iter()
                    .filter(|r| r["delegated_credential"].is_null())
                    .count(),
                1
            );
            assert!(intents.iter().all(|r| {
                !r["preparation_request"].is_null()
                    && r["submitted_attempts"].as_array().unwrap().len() == 1
            }));
            assert!(
                before["leases"].as_array().unwrap().iter().all(|r| {
                    r["completed_at"].is_null() && r["completion_received_at"].is_null()
                })
            );
            let history = migration_catalog_history(&checked).await;
            assert_eq!(history.as_array().unwrap().last().unwrap()["version"], 94);
            let guards: Value = sqlx::query_scalar(GUARDS)
                .fetch_one(&checked)
                .await
                .unwrap();
            assert_eq!(guards.as_object().unwrap().len(), 2);

            successor.run(&checked).await.unwrap();
            let upgraded = migration_catalog_history(&checked).await;
            let old_rows: Vec<_> = upgraded
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["version"].as_i64().unwrap() <= 94)
                .cloned()
                .collect();
            assert_eq!(
                json!(old_rows),
                history,
                "all old ledger fields/checksums retained"
            );
            assert_eq!(
                upgraded.as_array().unwrap().len(),
                history.as_array().unwrap().len() + 1
            );
            assert_eq!(upgraded.as_array().unwrap().last().unwrap()["version"], 95);
            let checksum: Vec<u8> =
                sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=95")
                    .fetch_one(&checked)
                    .await
                    .unwrap();
            assert_eq!(
                checksum.as_slice(),
                successor
                    .iter()
                    .find(|m| m.version == 95)
                    .unwrap()
                    .checksum
                    .as_ref()
            );

            for replay in 0..2 {
                if replay != 0 {
                    successor.run(&checked).await.unwrap();
                }
                assert_eq!(
                    sqlx::query_scalar::<_, Value>(BUSINESS)
                        .fetch_one(&checked)
                        .await
                        .unwrap(),
                    before,
                    "all original custody and business rows, including NULLs, retained"
                );
                assert_eq!(
                    sqlx::query_scalar::<_, Value>(GUARDS)
                        .fetch_one(&checked)
                        .await
                        .unwrap(),
                    guards,
                    "0092/0094 guard OIDs and complete definitions unchanged"
                );
                assert_eq!(migration_catalog_history(&checked).await, upgraded);
                assert_eq!(
                    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM task_credential_successors")
                        .fetch_one(&checked)
                        .await
                        .unwrap(),
                    0,
                    "migration/replay must not mint successor custody"
                );
            }
            // Qualify the same populated predecessor through the current cohort
            // as well; keep the exact 0094->0095 custody regression above.
            full.run(&checked).await.unwrap();
            let current = migration_catalog_history(&checked).await;
            let retained: Vec<_> = current
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["version"].as_i64().unwrap() <= 95)
                .cloned()
                .collect();
            assert_eq!(json!(retained), upgraded, "all 0095 ledger fields retained");
            assert_eq!(current.as_array().unwrap().last().unwrap()["version"], 99);
            assert_eq!(
                current.as_array().unwrap().len(),
                upgraded.as_array().unwrap().len() + 4
            );
            for _ in 0..2 {
                full.run(&checked).await.unwrap();
                assert_eq!(migration_catalog_history(&checked).await, current);
                assert_eq!(
                    sqlx::query_scalar::<_, Value>(BUSINESS)
                        .fetch_one(&checked)
                        .await
                        .unwrap(),
                    before
                );
                assert_eq!(
                    sqlx::query_scalar::<_, Value>(GUARDS)
                        .fetch_one(&checked)
                        .await
                        .unwrap(),
                    guards
                );
                assert_eq!(
                    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM task_credential_successors")
                        .fetch_one(&checked)
                        .await
                        .unwrap(),
                    0
                );
            }
        })
        .await;
    migration_catalog_cleanup(pool, admin, schema).await;
    result.unwrap();
}
