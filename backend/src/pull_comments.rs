//! PR comments retain the exact diff reviewed; retries never reinterpret an old anchor.
use std::{path::Path, process::Stdio, sync::Arc};

use axum::{
    Extension, Json,
    body::Body,
    extract::{Path as RoutePath, Query, State},
    response::Response,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, types::Json as SqlJson};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::{
    api::{ApiError, AppState},
    auth::AccessClaims,
    space_admission::{RepositoryWrite, require_write},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DiffSide {
    Old,
    New,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentAnchor {
    pub path: String,
    pub side: DiffSide,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub enum CommentSeverity {
    #[serde(rename = "note")]
    Note,
    P0,
    P1,
    P2,
    P3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePullComment {
    pub operation_id: Uuid,
    pub base_sha: String,
    pub head_sha: String,
    pub body: String,
    pub severity: CommentSeverity,
    pub anchor: Option<CommentAnchor>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PullReviewContext {
    pub pull_request_id: Uuid,
    pub base_sha: String,
    pub head_sha: String,
    pub merge_base: String,
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct PullComment {
    pub id: Uuid,
    pub pull_request_id: Uuid,
    pub author_id: Uuid,
    #[sqlx(default)]
    pub author: Option<String>,
    pub operation_id: Uuid,
    pub base_sha: String,
    pub head_sha: String,
    pub body: String,
    pub severity: String,
    #[schema(value_type=Option<CommentAnchor>)]
    pub anchor: Option<SqlJson<CommentAnchor>>,
    pub created_at: DateTime<Utc>,
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn validate(input: &CreatePullComment) -> Result<(), ApiError> {
    if input.operation_id.is_nil()
        || !valid_sha(&input.base_sha)
        || !valid_sha(&input.head_sha)
        || input.body.trim().is_empty()
    {
        return Err(ApiError::bad_request("invalid_pull_comment"));
    }
    if input
        .anchor
        .as_ref()
        .is_some_and(|a| a.line == 0 || a.path.is_empty() || a.path.chars().any(char::is_control))
    {
        return Err(ApiError::bad_request("invalid_comment_anchor"));
    }
    Ok(())
}

async fn git(path: &Path, args: &[&str]) -> Result<String, ApiError> {
    let output = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", path.display()))
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
    if !output.status.success() {
        return Err(ApiError::conflict("pull_revision_unavailable"));
    }
    String::from_utf8(output.stdout).map_err(|_| ApiError::conflict("invalid_git_output"))
}

pub(crate) async fn resolve_commit(path: &Path, reference: &str) -> Result<String, ApiError> {
    let value = git(
        path,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )
    .await?;
    let sha = value.trim();
    if !valid_sha(sha) {
        return Err(ApiError::conflict("pull_revision_unavailable"));
    }
    Ok(sha.to_owned())
}

async fn context(
    state: &AppState,
    storage: &str,
    number: i32,
) -> Result<PullReviewContext, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let (id,source,target,status,merge): (Uuid,String,String,String,Option<String>)=
        sqlx::query_as("SELECT id,source_branch,target_branch,status,merge_commit_sha FROM pull_requests WHERE repository_name=$1 AND number=$2")
        .bind(storage).bind(number).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(ApiError::not_found)?;
    let path = state.git.root.join(format!("{storage}.git"));
    let (base_sha, head_sha) = if status == "merged" {
        let merge = merge
            .filter(|s| valid_sha(s))
            .ok_or_else(|| ApiError::conflict("pull_revision_unavailable"))?;
        (
            resolve_commit(&path, &format!("{merge}^1")).await?,
            resolve_commit(&path, &format!("{merge}^2")).await?,
        )
    } else {
        (
            resolve_commit(&path, &format!("refs/heads/{target}")).await?,
            resolve_commit(&path, &format!("refs/heads/{source}")).await?,
        )
    };
    let merge_base = git(&path, &["merge-base", &base_sha, &head_sha])
        .await?
        .trim()
        .to_owned();
    Ok(PullReviewContext {
        pull_request_id: id,
        base_sha,
        head_sha,
        merge_base,
    })
}

#[utoipa::path(get,operation_id="forge_pull_review_context",path="/api/v1/catalog/repositories/{id}/pulls/{number}/review-context",tag="pulls",
 params(("id"=Uuid,Path),("number"=i32,Path)),responses((status=200,body=PullReviewContext),(status=409)))]
pub async fn review_context(
    State(state): State<Arc<AppState>>,
    RoutePath((id, number)): RoutePath<(Uuid, i32)>,
) -> Result<Json<PullReviewContext>, ApiError> {
    let storage = crate::repository_catalog::storage_by_id(
        state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
        id,
    )
    .await?;
    Ok(Json(context(&state, &storage, number).await?))
}

fn severity(value: &CommentSeverity) -> &'static str {
    match value {
        CommentSeverity::Note => "note",
        CommentSeverity::P0 => "P0",
        CommentSeverity::P1 => "P1",
        CommentSeverity::P2 => "P2",
        CommentSeverity::P3 => "P3",
    }
}

fn same_input(saved: &PullComment, input: &CreatePullComment) -> bool {
    saved.base_sha == input.base_sha
        && saved.head_sha == input.head_sha
        && saved.body == input.body
        && saved.severity == severity(&input.severity)
        && saved.anchor.as_ref().map(|a| &a.0) == input.anchor.as_ref()
}

/// Hunk syntax is Git's format, not a semantic judgment about the finding.
fn contains_anchor(patch: &str, anchor: &CommentAnchor) -> bool {
    let (mut old, mut new) = (0u32, 0u32);
    let mut hunk = false;
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            hunk = false;
            continue;
        }
        if line.starts_with("@@ ") {
            let mut fields = line.split_whitespace().skip(1);
            let start = |value: Option<&str>, prefix: char| {
                value?
                    .strip_prefix(prefix)?
                    .split(',')
                    .next()?
                    .parse::<u32>()
                    .ok()
            };
            let positions = (start(fields.next(), '-'), start(fields.next(), '+'));
            if let (Some(a), Some(b)) = positions {
                old = a;
                new = b;
                hunk = true;
            } else {
                return false;
            }
            continue;
        }
        if !hunk {
            continue;
        }
        let present = match line.as_bytes().first() {
            Some(b' ') => (true, true),
            Some(b'-') => (true, false),
            Some(b'+') => (false, true),
            _ => continue,
        };
        if (anchor.side == DiffSide::Old && present.0 && old == anchor.line)
            || (anchor.side == DiffSide::New && present.1 && new == anchor.line)
        {
            return true;
        }
        if present.0 {
            old = old.saturating_add(1);
        }
        if present.1 {
            new = new.saturating_add(1);
        }
    }
    false
}

#[utoipa::path(post,operation_id="forge_pull_comment_create",path="/api/v1/catalog/repositories/{id}/pulls/{number}/comments",tag="pulls",
 params(("id"=Uuid,Path),("number"=i32,Path)),request_body=CreatePullComment,responses((status=200,body=PullComment),(status=409)))]
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    RoutePath((id, number)): RoutePath<(Uuid, i32)>,
    Extension(claims): Extension<AccessClaims>,
    permit: Option<Extension<RepositoryWrite>>,
    identity: Option<Extension<crate::api::authz_mw::VerifiedRequestIdentity>>,
    Json(input): Json<CreatePullComment>,
) -> Result<Json<PullComment>, ApiError> {
    validate(&input)?;
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let storage = crate::repository_catalog::storage_by_id(pool, id).await?;
    let _lease = crate::repository_catalog::write_lease(&state, &storage).await?;
    let native = identity
        .as_ref()
        .filter(|i| crate::development_workspaces::machine::principal(&i.0));
    if claims.role == "service_account" && native.is_none() {
        return Err(ApiError::forbidden());
    }
    let write = if native.is_some() {
        None
    } else {
        RepositoryWrite::for_repository(pool, &storage, permit.map(|p| p.0)).await?
    };
    let _git_guard = cicd_infra::sync_git::MutationGuard::acquire(&state.git.root, id)
        .await
        .map_err(crate::repository_sync::api_error)?;
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let native_clock = if let Some(identity) = native {
        Some(
            crate::development_workspaces::machine::lock_repository(&mut tx, &storage, &identity.0)
                .await?,
        )
    } else {
        None
    };
    if let Some(proof) = &write {
        proof.lock(&mut tx).await?;
    }
    let pull: Uuid = sqlx::query_scalar(
        "SELECT id FROM pull_requests WHERE repository_id=$1 AND number=$2 FOR SHARE",
    )
    .bind(id)
    .bind(number)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;
    let saved = sqlx::query_as::<_, PullComment>(
        "SELECT id,pull_request_id,author_id,operation_id,base_sha,head_sha,body,severity,anchor,created_at FROM pull_comments WHERE pull_request_id=$1 AND author_id=$2 AND operation_id=$3",
    )
    .bind(pull)
    .bind(claims.sub)
    .bind(input.operation_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    require_write(write.as_ref())?;
    if let Some(clock) = native_clock {
        clock.require()?;
    }
    if let Some(saved) = saved {
        return if same_input(&saved, &input) {
            Ok(Json(saved))
        } else {
            Err(ApiError::conflict("comment_operation_conflict"))
        };
    }
    let current = context(&state, &storage, number).await?;
    if current.base_sha != input.base_sha || current.head_sha != input.head_sha {
        return Err(ApiError::conflict("pull_review_revision_changed"));
    }
    if let Some(anchor) = &input.anchor {
        let revision = match anchor.side {
            DiffSide::Old => &current.merge_base,
            DiffSide::New => &current.head_sha,
        };
        let pathspec = format!(":(literal){}", anchor.path);
        let tree = git(
            &state.git.root.join(format!("{storage}.git")),
            &["ls-tree", "-z", revision, "--", &pathspec],
        )
        .await?;
        let exact_file = tree.split('\0').any(|entry| {
            entry.split_once('\t').is_some_and(|(metadata, path)| {
                path == anchor.path && metadata.split_whitespace().nth(1) == Some("blob")
            })
        });
        if !exact_file {
            return Err(ApiError::bad_request("comment_anchor_not_in_diff"));
        }
        let patch = git(
            &state.git.root.join(format!("{storage}.git")),
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                &current.merge_base,
                &current.head_sha,
                "--",
                &format!(":(literal){}", anchor.path),
            ],
        )
        .await?;
        if !contains_anchor(&patch, anchor) {
            return Err(ApiError::bad_request("comment_anchor_not_in_diff"));
        }
    }
    let saved=sqlx::query_as::<_,PullComment>("INSERT INTO pull_comments(id,pull_request_id,author_id,operation_id,base_sha,head_sha,severity,body,anchor) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id,pull_request_id,author_id,operation_id,base_sha,head_sha,body,severity,anchor,created_at")
        .bind(Uuid::now_v7()).bind(pull).bind(claims.sub).bind(input.operation_id)
        .bind(&input.base_sha).bind(&input.head_sha).bind(severity(&input.severity)).bind(&input.body)
        .bind(input.anchor.as_ref().map(SqlJson)).fetch_one(&mut *tx).await.map_err(ApiError::internal)?;
    require_write(write.as_ref())?;
    if let Some(clock) = native_clock {
        clock.require()?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(saved))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct CommentQuery {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub operation_id: Option<Uuid>,
}

#[utoipa::path(get,operation_id="forge_pull_comment_list",path="/api/v1/catalog/repositories/{id}/pulls/{number}/comments",tag="pulls",
 params(("id"=Uuid,Path),("number"=i32,Path),CommentQuery),responses((status=200,body=Vec<PullComment>)))]
pub async fn list(
    State(state): State<Arc<AppState>>,
    RoutePath((id, number)): RoutePath<(Uuid, i32)>,
    Extension(claims): Extension<AccessClaims>,
    Query(query): Query<CommentQuery>,
) -> Result<Json<Vec<PullComment>>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let comments=sqlx::query_as::<_,PullComment>("SELECT c.id,c.pull_request_id,c.author_id,c.operation_id,c.base_sha,c.head_sha,c.body,c.severity,c.anchor,c.created_at,u.username AS author FROM pull_comments c JOIN pull_requests p ON p.id=c.pull_request_id JOIN users u ON u.id=c.author_id WHERE p.repository_id=$1 AND p.number=$2 AND ($3::uuid IS NULL OR (c.author_id=$4 AND c.operation_id=$3)) ORDER BY c.id LIMIT $5 OFFSET $6")
        .bind(id).bind(number).bind(query.operation_id).bind(claims.sub).bind(query.limit.unwrap_or(50).clamp(1,100))
        .bind(query.offset.unwrap_or(0).max(0)).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(comments))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct PatchQuery {
    pub base_sha: String,
    pub head_sha: String,
    pub path: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct CheckQuery {
    pub head_sha: String,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[utoipa::path(get,operation_id="forge_pull_checks",path="/api/v1/catalog/repositories/{id}/pulls/{number}/checks",tag="pulls",
 params(("id"=Uuid,Path),("number"=i32,Path),CheckQuery),
 responses((status=200,body=Vec<pdlc_shared::resource_context::RepositoryCheckRef>)))]
pub async fn checks(
    State(state): State<Arc<AppState>>,
    RoutePath((id, number)): RoutePath<(Uuid, i32)>,
    Query(query): Query<CheckQuery>,
) -> Result<Json<Vec<pdlc_shared::resource_context::RepositoryCheckRef>>, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let storage = crate::repository_catalog::storage_by_id(pool, id).await?;
    let current = context(&state, &storage, number).await?;
    if current.head_sha != query.head_sha {
        return Err(ApiError::conflict("pull_review_revision_changed"));
    }
    let rows:Vec<(Uuid,String,String)>=sqlx::query_as("SELECT p.id,p.commit_sha,p.status FROM pipelines p JOIN projects fp ON fp.id=p.project_id WHERE fp.repository_id=$1 AND p.commit_sha=$2 ORDER BY p.created_at,p.id LIMIT $3 OFFSET $4")
        .bind(id).bind(query.head_sha).bind(query.limit.unwrap_or(50).clamp(1,100)).bind(query.offset.unwrap_or(0).max(0))
        .fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(
        rows.into_iter()
            .map(|(pipeline_id, commit_sha, status)| {
                pdlc_shared::resource_context::RepositoryCheckRef {
                    pipeline_id,
                    commit_sha,
                    status,
                }
            })
            .collect(),
    ))
}

#[utoipa::path(get,operation_id="forge_pull_patch",path="/api/v1/catalog/repositories/{id}/pulls/{number}/patch",tag="pulls",
 params(("id"=Uuid,Path),("number"=i32,Path),PatchQuery),responses((status=200,body=String,content_type="text/plain")))]
pub async fn patch(
    State(state): State<Arc<AppState>>,
    RoutePath((id, number)): RoutePath<(Uuid, i32)>,
    Query(query): Query<PatchQuery>,
) -> Result<Response, ApiError> {
    let storage = crate::repository_catalog::storage_by_id(
        state.pool.as_ref().ok_or_else(ApiError::unavailable)?,
        id,
    )
    .await?;
    let current = context(&state, &storage, number).await?;
    if current.base_sha != query.base_sha || current.head_sha != query.head_sha {
        return Err(ApiError::conflict("pull_review_revision_changed"));
    }
    let mut command = tokio::process::Command::new("git");
    command
        .arg(format!(
            "--git-dir={}",
            state.git.root.join(format!("{storage}.git")).display()
        ))
        .args([
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            &current.merge_base,
            &current.head_sha,
            "--",
        ]);
    if let Some(path) = query.path {
        command.arg(format!(":(literal){path}"));
    }
    let mut child = command
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ApiError::conflict("git_output_unavailable"))?;
    let stream =
        futures_util::stream::try_unfold((child, stdout), |(mut child, mut stdout)| async move {
            let mut bytes = vec![0u8; 65536];
            let count = stdout.read(&mut bytes).await?;
            if count == 0 {
                if !child.wait().await?.success() {
                    return Err(std::io::Error::other("git diff failed"));
                }
                return Ok(None);
            }
            bytes.truncate(count);
            Ok(Some((bytes, (child, stdout))))
        });
    Response::builder()
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Cache-Control", "private, no-store")
        .body(Body::from_stream(stream))
        .map_err(|_| ApiError::conflict("invalid_patch_response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchors_preserve_both_sides_and_reject_outside_lines() {
        let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -9,2 +9,2 @@\n same\n-old\n+new\n";
        for side in [DiffSide::Old, DiffSide::New] {
            assert!(contains_anchor(
                patch,
                &CommentAnchor {
                    path: "a".into(),
                    side: side.clone(),
                    line: 10
                }
            ));
            assert!(!contains_anchor(
                patch,
                &CommentAnchor {
                    path: "a".into(),
                    side,
                    line: 11
                }
            ));
        }
        assert!(!valid_sha("HEAD"));
        assert!(!valid_sha(&"A".repeat(40)));
    }
}
