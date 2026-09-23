use std::{collections::HashMap, path::PathBuf, process::Stdio};

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
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

const DEFAULT_REF_LIMIT: usize = 100;
const MAX_REF_LIMIT: usize = 200;

#[derive(Clone, Copy)]
struct GitListPage<'a> {
    limit: usize,
    offset: usize,
    search: Option<&'a str>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct RefsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    search: Option<String>,
    kind: Option<String>,
}

impl RefsQuery {
    fn page(&self) -> Result<Option<(GitListPage<'_>, Option<&str>)>, ApiError> {
        let search = normalized_search(self.search.as_deref());
        let kind = normalized_search(self.kind.as_deref());
        if self.limit.is_none() && self.offset.is_none() && search.is_none() && kind.is_none() {
            return Ok(None);
        }
        if kind.is_some_and(|value| !matches!(value, "branch" | "tag" | "other")) {
            return Err(ApiError::bad_request(
                "ref kind must be branch, tag, or other",
            ));
        }
        Ok(Some((
            GitListPage {
                limit: self
                    .limit
                    .unwrap_or(DEFAULT_REF_LIMIT)
                    .clamp(1, MAX_REF_LIMIT),
                offset: self.offset.unwrap_or(0),
                search,
            },
            kind,
        )))
    }
}

fn normalized_search(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo}/refs",
    tag = "repos",
    params(("repo" = String, Path, description = "Repository name"), ("limit" = Option<usize>, Query), ("offset" = Option<usize>, Query), ("search" = Option<String>, Query), ("kind" = Option<String>, Query)),
    responses((status = 200, body = [RefInfo]), (status = 404)),
)]
pub async fn list_refs(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    Query(params): Query<RefsQuery>,
) -> Result<Json<Vec<RefInfo>>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    if let Some((page, kind)) = params.page()? {
        return Ok(Json(read_refs_page(&path, page, kind).await?));
    }
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
        .filter_map(parse_ref_line)
        .collect();
    Ok(Json(refs))
}

async fn read_refs_page(
    path: &std::path::Path,
    page: GitListPage<'_>,
    kind: Option<&str>,
) -> Result<Vec<RefInfo>, ApiError> {
    let pattern = match kind {
        Some("branch") => Some("refs/heads"),
        Some("tag") => Some("refs/tags"),
        _ => None,
    };
    let mut arguments = vec!["--format=%(refname) %(objectname) %(contents:subject)"];
    if let Some(pattern) = pattern {
        arguments.push(pattern);
    }
    stream_for_each_ref(path, &arguments, page, |line| {
        let entry = parse_ref_line(line)?;
        if kind.is_some_and(|expected| entry.kind != expected) {
            return None;
        }
        Some((entry.name.clone(), entry))
    })
    .await
}

fn parse_ref_line(line: &str) -> Option<RefInfo> {
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
}

async fn stream_for_each_ref<T, F>(
    path: &std::path::Path,
    arguments: &[&str],
    page: GitListPage<'_>,
    mut parse: F,
) -> Result<Vec<T>, ApiError>
where
    F: FnMut(&str) -> Option<(String, T)>,
{
    let search = page.search.map(str::to_lowercase);
    let mut child = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .arg("for-each-ref")
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        ApiError::internal(sqlx::Error::Io(std::io::Error::other(
            "git for-each-ref stdout is unavailable",
        )))
    })?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut seen = 0;
    let mut items = Vec::with_capacity(page.limit);
    let mut page_is_full = false;

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
        if read == 0 {
            break;
        }
        let Some((name, item)) = parse(line.trim_end_matches(['\r', '\n'])) else {
            continue;
        };
        if search
            .as_deref()
            .is_some_and(|needle| !name.to_lowercase().contains(needle))
        {
            continue;
        }
        if seen < page.offset {
            seen += 1;
            continue;
        }
        seen += 1;
        items.push(item);
        if items.len() >= page.limit {
            page_is_full = true;
            break;
        }
    }
    drop(reader);

    if page_is_full {
        let _ = child.start_kill();
    }
    let status = child
        .wait()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !page_is_full && !status.success() {
        return Err(ApiError::bad_request("git for-each-ref failed"));
    }
    Ok(items)
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
    let ref_spec = params.branch.unwrap_or_else(|| "HEAD".into());
    let ref_spec = resolve_view_ref(&path, &ref_spec).await;
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

    // patch
    let patch_output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["diff", "--no-renames", &merge_base, to])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !patch_output.status.success() {
        return Err(ApiError::bad_request("git diff --patch failed"));
    }
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

#[utoipa::path(get, path = "/api/v1/repos/{repo}/tree", tag = "git", params(("repo" = String, Path), ("ref" = Option<String>, Query), ("path" = Option<String>, Query)), responses((status = 200, body = [TreeEntry])))]
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
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["ls-tree", "-l", "--", &spec])
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
    let entries: Vec<TreeEntry> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            // "<mode> <type> <sha> <size>\t<path>"
            let (meta, name) = line.split_once('\t')?;
            let mut parts = meta.split_whitespace();
            let _mode = parts.next()?;
            let kind = parts.next()?;
            let sha = parts.next()?.to_string();
            let size = match parts.next()? {
                "-" => None,
                s => s.parse().ok(),
            };
            let name = name.to_string();
            Some(TreeEntry {
                path: tree_entry_path(subpath, &name),
                name: name.rsplit('/').next().unwrap_or(&name).to_string(),
                kind: kind.to_string(),
                size,
                sha,
            })
        })
        .collect();
    Ok(Json(entries))
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
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(["show", &spec])
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !output.status.success() {
        return Err(ApiError::not_found_named("blob not found"));
    }
    let bytes = output.stdout;
    let binary = bytes.iter().take(8000).any(|b| *b == 0);
    const MAX_LEN: usize = 512 * 1024;
    let truncated = bytes.len() > MAX_LEN;
    let content = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_LEN)]).into_owned()
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
        size: bytes.len() as i64,
        content,
        binary,
        truncated,
    }))
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct BlobQuery {
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    path: String,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct TagsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    search: Option<String>,
}

impl TagsQuery {
    fn page(&self) -> Option<GitListPage<'_>> {
        let search = normalized_search(self.search.as_deref());
        if self.limit.is_none() && self.offset.is_none() && search.is_none() {
            return None;
        }
        Some(GitListPage {
            limit: self
                .limit
                .unwrap_or(DEFAULT_REF_LIMIT)
                .clamp(1, MAX_REF_LIMIT),
            offset: self.offset.unwrap_or(0),
            search,
        })
    }
}

#[utoipa::path(get, path = "/api/v1/repos/{repo}/tags", tag = "git", params(("repo" = String, Path), ("limit" = Option<usize>, Query), ("offset" = Option<usize>, Query), ("search" = Option<String>, Query)), responses((status = 200, body = [TagInfo])))]
pub async fn list_tags(
    State(state): State<std::sync::Arc<AppState>>,
    AxumPath(repo): AxumPath<String>,
    Query(params): Query<TagsQuery>,
) -> Result<Json<Vec<TagInfo>>, ApiError> {
    let path = resolve_repo_path(&state, &repo).await?;
    if let Some(page) = params.page() {
        return Ok(Json(read_tags_page(&path, page).await?));
    }
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
        .filter_map(parse_tag_line)
        .collect();
    Ok(Json(tags))
}

async fn read_tags_page(
    path: &std::path::Path,
    page: GitListPage<'_>,
) -> Result<Vec<TagInfo>, ApiError> {
    stream_for_each_ref(
        path,
        &[
            "--sort=refname",
            "--sort=-creatordate",
            "--format=%(refname:short) %(objectname) %(contents:subject)",
            "refs/tags",
        ],
        page,
        |line| {
            let tag = parse_tag_line(line)?;
            Some((tag.name.clone(), tag))
        },
    )
    .await
}

fn parse_tag_line(line: &str) -> Option<TagInfo> {
    let mut parts = line.splitn(3, ' ');
    let name = parts.next()?.to_string();
    let sha = parts.next()?.to_string();
    let message = parts.next().unwrap_or("").to_string();
    Some(TagInfo { name, sha, message })
}

#[cfg(test)]
mod tests {
    use super::{
        GitListPage, MAX_REF_LIMIT, RefsQuery, TagsQuery, classify_ref, parse_diff_files,
        read_refs_page, read_tags_page, tree_entry_path,
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
    fn ref_page_parameters_are_optional_bounded_and_validated() {
        let legacy = RefsQuery {
            limit: None,
            offset: None,
            search: Some("  ".to_string()),
            kind: None,
        };
        assert!(legacy.page().expect("legacy params").is_none());

        let paged = RefsQuery {
            limit: Some(usize::MAX),
            offset: Some(25),
            search: Some("  release  ".to_string()),
            kind: Some("branch".to_string()),
        };
        let (page, kind) = paged.page().expect("valid params").expect("bounded page");
        assert_eq!(page.limit, MAX_REF_LIMIT);
        assert_eq!(page.offset, 25);
        assert_eq!(page.search, Some("release"));
        assert_eq!(kind, Some("branch"));

        let invalid = RefsQuery {
            limit: None,
            offset: None,
            search: None,
            kind: Some("commit".to_string()),
        };
        assert!(invalid.page().is_err());

        let tags = TagsQuery {
            limit: Some(0),
            offset: None,
            search: None,
        };
        assert_eq!(tags.page().expect("tag page").limit, 1);
    }

    #[tokio::test]
    async fn ref_pages_stream_kind_search_and_offset() {
        let directory = std::env::temp_dir().join(format!("forge-many-refs-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory)
            .await
            .expect("create repository directory");
        run_git(&directory, &["init", "--quiet", "--initial-branch=primary"]).await;
        run_git(&directory, &["config", "user.name", "Forge Test"]).await;
        run_git(&directory, &["config", "user.email", "forge@example.test"]).await;
        tokio::fs::write(directory.join("README.md"), b"fixture\n")
            .await
            .expect("write repository fixture");
        run_git(&directory, &["add", "."]).await;
        run_git(&directory, &["commit", "--quiet", "-m", "Fixture"]).await;

        for index in 1..=12 {
            run_git(
                &directory,
                &[
                    "update-ref",
                    &format!("refs/heads/feature/ref-{index:04}"),
                    "HEAD",
                ],
            )
            .await;
        }
        for index in 1..=5 {
            run_git(
                &directory,
                &["update-ref", &format!("refs/tags/v1.0.{index:04}"), "HEAD"],
            )
            .await;
        }
        run_git(&directory, &["update-ref", "refs/notes/review", "HEAD"]).await;

        let git_dir = directory.join(".git");
        let branches = read_refs_page(
            &git_dir,
            GitListPage {
                limit: 5,
                offset: 10,
                search: None,
            },
            Some("branch"),
        )
        .await
        .expect("read branch page");
        assert_eq!(
            branches
                .iter()
                .map(|reference| reference.name.as_str())
                .collect::<Vec<_>>(),
            ["feature/ref-0011", "feature/ref-0012", "primary"]
        );
        assert!(branches.iter().all(|reference| reference.kind == "branch"));

        let matches = read_refs_page(
            &git_dir,
            GitListPage {
                limit: 10,
                offset: 0,
                search: Some("REF-001"),
            },
            Some("branch"),
        )
        .await
        .expect("search branch page");
        assert_eq!(matches.len(), 3);

        let other = read_refs_page(
            &git_dir,
            GitListPage {
                limit: 10,
                offset: 0,
                search: None,
            },
            Some("other"),
        )
        .await
        .expect("read other refs");
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].name, "refs/notes/review");

        let tags = read_tags_page(
            &git_dir,
            GitListPage {
                limit: 3,
                offset: 1,
                search: Some("V1.0"),
            },
        )
        .await
        .expect("read tag page");
        assert_eq!(tags.len(), 3);
        assert!(tags.iter().all(|tag| tag.name.starts_with("v1.0.")));

        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[test]
    fn tree_entries_keep_their_full_path() {
        assert_eq!(tree_entry_path("", "src"), "src");
        assert_eq!(
            tree_entry_path("src/pages", "index.tsx"),
            "src/pages/index.tsx"
        );
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
}
