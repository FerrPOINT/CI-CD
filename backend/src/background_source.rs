//! Narrow, workspace-free source reads for a Fleet-owned research run.
use crate::api::{ApiError, AppState};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    routing::get,
};
use pdlc_auth_core::service_bridge::{VerifiedBridgeError, VerifiedCentralToken};
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/background/runs/{run}/repositories/{repository}/tree",
            get(tree),
        )
        .route(
            "/api/v1/background/runs/{run}/repositories/{repository}/blob",
            get(blob),
        )
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Source {
    repository: Uuid,
    branch: String,
    sha: String,
}
#[derive(Deserialize)]
struct Grant {
    repository: Uuid,
    forge_instance_id: Uuid,
    branch: String,
    namespace: NamespaceRef,
}
#[derive(Deserialize)]
struct Context {
    machine_subject: Uuid,
    repositories: Vec<Grant>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    run_id: Uuid,
    project_id: Uuid,
    process_id: Uuid,
    session_id: Uuid,
    agent_id: Uuid,
    state: String,
    workflow_revision: String,
    context: Context,
    sources: Vec<Source>,
}

fn unavailable() -> ApiError {
    ApiError::service_unavailable("Фоновое чтение исходников недоступно")
}

struct FleetReader {
    client: reqwest::Client,
    root: reqwest::Url,
    token: String,
}
impl FleetReader {
    fn configured() -> Result<Self, ApiError> {
        let raw = std::env::var("CICD_BACKGROUND__FLEET_URL").map_err(|_| unavailable())?;
        let root = reqwest::Url::parse(&raw).map_err(|_| unavailable())?;
        let token =
            std::env::var("CICD_BACKGROUND__FLEET_READER_TOKEN").map_err(|_| unavailable())?;
        if !matches!(root.scheme(), "http" | "https")
            || root.host_str().is_none()
            || !root.username().is_empty()
            || root.password().is_some()
            || root.path() != "/"
            || root.query().is_some()
            || root.fragment().is_some()
            || raw.chars().any(char::is_whitespace)
            || token.is_empty()
            || token.chars().any(char::is_whitespace)
        {
            return Err(unavailable());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            client,
            root,
            token,
        })
    }
    async fn admission(&self, run: Uuid) -> Result<Admission, ApiError> {
        let url = self
            .root
            .join(&format!("internal/background/v1/runs/{run}"))
            .map_err(|_| unavailable())?;
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ApiError::forbidden());
        }
        if !response.status().is_success() {
            return Err(unavailable());
        }
        response.json().await.map_err(|_| unavailable())
    }
    async fn record(&self, run: Uuid, source: &Source) -> Result<(), ApiError> {
        let url = self
            .root
            .join(&format!("internal/background/v1/runs/{run}/sources"))
            .map_err(|_| unavailable())?;
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(source)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(ApiError::forbidden());
        }
        if !response.status().is_success() {
            return Err(unavailable());
        }
        Ok(())
    }
}

async fn credential(headers: &HeaderMap) -> Result<VerifiedCentralToken, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorized)?;
    let credential = crate::central_auth::BRIDGE
        .try_verified_token(token)
        .await
        .map_err(|error| match error {
            VerifiedBridgeError::Unavailable | VerifiedBridgeError::NotConfigured => unavailable(),
            _ => ApiError::unauthorized(),
        })?;
    if credential.context.session_id.is_some()
        || credential.subject_role.as_deref() != Some("service_account")
        || !credential.context.has_scope("ci-cd:background:read")
    {
        return Err(ApiError::forbidden());
    }
    current(&credential)?;
    Ok(credential)
}
fn current(credential: &VerifiedCentralToken) -> Result<(), ApiError> {
    if !credential.lifetime.is_current() {
        return Err(ApiError::unauthorized());
    }
    Ok(())
}
fn grant<'a>(
    admission: &'a Admission,
    run: Uuid,
    subject: &str,
    repository: Uuid,
    branch: &str,
) -> Result<&'a Grant, ApiError> {
    if admission.run_id != run
        || admission.context.machine_subject.to_string() != subject
        || admission.project_id.is_nil()
        || admission.process_id.is_nil()
        || admission.session_id.is_nil()
        || admission.agent_id.is_nil()
        || admission.workflow_revision.len() != 64
        || !admission
            .workflow_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !matches!(admission.state.as_str(), "queued" | "running")
        || !crate::registered_source::full_branch(branch)
    {
        return Err(ApiError::forbidden());
    }
    let selected = admission
        .context
        .repositories
        .iter()
        .filter(|item| item.repository == repository && item.branch == branch)
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err(ApiError::forbidden());
    }
    Ok(selected[0])
}

async fn repository_path(
    state: &AppState,
    connection: &mut sqlx::PgConnection,
    grant: &Grant,
) -> Result<PathBuf, ApiError> {
    let own_instance = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
        .ok()
        .and_then(|value| value.parse::<Uuid>().ok())
        .filter(|id| !id.is_nil())
        .ok_or_else(unavailable)?;
    if grant.forge_instance_id != own_instance {
        return Err(ApiError::forbidden());
    }
    let row = sqlx::query(
        "SELECT r.group_id,r.kind,r.ready,r.storage_name,b.command FROM repository_catalog r
        JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE r.id=$1",
    )
    .bind(grant.repository)
    .fetch_optional(connection)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    let command: OwnerCommand =
        serde_json::from_value(row.try_get("command").map_err(ApiError::internal)?)
            .map_err(|_| unavailable())?;
    crate::namespace::validate_projection(&command)?;
    crate::registered_source::validate_catalog(
        &command,
        &grant.namespace,
        row.try_get("group_id").map_err(ApiError::internal)?,
        &row.try_get::<String, _>("kind")
            .map_err(ApiError::internal)?,
        row.try_get("ready").map_err(ApiError::internal)?,
    )?;
    let storage: String = row.try_get("storage_name").map_err(ApiError::internal)?;
    if crate::git_host::validate_repo_name(&storage).map_err(ApiError::bad_request)? != storage {
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
    Ok(path)
}
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
async fn source(
    path: &std::path::Path,
    admission: &Admission,
    grant: &Grant,
) -> Result<Source, ApiError> {
    let saved = admission
        .sources
        .iter()
        .filter(|item| item.repository == grant.repository && item.branch == grant.branch)
        .collect::<Vec<_>>();
    if saved.len() > 1 {
        return Err(unavailable());
    }
    let sha = if let Some(saved) = saved.first() {
        saved.sha.clone()
    } else {
        crate::pulls::branch_snapshot(path, &grant.branch)
            .await?
            .oid
    };
    if !valid_commit(&sha) {
        return Err(unavailable());
    }
    Ok(Source {
        repository: grant.repository,
        branch: grant.branch.clone(),
        sha,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeQuery {
    branch: String,
    path: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobQuery {
    branch: String,
    path: String,
}

pub async fn tree(
    State(state): State<Arc<AppState>>,
    Path((run, repository)): Path<(Uuid, Uuid)>,
    Query(query): Query<TreeQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let credential = credential(&headers).await?;
    let reader = FleetReader::configured()?;
    let admission = reader.admission(run).await?;
    let grant = grant(
        &admission,
        run,
        &credential.context.user_id,
        repository,
        &query.branch,
    )?;
    let limit = query.limit.unwrap_or(100);
    if !(1..=200).contains(&limit) {
        return Err(ApiError::bad_request("invalid_tree_limit"));
    }
    let offset = query.offset.unwrap_or(0);
    let mut lease = crate::repository_catalog::catalog_write_lease(&state, repository).await?;
    let path = repository_path(&state, &mut lease, grant).await?;
    let source = source(&path, &admission, grant).await?;
    let mut items = crate::pulls::read_committed_tree(
        &path,
        &source.sha,
        query.path.as_deref().unwrap_or(""),
        limit + 1,
        offset,
    )
    .await?;
    let next_offset = if items.len() > limit {
        items.pop();
        Some(
            offset
                .checked_add(limit)
                .ok_or_else(|| ApiError::bad_request("invalid_tree_offset"))?,
        )
    } else {
        None
    };
    current(&credential)?;
    reader.record(run, &source).await?;
    current(&credential)?;
    lease.commit().await.map_err(ApiError::internal)?;
    Ok(Json(
        json!({"source":source,"items":items,"next_offset":next_offset}),
    ))
}
pub async fn blob(
    State(state): State<Arc<AppState>>,
    Path((run, repository)): Path<(Uuid, Uuid)>,
    Query(query): Query<BlobQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let credential = credential(&headers).await?;
    let reader = FleetReader::configured()?;
    let admission = reader.admission(run).await?;
    let grant = grant(
        &admission,
        run,
        &credential.context.user_id,
        repository,
        &query.branch,
    )?;
    if query.path.is_empty() {
        return Err(ApiError::bad_request("source_path_required"));
    }
    let mut lease = crate::repository_catalog::catalog_write_lease(&state, repository).await?;
    let path = repository_path(&state, &mut lease, grant).await?;
    let source = source(&path, &admission, grant).await?;
    let result = crate::pulls::read_blob_content(&path, &source.sha, query.path).await?;
    current(&credential)?;
    reader.record(run, &source).await?;
    current(&credential)?;
    lease.commit().await.map_err(ApiError::internal)?;
    Ok(Json(json!({"source":source,"blob":result})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Admission {
        serde_json::from_value(json!({"run_id":Uuid::new_v4(),"project_id":Uuid::new_v4(),"process_id":Uuid::new_v4(),
            "session_id":Uuid::new_v4(),"agent_id":Uuid::new_v4(),"state":"running","workflow_revision":"a".repeat(64),
            "context":{"machine_subject":Uuid::new_v4(),"repositories":[{"repository":Uuid::new_v4(),"forge_instance_id":Uuid::new_v4(),
                "branch":"refs/heads/fixture","namespace":{"registry_instance_id":Uuid::new_v4(),"namespace_id":Uuid::new_v4()}}]},"sources":[]})).unwrap()
    }

    #[test]
    fn exact_launch_subject_repository_and_branch_are_required() {
        let mut admission = fixture();
        let subject = admission.context.machine_subject.to_string();
        let repository = admission.context.repositories[0].repository;
        assert!(
            grant(
                &admission,
                admission.run_id,
                &subject,
                repository,
                "refs/heads/fixture"
            )
            .is_ok()
        );
        assert!(
            grant(
                &admission,
                Uuid::new_v4(),
                &subject,
                repository,
                "refs/heads/fixture"
            )
            .is_err()
        );
        assert!(
            grant(
                &admission,
                admission.run_id,
                &Uuid::new_v4().to_string(),
                repository,
                "refs/heads/fixture"
            )
            .is_err()
        );
        assert!(
            grant(
                &admission,
                admission.run_id,
                &subject,
                Uuid::new_v4(),
                "refs/heads/fixture"
            )
            .is_err()
        );
        for branch in ["HEAD", "main", "refs/tags/fixture", "refs/heads/other"] {
            assert!(grant(&admission, admission.run_id, &subject, repository, branch).is_err());
        }
        for state in [
            "completed",
            "incomplete",
            "cancelled",
            "failed",
            "preparing",
        ] {
            admission.state = state.into();
            assert!(
                grant(
                    &admission,
                    admission.run_id,
                    &subject,
                    repository,
                    "refs/heads/fixture"
                )
                .is_err()
            );
        }
    }

    fn git(path: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    #[tokio::test]
    async fn bare_reads_keep_the_recorded_commit_after_branch_moves() {
        let directory = tempfile::tempdir().unwrap();
        let work = directory.path().join("work");
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "--quiet"]);
        git(&work, &["symbolic-ref", "HEAD", "refs/heads/fixture"]);
        git(&work, &["config", "user.name", "Source Fixture"]);
        git(&work, &["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(work.join("example.txt"), "original committed source").unwrap();
        git(&work, &["add", "example.txt"]);
        git(&work, &["commit", "--quiet", "-m", "fixture source"]);
        let original = git(&work, &["rev-parse", "HEAD"]);
        std::fs::write(work.join("example.txt"), "later source").unwrap();
        git(&work, &["commit", "--quiet", "-am", "next fixture source"]);
        let current = git(&work, &["rev-parse", "HEAD"]);
        let bare = directory.path().join("source.git");
        git(
            directory.path(),
            &[
                "clone",
                "--quiet",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let mut admission = fixture();
        let selected = &admission.context.repositories[0];
        let initial = source(&bare, &admission, selected).await.unwrap();
        assert_eq!(initial.sha, current);
        admission.sources.push(Source {
            repository: initial.repository,
            branch: initial.branch,
            sha: original.clone(),
        });
        let pinned = source(&bare, &admission, &admission.context.repositories[0])
            .await
            .unwrap();
        assert_eq!(pinned.sha, original);
        let content = crate::pulls::read_blob_content(&bare, &pinned.sha, "example.txt".into())
            .await
            .unwrap();
        let content = serde_json::to_value(content).unwrap();
        assert_eq!(content["content"], "original committed source");
        assert_eq!(content["truncated"], false);
        let entries = crate::pulls::read_committed_tree(&bare, &pinned.sha, "", 2, 0)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        let original_tree = serde_json::to_value(&entries).unwrap();
        git(&bare, &["replace", &original, &current]);
        let still_original =
            crate::pulls::read_blob_content(&bare, &pinned.sha, "example.txt".into())
                .await
                .unwrap();
        assert_eq!(
            serde_json::to_value(still_original).unwrap()["content"],
            "original committed source"
        );
        let still_original_tree = crate::pulls::read_committed_tree(&bare, &pinned.sha, "", 2, 0)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(still_original_tree).unwrap(),
            original_tree
        );
    }
}
