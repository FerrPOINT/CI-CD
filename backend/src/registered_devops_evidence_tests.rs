use super::*;
use crate::delegated_credentials::TaskCredentialContext;
use serde_json::json;

fn fixture() -> (
    EvidenceQuery,
    TaskPipelineJournal,
    WorkspaceOperationReceipt,
) {
    let task = Uuid::new_v4();
    let intent = TaskWorkspaceIntent {
        operation_key: "developer:original".into(),
        binding: WorkspaceTaskBinding {
            tracker_instance_id: Uuid::new_v4().to_string(),
            tracker_project_id: Uuid::new_v4(),
            task_id: task,
            root_task_id: task,
            assignment_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            routing_snapshot_id: Uuid::new_v4(),
            requirement_revision: 1,
            fencing_token: 1,
            assignment_hash: "a".repeat(64),
            workflow_task_ref: "SDLC-27".into(),
        },
        repository_id: Uuid::new_v4(),
        source_commit: "b".repeat(40),
        role: WorkspaceRole::Developer,
        agent_id: Uuid::new_v4(),
        tracker_lease_id: Uuid::new_v4(),
        holder_subject: "fixture-fleet".into(),
        expected_lease_version: 1,
    };
    let project = Uuid::new_v4();
    let context = TaskCredentialContext {
        version: 1,
        project_id: project,
        intent: intent.clone(),
        run_id: Uuid::new_v4(),
        chat_id: Uuid::new_v4(),
        machine_subject: "fixture-native-machine".into(),
        trigger_id: Uuid::new_v4(),
        variables_sha256: "c".repeat(64),
    };
    let attempt = Uuid::new_v4();
    let request = intent
        .request(
            Uuid::new_v4(),
            attempt,
            1,
            format!("attempt-{attempt}-1-{}", Uuid::new_v4().simple()),
        )
        .unwrap();
    let receipt = WorkspaceOperationReceipt {
        schema: "forge/workspace-operation-receipt/v1".into(),
        operation_id: Uuid::new_v4(),
        project_id: project,
        request_hash: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&request).unwrap())
        ),
        request,
        recorded_at: chrono::Utc::now(),
        status: WorkspaceOperationStatus::Blocked,
        blockers: vec![WorkspaceOperationBlocker::TrackerAdmissionUnavailable],
        physical_source_observed: false,
        dispatch_allowed: false,
    };
    let record = TaskPipelineJournal {
        pipeline_id: Uuid::new_v4(),
        project_id: project,
        service_account_id: None,
        api_token_id: None,
        delegated_credential: Some(sqlx::types::Json(json!({
            "subject":Uuid::new_v4(), "fingerprint":"d".repeat(64),
            "context_hash":digest(&context).unwrap(), "context":context,
            "encrypted_child":"fixture-private-ciphertext-must-not-be-read"
        }))),
        preparation_job_id: Uuid::new_v4(),
        intent: sqlx::types::Json(intent),
        preparation_request: Some(sqlx::types::Json(receipt.request.clone())),
    };
    let binding = &receipt.request.binding;
    let query = EvidenceQuery {
        registry_instance_id: Uuid::new_v4(),
        namespace_id: Uuid::new_v4(),
        base_ref: "refs/heads/main".into(),
        base_commit: "e".repeat(40),
        delivery_configuration_name: "pipeline".into(),
        project_id: project,
        source_pipeline_id: record.pipeline_id,
        operation_key: receipt.request.operation_key.clone(),
        source_commit: receipt.request.source_commit.clone(),
        request_hash: receipt.request_hash.clone(),
        task_id: binding.task_id,
        root_task_id: binding.root_task_id,
        assignment_id: binding.assignment_id,
        execution_id: binding.execution_id,
        fencing_token: binding.fencing_token,
    };
    (query, record, receipt)
}

#[test]
fn closed_exact_selectors_reject_authority_fields_and_unsafe_keys() {
    let (query, _, _) = fixture();
    assert!(query.source_query().is_ok());
    let valid = json!({
        "registry_instance_id":query.registry_instance_id, "namespace_id":query.namespace_id,
        "base_ref":query.base_ref, "base_commit":query.base_commit,
        "delivery_configuration_name":query.delivery_configuration_name,
        "project_id":query.project_id, "source_pipeline_id":query.source_pipeline_id,
        "operation_key":query.operation_key, "source_commit":query.source_commit,
        "request_hash":query.request_hash, "task_id":query.task_id, "root_task_id":query.root_task_id,
        "assignment_id":query.assignment_id, "execution_id":query.execution_id,
        "fencing_token":query.fencing_token,
    });
    assert!(serde_json::from_value::<EvidenceQuery>(valid.clone()).is_ok());
    for field in [
        "accepted",
        "dispatch_allowed",
        "server_evidence",
        "role",
        "target_policy",
    ] {
        let mut injected = valid.clone();
        injected[field] = json!(true);
        let error = serde_json::from_value::<EvidenceQuery>(injected)
            .err()
            .unwrap();
        assert!(error.to_string().contains("unknown field"));
        assert!(error.to_string().contains(field));
    }
    for key in ["", "../key", " key", "key?token=x"] {
        let mut invalid = query.clone();
        invalid.operation_key = key.into();
        assert!(invalid.source_query().is_err());
    }
    let mut invalid = query.clone();
    invalid.base_ref = "HEAD".into();
    assert!(invalid.source_query().is_err());
    let mut invalid = query;
    invalid.request_hash = "0".repeat(64);
    assert!(invalid.source_query().is_err());
}

#[test]
fn retained_developer_receipt_not_child_token_is_the_owner_locator() {
    let (query, record, receipt) = fixture();
    assert!(retained_matches(&record, &receipt, &query, receipt.request.repository_id).is_ok());
    // The deliberately unusable ciphertext above proves no historical token needs decrypting.
    let mut other = query.clone();
    other.assignment_id = Uuid::new_v4();
    assert!(retained_matches(&record, &receipt, &other, receipt.request.repository_id).is_err());
    let mut other = query.clone();
    other.source_pipeline_id = Uuid::new_v4();
    assert!(retained_matches(&record, &receipt, &other, receipt.request.repository_id).is_err());
    let mut other = query;
    other.source_commit = "f".repeat(40);
    assert!(retained_matches(&record, &receipt, &other, receipt.request.repository_id).is_err());
    assert!(retained_matches(&record, &receipt, &other, Uuid::new_v4()).is_err());
}

#[test]
fn legacy_sat_mixed_custody_missing_receipt_and_tampered_context_deny() {
    let (query, mut record, receipt) = fixture();
    record.service_account_id = Some(Uuid::new_v4());
    assert!(retained_matches(&record, &receipt, &query, receipt.request.repository_id).is_err());
    record.service_account_id = None;
    record.api_token_id = Some(Uuid::new_v4());
    assert!(retained_matches(&record, &receipt, &query, receipt.request.repository_id).is_err());
    record.api_token_id = None;
    record.preparation_request = None;
    assert!(retained_matches(&record, &receipt, &query, receipt.request.repository_id).is_err());
    record.preparation_request = Some(sqlx::types::Json(receipt.request.clone()));
    record.delegated_credential.as_mut().unwrap().0["context_hash"] = json!("f".repeat(64));
    assert!(retained_matches(&record, &receipt, &query, receipt.request.repository_id).is_err());
    record.delegated_credential = None;
    assert!(retained_matches(&record, &receipt, &query, receipt.request.repository_id).is_err());
}

#[test]
fn configuration_readback_requires_exact_project_and_repository() {
    let (query, _, receipt) = fixture();
    let repository = receipt.request.repository_id;
    let mut source = VerifiedSource {
        namespace: query.source_query().unwrap().namespace(),
        forge_instance_id: Uuid::new_v4(),
        repository_id: repository,
        public_name: "fixture/repo".into(),
        base_ref: query.base_ref.clone(),
        base_commit: query.base_commit.clone(),
        default_ref: query.base_ref.clone(),
        protected_refs: vec![],
        observed_at: chrono::Utc::now(),
        delivery_configuration: Some(crate::registered_source::DeliveryConfigurationReadback {
            id: query.project_id,
            name: query.delivery_configuration_name.clone(),
            repository_id: repository,
            repository_name: "fixture/repo".into(),
        }),
    };
    assert!(configuration_matches(&source, &query, repository));
    source.delivery_configuration.as_mut().unwrap().id = Uuid::new_v4();
    assert!(!configuration_matches(&source, &query, repository));
    source.delivery_configuration.as_mut().unwrap().id = query.project_id;
    source
        .delivery_configuration
        .as_mut()
        .unwrap()
        .repository_id = Uuid::new_v4();
    assert!(!configuration_matches(&source, &query, repository));
    source.delivery_configuration = None;
    assert!(!configuration_matches(&source, &query, repository));
}

#[test]
fn candidate_requires_actual_observed_artifacts_and_never_dispatch_authority() {
    let (query, _, receipt) = fixture();
    let mut value = CandidateEvidenceReadback {
        schema: "forge/candidate-evidence-readback/v1".into(),
        operation_receipt: receipt,
        pipeline_id: query.source_pipeline_id,
        observed_at: chrono::Utc::now(),
        source_commit: query.source_commit.clone(),
        config_sha256: Some("a".repeat(64)),
        plan_sha256: Some("b".repeat(64)),
        candidate_observed: true,
        artifacts: vec![CandidateArtifactEvidence {
            artifact_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            sha256: "c".repeat(64),
            size_bytes: 1,
        }],
        blockers: vec!["tracker_admission_unavailable".into()],
        dispatch_allowed: false,
        deployment_verified: false,
        acceptance_verified: false,
    };
    assert!(candidate_matches(&value, &query));
    value.candidate_observed = false;
    assert!(!candidate_matches(&value, &query));
    value.candidate_observed = true;
    value.dispatch_allowed = true;
    assert!(!candidate_matches(&value, &query));
    value.dispatch_allowed = false;
    value.artifacts.clear();
    assert!(!candidate_matches(&value, &query));
}

#[tokio::test]
async fn route_denies_missing_reader_before_database_or_files() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    let app = protect(Router::new().route("/proof", get(|| async { "must-not-run" })));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/proof")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
