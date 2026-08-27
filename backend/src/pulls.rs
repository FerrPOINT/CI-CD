use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::{ApiError, AppState};

// ─── refs & commits ───

#[derive(Serialize, utoipa::ToSchema)]
pub struct RefInfo {
    pub name: String,
    pub sha: String,
    pub target: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub author: String,
    pub email: String,
    pub message: String,
    pub date: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/refs",
    tag = "repos",
    params(("repo" = String, Path, description = "Repository name")),
    responses((status = 200, body = [RefInfo]), (status = 404)),
)]
pub async fn list_refs(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Vec<RefInfo>>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args([
            "for-each-ref",
            "--format=%(refname) %(objectname) %(contents:subject)",
        ])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !output.status.success() {
        return Err(ApiError::bad_request("git for-each-ref failed"));
    }
    let refs: Vec<RefInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            let raw_ref = parts.next()?;
            let sha = parts.next()?;
            let target = parts.next().unwrap_or("");
            let name = raw_ref
                .strip_prefix("refs/heads/")
                .or_else(|| raw_ref.strip_prefix("refs/tags/"))
                .unwrap_or(raw_ref)
                .to_string();
            Some(RefInfo {
                name,
                sha: sha.to_string(),
                target: target.to_string(),
            })
        })
        .collect();
    Ok(Json(refs))
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/commits",
    tag = "repos",
    params(CommitParams, ("repo" = String, Path, description = "Repository name")),
    responses((status = 200, body = [CommitInfo]), (status = 400), (status = 404)),
)]
pub async fn list_commits(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    Query(params): Query<CommitParams>,
) -> Result<Json<Vec<CommitInfo>>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    let ref_spec = params.branch.unwrap_or_else(|| "HEAD".into());
    let limit = params.limit.unwrap_or(50).min(200);
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args([
            "log",
            &format!("-{limit}"),
            "--format=%H%n%an%n%ae%n%s%n%ci",
            &ref_spec,
        ])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !output.status.success() {
        return Err(ApiError::bad_request(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let mut commits = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = stdout.lines().collect();
    for chunk in lines.chunks(5) {
        if chunk.len() < 5 {
            break;
        }
        commits.push(CommitInfo {
            sha: chunk[0].to_string(),
            short_sha: chunk[0][..7.min(chunk[0].len())].to_string(),
            author: chunk[1].to_string(),
            email: chunk[2].to_string(),
            message: chunk[3].to_string(),
            date: chunk[4].to_string(),
        });
    }
    Ok(Json(commits))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CommitParams {
    pub branch: Option<String>,
    pub limit: Option<u32>,
}

// ─── compare (diff) ───

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CompareParams {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DiffResult {
    pub from: String,
    pub to: String,
    pub merge_base: String,
    pub files: Vec<DiffFile>,
    pub patch: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DiffFile {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/compare",
    tag = "repos",
    params(CompareParams, ("repo" = String, Path, description = "Repository name")),
    responses((status = 200, body = DiffResult), (status = 400), (status = 404)),
)]
pub async fn compare_refs(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    Query(params): Query<CompareParams>,
) -> Result<Json<DiffResult>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    let from = &params.from;
    let to = &params.to;

    // merge-base
    let mb_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["merge-base", from, to])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !mb_output.status.success() {
        return Err(ApiError::bad_request(
            String::from_utf8_lossy(&mb_output.stderr).to_string(),
        ));
    }
    let merge_base = String::from_utf8_lossy(&mb_output.stdout)
        .trim()
        .to_string();

    // numstat
    let stat_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["diff", "--numstat", &merge_base, to])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let files: Vec<DiffFile> = String::from_utf8_lossy(&stat_output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let additions = parts.next()?.parse::<u32>().ok()?;
            let deletions = parts.next()?.parse::<u32>().ok()?;
            let rest = parts.next()?;
            let (status, path_str) = if let Some(stripped) = rest.strip_prefix("A\t") {
                ("added", stripped)
            } else if let Some(stripped) = rest.strip_prefix("D\t") {
                ("deleted", stripped)
            } else if let Some(stripped) = rest.strip_prefix("M\t") {
                ("modified", stripped)
            } else {
                ("modified", rest)
            };
            Some(DiffFile {
                path: path_str.to_string(),
                status: status.to_string(),
                additions,
                deletions,
            })
        })
        .collect();

    // patch
    let patch_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["diff", &merge_base, to])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let patch = String::from_utf8_lossy(&patch_output.stdout).to_string();

    Ok(Json(DiffResult {
        from: from.clone(),
        to: to.clone(),
        merge_base,
        files,
        patch,
    }))
}

// ─── pull requests ───

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct PullRequest {
    pub id: Uuid,
    pub repository_name: String,
    pub number: i32,
    pub title: String,
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
    pub status: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
    pub merge_commit_sha: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePullRequest {
    pub repository_name: String,
    pub title: String,
    pub description: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    /// Optional author label; overridden by the authenticated identity.
    #[serde(default)]
    pub author: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/pulls",
    tag = "pulls",
    params(("repo" = String, Path, description = "Repository name")),
    responses((status = 200, body = [PullRequest]), (status = 404)),
)]
pub async fn list_pull_requests(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Vec<PullRequest>>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let prs = sqlx::query_as::<_, PullRequest>(
        "SELECT id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha FROM pull_requests WHERE repository_name = $1 ORDER BY number DESC",
    )
    .bind(&repo)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(prs))
}

#[utoipa::path(
    post,
    path = "/api/v1/repos/{repo}/pulls",
    tag = "pulls",
    request_body = CreatePullRequest,
    params(("repo" = String, Path, description = "Repository name")),
    responses((status = 200, body = PullRequest), (status = 400)),
)]
pub async fn create_pull_request(
    State(state): State<std::sync::Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::AccessClaims>>,
    Json(input): Json<CreatePullRequest>,
) -> Result<Json<PullRequest>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    if input.title.trim().is_empty()
        || input.source_branch.trim().is_empty()
        || input.target_branch.trim().is_empty()
    {
        return Err(ApiError::bad_request(
            "title, source_branch and target_branch are required",
        ));
    }
    if input.source_branch == input.target_branch {
        return Err(ApiError::bad_request(
            "source and target branches must differ",
        ));
    }
    let next_number: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM pull_requests WHERE repository_name = $1",
    )
    .bind(&input.repository_name)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    // Author: authenticated identity wins; explicit input is a fallback for
    // trusted-network mode where no claims exist.
    let author = match claims.as_ref().map(|c| c.0.sub) {
        Some(user_id) => {
            sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
        }
        None => input.author.clone().unwrap_or_default(),
    };
    let pr = sqlx::query_as::<_, PullRequest>(
        "INSERT INTO pull_requests (id, repository_name, number, title, description, source_branch, target_branch, status, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, 'open', $8) RETURNING id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha",
    )
    .bind(Uuid::new_v4())
    .bind(&input.repository_name)
    .bind(next_number)
    .bind(input.title.trim())
    .bind(input.description.unwrap_or_default())
    .bind(input.source_branch.trim())
    .bind(input.target_branch.trim())
    .bind(&author)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(pr))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PrAction {
    pub action: String, // "merge" | "close" | "reopen"
}

#[utoipa::path(
    post,
    path = "/api/v1/repos/{repo}/pulls/{number}/action",
    tag = "pulls",
    request_body = PrAction,
    params(
        ("repo" = String, Path, description = "Repository name"),
        ("number" = i32, Path, description = "Pull request number"),
    ),
    responses((status = 200, body = PullRequest), (status = 400), (status = 404), (status = 409)),
)]
pub async fn pr_action(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath((repo, number)): AxumPath<(String, i32)>,
    Json(input): Json<PrAction>,
) -> Result<Json<PullRequest>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let pr = sqlx::query_as::<_, PullRequest>(
        "SELECT id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha FROM pull_requests WHERE repository_name = $1 AND number = $2",
    )
    .bind(&repo)
    .bind(number)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;

    match input.action.as_str() {
        "merge" => {
            if pr.status != "open" {
                return Err(ApiError::conflict("pull request is not open"));
            }
            let path = resolve_repo_path(&state, &repo).await?;
            // Use git merge-tree for bare repos (no worktree needed)
            let merge_output = tokio::process::Command::new("git")
                .arg(format!("--git-dir={}", path.display()))
                .args([
                    "merge-tree",
                    "--write-tree",
                    "-z",
                    &pr.target_branch,
                    &pr.source_branch,
                ])
                .output()
                .await
                .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
            if !merge_output.status.success() {
                return Err(ApiError::conflict(
                    String::from_utf8_lossy(&merge_output.stderr).to_string(),
                ));
            }
            let tree_sha = String::from_utf8_lossy(&merge_output.stdout)
                .split('\0')
                .next()
                .unwrap_or("")
                .to_string();
            // Create merge commit
            let commit_output = tokio::process::Command::new("git")
                .arg(format!("--git-dir={}", path.display()))
                .args([
                    "-c",
                    "user.name=Forge CI/CD",
                    "-c",
                    "user.email=forge@localhost",
                    "commit-tree",
                    &tree_sha,
                    "-p",
                    &pr.target_branch,
                    "-p",
                    &pr.source_branch,
                    "-m",
                    &format!("Merge PR #{}: {}", pr.number, pr.title),
                ])
                .output()
                .await
                .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
            if !commit_output.status.success() {
                return Err(ApiError::bad_request(
                    String::from_utf8_lossy(&commit_output.stderr).to_string(),
                ));
            }
            let merge_sha = String::from_utf8_lossy(&commit_output.stdout)
                .trim()
                .to_string();
            // Update target branch ref
            let ref_output = tokio::process::Command::new("git")
                .arg(format!("--git-dir={}", path.display()))
                .args([
                    "update-ref",
                    &format!("refs/heads/{}", pr.target_branch),
                    &merge_sha,
                ])
                .output()
                .await
                .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
            if !ref_output.status.success() {
                return Err(ApiError::bad_request(
                    String::from_utf8_lossy(&ref_output.stderr).to_string(),
                ));
            }
            let updated = sqlx::query_as::<_, PullRequest>(
                "UPDATE pull_requests SET status = 'merged', merged_at = now(), updated_at = now(), merge_commit_sha = $2 WHERE id = $1 RETURNING id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha",
            )
            .bind(pr.id)
            .bind(&merge_sha)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
            Ok(Json(updated))
        }
        "close" => {
            if pr.status != "open" {
                return Err(ApiError::conflict("pull request is not open"));
            }
            let updated = sqlx::query_as::<_, PullRequest>(
                "UPDATE pull_requests SET status = 'closed', updated_at = now() WHERE id = $1 RETURNING id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha",
            )
            .bind(pr.id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
            Ok(Json(updated))
        }
        "reopen" => {
            if pr.status != "closed" {
                return Err(ApiError::conflict(
                    "only closed pull requests can be reopened",
                ));
            }
            let updated = sqlx::query_as::<_, PullRequest>(
                "UPDATE pull_requests SET status = 'open', updated_at = now() WHERE id = $1 RETURNING id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha",
            )
            .bind(pr.id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
            Ok(Json(updated))
        }
        _ => Err(ApiError::bad_request(
            "action must be merge, close or reopen",
        )),
    }
}

// ─── helpers ───

async fn resolve_repo_path(state: &AppState, raw: &str) -> Result<PathBuf, ApiError> {
    let name = crate::git_host::validate_repo_name(raw).map_err(ApiError::bad_request)?;
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM repositories WHERE name = $1)")
            .bind(&name)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if !exists {
        return Err(ApiError::not_found());
    }
    Ok(state.git.root.join(format!("{name}.git")))
}
