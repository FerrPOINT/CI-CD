//! Private Base operator bridge. Public clients supply neither commands nor paths.
use super::catalog;
use crate::{
    api::{ApiError, AppState},
    quiescence::Lease,
};
use cicd_infra::development_workspace::Workspace;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
};
use uuid::Uuid;

fn unavailable() -> ApiError {
    ApiError::service_unavailable("workspace_native_custody_unconfirmed")
}

fn require_registered_target(
    profile: &Value,
    target: Uuid,
    runtime: &Value,
    plan: &Value,
) -> Result<(), ApiError> {
    let source_only = profile["services"].as_array().is_some_and(Vec::is_empty);
    if runtime["id"] != target.to_string()
        || plan["runtimeHash"]
            != cicd_infra::development_workspace::request_hash(runtime).map_err(super::error)?
        || (!source_only
            && runtime["adapter"] == "kubernetes"
            && (runtime["namespace"] != plan["environment"]["namespace"]
                || runtime["inventory"]["release"] != plan["environment"]["release"]))
    {
        return Err(ApiError::conflict("workspace_registered_target_drift"));
    }
    Ok(())
}

pub(super) fn journal(state: &AppState, operation: Uuid) -> Result<PathBuf, ApiError> {
    Ok(catalog::root(state)?
        .join("operations")
        .join(operation.to_string()))
}

#[cfg(unix)]
fn lock(path: &Path) -> Result<Option<File>, ApiError> {
    use std::os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    };
    fs::create_dir_all(path).map_err(|_| unavailable())?;
    let metadata = fs::symlink_metadata(path).map_err(|_| unavailable())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(unavailable());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| unavailable())?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.join("custody.lock"))
        .map_err(|_| unavailable())?;
    // SAFETY: this invocation owns the descriptor; nonblocking flock never uses expiry.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
        return Ok(None);
    }
    Err(unavailable())
}
#[cfg(not(unix))]
fn lock(_: &Path) -> Result<Option<File>, ApiError> {
    Err(unavailable())
}

fn bind(command: &mut std::process::Command, file: &File) -> Result<(), ApiError> {
    #[cfg(unix)]
    {
        use std::os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::process::CommandExt,
        };
        // Reserve the source outside the target descriptor range before fork.
        // SAFETY: the live source descriptor is owned by this caller.
        let duplicate = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 200) };
        if duplicate < 0 {
            return Err(unavailable());
        }
        // SAFETY: fcntl returned a new descriptor whose ownership is transferred once.
        let descriptor = unsafe { OwnedFd::from_raw_fd(duplicate) };
        // SAFETY: only dup2 runs after fork; the closure owns its source descriptor.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(descriptor.as_raw_fd(), 197) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.env("PDLC_DEVELOPMENT_CUSTODY_FD", "197");
    }
    #[cfg(not(unix))]
    {
        let _ = (command, file);
        return Err(unavailable());
    }
    Ok(())
}

pub(super) async fn operate(
    state: &AppState,
    workspace: &Workspace,
    operation: Uuid,
    action: &str,
    reconcile: bool,
    effect: &Lease,
    plan: &Value,
) -> Result<Value, ApiError> {
    let directory = journal(state, operation)?;
    let parent = directory.parent().ok_or_else(unavailable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(parent).map_err(|_| unavailable())?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|_| unavailable())?;
    }
    let Some(custody) = lock(&directory)? else {
        return Ok(json!({"state":"unknown","reason":"native_writer_still_active"}));
    };
    if reconcile && !directory.join("intent.json").exists() {
        // The inherited lock covers even the child-before-Python startup gap.
        // Once it is acquired and no intent exists, no Base effect was admitted.
        return Ok(
            json!({"state":"failed","reason":"native_launch_not_recorded","reconciled":true}),
        );
    }
    let root = catalog::root(state)?;
    let target = catalog::runtime_path(root, workspace.target_id, workspace.catalog_target_id);
    let profile = catalog::target_path(root, workspace.catalog_target_id, "profile.json");
    // Validate confinement and frozen inputs before selecting a private operator.
    if catalog::read_json(root, &profile)? != workspace.profile {
        return Err(ApiError::conflict("workspace_registered_profile_drift"));
    }
    let creation = action == "create";
    let input = if creation {
        let path = catalog::target_path(root, workspace.catalog_target_id, "create.json");
        let blueprint = catalog::read_json(root, &path)?;
        if blueprint != plan["creation"]
            || blueprint["reservationId"] != workspace.catalog_target_id.to_string()
            || blueprint["seed"]["namespace"] != plan["environment"]["namespace"]
            || blueprint["seed"]["release"] != plan["environment"]["release"]
        {
            return Err(ApiError::conflict("workspace_creation_binding_drift"));
        }
        path
    } else {
        let runtime = catalog::read_json(root, &target)?;
        require_registered_target(&workspace.profile, workspace.target_id, &runtime, plan)?;
        target.clone()
    };
    let configured = if creation {
        &state.config.development_workspaces.creator_program
    } else {
        &state.config.development_workspaces.operator_program
    };
    let program = configured
        .as_ref()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| ApiError::service_unavailable("workspace_operator_not_configured"))?;
    let mut command = tokio::process::Command::new(program);
    command
        .arg(action)
        .arg("--profile")
        .arg(profile)
        .arg("--target")
        .arg(input)
        .arg("--operation-id")
        .arg(operation.to_string())
        .arg("--journal-directory")
        .arg(parent)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if creation {
        command
            .arg("--target-id")
            .arg(workspace.target_id.to_string())
            .arg("--output-target")
            .arg(target);
    }
    if reconcile {
        command.arg("--reconcile");
    }
    if let Some(until) = crate::pipeline_authority::current_clock()
        .map(|clock| clock.valid_until())
        .transpose()?
        .flatten()
    {
        command.env("PDLC_DEVELOPMENT_AUTHORITY_UNTIL", until.timestamp().to_string());
    } else {
        // Never inherit an unrelated parent process's credential clock.
        command.env_remove("PDLC_DEVELOPMENT_AUTHORITY_UNTIL");
    }
    bind(command.as_std_mut(), &custody)?;
    effect
        .inherit_workspace_operator(command.as_std_mut())
        .map_err(|_| unavailable())?;
    let mut child = command.spawn().map_err(|_| unavailable())?;
    // Drop pre_exec-owned source descriptors before proving child closure later.
    drop(command);
    let status = child.wait().await.map_err(|_| unavailable())?;
    drop(custody);
    let Some(_closed) = lock(&directory)? else {
        return Ok(json!({"state":"unknown","reason":"native_writer_still_active"}));
    };
    for name in ["confirmed.json", "result.json"] {
        let path = directory.join(name);
        if path.exists() {
            let result = catalog::read_json(root, &path)?;
            if result["operationId"] != operation.to_string() {
                return Err(unavailable());
            }
            return Ok(result);
        }
    }
    if !status.success() {
        return Ok(json!({"state":"unknown","reason":"operator_response_unconfirmed"}));
    }
    Ok(json!({"state":"unknown","reason":"original_operator_result_missing"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> (Uuid, Value, Value) {
        let target = Uuid::new_v4();
        let runtime = json!({"id":target,"adapter":"kubernetes","namespace":"agent-storage",
            "namespaceUid":Uuid::new_v4(),"services":[],"storage":[],"secrets":[]});
        let plan = json!({"runtimeHash":cicd_infra::development_workspace::request_hash(&runtime).unwrap(),
            "environment":{"adapter":"compose","environmentId":Uuid::new_v4()}});
        (target, runtime, plan)
    }

    #[test]
    fn source_only_registration_does_not_require_an_application_environment() {
        let (target, runtime, plan) = binding();
        require_registered_target(&json!({"services":[]}), target, &runtime, &plan).unwrap();
    }

    #[test]
    fn application_services_keep_namespace_and_release_fences() {
        let (target, mut runtime, mut plan) = binding();
        let profile = json!({"services":[{"key":"app","dependsOn":[]}]});
        assert!(require_registered_target(&profile, target, &runtime, &plan).is_err());
        runtime["inventory"] = json!({"release":"application"});
        plan["runtimeHash"] = json!(cicd_infra::development_workspace::request_hash(&runtime).unwrap());
        plan["environment"] = json!({"namespace":"agent-storage","release":"application"});
        require_registered_target(&profile, target, &runtime, &plan).unwrap();
        for field in ["namespace", "release"] {
            let mut changed = plan.clone();
            changed["environment"][field] = json!("foreign");
            assert!(require_registered_target(&profile, target, &runtime, &changed).is_err());
            assert!(require_registered_target(&json!({}), target, &runtime, &changed).is_err());
        }
    }

    #[test]
    fn source_only_registration_keeps_target_and_frozen_inventory_identity() {
        let (target, runtime, plan) = binding();
        let profile = json!({"services":[]});
        assert!(require_registered_target(&profile, Uuid::new_v4(), &runtime, &plan).is_err());
        let mut changed = runtime.clone();
        changed["namespaceUid"] = json!(Uuid::new_v4());
        assert!(require_registered_target(&profile, target, &changed, &plan).is_err());
        let mut changed = plan;
        changed["runtimeHash"] = Value::Null;
        assert!(require_registered_target(&profile, target, &runtime, &changed).is_err());
    }
}
