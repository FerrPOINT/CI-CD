//! Disjoint own-task Git child. No issuer, persistent credential or pipeline trigger.
use crate::{
    api::{ApiError, AppState},
    config::SdlcWorkspaceConfig,
    delegated_credentials::digest,
    domain::sdlc_workspace::{TaskWorkspaceIntent, WorkspaceRole},
};
use axum::http::{HeaderMap, header};
use chrono::{DateTime, Utc};
use pdlc_auth_core::AuthContext;
use pdlc_shared::resource_context::{NamespaceRef, OwnerCommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::time::Instant;
use uuid::Uuid;

pub(crate) mod hooks;
pub const HEADER: &str = "x-forge-task-git-context";
pub const GRANT_PREFIX: &str = "ci-cd:sdlc:git:v1:";

// Tracker165 wire, not a Forge-authored source selection.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub contract_version: u8,
    pub tracker_instance_id: String,
    pub project_id: Uuid,
    pub task_id: Uuid,
    pub root_task_id: Uuid,
    pub confirmation_id: Uuid,
    pub requirement_revision: i64,
    pub content_hash: String,
    pub routing_snapshot_id: Uuid,
    pub namespace: NamespaceRef,
    pub forge_instance_id: Uuid,
    pub repository_id: Uuid,
    pub base_ref: String,
    pub base_commit: String,
    pub candidate_ref: String,
    pub source_revision: i64,
    pub policy_hash: String,
    pub source_hash: String,
    pub owner_subject: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitContext {
    pub version: u32,
    pub project_id: Uuid,
    pub repository: String,
    pub source: SourceBinding,
    pub intent: TaskWorkspaceIntent,
    pub run_id: Uuid,
    pub chat_id: Uuid,
    pub machine_subject: String,
    pub old_oid: String,
    pub new_oid: String,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl GitContext {
    pub fn validate(&self) -> Result<(), ApiError> {
        self.intent.validate().map_err(ApiError::bad_request)?;
        let s = &self.source;
        let b = &self.intent.binding;
        let mut sealed = serde_json::to_value(s).map_err(|_| ApiError::forbidden())?;
        sealed
            .as_object_mut()
            .ok_or_else(ApiError::forbidden)?
            .remove("source_hash");
        let canonical_ref = format!("refs/heads/sdlc/{}", b.task_id);
        if self.version != 1
            || s.contract_version != 1
            || self.intent.role != WorkspaceRole::Developer
            || [
                self.project_id,
                self.run_id,
                self.chat_id,
                s.confirmation_id,
                s.forge_instance_id,
                s.namespace.registry_instance_id,
                s.namespace.namespace_id,
            ]
            .iter()
            .any(Uuid::is_nil)
            || s.tracker_instance_id != b.tracker_instance_id
            || s.project_id != b.tracker_project_id
            || s.task_id != b.task_id
            || s.root_task_id != b.root_task_id
            || s.repository_id != self.intent.repository_id
            || s.routing_snapshot_id != b.routing_snapshot_id
            || s.requirement_revision != b.requirement_revision
            || s.base_commit != self.intent.source_commit
            || s.candidate_ref != canonical_ref
            || s.base_ref == canonical_ref
            || !s.base_ref.starts_with("refs/heads/")
            || !(1..=9007199254740991).contains(&s.source_revision)
            || !hex(&s.policy_hash, 64)
            || !hex(&s.content_hash, 64)
            || !hex(&s.source_hash, 64)
            || digest(&sealed)? != s.source_hash
            || !hex(&self.old_oid, 40)
            || !hex(&self.new_oid, 40)
            || self.new_oid.bytes().all(|b| b == b'0')
            || self.old_oid == self.new_oid
            || self.repository.is_empty()
            || self.repository.len() > 200
            || self.repository.split('/').count() != 2
            || self
                .repository
                .split('/')
                .any(|p| crate::git_host::validate_repo_name(p).ok().as_deref() != Some(p))
            || self.machine_subject.is_empty()
            || self.machine_subject.len() > 256
            || self
                .machine_subject
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || s.owner_subject.is_empty()
            || self.machine_subject == s.owner_subject
            || self.machine_subject == self.intent.holder_subject
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }

    pub fn grant(&self) -> Result<String, ApiError> {
        self.validate()?;
        Ok(format!("{GRANT_PREFIX}{}", digest(self)?))
    }

    fn current(&self, value: &Value) -> Result<std::time::Duration, ApiError> {
        let source: SourceBinding =
            serde_json::from_value(value["binding"].clone()).map_err(|_| ApiError::forbidden())?;
        if value["state"] != "bound" || source != self.source {
            return Err(ApiError::forbidden());
        }
        let current = &value["current"];
        if current["receipt"]["task"]["owner_subject"] != self.source.owner_subject
            || current["receipt"]["binding"]["run_id"] != json!(self.run_id)
            || current["receipt"]["binding"]["chat_id"] != json!(self.chat_id)
            || current["receipt"]["binding"]["machine_subject"] != self.machine_subject
        {
            return Err(ApiError::forbidden());
        }
        crate::api::sdlc_workspace::task_pipeline::current(current, &self.intent)
            .map_err(|_| ApiError::conflict("Текущий Developer assignment/lease изменился"))
    }
}

/// Pure input to Base's EXISTING delegate API. No root is accepted or minted here.
/// Owner must configure the exact subject/service prefix policy and existing root service scopes.
fn delegation_request(context: &GitContext, key: &str, ttl: u32) -> Result<Value, ApiError> {
    if !(1..=1800).contains(&ttl)
        || key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
    {
        return Err(ApiError::bad_request("Неверный delegation key/TTL"));
    }
    Ok(json!({"service":"ci-cd","label":"own-task Git child",
        "scopes":["ci-cd:read","ci-cd:write",context.grant()?],
        "idempotency_key":key,"expires_in_seconds":ttl}))
}

fn identity(
    ctx: &AuthContext,
    config: &SdlcWorkspaceConfig,
    context: &GitContext,
) -> Result<(), ApiError> {
    let subject = Uuid::parse_str(&ctx.user_id).map_err(|_| ApiError::forbidden())?;
    let grant = context.grant()?;
    if subject.is_nil()
        || subject.to_string() != ctx.user_id
        || config.delegated_subject != Some(subject)
        || config.operation_subject == Some(subject)
        || crate::namespace::registered_machine(&ctx.user_id)
        || ctx.session_id.is_some()
        || ctx.role.is_some()
        || !pdlc_auth_core::is_personal_token(&ctx.token)
        || ctx.token.len() > 128
        || ctx.token.chars().any(char::is_whitespace)
        || ctx.scopes.len() != 3
        || !ctx.scopes.contains("ci-cd:read")
        || !ctx.scopes.contains("ci-cd:write")
        || !ctx.scopes.contains(grant.as_str())
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

// Private in-memory request custody. Deliberately no Debug/Serialize.
pub(crate) struct Authorization {
    pub context: GitContext,
    pub token: String,
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, ApiError> {
    let values: Vec<_> = headers.get_all(name).iter().collect();
    match values.as_slice() {
        [v] => v.to_str().map_err(|_| ApiError::forbidden()),
        _ => Err(ApiError::forbidden()),
    }
}

fn repository_matches(raw: &str, expected: &str) -> bool {
    raw == expected || raw.strip_suffix(".git") == Some(expected)
}

pub(crate) async fn admit(
    state: &AppState,
    repository: &str,
    headers: &HeaderMap,
) -> Result<Option<Authorization>, ApiError> {
    if !headers.contains_key(HEADER) {
        return Ok(None);
    }
    let text = one_header(headers, HEADER)?;
    if text.len() > 16_384 {
        return Err(ApiError::forbidden());
    }
    let context: GitContext = serde_json::from_str(text).map_err(|_| ApiError::forbidden())?;
    if !repository_matches(repository, &context.repository) {
        return Err(ApiError::forbidden());
    }
    let token = one_header(headers, header::AUTHORIZATION.as_str())?
        .strip_prefix("Bearer ")
        .ok_or_else(ApiError::forbidden)?
        .to_owned();
    verify(state, &context, &token).await?;
    Ok(Some(Authorization { context, token }))
}

async fn verify(state: &AppState, context: &GitContext, token: &str) -> Result<(), ApiError> {
    context.validate()?;
    let (ctx, child_until) = crate::central_auth::try_central_child(token)
        .await?
        .ok_or_else(ApiError::forbidden)?;
    identity(&ctx, &state.config.sdlc_workspace, context)?;
    let until = verify_context(state, context).await?;
    if Instant::now() >= until.min(child_until) {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

/// Server-only caller supplies its current assignment/run and proposed exact OID CAS.
/// Fresh registered source readback MUST succeed before the existing Base issuer is called.
pub async fn prepare_delegation(
    state: &AppState,
    context: &GitContext,
    key: &str,
    ttl: u32,
) -> Result<Value, ApiError> {
    context.validate()?;
    let subject = state
        .config
        .sdlc_workspace
        .delegated_subject
        .ok_or_else(ApiError::forbidden)?;
    if subject.is_nil()
        || state.config.sdlc_workspace.operation_subject == Some(subject)
        || crate::namespace::registered_machine(&subject.to_string())
    {
        return Err(ApiError::forbidden());
    }
    let until = verify_context(state, context).await?;
    let request = delegation_request(context, key, ttl)?;
    if Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    Ok(request)
}

async fn verify_context(state: &AppState, context: &GitContext) -> Result<Instant, ApiError> {
    let pool = state.pool.as_ref().ok_or_else(ApiError::unavailable)?;
    let mut conn = pool.acquire().await.map_err(ApiError::internal)?;
    if !cicd_infra::sdlc_workspace::task_project_matches(
        &mut conn,
        context.project_id,
        &context.intent,
    )
    .await
    .map_err(ApiError::internal)?
    {
        return Err(ApiError::forbidden());
    }
    let row = sqlx::query("SELECT r.group_id,g.slug||'/'||r.slug AS public_name,b.command,r.storage_name FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id JOIN forge_namespace_bindings b ON b.resource_id=g.id WHERE r.id=$1 AND r.kind='hosted' AND r.ready AND b.state='active'")
        .bind(context.source.repository_id).fetch_optional(&mut *conn).await.map_err(ApiError::internal)?.ok_or_else(ApiError::forbidden)?;
    let command: OwnerCommand =
        serde_json::from_value(row.try_get("command").map_err(ApiError::internal)?)
            .map_err(|_| ApiError::forbidden())?;
    crate::namespace::validate_projection(&command)?;
    if command.namespace != context.source.namespace
        || command.resource.instance_id != context.source.forge_instance_id
        || command.resource.resource_id
            != row
                .try_get::<Uuid, _>("group_id")
                .map_err(ApiError::internal)?
        || command.state != "active"
        || context.repository
            != row
                .try_get::<String, _>("public_name")
                .map_err(ApiError::internal)?
    {
        return Err(ApiError::forbidden());
    }
    let storage: String = row.try_get("storage_name").map_err(ApiError::internal)?;
    let protected: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE repository_id=$1 AND ($2='refs/heads/'||default_branch OR $2=ANY(SELECT 'refs/heads/'||unnest(protected_branches))))")
        .bind(context.source.repository_id).bind(&context.source.candidate_ref).fetch_one(&mut *conn).await.map_err(ApiError::internal)?;
    if protected {
        return Err(ApiError::forbidden());
    }
    drop(conn);
    crate::repository_catalog::require_storage_writable(pool, &storage).await?;
    let (start, value) = crate::task_delivery::tracker::read_source_binding(
        &state.config.sdlc_workspace,
        context.source.task_id,
    )
    .await?;
    let until = start + context.current(&value)?;
    if Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    Ok(until)
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateCheck {
    pub task_candidate: GitContext,
}

#[derive(utoipa::ToSchema)]
#[serde(untagged)]
pub enum PushRequest {
    Candidate(Box<CandidateCheck>),
    Legacy(crate::git_host::GitPushEvent),
}

impl<'de> Deserialize<'de> for PushRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        // A malformed/mixed task variant must NEVER fall through to legacy auto-pipeline.
        if value.get("task_candidate").is_some() {
            serde_json::from_value(value)
                .map(Self::Candidate)
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(Self::Legacy)
                .map_err(serde::de::Error::custom)
        }
    }
}

pub(crate) async fn check_callback(
    state: &AppState,
    headers: &HeaderMap,
    request: CandidateCheck,
) -> Result<Value, ApiError> {
    let internal = state
        .git
        .internal_token
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(ApiError::forbidden)?;
    if one_header(headers, "x-internal-token")? != internal {
        return Err(ApiError::forbidden());
    }
    let token = one_header(headers, "authorization")?
        .strip_prefix("Bearer ")
        .ok_or_else(ApiError::forbidden)?;
    verify(state, &request.task_candidate, token).await?;
    // Verification only. NEVER enter the legacy push_decision/create_pipeline branch.
    Ok(json!({"verified":true,"triggered":false}))
}

#[cfg(test)]
mod tests;
