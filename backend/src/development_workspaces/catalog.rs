//! Published owner catalog. Public requests select UUIDs, never paths or commands.

use crate::api::{ApiError, AppState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRepository {
    pub key: String,
    pub repository_id: Uuid,
    pub clone_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DevelopmentWorkspaceTargetChoice)]
pub struct TargetChoice {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub configuration_id: Option<Uuid>,
    pub profile_id: Uuid,
    pub environment_id: Uuid,
    pub environment_hash: String,
    pub name: String,
    pub kind: String,
    pub public_origin: Option<String>,
    pub profile_hash: String,
    pub profile: Value,
    pub repositories: Vec<SourceRepository>,
}

pub(crate) fn root(state: &AppState) -> Result<&Path, ApiError> {
    state
        .config
        .development_workspaces
        .catalog_root
        .as_deref()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| ApiError::service_unavailable("workspace_catalog_not_configured"))
}

pub(crate) fn read_json(root: &Path, path: &Path) -> Result<Value, ApiError> {
    let base = root
        .canonicalize()
        .map_err(|_| ApiError::service_unavailable("workspace_catalog_unavailable"))?;
    let canonical = path.canonicalize().map_err(|_| ApiError::not_found())?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ApiError::not_found())?;
    if !canonical.starts_with(base)
        || metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > 256 * 1024
    {
        return Err(ApiError::service_unavailable(
            "workspace_catalog_identity_invalid",
        ));
    }
    let data = std::fs::read(path)
        .map_err(|_| ApiError::service_unavailable("workspace_catalog_unavailable"))?;
    serde_json::from_slice(&data)
        .map_err(|_| ApiError::service_unavailable("workspace_catalog_invalid"))
}

pub(crate) fn target_path(root: &Path, id: Uuid, file: &str) -> PathBuf {
    root.join("targets").join(id.to_string()).join(file)
}

pub(crate) fn target(state: &AppState, id: Uuid) -> Result<TargetChoice, ApiError> {
    let root = root(state)?;
    let choice: TargetChoice =
        serde_json::from_value(read_json(root, &target_path(root, id, "public.json"))?)
            .map_err(|_| ApiError::service_unavailable("workspace_public_target_invalid"))?;
    let hash = cicd_infra::development_workspace::request_hash(&choice.profile)
        .map_err(|_| ApiError::service_unavailable("workspace_profile_invalid"))?;
    if choice.id != id
        || choice.repository_id.is_nil()
        || choice.profile_id.is_nil()
        || choice.environment_id.is_nil()
        || choice.profile["apiVersion"] != "pdlc.dev/workspace/v1"
        || choice.profile_hash != hash
        || !matches!(choice.kind.as_str(), "existing" | "template")
    {
        return Err(ApiError::service_unavailable(
            "workspace_profile_identity_mismatch",
        ));
    }
    if choice.profile.get("requiredDeveloperChecks").is_some() {
        let recipes = read_json(root, &target_path(root, id, "recipes.json"))?;
        validate_developer_checks(&choice.profile, &choice.profile_hash, &recipes)?;
    }
    let keys: std::collections::BTreeSet<_> = choice.profile["repositories"]
        .as_array()
        .ok_or_else(|| ApiError::conflict("workspace_profile_repositories_missing"))?
        .iter()
        .filter_map(|item| item["key"].as_str())
        .collect();
    let supplied: std::collections::BTreeSet<_> = choice
        .repositories
        .iter()
        .map(|item| item.key.as_str())
        .collect();
    if keys != supplied
        || supplied.len() != choice.repositories.len()
        || !choice
            .repositories
            .iter()
            .any(|repo| repo.repository_id == choice.repository_id)
    {
        return Err(ApiError::conflict(
            "workspace_profile_repository_bindings_invalid",
        ));
    }
    if let Some(origin) = &choice.public_origin {
        let url = reqwest::Url::parse(origin)
            .map_err(|_| ApiError::conflict("workspace_public_origin_invalid"))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ApiError::conflict("workspace_public_origin_invalid"));
        }
    }
    for repository in &choice.repositories {
        let url = reqwest::Url::parse(&repository.clone_url)
            .map_err(|_| ApiError::conflict("workspace_clone_url_invalid"))?;
        if repository.repository_id.is_nil()
            || !matches!(url.scheme(), "https" | "ssh")
            || url.password().is_some()
            || !(url.username().is_empty() || url.scheme() == "ssh" && url.username() == "git")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ApiError::conflict("workspace_clone_url_invalid"));
        }
    }
    Ok(choice)
}

pub(crate) fn choices(
    state: &AppState,
    repository: Uuid,
    configuration: Uuid,
) -> Result<Vec<TargetChoice>, ApiError> {
    let entries = std::fs::read_dir(root(state)?.join("targets"))
        .map_err(|_| ApiError::service_unavailable("workspace_catalog_unavailable"))?;
    let mut choices = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= 256 {
            return Err(ApiError::service_unavailable("workspace_catalog_limit"));
        }
        let entry =
            entry.map_err(|_| ApiError::service_unavailable("workspace_catalog_unavailable"))?;
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            continue;
        };
        let choice = target(state, id)?;
        if choice.repository_id == repository
            && choice.configuration_id.is_none_or(|id| id == configuration)
        {
            choices.push(choice);
        }
    }
    choices.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(choices)
}

/// Freeze the owner-published execution inputs before accepting a mutation.
/// Original-key readback happens before this function; catalog changes cannot
/// silently change an accepted command or prevent reading its stored result.
pub(crate) fn operation_plan(
    state: &AppState,
    workspace: &cicd_infra::development_workspace::Workspace,
    action: &cicd_domain::development_workspace::WorkspaceAction,
) -> Result<Value, ApiError> {
    use cicd_domain::development_workspace::WorkspaceAction;
    if matches!(
        action,
        WorkspaceAction::BeginSession { .. }
            | WorkspaceAction::EndSession { .. }
            | WorkspaceAction::RecoverSession { .. }
            | WorkspaceAction::Close {}
            | WorkspaceAction::ConvertToStand {}
    ) {
        return Ok(serde_json::json!({}));
    }
    let choice = target(state, workspace.catalog_target_id)?;
    let value = serde_json::to_value(&choice)
        .map_err(|_| ApiError::internal_with_message("workspace_catalog_encoding_failed"))?;
    let hash = cicd_infra::development_workspace::request_hash(&value).map_err(super::error)?;
    if hash != workspace.catalog_hash
        || choice.profile != workspace.profile
        || choice.environment_id != workspace.environment_id
    {
        return Err(ApiError::conflict("workspace_published_catalog_changed"));
    }
    let root = root(state)?;
    let mut plan = serde_json::json!({"catalogHash":hash,"profileHash":workspace.profile_hash,"repositories":choice.repositories});
    if matches!(action, WorkspaceAction::Create {}) {
        plan["creation"] = read_json(
            root,
            &target_path(root, workspace.catalog_target_id, "create.json"),
        )?;
        return Ok(plan);
    }
    if matches!(
        action,
        WorkspaceAction::Register {}
            | WorkspaceAction::Observe {}
            | WorkspaceAction::Start {}
            | WorkspaceAction::Stop {}
    ) {
        let path = if workspace.target_id == workspace.catalog_target_id {
            target_path(root, workspace.target_id, "runtime.json")
        } else {
            root.join("instances")
                .join(workspace.target_id.to_string())
                .join("runtime.json")
        };
        let runtime = read_json(root, &path)?;
        if runtime["id"] != workspace.target_id.to_string() {
            return Err(ApiError::conflict("workspace_runtime_identity_changed"));
        }
        plan["runtimeHash"] = serde_json::json!(
            cicd_infra::development_workspace::request_hash(&runtime).map_err(super::error)?
        );
        return Ok(plan);
    }
    let recipes = read_json(
        root,
        &target_path(root, workspace.catalog_target_id, "recipes.json"),
    )?;
    if recipes["profileHash"] != workspace.profile_hash {
        return Err(ApiError::conflict("workspace_recipe_profile_mismatch"));
    }
    let recipe = match action {
        WorkspaceAction::Prepare { .. } => "prepare",
        WorkspaceAction::Build { .. } => "build",
        WorkspaceAction::Check { recipe, .. } => recipe,
        _ => return Err(ApiError::bad_request("unsupported_workspace_action")),
    };
    let check = workspace.profile["checks"]
        .as_array()
        .and_then(|checks| checks.iter().find(|item| item["key"] == recipe));
    if recipe != "prepare"
        && (check.is_none() || check.is_some_and(|item| item["executor"] == "interactive-browser"))
    {
        return Err(ApiError::conflict("workspace_recipe_not_executable"));
    }
    if check.is_some_and(|item| item["requiresRuntime"] == true)
        && (workspace.runtime != "running" || workspace.desired_runtime != "running")
    {
        return Err(ApiError::conflict("workspace_runtime_required"));
    }
    let pipeline = recipes["recipes"]
        .get(recipe)
        .ok_or_else(|| ApiError::conflict("workspace_recipe_not_published"))?;
    if pipeline.get("base_resource_rollout").is_some() {
        let target = super::base_delivery::recipe(check.ok_or_else(|| ApiError::conflict("workspace_recipe_not_executable"))?, pipeline)?;
        plan["recipe"] = serde_json::json!(recipe);
        plan["recipeHash"] = serde_json::json!(cicd_infra::development_workspace::request_hash(pipeline).map_err(super::error)?);
        plan["baseResourceTarget"] = serde_json::to_value(target).map_err(|_| ApiError::conflict("workspace_base_delivery_identity_invalid"))?;
        super::base_delivery::require_action(action, &plan, &workspace.profile)?;
        return Ok(plan);
    }
    validate_recipe(&workspace.profile, recipe, pipeline)?;
    if pipeline["version"] != 1
        || pipeline["jobs"]
            .as_object()
            .is_none_or(|jobs| jobs.is_empty())
    {
        return Err(ApiError::conflict("workspace_published_pipeline_invalid"));
    }
    let source_pipeline_id = match action {
        WorkspaceAction::Check {
            source_pipeline_id, ..
        } => *source_pipeline_id,
        _ => None,
    };
    let mut pipeline = pipeline.clone();
    let jobs = pipeline["jobs"]
        .as_object_mut()
        .ok_or_else(|| ApiError::conflict("workspace_published_pipeline_invalid"))?;
    let mut delivery_job = None;
    for (name, job) in jobs.iter() {
        if let Some(deployment) = job.get("helm_deploy") {
            if deployment["environment_id"] != workspace.environment_id.to_string() {
                return Err(ApiError::conflict("workspace_recipe_environment_mismatch"));
            }
            if deployment.get("source_pipeline_id").is_some()
                || (delivery_job.is_some() && source_pipeline_id.is_some())
            {
                return Err(ApiError::conflict(
                    "workspace_recipe_invalid_source_pipeline_input",
                ));
            }
            delivery_job = Some(name.clone());
        }
    }
    if let Some(id) = source_pipeline_id {
        let name = delivery_job
            .filter(|_| check.is_some_and(|item| item["executor"] == "forge-delivery"))
            .ok_or_else(|| {
                ApiError::bad_request("workspace_recipe_does_not_accept_source_pipeline")
            })?;
        if jobs
            .iter()
            .any(|(key, job)| key != &name && job.get("image_publish").is_none())
        {
            return Err(ApiError::conflict(
                "workspace_recipe_requires_non_publication_jobs",
            ));
        }
        let mut delivery = jobs
            .remove(&name)
            .ok_or_else(|| ApiError::conflict("workspace_published_pipeline_invalid"))?;
        let job = delivery
            .as_object_mut()
            .ok_or_else(|| ApiError::conflict("workspace_published_pipeline_invalid"))?;
        job.remove("needs");
        let deployment = job
            .get_mut("helm_deploy")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| ApiError::conflict("workspace_published_pipeline_invalid"))?;
        deployment.insert("source_pipeline_id".into(), serde_json::json!(id));
        jobs.clear();
        jobs.insert(name, delivery);
    }
    plan["recipe"] = serde_json::json!(recipe);
    plan["recipeHash"] = serde_json::json!(
        cicd_infra::development_workspace::request_hash(&pipeline).map_err(super::error)?
    );
    plan["pipeline"] = pipeline;
    Ok(plan)
}

fn validate_developer_checks(
    profile: &Value,
    profile_hash: &str,
    published: &Value,
) -> Result<(), ApiError> {
    let invalid = || ApiError::conflict("workspace_required_developer_checks_invalid");
    let required = profile["requiredDeveloperChecks"]
        .as_array()
        .filter(|keys| !keys.is_empty() && keys.len() <= 32)
        .ok_or_else(invalid)?;
    let checks = profile["checks"].as_array().ok_or_else(invalid)?;
    if published["profileHash"] != profile_hash {
        return Err(invalid());
    }
    let mut seen = std::collections::BTreeSet::new();
    for key in required {
        let key = key
            .as_str()
            .filter(|key| !key.is_empty() && *key != "prepare")
            .filter(|key| seen.insert(*key))
            .ok_or_else(invalid)?;
        let mut definitions = checks.iter().filter(|check| check["key"] == key);
        let check = definitions.next().ok_or_else(invalid)?;
        if definitions.next().is_some()
            || !matches!(check["executor"].as_str(), Some("forge" | "forge-build"))
            || check["requiresRuntime"] != false
            || (check["executor"] == "forge" && check["commands"].as_array().is_none_or(Vec::is_empty))
        {
            return Err(invalid());
        }
        let pipeline = &published["recipes"][key];
        if pipeline["version"] != 1 {
            return Err(invalid());
        }
        validate_recipe(profile, key, pipeline)?;
    }
    Ok(())
}

/// The existing forge-build profile executor grants one publication, never shell or deployment.
pub(super) fn validate_publication_recipe(check: &Value, pipeline: &Value) -> Result<(), ApiError> {
    let invalid = || ApiError::conflict("workspace_publication_recipe_invalid");
    if check["executor"] != "forge-build" || check["kind"] != "build"
        || check["requiresRuntime"] != false || check.get("commands").is_some()
        || pipeline["version"] != 1
        || pipeline.as_object().is_none_or(|value| value.len() != 2)
    { return Err(invalid()); }
    let jobs = pipeline["jobs"].as_object().filter(|jobs| jobs.len() == 1).ok_or_else(invalid)?;
    let job = jobs.values().next().and_then(Value::as_object)
        .filter(|job| job.len() == 1).ok_or_else(invalid)?;
    let spec: cicd_domain::image_publish::ImagePublishSpec = serde_json::from_value(
        job.get("image_publish").cloned().ok_or_else(invalid)?,
    ).map_err(|_| invalid())?;
    spec.validate().map_err(|_| invalid())
}

/// A published test must execute the commands declared by the versioned
/// profile. Preparation executes every declared readiness check exactly once.
fn validate_recipe(profile: &Value, recipe: &str, pipeline: &Value) -> Result<(), ApiError> {
    let invalid = || ApiError::conflict("workspace_recipe_profile_commands_mismatch");
    let checks = profile["checks"].as_array().ok_or_else(invalid)?;
    let selected = if recipe == "prepare" {
        let keys = profile["preparationChecks"]
            .as_array()
            .ok_or_else(invalid)?;
        if keys.is_empty() {
            return Err(invalid());
        }
        keys.iter()
            .map(|key| {
                checks
                    .iter()
                    .find(|check| check["key"] == *key)
                    .ok_or_else(invalid)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let check = checks
            .iter()
            .find(|check| check["key"] == recipe)
            .ok_or_else(invalid)?;
        if check["executor"] == "forge-build" {
            return validate_publication_recipe(check, pipeline);
        }
        if check["executor"] != "forge" {
            return Ok(());
        }
        vec![check]
    };
    let jobs = pipeline["jobs"].as_object().ok_or_else(invalid)?;
    if selected.len() != jobs.len() {
        return Err(invalid());
    }
    let mut remaining = selected;
    for job in jobs.values() {
        let image = job["image"].as_str().ok_or_else(invalid)?;
        let Some((name, digest)) = image.split_once("@sha256:") else {
            return Err(invalid());
        };
        if name.is_empty()
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || job.get("image_publish").is_some()
            || job.get("helm_deploy").is_some()
        {
            return Err(invalid());
        }
        let Some(index) = remaining.iter().position(|check| {
            check["executor"] == "forge"
                && check["commands"].is_array()
                && check["commands"] == job["commands"]
        }) else {
            return Err(invalid());
        };
        remaining.remove(index);
    }
    Ok(())
}

pub(crate) async fn execution_target<'e, E>(
    executor: E,
    id: Uuid,
    project: Uuid,
) -> Result<Value, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar("SELECT COALESCE(kubernetes_target,jsonb_build_object('adapter','compose','environmentId',id,'projectId',project_id,'url',url)) FROM environments WHERE id=$1 AND project_id=$2")
        .bind(id).bind(project).fetch_optional(executor).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)
}

#[cfg(test)]
mod tests {
    use super::{validate_developer_checks, validate_recipe};
    use serde_json::json;

    #[test]
    fn developer_policy_requires_distinct_published_executable_checks() {
        let mut profile = json!({"requiredDeveloperChecks":["unit"],"checks":[
            {"key":"unit","executor":"forge","requiresRuntime":false,"commands":["check unit"]},
            {"key":"build","executor":"forge-build"}
        ]});
        let job = json!({"image":format!("tools@sha256:{}", "a".repeat(64)),"commands":["check unit"]});
        let mut published = json!({"profileHash":"profile","recipes":{
            "unit":{"version":1,"jobs":{"unit":job}}
        }});
        assert!(validate_developer_checks(&profile, "profile", &published).is_ok());
        for keys in [json!([]), json!(["unit", "unit"]), json!(["build"]), json!(["missing"])] {
            profile["requiredDeveloperChecks"] = keys;
            assert!(validate_developer_checks(&profile, "profile", &published).is_err());
        }
        profile["requiredDeveloperChecks"] = json!(["unit"]);
        assert!(validate_developer_checks(&profile, "other", &published).is_err());
        let duplicate = profile["checks"][0].clone();
        profile["checks"].as_array_mut().unwrap().push(duplicate);
        assert!(validate_developer_checks(&profile, "profile", &published).is_err());
        profile["checks"].as_array_mut().unwrap().pop();
        profile["checks"][1] = json!({"key":"build","kind":"build","executor":"forge-build","requiresRuntime":false});
        profile["requiredDeveloperChecks"] = json!(["unit","build"]);
        published["recipes"]["build"] = json!({"version":1,"jobs":{"publish":{"image_publish":{
            "context":".","dockerfile":"Dockerfile","repository":"registry.test/owned",
            "descriptor_artifact":"out/build.json","published_artifact":"out/image.json"
        }}}});
        assert!(validate_developer_checks(&profile, "profile", &published).is_ok());
        profile["checks"][1]["commands"] = json!(["unapproved shell"]);
        assert!(validate_developer_checks(&profile, "profile", &published).is_err());
        profile["checks"][1].as_object_mut().unwrap().remove("commands");
        published["recipes"]["unit"] = serde_json::Value::Null;
        assert!(validate_developer_checks(&profile, "profile", &published).is_err());
    }

    #[test]
    fn published_checks_bind_profile_commands_and_immutable_tools() {
        let profile = json!({"checks":[{"key":"unit","executor":"forge","commands":["python check.py unit"]}]});
        let mut pipeline = json!({"version":1,"jobs":{"unit":{"image":format!("tools@sha256:{}", "a".repeat(64)),"commands":["python check.py unit"]}}});
        assert!(validate_recipe(&profile, "unit", &pipeline).is_ok());
        pipeline["jobs"]["unit"]["commands"] = json!(["python check.py other"]);
        assert!(validate_recipe(&profile, "unit", &pipeline).is_err());
        pipeline["jobs"]["unit"]["commands"] = json!(["python check.py unit"]);
        pipeline["jobs"]["unit"]["image"] = json!("tools:latest");
        assert!(validate_recipe(&profile, "unit", &pipeline).is_err());
    }

    #[test]
    fn publication_check_is_one_typed_build_without_commands_or_deployment() {
        let check = json!({"key":"publish","kind":"build","executor":"forge-build","requiresRuntime":false});
        let profile = json!({"checks":[check]});
        let spec = json!({"context":"backend","dockerfile":"Dockerfile","repository":"registry.example.test/team/application",
            "descriptor_artifact":"out/build.json","published_artifact":"out/image.json"});
        let pipeline = json!({"version":1,"jobs":{"publish":{"image_publish":spec}}});
        assert!(validate_recipe(&profile,"publish",&pipeline).is_ok());
        for job in [json!({"image_publish":spec,"commands":["false"]}),
            json!({"helm_deploy":{}}), json!({"image_publish":spec,"needs":[]}),
            json!({"image_publish":{}})] {
            assert!(validate_recipe(&profile,"publish",&json!({"version":1,"jobs":{"publish":job}})).is_err());
        }
        for (path,value) in [("/jobs/publish/image_publish/context",json!("../foreign")),
            ("/jobs/publish/image_publish/repository",json!("registry.example.test/team/application:latest")),
            ("/jobs/publish/image_publish/published_artifact",json!("out/build.json"))] {
            let mut changed = pipeline.clone(); *changed.pointer_mut(path).unwrap() = value;
            assert!(validate_recipe(&profile,"publish",&changed).is_err());
        }
        let mut mixed = pipeline.clone(); mixed["jobs"]["shell"] = json!({"commands":["false"]});
        assert!(validate_recipe(&profile,"publish",&mixed).is_err());
        for (field,value) in [("kind",json!("delivery")),("requiresRuntime",json!(true)),("commands",json!(["false"]))] {
            let mut changed = profile.clone(); changed["checks"][0][field] = value;
            assert!(validate_recipe(&changed,"publish",&pipeline).is_err());
        }
        assert!(validate_recipe(&profile,"foreign",&pipeline).is_err());
    }

    #[test]
    fn preparation_requires_each_published_readiness_check_once() {
        let profile = json!({"preparationChecks":["backend","frontend"],"checks":[
            {"key":"backend","executor":"forge","commands":["check backend"]},
            {"key":"frontend","executor":"forge","commands":["check frontend"]}]});
        let job = json!({"image":format!("tools@sha256:{}", "b".repeat(64)),"commands":["check backend"]});
        let mut pipeline = json!({"jobs":{"backend":job,"frontend":job}});
        assert!(validate_recipe(&profile, "prepare", &pipeline).is_err());
        pipeline["jobs"]["frontend"]["commands"] = json!(["check frontend"]);
        assert!(validate_recipe(&profile, "prepare", &pipeline).is_ok());
        pipeline["jobs"].as_object_mut().unwrap().remove("frontend");
        assert!(validate_recipe(&profile, "prepare", &pipeline).is_err());
    }
}
