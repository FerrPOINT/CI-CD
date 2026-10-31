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
    validate_recipe(&workspace.profile, recipe, pipeline)?;
    if pipeline["version"] != 1
        || pipeline["jobs"]
            .as_object()
            .is_none_or(|jobs| jobs.is_empty())
    {
        return Err(ApiError::conflict("workspace_published_pipeline_invalid"));
    }
    for job in pipeline["jobs"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, job)| job)
    {
        if let Some(deployment) = job.get("helm_deploy") {
            if deployment["environment_id"] != workspace.environment_id.to_string() {
                return Err(ApiError::conflict("workspace_recipe_environment_mismatch"));
            }
        }
    }
    plan["recipe"] = serde_json::json!(recipe);
    plan["pipeline"] = pipeline.clone();
    plan["recipeHash"] = serde_json::json!(
        cicd_infra::development_workspace::request_hash(pipeline).map_err(super::error)?
    );
    Ok(plan)
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
    use super::validate_recipe;
    use serde_json::json;

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
