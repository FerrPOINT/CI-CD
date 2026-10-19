//! Source observations behind the existing namespace repository reader, never Git authority.
use crate::api::{ApiError, AppState};
use chrono::{DateTime, Utc};
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{collections::BTreeSet, path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};
use uuid::Uuid;

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
pub struct RepositoryQuery {
    pub registry_instance_id: Uuid,
    pub namespace_id: Uuid,
    pub base_ref: String,
    pub base_commit: String,
    pub delivery_configuration_name: Option<String>,
}

impl RepositoryQuery {
    pub fn source(&self) -> Result<ExpectedSource, ApiError> {
        if self
            .delivery_configuration_name
            .as_deref()
            .is_some_and(|name| {
                name.is_empty()
                    || name.len() > 256
                    || name.trim() != name
                    || name.chars().any(char::is_control)
            })
        {
            return Err(ApiError::bad_request("invalid_delivery_configuration_name"));
        }
        if full_branch(&self.base_ref) && sha1(&self.base_commit) {
            Ok(ExpectedSource {
                reference: self.base_ref.clone(),
                commit: self.base_commit.clone(),
            })
        } else {
            Err(ApiError::bad_request(
                "Нужны полная ветка base_ref и точный base_commit SHA1",
            ))
        }
    }
    pub fn namespace(&self) -> NamespaceRef {
        NamespaceRef {
            registry_instance_id: self.registry_instance_id,
            namespace_id: self.namespace_id,
        }
    }
}

pub struct ExpectedSource {
    reference: String,
    commit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifiedSource {
    pub namespace: NamespaceRef,
    pub forge_instance_id: Uuid,
    pub repository_id: Uuid,
    pub public_name: String,
    pub base_ref: String,
    pub base_commit: String,
    pub default_ref: String,
    pub protected_refs: Vec<String>,
    pub observed_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_configuration: Option<DeliveryConfigurationReadback>,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryConfigurationReadback {
    pub id: Uuid,
    pub name: String,
    pub repository_id: Uuid,
    pub repository_name: String,
}

fn exact_delivery_configuration(
    rows: Vec<(Uuid, String, Uuid)>,
    repository: Uuid,
    public_name: &str,
    name: &str,
) -> Result<DeliveryConfigurationReadback, ApiError> {
    if rows.is_empty() {
        return Err(ApiError::not_found());
    }
    if rows.len() != 1 {
        return Err(ApiError::conflict("delivery_configuration_ambiguous"));
    }
    let (id, actual_name, actual_repository) = rows.into_iter().next().ok_or_else(unavailable)?;
    if id.is_nil() || actual_name != name || actual_repository != repository {
        return Err(ApiError::forbidden());
    }
    Ok(DeliveryConfigurationReadback {
        id,
        name: actual_name,
        repository_id: actual_repository,
        repository_name: public_name.to_owned(),
    })
}

fn unavailable() -> ApiError {
    ApiError::service_unavailable("Проверенный источник репозитория недоступен")
}

fn sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

// Matches Tracker's portable input contract; native Git still validates/resolves the branch.
fn full_branch(value: &str) -> bool {
    value.starts_with("refs/heads/")
        && value.len() <= 255
        && !value.ends_with(['/', '.'])
        && !["..", "@{", "//"].iter().any(|part| value.contains(part))
        && value
            .bytes()
            .all(|c| c.is_ascii_graphic() && !b"~^:?*[\\".contains(&c))
        && value
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

fn validate_catalog(
    command: &OwnerCommand,
    namespace: &NamespaceRef,
    group: Uuid,
    kind: &str,
    ready: bool,
) -> Result<(), ApiError> {
    if command.namespace != *namespace
        || command.resource.resource_id != group
        || command.state != "active"
    {
        return Err(ApiError::forbidden());
    }
    if kind != "hosted" || !ready {
        return Err(unavailable());
    }
    Ok(())
}

fn protected_refs(
    default: &str,
    policies: Vec<(String, Vec<String>)>,
) -> Result<Vec<String>, ApiError> {
    if !full_branch(default) || policies.len() > 256 {
        return Err(unavailable());
    }
    let mut refs = BTreeSet::from([default.to_owned()]);
    // Same ALL-linked-project policy as native receive; include even branches absent from Git.
    for (default, protected) in policies {
        for branch in std::iter::once(default).chain(protected) {
            let reference = format!("refs/heads/{branch}");
            if !full_branch(&reference) {
                return Err(unavailable());
            }
            refs.insert(reference);
            if refs.len() > 256 {
                return Err(unavailable());
            }
        }
    }
    Ok(refs.into_iter().collect())
}

async fn git_line(path: &Path, args: &[&str]) -> Result<String, ApiError> {
    let mut child = Command::new("git")
        .arg("--git-dir")
        .arg(path)
        .args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| unavailable())?;
    let stdout = child.stdout.take().ok_or_else(unavailable)?;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        stdout
            .take(4097)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| unavailable())?;
        if bytes.len() > 4096 {
            return Err(unavailable());
        }
        if !child.wait().await.map_err(|_| unavailable())?.success() {
            return Err(unavailable());
        }
        let text = String::from_utf8(bytes).map_err(|_| unavailable())?;
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() || text.contains(['\r', '\n']) {
            return Err(unavailable());
        }
        Ok(text.to_owned())
    })
    .await
    .map_err(|_| unavailable())?;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}

async fn native_source(
    path: &Path,
    expected: &ExpectedSource,
) -> Result<(String, String), ApiError> {
    if !full_branch(&expected.reference) || !sha1(&expected.commit) {
        return Err(ApiError::bad_request("Недопустимый источник Git"));
    }
    if git_line(path, &["rev-parse", "--is-bare-repository"]).await? != "true" {
        return Err(unavailable());
    }
    // Reuse the exact branch snapshot used by PR merge: no tag, implicit HEAD or main/master fallback.
    let snapshot = crate::pulls::branch_snapshot(path, &expected.reference).await?;
    if snapshot.reference != expected.reference || !sha1(&snapshot.oid) {
        return Err(unavailable());
    }
    if snapshot.oid != expected.commit {
        return Err(ApiError::conflict("SHA выбранной ветки изменился"));
    }
    if git_line(path, &["cat-file", "-t", &snapshot.oid]).await? != "commit" {
        return Err(unavailable());
    }
    let default = git_line(path, &["symbolic-ref", "--quiet", "HEAD"]).await?;
    if !full_branch(&default) {
        return Err(unavailable());
    }
    Ok((snapshot.oid, default))
}

pub async fn observe(
    state: &AppState,
    repository: Uuid,
    namespace: NamespaceRef,
    expected: ExpectedSource,
    delivery_configuration_name: Option<&str>,
) -> Result<VerifiedSource, ApiError> {
    // The dedicated route keeps reader auth; this lease closes catalog/namespace relocation.
    let mut tx = crate::repository_catalog::catalog_write_lease(state, repository).await?;
    let observation = observe_on(
        state,
        &mut tx,
        repository,
        namespace,
        expected,
        delivery_configuration_name,
    )
    .await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(observation)
}

/// The caller retains the existing catalog/namespace lease for the complete observation.
pub(crate) async fn observe_on(
    state: &AppState,
    connection: &mut sqlx::PgConnection,
    repository: Uuid,
    namespace: NamespaceRef,
    expected: ExpectedSource,
    delivery_configuration_name: Option<&str>,
) -> Result<VerifiedSource, ApiError> {
    let row = sqlx::query("SELECT r.group_id,r.kind,r.ready,r.storage_name,g.slug||'/'||r.slug AS public_name,b.command
        FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id
        JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE r.id=$1")
        .bind(repository).fetch_optional(&mut *connection).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let command: OwnerCommand =
        serde_json::from_value(row.try_get("command").map_err(ApiError::internal)?)
            .map_err(|_| unavailable())?;
    crate::namespace::validate_projection(&command)?;
    validate_catalog(
        &command,
        &namespace,
        row.try_get("group_id").map_err(ApiError::internal)?,
        &row.try_get::<String, _>("kind")
            .map_err(ApiError::internal)?,
        row.try_get("ready").map_err(ApiError::internal)?,
    )?;
    let storage: String = row.try_get("storage_name").map_err(ApiError::internal)?;
    if crate::git_host::validate_repo_name(&storage).map_err(|_| unavailable())? != storage {
        return Err(unavailable());
    }
    let root = tokio::fs::canonicalize(&state.git.root)
        .await
        .map_err(|_| unavailable())?;
    let path = tokio::fs::canonicalize(root.join(format!("{storage}.git")))
        .await
        .map_err(|_| unavailable())?;
    if !path.starts_with(&root) || path == root {
        return Err(unavailable());
    }
    let public_name: String = row.try_get("public_name").map_err(ApiError::internal)?;
    if public_name.is_empty()
        || public_name.len() > 512
        || public_name.chars().any(char::is_control)
    {
        return Err(unavailable());
    }
    let policies = sqlx::query_as::<_,(String,Vec<String>)>(
        "SELECT default_branch,protected_branches FROM projects WHERE repository_id=$1 ORDER BY id LIMIT 257")
        .bind(repository).fetch_all(&mut *connection).await.map_err(ApiError::internal)?;
    let (commit, default) = native_source(&path, &expected).await?;
    let protected_refs = protected_refs(&default, policies)?;
    // Same catalog/namespace fence, exact repo and name; never a human catalog listing.
    let delivery_configuration = if let Some(name) = delivery_configuration_name {
        let rows = sqlx::query_as::<_, (Uuid, String, Uuid)>(
            "SELECT id,name,repository_id FROM projects WHERE repository_id=$1 AND name=$2 ORDER BY id LIMIT 2",
        )
        .bind(repository)
        .bind(name)
        .fetch_all(&mut *connection)
        .await
        .map_err(ApiError::internal)?;
        Some(exact_delivery_configuration(
            rows,
            repository,
            &public_name,
            name,
        )?)
    } else {
        None
    };
    let observation = VerifiedSource {
        namespace,
        forge_instance_id: command.resource.instance_id,
        repository_id: repository,
        public_name,
        base_ref: expected.reference,
        base_commit: commit,
        default_ref: default,
        protected_refs,
        observed_at: Utc::now(),
        delivery_configuration,
    };
    Ok(observation)
}

#[cfg(test)]
#[path = "registered_source_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "registered_source_delivery_tests.rs"]
mod delivery_configuration_tests;
