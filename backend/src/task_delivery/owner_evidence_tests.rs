use super::*;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("forge-owner-reader-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        for name in ["operations", "manifests", "objects"] {
            std::fs::create_dir(path.join(name)).unwrap();
        }
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn absent_pointers_require_empty_owner_inventory_not_just_null() {
    let temp = Temp::new();
    let value = pointers(&temp.0, &[], &["manifests", "objects"], |_| {
        panic!("not a manifest")
    });
    assert!(matches!(
        value,
        Ok((ManifestEvidence::Absent, ManifestEvidence::Absent))
    ));
    std::fs::write(temp.0.join("manifests/orphan.json"), b"{}").unwrap();
    assert!(pointers(&temp.0, &[], &["manifests", "objects"], |_| Ok(())).is_err());
    assert!(
        pointers(
            &temp.0,
            &[(Some("a".repeat(64)), DeliveryStatus::Verified)],
            &["manifests"],
            |_| Ok(())
        )
        .is_err()
    );
}

#[test]
fn pointer_receipt_stale_missing_torn_or_unverified_denies() {
    let temp = Temp::new();
    let hash = "a".repeat(64);
    let pointer = serde_json::to_vec(&serde_json::json!({"manifestSha256":hash})).unwrap();
    std::fs::write(temp.0.join("current.json"), &pointer).unwrap();
    assert!(pointers(&temp.0, &[], &["manifests"], |_| Ok(())).is_err());
    std::fs::write(temp.0.join("confirmed.json"), &pointer).unwrap();
    assert!(pointers(&temp.0, &[], &["manifests"], |_| Ok(())).is_err());
    assert!(
        pointers(
            &temp.0,
            &[(Some(hash.clone()), DeliveryStatus::Failed)],
            &["manifests"],
            |_| Ok(())
        )
        .is_err()
    );
    let known = [(Some(hash.clone()), DeliveryStatus::Verified)];
    assert!(
        pointers(&temp.0, &known, &["manifests"], |_| anyhow::bail!(
            "tampered manifest"
        ))
        .is_err()
    );
    assert!(
        pointers(&temp.0, &known, &["manifests"], |value| {
            assert_eq!(value, hash);
            Ok(())
        })
        .is_ok()
    );
}

#[test]
fn owner_errors_are_fixed_and_exclude_private_paths_and_receipts() {
    let error = unavailable();
    assert_eq!(error.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    // Inspect the serialized HTTP boundary, not Debug output from private owner errors.
    let value = serde_json::to_value(ManifestEvidence::Absent).unwrap();
    assert_eq!(value, serde_json::json!({"state":"absent"}));
}

#[tokio::test]
async fn owner_failure_response_contains_only_fixed_safe_code() {
    use axum::response::IntoResponse;
    let response = unavailable().into_response();
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("devops_owner_evidence_unavailable"));
    for private in [
        "/owner/",
        "encrypted_child",
        "token",
        "policy_path",
        "manifest.json",
    ] {
        assert!(!text.contains(private));
    }
}

#[test]
fn target_policy_cannot_cross_repository_or_tracker_project() {
    let binding = crate::domain::sdlc_workspace::WorkspaceTaskBinding {
        tracker_instance_id: Uuid::new_v4().to_string(),
        tracker_project_id: Uuid::new_v4(),
        task_id: Uuid::new_v4(),
        root_task_id: Uuid::new_v4(),
        assignment_id: Uuid::new_v4(),
        execution_id: Uuid::new_v4(),
        routing_snapshot_id: Uuid::new_v4(),
        requirement_revision: 1,
        fencing_token: 1,
        assignment_hash: "a".repeat(64),
        workflow_task_ref: "SDLC-27".into(),
    };
    let repository = Uuid::new_v4();
    let mut policy = probes::TaskExecutionPolicy {
        environment: "nonproduction".into(),
        environment_id: Uuid::new_v4(),
        repository_id: repository,
        tracker_instance_id: binding.tracker_instance_id.clone(),
        tracker_project_id: binding.tracker_project_id,
        holder_subject: "fixture-owner".into(),
    };
    assert!(matches_binding(&policy, repository, &binding));
    policy.tracker_project_id = Uuid::new_v4();
    assert!(!matches_binding(&policy, repository, &binding));
    policy.tracker_project_id = binding.tracker_project_id;
    assert!(!matches_binding(&policy, Uuid::new_v4(), &binding));
    policy.environment = "production".into();
    assert!(!matches_binding(&policy, repository, &binding));
}

#[test]
fn missing_existing_lock_never_creates_an_enrollment_file() {
    let temp = Temp::new();
    assert!(ReadLock::acquire(&temp.0).is_err());
    assert!(!temp.0.join(".lock").exists());
}

#[cfg(unix)]
#[test]
fn read_lock_does_not_write_and_excludes_inflight_publication() {
    let temp = Temp::new();
    std::fs::write(temp.0.join(".lock"), b"existing-owner-lock").unwrap();
    let before = std::fs::read(temp.0.join(".lock")).unwrap();
    let writer = Lock::acquire(&temp.0).unwrap();
    assert!(ReadLock::acquire(&temp.0).is_err());
    drop(writer);
    let reader = ReadLock::acquire(&temp.0).unwrap();
    assert!(Lock::acquire(&temp.0).is_err());
    drop(reader);
    assert_eq!(std::fs::read(temp.0.join(".lock")).unwrap(), before);
    let enrolled = temp.0.join("enrolled");
    std::fs::create_dir(&enrolled).unwrap();
    let target = Target::open(
        &enrolled,
        Uuid::new_v4(),
        DeliveryPolicy {
            origin: "http://127.0.0.1:3456".into(),
            health_path: "/health".into(),
            health_body_sha256: "a".repeat(64),
            acceptance_path: "/acceptance".into(),
            acceptance_body_sha256: "b".repeat(64),
            task_execution: None,
            delivery_selection: None,
        },
        true,
    )
    .unwrap();
    let marker = std::fs::read(enrolled.join("owner.json")).unwrap();
    assert!(matches!(
        static_snapshot(&target),
        Ok((ManifestEvidence::Absent, ManifestEvidence::Absent))
    ));
    assert_eq!(std::fs::read(enrolled.join("owner.json")).unwrap(), marker);
}
