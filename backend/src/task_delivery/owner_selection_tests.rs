use super::*;
use crate::task_delivery::probes::{DeliveryPolicy, OwnerSelectionAction};
use serde_json::json;

fn artifact() -> CandidateArtifactEvidence {
    CandidateArtifactEvidence {
        artifact_id: Uuid::new_v4(),
        job_id: Uuid::new_v4(),
        attempt_id: Uuid::new_v4(),
        sha256: "a".repeat(64),
        size_bytes: 42,
    }
}

#[test]
fn explicit_selection_never_comes_from_inventory_order_or_foreign_ids() {
    let observed = vec![artifact()];
    assert!(selected_artifact(&observed, &[]).is_err());
    assert!(selected_artifact(&observed, &[Uuid::new_v4()]).is_err());
    assert_eq!(
        selected_artifact(&observed, &[observed[0].artifact_id])
            .unwrap()
            .artifact_id,
        observed[0].artifact_id
    );
    assert!(
        selected_artifact(
            &observed,
            &[observed[0].artifact_id, observed[0].artifact_id]
        )
        .is_err()
    );
    let id = observed[0].artifact_id;
    let mut duplicate = artifact();
    duplicate.artifact_id = id;
    assert!(selected_artifact(&[observed.into_iter().next().unwrap(), duplicate], &[id]).is_err());
}

#[test]
fn malformed_or_empty_selected_artifact_cannot_be_published() {
    for change in ["zero", "digest", "job", "attempt"] {
        let mut value = artifact();
        match change {
            "zero" => value.size_bytes = 0,
            "digest" => value.sha256 = "not-a-digest".into(),
            "job" => value.job_id = Uuid::nil(),
            _ => value.attempt_id = Uuid::nil(),
        }
        let id = value.artifact_id;
        assert!(selected_artifact(&[value], &[id]).is_err(), "{change}");
    }
}

#[test]
fn absent_selector_keeps_legacy_policy_hash_and_explicit_selector_changes_it() {
    let mut policy:DeliveryPolicy=serde_json::from_value(json!({"origin":"http://127.0.0.1:3456","healthPath":"/health","healthBodySha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","acceptancePath":"/acceptance","acceptanceBodySha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","taskExecution":{"environment":"nonproduction","environmentId":"00000000-0000-0000-0000-000000000001","repositoryId":"00000000-0000-0000-0000-000000000002","trackerInstanceId":"tracker-test","trackerProjectId":"00000000-0000-0000-0000-000000000003","holderSubject":"fleet:test"}})).unwrap();
    assert_eq!(
        policy.sha256().unwrap(),
        "c866ce2c3c4e98556ff2b7ec2c3196f1554c36e2a8fd9c2e90ad9ac8d32caaaa"
    );
    assert!(
        serde_json::to_value(&policy)
            .unwrap()
            .get("taskDeliverySelection")
            .is_none()
    );
    policy.task_delivery_selection = Some(OwnerSelectionPolicy {
        action: OwnerSelectionAction::Deploy,
        job_name: "package application".into(),
        artifact_name: "dist/site.zip".into(),
    });
    policy.validate().unwrap();
    assert_ne!(
        policy.sha256().unwrap(),
        "c866ce2c3c4e98556ff2b7ec2c3196f1554c36e2a8fd9c2e90ad9ac8d32caaaa"
    );
    let mut no_target = policy.clone();
    no_target.task_execution = None;
    assert!(no_target.validate().is_err());
    for bad in [
        "".to_owned(),
        " leading".to_owned(),
        "line\nbreak".to_owned(),
        "x".repeat(257),
    ] {
        let mut changed = policy.clone();
        changed
            .task_delivery_selection
            .as_mut()
            .unwrap()
            .artifact_name = bad;
        assert!(changed.validate().is_err());
    }
    let mut invalid = serde_json::to_value(&policy).unwrap();
    invalid["taskDeliverySelection"]["action"] = json!("approve-production");
    assert!(serde_json::from_value::<DeliveryPolicy>(invalid).is_err());
}

#[test]
fn selected_wire_matches_the_consumer_contract_without_policy_names() {
    let selected = OwnerDeliverySelection {
        schema: "forge/owner-delivery-selection/v1",
        action: "deploy",
        source_pipeline_id: Uuid::new_v4(),
        config_sha256: "b".repeat(64),
        plan_sha256: "c".repeat(64),
        artifact: artifact(),
    };
    let value = serde_json::to_value(&selected).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 6);
    assert_eq!(
        value["sourcePipelineId"],
        selected.source_pipeline_id.to_string()
    );
    assert_eq!(
        value["artifact"],
        serde_json::to_value(&selected.artifact).unwrap()
    );
    assert!(value.get("jobName").is_none());
    assert!(value.get("artifactName").is_none());
}
