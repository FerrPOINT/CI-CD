//! Populated 0091 -> 0092 compatibility, not evidence of actual runner execution.
use super::*;
use cicd::domain::sdlc_workspace::{
    WorkspaceAccess, WorkspaceOperationBlocker, WorkspaceOperationReceipt,
    WorkspaceOperationRequest, WorkspaceOperationStatus, WorkspaceRole, WorkspaceTaskBinding,
};
use cicd::store::sdlc_workspace as ledger;
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand, ResourceKind, ResourceRef};

// Full values, including nullable completion provenance and original JSON, not just row counts.
const SNAPSHOT: &str = "SELECT jsonb_build_object(
    'projects',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM projects r),
    'pipelines',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM pipelines r),
    'plans',(SELECT jsonb_agg(to_jsonb(r) ORDER BY pipeline_id) FROM pipeline_plans r),
    'triggers',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM pipeline_triggers r),
    'stages',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM stages r),
    'jobs',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM jobs r),
    'attempts',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM execution_attempts r),
    'leases',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM job_leases r),
    'receipts',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM sdlc_workspace_operations r),
    'repositories',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM repositories r),
    'catalog',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM repository_catalog r),
    'aliases',(SELECT jsonb_agg(to_jsonb(r) ORDER BY alias) FROM repository_aliases r),
    'groups',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM git_groups r),
    'bindings',(SELECT jsonb_agg(to_jsonb(r) ORDER BY resource_id) FROM forge_namespace_bindings r),
    'workspace_projects',(SELECT jsonb_agg(to_jsonb(r) ORDER BY registry_instance_id,namespace_id) FROM forge_workspace_projects r))";

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; no Git, runner or stand needed"]
async fn populated_0091_to_0092_preserves_workspace_pipeline_history_and_old_guards() {
    let full_catalog = cicd::migrations().await.unwrap();
    // Keep this historical 0091 -> 0092 regression exact as later migrations are added.
    let catalog = migration_catalog_subset(&full_catalog, 92);
    assert_eq!(
        catalog.iter().map(|m| m.version).collect::<Vec<_>>(),
        (1..=40).chain([90, 91, 92]).collect::<Vec<_>>()
    );
    let prior = migration_catalog_subset(&catalog, 91);
    let (pool, admin, schema) = migration_catalog_empty_pool().await;
    prior.run(&pool).await.unwrap();
    let (project, repository, group, registry, namespace) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let command = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: registry,
            namespace_id: namespace,
        },
        resource: ResourceRef {
            instance_id: Uuid::new_v4(),
            kind: ResourceKind::GitGroup,
            resource_id: group,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: None,
    };
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Historical group')")
        .bind(group)
        .bind(format!("history-{}", group.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
        .bind(group).bind(registry).bind(namespace).bind(serde_json::to_value(command).unwrap()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(format!("history_{}", repository.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
        .bind(repository)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'Historical config','https://example.invalid/history.git',$2)")
        .bind(project).bind(repository).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO forge_workspace_projects(registry_instance_id,namespace_id,tracker_instance_id,tracker_project_id,generation,name,project_key,state,observed_at) VALUES($1,$2,$3,$4,1,'Historical project','HIST','active',now())")
        .bind(registry).bind(namespace).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    let (pipeline, stage, job, attempt, lease, subject) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
        .bind(subject)
        .bind(format!("history-{subject}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,commit_sha,status,finished_at) VALUES($1,$2,'main',$3,'success',now())")
        .bind(pipeline).bind(project).bind("b".repeat(40)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_plans(pipeline_id,config_source,parser_version,git_ref,resolved_commit_sha,config_sha256,plan_sha256,raw_config,plan) VALUES($1,'legacy_template','history/1','main',$2,$3,$4,'historical config','{\"stages\":[]}'::jsonb)")
        .bind(pipeline).bind("b".repeat(40)).bind("c".repeat(64)).bind("d".repeat(64)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_triggers(id,project_id,source,idempotency_key,request_fingerprint,pipeline_id) VALUES($1,$2,'api','original-trigger',$3,$4)")
        .bind(Uuid::new_v4()).bind(project).bind("a".repeat(64)).bind(pipeline).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'historical',0,'success')")
        .bind(stage).bind(pipeline).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status,finished_at) VALUES($1,$2,'historical','unused','unused',0,'success',now())")
        .bind(job).bind(stage).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,finished_at,exit_code) VALUES($1,$2,1,'success',now(),0)")
        .bind(attempt).bind(job).execute(&pool).await.unwrap();
    // Historical terminal rows do not prove an authenticated completion ACK.
    sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_status,lease_expires_at,completed_at,terminal_status) VALUES($1,$2,$3,'historical',1,'completed',now()-interval '1 day',now()-interval '1 day','success')")
        .bind(lease).bind(job).bind(attempt).execute(&pool).await.unwrap();
    let request = WorkspaceOperationRequest {
        contract_version: 1,
        operation_key: "historical:original".into(),
        binding: WorkspaceTaskBinding {
            // Valid 0040 history, deliberately NOT a new 0092 UUID-only intent.
            tracker_instance_id: "historical-tracker".into(),
            tracker_project_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            root_task_id: Uuid::new_v4(),
            assignment_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            routing_snapshot_id: Uuid::new_v4(),
            requirement_revision: 3,
            fencing_token: 11,
            assignment_hash: "a".repeat(64),
            workflow_task_ref: "SDLC-17".into(),
        },
        repository_id: repository,
        source_commit: "b".repeat(40),
        lease_id: lease,
        attempt_id: attempt,
        workspace_generation: 1,
        workspace_id: format!("attempt-{attempt}-1-{}", Uuid::new_v4().simple()),
        role: WorkspaceRole::Analyst,
        access: WorkspaceAccess::ReadOnly,
    };
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
    // Seed the 0091 schema using its original 0040 insert shape, not the
    // current store writer whose delegated columns are introduced by 0094.
    let r = &receipt.request;
    let b = &r.binding;
    sqlx::query("INSERT INTO sdlc_workspace_operations (id,project_id,service_account_id,operation_key,request_hash,task_id,root_task_id,assignment_id,execution_id,fencing_token,lease_id,attempt_id,workspace_generation,repository_id,source_commit,receipt,recorded_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)")
        .bind(receipt.operation_id).bind(receipt.project_id).bind(subject).bind(&r.operation_key)
        .bind(&receipt.request_hash).bind(b.task_id).bind(b.root_task_id).bind(b.assignment_id)
        .bind(b.execution_id).bind(b.fencing_token).bind(r.lease_id).bind(r.attempt_id)
        .bind(r.workspace_generation).bind(r.repository_id).bind(&r.source_commit)
        .bind(sqlx::types::Json(&receipt)).bind(receipt.recorded_at).execute(&pool).await.unwrap();
    let before: serde_json::Value = sqlx::query_scalar(SNAPSHOT).fetch_one(&pool).await.unwrap();
    let history = migration_catalog_history(&pool).await;
    catalog.run(&pool).await.unwrap();
    let upgraded: serde_json::Value = sqlx::query_scalar(SNAPSHOT).fetch_one(&pool).await.unwrap();
    assert_eq!(
        upgraded, before,
        "0092 must not rewrite old task/pipeline/config/namespace history"
    );
    assert_eq!(upgraded["leases"].as_array().unwrap().len(), 1);
    assert!(upgraded["leases"][0]["acknowledged_at"].is_null());
    assert!(
        upgraded["leases"][0]["completion_received_at"].is_null(),
        "historical terminal status is not a new authenticated ACK"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM task_pipeline_intents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0,
        "no automatic enrollment of legacy history"
    );
    let after = migration_catalog_history(&pool).await;
    assert_eq!(
        serde_json::Value::Array(
            after
                .as_array()
                .unwrap()
                .iter()
                .filter(|m| m["version"].as_i64().unwrap() <= 91)
                .cloned()
                .collect()
        ),
        history,
        "old applied checksums and ledger rows unchanged"
    );
    assert_eq!(
        after.as_array().unwrap().len(),
        history.as_array().unwrap().len() + 1
    );
    assert_eq!(after.as_array().unwrap().last().unwrap()["version"], 92);
    let mut connection = pool.acquire().await.unwrap();
    let retained = ledger::find_receipt(
        &mut connection,
        project,
        subject,
        &receipt.request.operation_key,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(retained).unwrap(),
        serde_json::to_value(&receipt).unwrap()
    );
    drop(connection);
    for (sql, id, code) in [
        (
            "UPDATE sdlc_workspace_operations SET receipt=receipt WHERE id=$1",
            receipt.operation_id,
            "P0001",
        ),
        (
            "UPDATE repository_catalog SET slug=slug||'-changed' WHERE id=$1",
            repository,
            "42501",
        ),
        (
            "UPDATE projects SET repository_url='https://example.invalid/changed.git' WHERE id=$1",
            project,
            "42501",
        ),
    ] {
        let mut tx = pool.begin().await.unwrap();
        let error = sqlx::query(sql)
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        let actual = error
            .as_database_error()
            .and_then(|e| e.code())
            .map(|c| c.into_owned());
        tx.rollback().await.unwrap();
        assert_eq!(
            actual.as_deref(),
            Some(code),
            "old constraint must remain enforced"
        );
    }
    catalog.run(&pool).await.unwrap();
    assert_eq!(
        migration_catalog_history(&pool).await,
        after,
        "restart/reapply is a no-op"
    );
    assert_eq!(
        sqlx::query_scalar::<_, serde_json::Value>(SNAPSHOT)
            .fetch_one(&pool)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM task_pipeline_intents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let token_id = Uuid::new_v4();
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,project_id,scopes,principal_type,service_account_id) VALUES($1,'legacy-custody',$2,'fixture',$3,ARRAY['api:write'],'service_account',$4)")
        .bind(token_id).bind(cicd::auth::hash_token("forge_sat_fixture_legacy")).bind(project).bind(subject).execute(&pool).await.unwrap();
    let mut binding = receipt.request.binding.clone();
    binding.tracker_instance_id = Uuid::new_v4().to_string();
    binding.root_task_id = binding.task_id;
    let legacy_intent = cicd::domain::sdlc_workspace::TaskWorkspaceIntent {
        operation_key: "legacy:before-0094".into(),
        binding,
        repository_id: repository,
        source_commit: receipt.request.source_commit.clone(),
        role: WorkspaceRole::Analyst,
        agent_id: Uuid::new_v4(),
        tracker_lease_id: Uuid::new_v4(),
        holder_subject: "legacy-fixture".into(),
        expected_lease_version: 1,
    };
    let mut conn = pool.acquire().await.unwrap();
    ledger::append_task_pipeline(
        &mut conn,
        pipeline,
        project,
        subject,
        token_id,
        job,
        &legacy_intent,
    )
    .await
    .unwrap();
    drop(conn);
    let task_before_94: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM task_pipeline_intents t WHERE pipeline_id=$1")
            .bind(pipeline)
            .fetch_one(&pool)
            .await
            .unwrap();
    let current = cicd::migrations().await.unwrap();
    current.run(&pool).await.unwrap();
    let mut task_after_94: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM task_pipeline_intents t WHERE pipeline_id=$1")
            .bind(pipeline)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        task_after_94
            .as_object_mut()
            .unwrap()
            .remove("delegated_credential"),
        Some(serde_json::Value::Null)
    );
    assert_eq!(
        task_after_94, task_before_94,
        "populated legacy SAT task custody is byte-identical"
    );
    let mut after_94: serde_json::Value =
        sqlx::query_scalar(SNAPSHOT).fetch_one(&pool).await.unwrap();
    for row in after_94["receipts"].as_array_mut().unwrap() {
        let row = row.as_object_mut().unwrap();
        assert_eq!(
            row.remove("delegated_subject"),
            Some(serde_json::Value::Null)
        );
        assert_eq!(
            row.remove("delegated_context_hash"),
            Some(serde_json::Value::Null)
        );
    }
    assert_eq!(
        after_94, before,
        "0094 adds only nullable custody columns; old receipt/data bytes remain"
    );
    let fks: i64=sqlx::query_scalar("SELECT count(*) FROM pg_constraint WHERE connamespace=current_schema()::regnamespace AND contype='f' AND conname IN ('task_pipeline_intents_service_account_id_fkey','task_pipeline_intents_api_token_id_fkey','sdlc_workspace_operations_service_account_id_fkey')")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(fks, 3, "all original SAT FKs remain installed");
    let history_94 = migration_catalog_history(&pool).await;
    current.run(&pool).await.unwrap();
    assert_eq!(
        migration_catalog_history(&pool).await,
        history_94,
        "0094 restart does not rewrite history"
    );
    migration_catalog_cleanup(pool, admin, schema).await;
}
