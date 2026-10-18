use super::*;
use crate::api::dto::{Pipeline, PipelinePlan};
use crate::domain::sdlc_workspace::WorkspaceRole;
use pdlc_auth_core::AuthContext;

fn fixture() -> (Identity, StoredCredential, ledger::TaskPipelineJournal) {
    let (intent, live) = super::super::tests::fixture(WorkspaceRole::Reviewer, "Review");
    let context = TaskCredentialContext {
        version: 1,
        project_id: Uuid::new_v4(),
        intent: intent.clone(),
        run_id: serde_json::from_value(live["receipt"]["binding"]["run_id"].clone()).unwrap(),
        chat_id: serde_json::from_value(live["receipt"]["binding"]["chat_id"].clone()).unwrap(),
        machine_subject: "role-agent".into(),
        trigger_id: Uuid::new_v4(),
        variables_sha256: "a".repeat(64),
    };
    let subject = Uuid::new_v4();
    let config = SdlcWorkspaceConfig {
        delegated_subject: Some(subject),
        ..Default::default()
    };
    let central = AuthContext {
        user_id: subject.to_string(),
        email: Some("fixture@example.invalid".into()),
        scopes: [
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            context.grant().unwrap(),
        ]
        .into(),
        role: None,
        session_id: None,
        token: format!("sdlc_pat_{}", "a".repeat(43)),
    };
    let identity = Identity::verified(&central, &config).unwrap();
    let stored = identity
        .store(
            context,
            &SecretsConfig {
                key: Some([17; 32]),
            },
        )
        .unwrap();
    let record = ledger::TaskPipelineJournal {
        pipeline_id: Uuid::new_v4(),
        project_id: stored.context.project_id,
        service_account_id: None,
        api_token_id: None,
        delegated_credential: Some(sqlx::types::Json(stored.value().unwrap())),
        preparation_job_id: Uuid::new_v4(),
        intent: sqlx::types::Json(intent),
        preparation_request: None,
    };
    (identity, stored, record)
}

fn detail(record: &ledger::TaskPipelineJournal) -> PipelineDetail {
    PipelineDetail {
        pipeline: Pipeline {
            id: record.pipeline_id,
            project_id: record.project_id,
            git_ref: record.intent.source_commit.clone(),
            status: "failed".into(),
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: None,
        },
        plan: Some(PipelinePlan {
            pipeline_id: record.pipeline_id,
            config_source: ".forge-ci.yml".into(),
            parser_version: "fixture".into(),
            git_ref: record.intent.source_commit.clone(),
            resolved_commit_sha: Some(record.intent.source_commit.clone()),
            config_sha256: "b".repeat(64),
            plan_sha256: "c".repeat(64),
            raw_config: "fixture".into(),
            plan: json!({}),
            created_at: chrono::Utc::now(),
        }),
        stages: vec![],
    }
}

#[test]
fn task_detail_exact_current_child_binds_original_pipeline_and_preserves_failed_ci() {
    let (identity, stored, record) = fixture();
    bind_read(&identity, record.pipeline_id, &record, &stored).unwrap();
    let detail = detail(&record);
    let before = serde_json::to_value(&detail).unwrap();
    bound_detail(&detail, &record).unwrap();
    assert_eq!(before["pipeline"]["status"], "failed");
    assert_eq!(serde_json::to_value(detail).unwrap(), before);
}

#[test]
fn task_detail_cross_project_and_other_pipeline_are_denied() {
    let (identity, stored, mut record) = fixture();
    assert!(bind_read(&identity, Uuid::new_v4(), &record, &stored).is_err());
    record.project_id = Uuid::new_v4();
    assert!(bind_read(&identity, record.pipeline_id, &record, &stored).is_err());
}

#[test]
fn task_detail_cross_task_assignment_fence_and_source_are_denied() {
    for change in 0..6 {
        let (identity, stored, mut record) = fixture();
        match change {
            0 => record.intent.binding.task_id = Uuid::new_v4(),
            1 => record.intent.binding.assignment_id = Uuid::new_v4(),
            2 => record.intent.binding.execution_id = Uuid::new_v4(),
            3 => record.intent.binding.fencing_token += 1,
            4 => record.intent.source_commit = "f".repeat(40),
            _ => record.intent.agent_id = Uuid::new_v4(),
        }
        assert!(bind_read(&identity, record.pipeline_id, &record, &stored).is_err());
    }
}

#[test]
fn task_detail_superseded_child_foreign_subject_and_legacy_sat_are_denied() {
    for change in 0..5 {
        let (identity, mut stored, mut record) = fixture();
        match change {
            0 => stored.fingerprint = "f".repeat(64),
            1 => stored.subject = Uuid::new_v4(),
            2 => record.service_account_id = Some(Uuid::new_v4()),
            3 => record.api_token_id = Some(Uuid::new_v4()),
            _ => record.delegated_credential = None,
        }
        assert!(bind_read(&identity, record.pipeline_id, &record, &stored).is_err());
    }
}

#[test]
fn task_detail_response_cannot_substitute_project_pipeline_or_plan_commit() {
    let (_, _, record) = fixture();
    for change in 0..5 {
        let mut detail = detail(&record);
        match change {
            0 => detail.pipeline.project_id = Uuid::new_v4(),
            1 => detail.pipeline.id = Uuid::new_v4(),
            2 => detail.plan.as_mut().unwrap().pipeline_id = Uuid::new_v4(),
            3 => detail.plan.as_mut().unwrap().resolved_commit_sha = Some("f".repeat(40)),
            _ => detail.plan.as_mut().unwrap().resolved_commit_sha = None,
        }
        assert!(bound_detail(&detail, &record).is_err());
    }
    let mut pending = detail(&record);
    pending.plan = None;
    pending.pipeline.status = "queued".into();
    bound_detail(&pending, &record).unwrap();
    assert_eq!(pending.pipeline.status, "queued");
}
