//! Owner-published Base effect, verified by Check without transferring physical authority.
use crate::api::{ApiError, AppState};
use cicd_domain::{development_workspace::WorkspaceAction, published_image::PublishedImage};
use cicd_infra::development_workspace::{self as store, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Target {
    schema: String,
    installation_id: String,
    slot: String,
    component: String,
    namespace: String,
    namespace_uid: Uuid,
    deployment: String,
    deployment_uid: Uuid,
    container: String,
    pub(super) publication_job: String,
    pub(super) publication_artifact: String,
}

fn invalid() -> ApiError { ApiError::conflict("workspace_base_delivery_identity_invalid") }
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}
impl Target {
    fn parse(value: &Value) -> Result<Self, ApiError> {
        let target: Self = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        if target.schema != "pdlc/base-resource-target/v1"
            || target.installation_id.is_empty() || !target.installation_id.is_ascii()
            || !key(&target.slot) || !key(&target.component) || !key(&target.namespace)
            || !key(&target.deployment) || !key(&target.container) || !key(&target.publication_job)
            || target.namespace_uid.is_nil() || target.deployment_uid.is_nil()
            || target.publication_artifact.is_empty() {
            return Err(invalid());
        }
        Ok(target)
    }
}

/// This recipe verifies an owner effect; it cannot enqueue jobs or deploy.
pub(super) fn recipe(check: &Value, value: &Value) -> Result<Target, ApiError> {
    if check["executor"] != "forge-delivery" || check["kind"] != "delivery"
        || check["requiresRuntime"] != false || check.get("commands").is_some()
        || value["version"] != 1 || value.as_object().is_none_or(|v| v.len() != 2) {
        return Err(invalid());
    }
    Target::parse(&value["base_resource_rollout"])
}

pub(super) fn require_action(action: &WorkspaceAction, plan: &Value, profile: &Value) -> Result<(), ApiError> {
    let WorkspaceAction::Check { recipe: name, source_pipeline_id: Some(id), .. } = action else { return Err(invalid()); };
    let check = profile["checks"].as_array().and_then(|checks|
        checks.iter().find(|check| check["key"] == *name)).ok_or_else(invalid)?;
    let target = recipe(check, &json!({"version":1,"base_resource_rollout":plan["baseResourceTarget"]}))?;
    if id.is_nil() || serde_json::to_value(target).map_err(|_| invalid())? != plan["baseResourceTarget"]
        || plan.get("pipeline").is_some() { return Err(invalid()); }
    Ok(())
}

/// Validate the bounded original Base receipt against the frozen target and
/// the exact original publication artifact, never an environment's latest image.
pub(super) fn validate(target: &Target, envelope: &Value, published: &PublishedImage) -> Result<(), ApiError> {
    let receipt = &envelope["receipt"];
    if envelope.as_object().is_none_or(|v| v.len() != 2)
        || receipt.as_object().is_none_or(|v| v.len() != 18)
        || envelope["sha256"].as_str() != Some(store::request_hash(receipt).map_err(super::error)?.as_str())
        || receipt["schema"] != "pdlc/resource-rollout-delivery/v1" || receipt["phase"] != "complete"
        || receipt["operationId"].as_str().is_none_or(|id| Uuid::parse_str(id).ok().is_none_or(|id| id.is_nil()))
        || receipt["installationId"] != target.installation_id || receipt["slot"] != target.slot
        || receipt["component"] != json!({"key":target.component,"namespace":target.namespace,
            "namespaceUid":target.namespace_uid,"deployment":target.deployment,
            "deploymentUid":target.deployment_uid,"container":target.container})
        || receipt["images"] != json!({(target.container.clone()):published.image_ref})
        || receipt["generation"].as_u64().is_none_or(|g| g == 0 || receipt["observedGeneration"].as_u64().is_none_or(|v| v < g))
        || receipt["replicas"] != 1 || receipt["readyReplicas"] != 1
        || receipt["ready"] != true || receipt["guard"] != "READY"
        || ["planSha256","sourceConfigurationSha256","configurationSha256","sourceCatalogSha256","normalReceiptSha256"]
            .iter().any(|key| receipt[*key].as_str().is_none_or(|value| !hash(value)))
        || published.validate().is_err() {
        return Err(invalid());
    }
    Ok(())
}

pub(super) async fn observation(
    state: &AppState, tx: &mut Transaction<'_, Postgres>, workspace: &Workspace,
    operation: Uuid, plan: &Value, action: &WorkspaceAction,
) -> Result<Value, ApiError> {
    require_action(action, plan, &workspace.profile)?;
    let WorkspaceAction::Check { source_commit, source_pipeline_id: Some(source), .. } = action else { return Err(invalid()); };
    let target = Target::parse(&plan["baseResourceTarget"])?;
    let root = super::catalog::root(state)?;
    let path = super::catalog::target_path(root, workspace.catalog_target_id, &format!("deliveries/{source}.json"));
    // Absence is an explicit owner handoff boundary; reconciliation retains this operation.
    if !path.exists() { return Err(ApiError::service_unavailable("workspace_base_delivery_receipt_missing")); }
    let envelope = super::catalog::read_json(root, &path)?;
    let published = crate::deployment_runtime::completed_publication_image(tx, &state.config.artifacts,
        *source, workspace.project_id, source_commit, &target.publication_job, &target.publication_artifact).await?;
    validate(&target, &envelope, &published)?;
    Ok(json!({"schema":"forge/base-operation-observation/v1","workspaceOperationId":operation,
        "workspaceId":workspace.id,"sourcePipelineId":source,"sourceCommit":source_commit,
        "target":target,"deliveryReceipt":envelope}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Target, Value, PublishedImage) {
        let target = Target::parse(&json!({"schema":"pdlc/base-resource-target/v1",
            "installationId":"installation-original","slot":"pdlc","component":"task-backend",
            "namespace":"pdlc","namespaceUid":Uuid::new_v4(),"deployment":"task-backend",
            "deploymentUid":Uuid::new_v4(),"container":"backend","publicationJob":"publish",
            "publicationArtifact":"out/image.json"})).unwrap();
        let published = PublishedImage::new("a".repeat(40),
            format!("registry.example.test/product/backend@sha256:{}", "b".repeat(64)), "c".repeat(64)).unwrap();
        let body = json!({"schema":"pdlc/resource-rollout-delivery/v1","phase":"complete",
            "operationId":Uuid::new_v4(),"installationId":target.installation_id,"slot":target.slot,
            "planSha256":"d".repeat(64),"sourceConfigurationSha256":"e".repeat(64),
            "configurationSha256":"f".repeat(64),"sourceCatalogSha256":"1".repeat(64),"normalReceiptSha256":"2".repeat(64),
            "component":{"key":target.component,"namespace":target.namespace,"namespaceUid":target.namespace_uid,
                "deployment":target.deployment,"deploymentUid":target.deployment_uid,"container":target.container},
            "images":{target.container.clone():published.image_ref},"generation":7,"observedGeneration":7,
            "replicas":1,"readyReplicas":1,"ready":true,"guard":"READY"});
        let envelope = json!({"sha256":store::request_hash(&body).unwrap(),"receipt":body});
        (target, envelope, published)
    }
    fn rehash(envelope: &mut Value) {
        envelope["sha256"] = json!(store::request_hash(&envelope["receipt"]).unwrap());
    }
    #[test]
    fn exact_base_effect_matches_the_published_image_without_a_delivery_pipeline() {
        let (target, envelope, image) = fixture();
        validate(&target, &envelope, &image).unwrap();
        assert!(envelope["receipt"].get("pipelineId").is_none());
        assert!(envelope["receipt"].get("workspaceId").is_none());
    }
    #[test]
    fn foreign_installation_namespace_controller_and_container_are_denied() {
        let (target, envelope, image) = fixture();
        for path in ["/installationId","/slot","/component/key","/component/namespace",
            "/component/namespaceUid","/component/deployment","/component/deploymentUid","/component/container"] {
            let mut other = envelope.clone();
            *other["receipt"].pointer_mut(path).unwrap() = json!("foreign");
            rehash(&mut other);
            assert!(validate(&target,&other,&image).is_err(),"{path}");
        }
    }
    #[test]
    fn receipt_hash_and_exact_publication_image_are_mandatory() {
        let (target, envelope, image) = fixture();
        let mut other=envelope.clone();other["sha256"]=json!("0".repeat(64));
        assert!(validate(&target,&other,&image).is_err());
        let mut other=envelope;other["receipt"]["images"][&target.container]=json!(format!("registry.example.test/neighbor@sha256:{}","b".repeat(64)));
        rehash(&mut other);assert!(validate(&target,&other,&image).is_err());
    }
    #[test]
    fn incomplete_effect_and_unobserved_generation_never_claim_success() {
        let (target, envelope, image) = fixture();
        for (field,value) in [("phase",json!("running")),("ready",json!(false)),("guard",json!("HOLD")),
            ("generation",json!(0)),("observedGeneration",json!(6)),("replicas",json!(2)),("readyReplicas",json!(0))] {
            let mut other=envelope.clone();other["receipt"][field]=value;rehash(&mut other);
            assert!(validate(&target,&other,&image).is_err(),"{field}");
        }
    }
    #[test]
    fn owner_receipt_references_and_original_operation_are_required() {
        let (target, envelope, image) = fixture();
        for field in ["operationId","planSha256","sourceConfigurationSha256","configurationSha256","sourceCatalogSha256","normalReceiptSha256"] {
            let mut other=envelope.clone();other["receipt"][field]=json!("");rehash(&mut other);
            assert!(validate(&target,&other,&image).is_err(),"{field}");
        }
    }
    #[test]
    fn base_recipe_cannot_smuggle_a_job_or_borrow_helm_execution() {
        let (target,_,_)=fixture();
        let check=json!({"key":"release","kind":"delivery","executor":"forge-delivery","requiresRuntime":false});
        let value=json!({"version":1,"base_resource_rollout":target});
        recipe(&check,&value).unwrap();
        for field in ["jobs","helm_deploy","commands"] {
            let mut other=value.clone();other[field]=json!({});assert!(recipe(&check,&other).is_err());
        }
        for (field,item) in [("executor",json!("forge")),("kind",json!("build")),("requiresRuntime",json!(true)),("commands",json!([]))] {
            let mut other=check.clone();other[field]=item;assert!(recipe(&other,&value).is_err());
        }
    }
    #[test]
    fn only_explicit_delivery_check_accepts_the_base_target() {
        let (target,_,_)=fixture();
        let profile=json!({"checks":[{"key":"release","kind":"delivery","executor":"forge-delivery","requiresRuntime":false}]});
        let plan=json!({"baseResourceTarget":target});
        let action=WorkspaceAction::Check{recipe:"release".into(),source_commit:"a".repeat(40),source_pipeline_id:Some(Uuid::new_v4())};
        require_action(&action,&plan,&profile).unwrap();
        let no_source=WorkspaceAction::Check{recipe:"release".into(),source_commit:"a".repeat(40),source_pipeline_id:None};
        assert!(require_action(&no_source,&plan,&profile).is_err());
        let mut mixed=plan;mixed["pipeline"]=json!({"version":1,"jobs":{}});
        assert!(require_action(&action,&mixed,&profile).is_err());
    }
}
