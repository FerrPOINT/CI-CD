//! V1 must not gain delivery authority by adding routes or caller-owned JSON.
use super::tests::fixture;
use super::*;
use crate::domain::sdlc_workspace::WorkspaceRole;

fn devops() -> (TaskCredentialContext, AuthContext, SdlcWorkspaceConfig) {
    let (mut context, mut central, config, _) = fixture();
    context.intent.role = WorkspaceRole::Devops;
    central.scopes = [
        "ci-cd:read".into(),
        "ci-cd:write".into(),
        context.grant().unwrap(),
    ]
    .into();
    (context, central, config)
}

#[test]
fn devops_route_grammar_does_not_admit_workspace_creation_or_generic_deployments() {
    let (context, central, config) = devops();
    let identity = Identity::verified(&central, &config).unwrap();
    let pipeline = Uuid::new_v4();
    identity
        .bind(&context, context.project_id, &context.intent)
        .unwrap();
    identity
        .route("GET", &format!("/api/v1/pipelines/{pipeline}"))
        .unwrap();
    identity.route("POST", &format!("/api/v1/projects/{}/sdlc/workspace-operations/developer%3Aoriginal/delivery-operations", context.project_id)).unwrap();
    for project in [context.project_id, Uuid::new_v4()] {
        let root = format!("/api/v1/projects/{project}/sdlc/workspace-operations");
        for (method, suffix) in [
            ("POST", ""),
            ("DELETE", "/developer:original"),
            ("POST", "/developer:original/candidate-evidence"),
            ("POST", "/../delivery-operations"),
            ("POST", "/bad%2Fkey/delivery-operations"),
        ] {
            assert!(identity.route(method, &format!("{root}{suffix}")).is_err());
        }
        let root = format!("/api/v1/projects/{project}");
        for path in [
            format!("{root}/deployments"),
            format!("{root}/deployments/{pipeline}/rollback"),
        ] {
            assert!(identity.route("POST", &path).is_err());
        }
    }
}

#[test]
fn devops_v1_cannot_smuggle_artifact_target_or_command_binding_into_context() {
    let (context, _, _) = devops();
    let value = serde_json::to_value(&context).unwrap();
    for field in [
        "delivery",
        "artifactId",
        "artifactSha256",
        "sourcePipelineId",
        "developmentResultId",
        "targetPolicySha256",
        "expectedManifestSha256",
    ] {
        let mut changed = value.clone();
        changed[field] = json!("caller-supplied");
        assert!(serde_json::from_value::<TaskCredentialContext>(changed).is_err());
    }
    let mut proposed = context;
    proposed.version = 2;
    assert!(proposed.validate().is_err());
    assert!(proposed.grant().is_err());
}

#[test]
fn devops_v1_cannot_borrow_developer_custody_or_broaden_scopes() {
    let (context, central, config) = devops();
    let identity = Identity::verified(&central, &config).unwrap();
    let mut developer = context.clone();
    developer.intent.role = WorkspaceRole::Developer;
    developer.intent.binding.assignment_id = Uuid::new_v4();
    developer.intent.binding.execution_id = Uuid::new_v4();
    developer.intent.binding.fencing_token += 1;
    assert!(
        identity
            .bind(&developer, developer.project_id, &developer.intent)
            .is_err()
    );
    for extra in ["api:read", "api:write", "ci-cd:admin", "git:write"] {
        let mut broadened = central.clone();
        broadened.scopes.insert(extra.into());
        assert!(Identity::verified(&broadened, &config).is_err());
    }
    for prefix in ["ci-cd:sdlc:task:v2:", "ci-cd:sdlc:delivery:v1:"] {
        let mut proposed = central.clone();
        proposed.scopes = [
            "ci-cd:read".into(),
            "ci-cd:write".into(),
            format!("{prefix}{}", digest(&context).unwrap()),
        ]
        .into();
        assert!(Identity::verified(&proposed, &config).is_err());
    }
}
