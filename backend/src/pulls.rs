use std::{collections::HashMap, path::PathBuf, process::Stdio};

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use crate::api::{ApiError, AppState};

// ─── refs & commits ───

#[derive(Serialize, utoipa::ToSchema)]
pub struct RefInfo {
    pub name: String,
    pub kind: String,
    pub sha: String,
    pub target: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub kind: String, // "blob" | "tree"
    pub size: Option<i64>,
    pub sha: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct BlobContent {
    pub path: String,
    pub sha: String,
    pub size: i64,
    pub content: String,
    pub binary: bool,
    pub truncated: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TagInfo {
    pub name: String,
    pub sha: String,
    pub message: String,
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
            let (name, kind) = classify_ref(raw_ref);
            Some(RefInfo {
                name: name.to_string(),
                kind: kind.to_string(),
                sha: sha.to_string(),
                target: target.to_string(),
            })
        })
        .collect();
    Ok(Json(refs))
}

fn classify_ref(raw_ref: &str) -> (&str, &str) {
    if let Some(name) = raw_ref.strip_prefix("refs/heads/") {
        (name, "branch")
    } else if let Some(name) = raw_ref.strip_prefix("refs/tags/") {
        (name, "tag")
    } else {
        (raw_ref, "other")
    }
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
    let (limit, offset) = params.page();
    let ref_spec = params.branch.unwrap_or_else(|| "HEAD".into());
    let ref_spec = resolve_view_ref(&path, &ref_spec).await;
    Ok(Json(read_commits(&path, &ref_spec, limit, offset).await?))
}

async fn read_commits(
    path: &std::path::Path,
    ref_spec: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<CommitInfo>, ApiError> {
    let max_count = format!("--max-count={limit}");
    let skip = format!("--skip={offset}");
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args([
            "log",
            &max_count,
            &skip,
            "--format=%H%n%an%n%ae%n%s%n%ci",
            ref_spec,
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
    Ok(commits)
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CommitParams {
    pub branch: Option<String>,
    #[param(default = 50, minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
    #[param(default = 0, minimum = 0)]
    pub offset: Option<u32>,
}

impl CommitParams {
    fn page(&self) -> (u32, u32) {
        (
            self.limit.unwrap_or(50).clamp(1, 200),
            self.offset.unwrap_or(0),
        )
    }
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
    pub patch_truncated: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DiffFile {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

fn parse_diff_files(numstat: &[u8], name_status: &[u8]) -> Result<Vec<DiffFile>, &'static str> {
    let mut statuses = HashMap::new();
    let mut status_fields = name_status
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(status) = status_fields.next() {
        let path = status_fields
            .next()
            .ok_or("missing path in git name-status")?;
        statuses.insert(path, status);
    }

    let mut files = Vec::new();
    for record in numstat
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let additions = fields.next().ok_or("missing additions in git numstat")?;
        let deletions = fields.next().ok_or("missing deletions in git numstat")?;
        let path = fields.next().ok_or("missing path in git numstat")?;
        let status = statuses.remove(path).ok_or("unmatched path in git diff")?;
        let status = match status.first() {
            Some(b'A') => "added",
            Some(b'D') => "deleted",
            Some(b'M' | b'T') => "modified",
            _ => return Err("unknown git diff status"),
        };
        let binary = additions == b"-" && deletions == b"-";
        if !binary && (additions == b"-" || deletions == b"-") {
            return Err("incomplete binary git numstat");
        }
        let parse_count = |value: &[u8]| -> Result<u32, &'static str> {
            std::str::from_utf8(value)
                .map_err(|_| "invalid git numstat count")?
                .parse()
                .map_err(|_| "invalid git numstat count")
        };
        files.push(DiffFile {
            path: String::from_utf8_lossy(path).into_owned(),
            status: status.to_string(),
            additions: if binary { 0 } else { parse_count(additions)? },
            deletions: if binary { 0 } else { parse_count(deletions)? },
            binary,
        });
    }
    if !statuses.is_empty() {
        return Err("unmatched status in git diff");
    }
    Ok(files)
}

const MAX_PATCH_LEN: usize = 512 * 1024;

async fn read_patch(
    path: &std::path::Path,
    merge_base: &str,
    to: &str,
) -> Result<(String, bool), ApiError> {
    let mut child = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["diff", "--no-renames", merge_base, to])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        ApiError::internal(sqlx::Error::Io(std::io::Error::other(
            "git diff stdout is unavailable",
        )))
    })?;
    let mut bytes = Vec::with_capacity(MAX_PATCH_LEN + 1);
    let mut bounded = stdout.take((MAX_PATCH_LEN + 1) as u64);
    bounded
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let truncated = bytes.len() > MAX_PATCH_LEN;
    if truncated {
        bytes.truncate(MAX_PATCH_LEN);
        let _ = child.start_kill();
    }
    let status = child
        .wait()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !truncated && !status.success() {
        return Err(ApiError::bad_request("git diff --patch failed"));
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
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

    // NUL-delimited outputs with rename detection disabled share exact paths.
    let stat_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["diff", "--numstat", "--no-renames", "-z", &merge_base, to])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !stat_output.status.success() {
        return Err(ApiError::bad_request("git diff --numstat failed"));
    }
    let status_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args([
            "diff",
            "--name-status",
            "--no-renames",
            "-z",
            &merge_base,
            to,
        ])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !status_output.status.success() {
        return Err(ApiError::bad_request("git diff --name-status failed"));
    }
    let files =
        parse_diff_files(&stat_output.stdout, &status_output.stdout).map_err(|message| {
            ApiError::internal(sqlx::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )))
        })?;

    let (patch, patch_truncated) = read_patch(&path, &merge_base, to).await?;

    Ok(Json(DiffResult {
        from: from.clone(),
        to: to.clone(),
        merge_base,
        files,
        patch,
        patch_truncated,
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PullRequestPage {
    pub items: Vec<PullRequest>,
    pub total: i64,
    #[schema(minimum = 1, maximum = 100)]
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestStatusFilter {
    Open,
    Closed,
    Merged,
}

impl PullRequestStatusFilter {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
        }
    }
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PullRequestListParams {
    #[param(default = 20, minimum = 1, maximum = 100)]
    pub limit: Option<u32>,
    #[param(default = 0, minimum = 0)]
    pub offset: Option<u32>,
    #[param(inline)]
    pub status: Option<PullRequestStatusFilter>,
    #[param(max_length = 200)]
    pub search: Option<String>,
}

struct NormalizedPullRequestListParams {
    limit: u32,
    offset: u32,
    status: Option<String>,
    search: Option<String>,
}

impl PullRequestListParams {
    fn normalize(self) -> Result<NormalizedPullRequestListParams, ApiError> {
        let limit = self.limit.unwrap_or(20);
        if !(1..=100).contains(&limit) {
            return Err(ApiError::bad_request("limit must be between 1 and 100"));
        }

        let status = self.status.map(|value| value.as_str().to_string());

        let search = self
            .search
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if search
            .as_deref()
            .is_some_and(|value| value.chars().count() > 200)
        {
            return Err(ApiError::bad_request(
                "search must not exceed 200 characters",
            ));
        }

        Ok(NormalizedPullRequestListParams {
            limit,
            offset: self.offset.unwrap_or(0),
            status,
            search,
        })
    }
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
        "SELECT id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha \
         FROM pull_requests WHERE repository_name = $1 ORDER BY number DESC",
    )
    .bind(&repo)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(prs))
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/pulls/page",
    tag = "pulls",
    params(PullRequestListParams, ("repo" = String, Path, description = "Repository name")),
    responses((status = 200, body = PullRequestPage), (status = 400)),
)]
pub async fn list_pull_request_page(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    Query(params): Query<PullRequestListParams>,
) -> Result<Json<PullRequestPage>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let params = params.normalize()?;
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pull_requests \
         WHERE repository_name = $1 \
           AND ($2::text IS NULL OR status = $2) \
           AND ($3::text IS NULL OR POSITION(LOWER($3::text) IN LOWER(CONCAT_WS(' ', number::text, title, source_branch, target_branch, created_by))) > 0)",
    )
    .bind(&repo)
    .bind(params.status.as_deref())
    .bind(params.search.as_deref())
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    let prs = sqlx::query_as::<_, PullRequest>(
        "SELECT id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha \
         FROM pull_requests \
         WHERE repository_name = $1 \
           AND ($2::text IS NULL OR status = $2) \
           AND ($3::text IS NULL OR POSITION(LOWER($3::text) IN LOWER(CONCAT_WS(' ', number::text, title, source_branch, target_branch, created_by))) > 0) \
         ORDER BY number DESC \
         LIMIT $4 OFFSET $5",
    )
    .bind(&repo)
    .bind(params.status.as_deref())
    .bind(params.search.as_deref())
    .bind(i64::from(params.limit))
    .bind(i64::from(params.offset))
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(PullRequestPage {
        items: prs,
        total,
        limit: params.limit,
        offset: params.offset,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/pulls/{number}",
    tag = "pulls",
    params(
        ("repo" = String, Path, description = "Repository name"),
        ("number" = i32, Path, description = "Pull request number"),
    ),
    responses((status = 200, body = PullRequest), (status = 404)),
)]
pub async fn get_pull_request(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath((repo, number)): AxumPath<(String, i32)>,
) -> Result<Json<PullRequest>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let pr = sqlx::query_as::<_, PullRequest>(
        "SELECT id, repository_name, number, title, description, source_branch, target_branch, status, created_by, created_at, updated_at, merged_at, merge_commit_sha \
         FROM pull_requests WHERE repository_name = $1 AND number = $2",
    )
    .bind(&repo)
    .bind(number)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(pr))
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
            // P0-4 merge gate: when the target branch is protected, require a
            // success pipeline on the PR source branch head (GitLab parity).
            let protected: Option<bool> = sqlx::query_scalar(
                "SELECT BOOL_OR($2 = ANY(p.protected_branches)) FROM projects p \
                 WHERE p.repository_url LIKE ('%' || $1 || '%')",
            )
            .bind(&pr.repository_name)
            .bind(&pr.target_branch)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?;
            if protected == Some(true) {
                let source_head: Option<String> = {
                    let path = resolve_repo_path(&state, &repo).await?;
                    let out = tokio::process::Command::new("git")
                        .arg(format!("--git-dir={}", path.display()))
                        .args(["rev-parse", &pr.source_branch])
                        .output()
                        .await
                        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
                    out.status
                        .success()
                        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
                };
                let green: Option<Uuid> = sqlx::query_scalar(
                    "SELECT pl.id FROM pipelines pl \
                     JOIN projects pr2 ON pr2.id = pl.project_id \
                     WHERE pr2.repository_url LIKE ('%' || $1 || '%') AND pl.git_ref = $2 \
                       AND pl.commit_sha = $3 AND pl.status = 'success' \
                     ORDER BY pl.created_at DESC LIMIT 1",
                )
                .bind(&pr.repository_name)
                .bind(&pr.source_branch)
                .bind(source_head.clone().unwrap_or_default())
                .fetch_optional(pool)
                .await
                .map_err(ApiError::internal)?;
                if green.is_none() {
                    return Err(ApiError::conflict(
                        "target branch is protected: a successful pipeline for the PR head commit is required before merge",
                    ));
                }
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

/// Resolves a user ref for bare repos: HEAD may be unset after fresh pushes,
/// so fall back to main, then master only for the implicit HEAD.
async fn resolve_view_ref(path: &std::path::Path, raw: &str) -> String {
    let candidates: Vec<&str> = if raw == "HEAD" {
        vec![raw, "main", "master"]
    } else {
        vec![raw]
    };
    for candidate in candidates {
        let ok = tokio::process::Command::new("git")
            .arg(format!("--git-dir={}", path.display()))
            .args(["rev-parse", "--verify", &format!("{candidate}^{{commit}}")])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return candidate.to_string();
        }
    }
    raw.to_string()
}

// ---- Code browsing: tree + blob (P0 git-server parity) ----

#[utoipa::path(get, path = "/api/v1/repos/{repo}/tree", tag = "git", params(("repo" = String, Path), ("ref" = Option<String>, Query), ("path" = Option<String>, Query), ("limit" = Option<usize>, Query), ("offset" = Option<usize>, Query), ("search" = Option<String>, Query)), responses((status = 200, body = [TreeEntry])))]
pub async fn list_tree(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    axum::extract::Query(params): axum::extract::Query<TreeQuery>,
) -> Result<Json<Vec<TreeEntry>>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    let git_ref = params
        .git_ref
        .as_deref()
        .filter(|r| !r.is_empty())
        .unwrap_or("HEAD");
    let git_ref = resolve_view_ref(&path, git_ref).await;
    let subpath = params.path.as_deref().unwrap_or("");
    let spec = if subpath.is_empty() {
        git_ref.to_string()
    } else {
        format!("{git_ref}:{subpath}")
    };
    if let Some(page) = params.page() {
        return Ok(Json(read_tree_page(&path, &spec, subpath, page).await?));
    }
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["ls-tree", "-l", "-z", "--", &spec])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::not_found_named(format!(
            "tree not found: {}",
            err.trim()
        )));
    }
    let entries: Vec<TreeEntry> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|record| parse_tree_entry(record, subpath))
        .collect();
    Ok(Json(entries))
}

const DEFAULT_TREE_LIMIT: usize = 100;
const MAX_TREE_LIMIT: usize = 200;

#[derive(Clone, Copy)]
struct TreePage<'a> {
    limit: usize,
    offset: usize,
    search: Option<&'a str>,
}

async fn read_tree_page(
    path: &std::path::Path,
    spec: &str,
    subpath: &str,
    page: TreePage<'_>,
) -> Result<Vec<TreeEntry>, ApiError> {
    let search = page.search.map(str::to_lowercase);
    let mut entries = Vec::with_capacity(page.limit);
    let mut seen = 0;

    let page_is_full = collect_tree_entries(
        path,
        spec,
        subpath,
        true,
        search.as_deref(),
        page,
        &mut seen,
        &mut entries,
    )
    .await?;
    if !page_is_full {
        collect_tree_entries(
            path,
            spec,
            subpath,
            false,
            search.as_deref(),
            page,
            &mut seen,
            &mut entries,
        )
        .await?;
    }
    Ok(entries)
}

#[allow(clippy::too_many_arguments)]
async fn collect_tree_entries(
    path: &std::path::Path,
    spec: &str,
    subpath: &str,
    directories_only: bool,
    search: Option<&str>,
    page: TreePage<'_>,
    seen: &mut usize,
    entries: &mut Vec<TreeEntry>,
) -> Result<bool, ApiError> {
    let mut command = tokio::process::Command::new("git");
    command
        .arg(format!("--git-dir={}", path.display()))
        .args(["ls-tree", "-l", "-z"]);
    if directories_only {
        command.arg("-d");
    }
    let mut child = command
        .args(["--", spec])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        ApiError::internal(sqlx::Error::Io(std::io::Error::other(
            "git ls-tree stdout is unavailable",
        )))
    })?;
    let mut reader = BufReader::new(stdout);
    let mut record = Vec::new();
    let mut stopped = false;

    loop {
        record.clear();
        let read = reader
            .read_until(0, &mut record)
            .await
            .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
        if read == 0 {
            break;
        }
        if record.last() == Some(&0) {
            record.pop();
        }
        let Some(entry) = parse_tree_entry(&record, subpath) else {
            continue;
        };
        if !directories_only && entry.kind == "tree" {
            continue;
        }
        if search.is_some_and(|needle| !entry.name.to_lowercase().contains(needle)) {
            continue;
        }
        if *seen < page.offset {
            *seen += 1;
            continue;
        }
        *seen += 1;
        entries.push(entry);
        if entries.len() >= page.limit {
            stopped = true;
            break;
        }
    }
    drop(reader);

    if stopped {
        let _ = child.start_kill();
    }
    let status = child
        .wait()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !stopped && !status.success() {
        return Err(ApiError::not_found_named("tree not found"));
    }
    Ok(stopped)
}

fn parse_tree_entry(record: &[u8], subpath: &str) -> Option<TreeEntry> {
    // "<mode> <type> <sha> <size>\t<path>\0"
    let separator = record.iter().position(|byte| *byte == b'\t')?;
    let meta = String::from_utf8_lossy(&record[..separator]);
    let name = String::from_utf8_lossy(&record[separator + 1..]).into_owned();
    let mut parts = meta.split_whitespace();
    let _mode = parts.next()?;
    let kind = parts.next()?;
    let sha = parts.next()?.to_string();
    let size = match parts.next()? {
        "-" => None,
        value => value.parse().ok(),
    };
    Some(TreeEntry {
        path: tree_entry_path(subpath, &name),
        name: name.rsplit('/').next().unwrap_or(&name).to_string(),
        kind: kind.to_string(),
        size,
        sha,
    })
}

fn tree_entry_path(subpath: &str, name: &str) -> String {
    if subpath.is_empty() {
        name.to_string()
    } else {
        format!("{subpath}/{name}")
    }
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct TreeQuery {
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    path: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    search: Option<String>,
}

impl TreeQuery {
    fn page(&self) -> Option<TreePage<'_>> {
        let search = self
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if self.limit.is_none() && self.offset.is_none() && search.is_none() {
            return None;
        }
        Some(TreePage {
            limit: self
                .limit
                .unwrap_or(DEFAULT_TREE_LIMIT)
                .clamp(1, MAX_TREE_LIMIT),
            offset: self.offset.unwrap_or(0),
            search,
        })
    }
}

#[utoipa::path(get, path = "/api/v1/repos/{repo}/blob", tag = "git", params(("repo" = String, Path), ("ref" = Option<String>, Query), ("path" = String, Query)), responses((status = 200, body = BlobContent)))]
pub async fn get_blob(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    axum::extract::Query(params): axum::extract::Query<BlobQuery>,
) -> Result<Json<BlobContent>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    let git_ref = params
        .git_ref
        .as_deref()
        .filter(|r| !r.is_empty())
        .unwrap_or("HEAD");
    let git_ref = resolve_view_ref(&path, git_ref).await;
    let spec = format!("{git_ref}:{}", params.path);
    let (bytes, size, truncated) = read_blob(&path, &spec).await?;
    let binary = bytes.iter().take(8000).any(|b| *b == 0);
    let content = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    // Resolve blob sha for the response.
    let sha_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["rev-parse", &spec])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();
    Ok(Json(BlobContent {
        path: params.path,
        sha,
        size,
        content,
        binary,
        truncated,
    }))
}

const MAX_BLOB_LEN: usize = 512 * 1024;

async fn read_blob(path: &std::path::Path, spec: &str) -> Result<(Vec<u8>, i64, bool), ApiError> {
    let size_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["cat-file", "-s", spec])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !size_output.status.success() {
        return Err(ApiError::not_found_named("blob not found"));
    }
    let size = String::from_utf8_lossy(&size_output.stdout)
        .trim()
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("git returned an invalid blob size"))?;
    if size < 0 {
        return Err(ApiError::bad_request("git returned an invalid blob size"));
    }

    let mut child = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["cat-file", "blob", spec])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        ApiError::internal(sqlx::Error::Io(std::io::Error::other(
            "git cat-file stdout is unavailable",
        )))
    })?;
    let mut bytes = Vec::with_capacity(MAX_BLOB_LEN + 1);
    let mut bounded = stdout.take((MAX_BLOB_LEN + 1) as u64);
    bounded
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    drop(bounded);

    let truncated = size > MAX_BLOB_LEN as i64 || bytes.len() > MAX_BLOB_LEN;
    if bytes.len() > MAX_BLOB_LEN {
        bytes.truncate(MAX_BLOB_LEN);
    }
    if truncated {
        let _ = child.start_kill();
    }
    let status = child
        .wait()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !truncated && !status.success() {
        return Err(ApiError::not_found_named("blob not found"));
    }
    Ok((bytes, size, truncated))
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct BlobQuery {
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    path: String,
}

#[utoipa::path(get, path = "/api/v1/repos/{repo}/tags", tag = "git", params(("repo" = String, Path)), responses((status = 200, body = [TagInfo])))]
pub async fn list_tags(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Vec<TagInfo>>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short) %(objectname) %(contents:subject)",
            "refs/tags",
        ])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !output.status.success() {
        return Err(ApiError::bad_request("git for-each-ref failed"));
    }
    let tags: Vec<TagInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            let name = parts.next()?.to_string();
            let sha = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").to_string();
            Some(TagInfo { name, sha, message })
        })
        .collect();
    Ok(Json(tags))
}

#[cfg(test)]
mod tests {
    use super::{
        CommitParams, MAX_BLOB_LEN, MAX_PATCH_LEN, MAX_TREE_LIMIT, PullRequestListParams,
        PullRequestStatusFilter, TreePage, TreeQuery, classify_ref, parse_diff_files,
        parse_tree_entry, read_blob, read_commits, read_patch, read_tree_page, tree_entry_path,
    };
    use uuid::Uuid;

    #[test]
    fn refs_keep_branch_and_tag_identity() {
        assert_eq!(
            classify_ref("refs/heads/release/v1"),
            ("release/v1", "branch")
        );
        assert_eq!(classify_ref("refs/tags/v1"), ("v1", "tag"));
        assert_eq!(
            classify_ref("refs/notes/review"),
            ("refs/notes/review", "other")
        );
    }

    #[test]
    fn tree_entries_keep_their_full_path() {
        assert_eq!(tree_entry_path("", "src"), "src");
        assert_eq!(
            tree_entry_path("src/pages", "index.tsx"),
            "src/pages/index.tsx"
        );
        let entry = parse_tree_entry(b"100644 blob abc123 12\tline\tbreak\n.txt", "src/pages")
            .expect("parse NUL-safe tree record");
        assert_eq!(entry.name, "line\tbreak\n.txt");
        assert_eq!(entry.path, "src/pages/line\tbreak\n.txt");
    }

    #[test]
    fn tree_page_parameters_are_optional_and_bounded() {
        let legacy = TreeQuery {
            git_ref: None,
            path: None,
            limit: None,
            offset: None,
            search: Some("  ".to_string()),
        };
        assert!(legacy.page().is_none());

        let paged = TreeQuery {
            git_ref: None,
            path: None,
            limit: Some(usize::MAX),
            offset: Some(25),
            search: Some("  FILE  ".to_string()),
        };
        let page = paged.page().expect("bounded page");
        assert_eq!(page.limit, MAX_TREE_LIMIT);
        assert_eq!(page.offset, 25);
        assert_eq!(page.search, Some("FILE"));
    }

    #[tokio::test]
    async fn tree_page_streams_directories_before_files_and_filters_names() {
        let directory = std::env::temp_dir().join(format!("forge-large-tree-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory)
            .await
            .expect("create repository directory");
        run_git(&directory, &["init", "--quiet"]).await;
        run_git(&directory, &["config", "user.name", "Forge Test"]).await;
        run_git(&directory, &["config", "user.email", "forge@example.test"]).await;

        for index in 1..=12 {
            let path = directory.join(format!("dir-{index:04}"));
            tokio::fs::create_dir_all(&path)
                .await
                .expect("create tree fixture directory");
            tokio::fs::write(path.join("README.txt"), b"directory\n")
                .await
                .expect("write tree fixture directory file");
        }
        for index in 1..=20 {
            tokio::fs::write(directory.join(format!("file-{index:04}.txt")), b"file\n")
                .await
                .expect("write tree fixture file");
        }
        run_git(&directory, &["add", "."]).await;
        run_git(&directory, &["commit", "--quiet", "-m", "Large tree"]).await;

        let git_dir = directory.join(".git");
        let page = read_tree_page(
            &git_dir,
            "HEAD",
            "",
            TreePage {
                limit: 5,
                offset: 10,
                search: None,
            },
        )
        .await
        .expect("read bounded tree page");
        assert_eq!(
            page.iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            [
                "dir-0011",
                "dir-0012",
                "file-0001.txt",
                "file-0002.txt",
                "file-0003.txt",
            ]
        );

        let matches = read_tree_page(
            &git_dir,
            "HEAD",
            "",
            TreePage {
                limit: 20,
                offset: 0,
                search: Some("FILE-001"),
            },
        )
        .await
        .expect("search bounded tree page");
        assert_eq!(matches.len(), 10);
        assert!(
            matches
                .iter()
                .all(|entry| entry.name.starts_with("file-001"))
        );

        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[test]
    fn parses_added_modified_and_deleted_files() {
        let numstat = [
            b"3\t0\tnew.txt\0".as_slice(),
            b"1\t2\tsrc/a\tb.txt\0",
            b"0\t4\told.txt\0",
        ]
        .concat();
        let names = b"A\0new.txt\0M\0src/a\tb.txt\0D\0old.txt\0";
        let files = parse_diff_files(&numstat, names).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, "added");
        assert_eq!(files[0].additions, 3);
        assert_eq!(files[1].path, "src/a\tb.txt");
        assert_eq!(files[1].status, "modified");
        assert_eq!(files[1].deletions, 2);
        assert_eq!(files[2].status, "deleted");
        assert_eq!(files[2].deletions, 4);
    }

    #[test]
    fn retains_binary_files_without_inventing_line_counts() {
        let files = parse_diff_files(b"-\t-\timage.png\0", b"A\0image.png\0").unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].binary);
        assert_eq!(files[0].additions, 0);
        assert_eq!(files[0].deletions, 0);
    }

    #[test]
    fn rejects_mismatched_git_outputs() {
        assert!(parse_diff_files(b"1\t0\tnew.txt\0", b"A\0other.txt\0").is_err());
        assert!(parse_diff_files(b"1\t0\tnew.txt\0", b"A\0new.txt\0D\0old.txt\0").is_err());
    }
    async fn run_git(directory: &std::path::Path, args: &[&str]) {
        let status = tokio::process::Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .status()
            .await
            .expect("run git command");
        assert!(status.success(), "git command failed: {args:?}");
    }

    #[tokio::test]
    async fn blob_read_stops_after_the_response_limit() {
        let directory = std::env::temp_dir().join(format!("forge-large-blob-{}", Uuid::new_v4()));

        tokio::fs::create_dir_all(&directory)
            .await
            .expect("create repository directory");

        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "Forge Test"],
            vec!["config", "user.email", "forge@example.test"],
        ] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(args)
                .status()
                .await
                .expect("run git setup command");
            assert!(status.success());
        }

        let full_size = MAX_BLOB_LEN + 4096;
        tokio::fs::write(directory.join("large.txt"), vec![b'x'; full_size])
            .await
            .expect("write large fixture");
        for args in [
            vec!["add", "large.txt"],
            vec!["commit", "--quiet", "-m", "Large blob"],
        ] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(args)
                .status()
                .await
                .expect("create blob commit");
            assert!(status.success());
        }

        let (bytes, size, truncated) = read_blob(&directory.join(".git"), "HEAD:large.txt")
            .await
            .expect("read bounded blob");
        assert!(truncated);
        assert_eq!(size, full_size as i64);
        assert_eq!(bytes.len(), MAX_BLOB_LEN);

        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn compare_patch_stops_after_the_response_limit() {
        let directory = std::env::temp_dir().join(format!("forge-large-patch-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory)
            .await
            .expect("create repository directory");

        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "Forge Test"],
            vec!["config", "user.email", "forge@example.test"],
        ] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(args)
                .status()
                .await
                .expect("run git setup command");
            assert!(status.success());
        }

        tokio::fs::write(directory.join("large.txt"), "base\n")
            .await
            .expect("write base fixture");
        for args in [
            vec!["add", "large.txt"],
            vec!["commit", "--quiet", "-m", "Base"],
        ] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(args)
                .status()
                .await
                .expect("create base commit");
            assert!(status.success());
        }

        tokio::fs::write(
            directory.join("large.txt"),
            vec![b'x'; MAX_PATCH_LEN + 4096],
        )
        .await
        .expect("write large fixture");
        for args in [
            vec!["add", "large.txt"],
            vec!["commit", "--quiet", "-m", "Large change"],
        ] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(args)
                .status()
                .await
                .expect("create large commit");
            assert!(status.success());
        }

        let (patch, truncated) = read_patch(&directory.join(".git"), "HEAD~1", "HEAD")
            .await
            .expect("read bounded patch");
        assert!(truncated);
        assert_eq!(patch.len(), MAX_PATCH_LEN);

        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[test]
    fn commit_pages_keep_the_requested_offset_and_bound_the_limit() {
        assert_eq!(
            CommitParams {
                branch: Some("main".to_string()),
                limit: Some(51),
                offset: Some(100),
            }
            .page(),
            (51, 100)
        );
        assert_eq!(
            CommitParams {
                branch: None,
                limit: Some(500),
                offset: None,
            }
            .page(),
            (200, 0)
        );
        assert_eq!(
            CommitParams {
                branch: None,
                limit: Some(0),
                offset: Some(50),
            }
            .page(),
            (1, 50)
        );
    }

    #[tokio::test]
    async fn commit_pages_read_history_after_the_first_window() {
        let directory = std::env::temp_dir().join(format!("forge-commit-page-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory)
            .await
            .expect("create repository directory");

        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "Forge Test"],
            vec!["config", "user.email", "forge@example.test"],
        ] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(args)
                .status()
                .await
                .expect("run git setup command");
            assert!(status.success());
        }

        for number in 1..=3 {
            tokio::fs::write(directory.join("history.txt"), number.to_string())
                .await
                .expect("write commit fixture");
            let add = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(["add", "history.txt"])
                .status()
                .await
                .expect("stage commit fixture");
            assert!(add.success());
            let commit = tokio::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(["commit", "--quiet", "-m", &format!("Commit {number}")])
                .status()
                .await
                .expect("create commit fixture");
            assert!(commit.success());
        }

        let commits = read_commits(&directory.join(".git"), "HEAD", 2, 1)
            .await
            .expect("read second commit page");
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].message, "Commit 2");
        assert_eq!(commits[1].message, "Commit 1");

        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[test]
    fn pull_request_list_params_are_normalized() {
        let params = PullRequestListParams {
            limit: Some(50),
            offset: Some(100),
            status: Some(PullRequestStatusFilter::Open),
            search: Some("  release branch  ".to_string()),
        }
        .normalize()
        .unwrap();

        assert_eq!(params.limit, 50);
        assert_eq!(params.offset, 100);
        assert_eq!(params.status.as_deref(), Some("open"));
        assert_eq!(params.search.as_deref(), Some("release branch"));
    }

    #[test]
    fn pull_request_list_params_reject_invalid_bounds_and_search() {
        assert!(
            PullRequestListParams {
                limit: Some(0),
                ..Default::default()
            }
            .normalize()
            .is_err()
        );
        assert!(
            PullRequestListParams {
                search: Some("x".repeat(201)),
                ..Default::default()
            }
            .normalize()
            .is_err()
        );
    }
}
