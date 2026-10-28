#[test]
fn ordinary_oci_rejection_keeps_original_closed_document() {
    let document = cicd::task_delivery::oci::rejection_document(&anyhow::anyhow!(
        "private-token-sql-path"
    ));
    assert_eq!(
        document,
        serde_json::json!({"schema":"forge/local-oci-rejection/v1",
            "status":"unknown_or_rejected","reason":"oci_command_rejected_or_unavailable",
            "dispatchAllowed":false,"sdlcAcceptanceVerified":false})
    );
    assert!(!document.to_string().contains("private"));
}

#[cfg(not(feature = "oci-integration"))]
#[test]
fn ordinary_build_has_no_child_diagnostic_field() {
    assert!(
        cicd::task_delivery::oci::rejection_document(&anyhow::anyhow!(
            "NATIVE_QA_COMPOSE_BRIDGE_FAILED:ValueError\n"
        ))["childErrorClass"]
            .is_null()
    );
}
