//! Ordinary Git transport for the assigned workspace, using permanent identity.
use super::{
    assignment::{AssignmentAccess, DeveloperAssignment},
    machine,
};
use crate::{
    api::{ApiError, AppState},
    git_host::GitDeadline,
};
use axum::http::{HeaderMap, header};
use base64::Engine;
use cicd_infra::development_workspace as store;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use uuid::Uuid;

const PURPOSE: &str = "forge/development-workspace-git/v1";

// No Debug: bearer/ciphertext stay inside server-owned hook custody.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    purpose: String,
    workspace: Uuid,
    actor: Uuid,
    subject: Uuid,
    issuer: String,
    assignment: DeveloperAssignment,
    token: String,
    until: i64,
}

pub(crate) struct Authorization {
    pub deadline: GitDeadline,
    pub(super) source: serde_json::Value,
    sealed: Option<String>,
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DevelopmentWorkspaceGitCheck)]
pub struct CandidateCheck {
    pub developer_workspace: String,
}

fn workspace_user(value: &str) -> Result<Option<Uuid>, ApiError> {
    let Some(raw) = value.strip_prefix("workspace-") else {
        return Ok(None);
    };
    Uuid::parse_str(raw)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == raw)
        .map(Some)
        .ok_or_else(ApiError::forbidden)
}

/// The Basic username selects a workspace; only the verified PAT and current
/// assignment grant access. It is not an alternative account identity.
pub(crate) async fn admit(
    state: &AppState,
    repo: &str,
    headers: &HeaderMap,
    write: bool,
) -> Result<Option<Authorization>, ApiError> {
    let Some(raw) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Basic "))
    else {
        return Ok(None);
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| ApiError::unauthorized())?;
    let text = String::from_utf8(bytes).map_err(|_| ApiError::unauthorized())?;
    let Some((username, token)) = text.split_once(':') else {
        return Err(ApiError::unauthorized());
    };
    let Some(workspace_id) = workspace_user(username)? else {
        return Ok(None);
    };
    let verified = crate::central_auth::try_verified_central(token)
        .await?
        .ok_or_else(ApiError::forbidden)?;
    let subject = Uuid::parse_str(&verified.context.user_id).map_err(|_| ApiError::forbidden())?;
    let pool = super::pool(state)?;
    let path = format!(
        "/api/v1/development-workspaces/{workspace_id}{}",
        if write { "/operations" } else { "" }
    );
    let identity =
        machine::authenticate(pool, verified, if write { "POST" } else { "GET" }, &path).await?;
    let workspace = store::workspace(pool, workspace_id)
        .await
        .map_err(super::error)?;
    let storage = crate::repository_catalog::resolve_storage(pool, repo).await?;
    let repository: Uuid =
        sqlx::query_scalar("SELECT id FROM repository_catalog WHERE storage_name=$1")
            .bind(storage)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if repository != workspace.repository_id {
        return Err(ApiError::forbidden());
    }
    let session = store::session(pool, workspace_id)
        .await
        .map_err(super::error)?
        .filter(|s| s.actor_id == identity.claims.sub && s.owner_subject == subject)
        .ok_or_else(ApiError::forbidden)?;
    let assignment: DeveloperAssignment =
        serde_json::from_value(session.machine_assignment.ok_or_else(ApiError::forbidden)?)
            .map_err(|_| ApiError::forbidden())?;
    let verified = identity.central.ok_or_else(ApiError::forbidden)?;
    let issuer = verified.issuer.clone();
    let proof = machine::confirm_identity(
        pool,
        &workspace,
        identity.claims.sub,
        verified,
        &assignment,
        write,
        if write {
            AssignmentAccess::Active
        } else {
            AssignmentAccess::Reading
        },
    )
    .await?;
    let sealed = if write {
        let ticket = Ticket {
            purpose: PURPOSE.into(),
            workspace: workspace_id,
            actor: identity.claims.sub,
            subject,
            issuer,
            assignment: proof.assignment.clone(),
            token: token.into(),
            until: proof.clock.valid_until()?.timestamp(),
        };
        let plain = serde_json::to_string(&ticket).map_err(|_| ApiError::forbidden())?;
        Some(
            crate::platform::encrypt_secret(&plain, &state.config.secrets).map_err(|_| {
                ApiError::service_unavailable("workspace_machine_credential_storage_unavailable")
            })?,
        )
    } else {
        None
    };
    Ok(Some(Authorization {
        deadline: proof.clock,
        source: proof.assignment.source,
        sealed,
    }))
}

#[utoipa::path(post,operation_id="forge_development_workspace_git_check",path="/api/v1/internal/development-workspace-git-check",tag="git",
    request_body=CandidateCheck,responses((status=200,body=crate::task_git::CandidateCheckReadback),(status=400),(status=401),(status=403),(status=409),(status=422),(status=503)))]
pub(crate) async fn callback(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<CandidateCheck>,
) -> Result<axum::Json<crate::task_git::CandidateCheckReadback>, ApiError> {
    check_callback(&state, &headers, request)
        .await
        .map(axum::Json)
}

async fn check_callback(
    state: &AppState,
    headers: &HeaderMap,
    request: CandidateCheck,
) -> Result<crate::task_git::CandidateCheckReadback, ApiError> {
    let internal = state
        .git
        .internal_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(ApiError::forbidden)?;
    if headers.get_all("x-internal-token").iter().count() != 1
        || headers
            .get("x-internal-token")
            .and_then(|v| v.to_str().ok())
            != Some(internal)
        || request.developer_workspace.len() > 65536
    {
        return Err(ApiError::forbidden());
    }
    let plain =
        crate::platform::decrypt_secret(&request.developer_workspace, &state.config.secrets)
            .map_err(|_| ApiError::forbidden())?;
    let ticket: Ticket = serde_json::from_str(&plain).map_err(|_| ApiError::forbidden())?;
    if ticket.purpose != PURPOSE || chrono::Utc::now().timestamp() >= ticket.until {
        return Err(ApiError::forbidden());
    }
    let pool = super::pool(state)?;
    let workspace = store::workspace(pool, ticket.workspace)
        .await
        .map_err(super::error)?;
    let session = store::session(pool, workspace.id)
        .await
        .map_err(super::error)?
        .filter(|s| s.actor_id == ticket.actor && s.owner_subject == ticket.subject)
        .ok_or_else(ApiError::forbidden)?;
    let current: DeveloperAssignment =
        serde_json::from_value(session.machine_assignment.ok_or_else(ApiError::forbidden)?)
            .map_err(|_| ApiError::forbidden())?;
    ticket.assignment.require_same(&current)?;
    let verified = crate::central_auth::try_verified_central(&ticket.token)
        .await?
        .ok_or_else(ApiError::forbidden)?;
    if verified.issuer != ticket.issuer || verified.context.user_id != ticket.subject.to_string() {
        return Err(ApiError::forbidden());
    }
    let proof = machine::confirm_identity(
        pool,
        &workspace,
        ticket.actor,
        verified,
        &ticket.assignment,
        true,
        AssignmentAccess::Active,
    )
    .await?;
    proof.clock.cap_until(ticket.until).require()?;
    Ok(crate::task_git::CandidateCheckReadback {
        verified: true,
        triggered: false,
    })
}

const PRE_RECEIVE: &str = r#"set -eu
umask 077
cat > "$dir/input"
if test "$(wc -l < "$dir/input")" -ne 1; then
    printf '%s\n' 'Forge: workspace push must update exactly one branch.' >&2
    exit 1
fi
read -r old new ref extra < "$dir/input"
if test -n "$extra" || test "$ref" != "$(cat "$dir/candidate-ref")"; then
    printf '%s\n' 'Forge: workspace push does not match the assigned candidate branch.' >&2
    exit 1
fi
printf '%s %s %s\n' "$old" "$new" "$ref" > "$dir/expected-update"
"$dir/protected-pre-receive" "$@" < "$dir/input"
"$dir/task-check" < "$dir/input"
"#;

pub(crate) fn install(
    directory: &Path,
    authorization: &Authorization,
    internal: &str,
    port: u16,
) -> Result<(), ApiError> {
    use crate::task_git::hooks::{CHECK, quote, write};
    let error = || ApiError::service_unavailable("Private workspace Git hook unavailable");
    if internal.is_empty() || !internal.bytes().all(|b| b.is_ascii_graphic()) || port == 0 {
        return Err(error());
    }
    let sealed = authorization.sealed.as_ref().ok_or_else(error)?;
    let candidate = authorization.source["candidate_ref"]
        .as_str()
        .ok_or_else(error)?;
    let base = authorization.source["base_commit"]
        .as_str()
        .ok_or_else(error)?;
    if !crate::registered_source::full_branch(candidate)
        || base.len() != 40
        || !base.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(error());
    }
    fs::rename(
        directory.join("pre-receive"),
        directory.join("protected-pre-receive"),
    )
    .map_err(|_| error())?;
    let transaction = directory.join("reference-transaction");
    let saved_transaction = directory.join("original-reference-transaction");
    let prior = transaction.exists();
    if prior {
        fs::rename(&transaction, &saved_transaction).map_err(|_| error())?;
    }
    let post = directory.join("post-receive");
    if post.exists() {
        fs::remove_file(&post).map_err(|_| error())?;
    }
    write(&post, b"#!/bin/sh\ncat >/dev/null\nexit 0\n", true)?;
    write(&directory.join("base"), base.as_bytes(), false)?;
    write(
        &directory.join("candidate-ref"),
        candidate.as_bytes(),
        false,
    )?;
    let body = serde_json::to_vec(&CandidateCheck {
        developer_workspace: sealed.clone(),
    })
    .map_err(|_| error())?;
    write(&directory.join("context.json"), &body, false)?;
    write(
        &directory.join("child-headers"),
        format!("x-internal-token: {internal}\n").as_bytes(),
        false,
    )?;
    let pre = format!("#!/bin/sh\ndir={}\n{PRE_RECEIVE}", quote(directory)?);
    write(&directory.join("pre-receive"), pre.as_bytes(), true)?;
    let check = format!(
        "#!/bin/sh\ndir={}\ncallback='http://127.0.0.1:{port}/api/v1/internal/development-workspace-git-check'\n{CHECK}",
        quote(directory)?
    );
    write(&directory.join("task-check"), check.as_bytes(), true)?;
    let delegate = if prior {
        format!(
            "{} \"$@\" < \"$dir/transaction-input\"\n",
            quote(&saved_transaction)?
        )
    } else {
        String::new()
    };
    let script = format!(
        "#!/bin/sh\nset -eu\numask 077\ndir={}\ncat > \"$dir/transaction-input\"\n{delegate}if test \"$1\" = prepared; then\n    {} < \"$dir/transaction-input\"\nfi\n",
        quote(directory)?,
        quote(&directory.join("task-check"))?
    );
    write(&transaction, script.as_bytes(), true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn authorization(source: serde_json::Value, deadline: GitDeadline) -> Authorization {
        Authorization {
            source,
            deadline,
            sealed: Some("synthetic-server-ciphertext".into()),
        }
    }
    #[test]
    fn private_git_check_cannot_fall_back_to_an_ordinary_pipeline_notification() {
        let body = serde_json::json!({"developer_workspace":"synthetic-ciphertext"});
        assert!(serde_json::from_value::<CandidateCheck>(body.clone()).is_ok());
        assert!(serde_json::from_value::<crate::task_git::CandidateCheck>(body.clone()).is_err());
        assert!(serde_json::from_value::<crate::task_git::PushRequest>(body).is_err());
        for body in [
            serde_json::json!({"developer_workspace":123}),
            serde_json::json!({"developer_workspace":"synthetic-ciphertext","task_candidate":{}}),
            serde_json::json!({"developer_workspace":"synthetic-ciphertext","ref":"main"}),
        ] {
            assert!(serde_json::from_value::<CandidateCheck>(body.clone()).is_err());
            assert!(serde_json::from_value::<crate::task_git::PushRequest>(body).is_err());
        }
    }
    #[test]
    fn git_workspace_locator_is_explicit_and_is_not_an_account_alias() {
        assert_eq!(workspace_user("git").unwrap(), None);
        let id = Uuid::new_v4();
        assert_eq!(
            workspace_user(&format!("workspace-{id}")).unwrap(),
            Some(id)
        );
        for username in [
            "workspace-current",
            "workspace-",
            "workspace-00000000-0000-0000-0000-000000000000",
        ] {
            assert!(workspace_user(username).is_err());
        }
    }
}
