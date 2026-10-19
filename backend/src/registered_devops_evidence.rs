//! Namespace R observations for the first DevOps publication, never dispatch admission.
use crate::{
    api::{ApiError, AppState, sdlc_workspace},
    delegated_credentials::{StoredCredential, digest},
    domain::sdlc_workspace::*,
    registered_source::{RepositoryQuery, VerifiedSource},
    store::sdlc_workspace::{self as ledger, TaskPipelineJournal},
    task_delivery::owner_evidence::TargetEvidence,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

/// All selectors are locators or comparison values, not caller-issued evidence.
#[derive(Clone, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct EvidenceQuery {
    pub registry_instance_id: Uuid,
    pub namespace_id: Uuid,
    pub base_ref: String,
    pub base_commit: String,
    pub delivery_configuration_name: String,
    pub project_id: Uuid,
    pub source_pipeline_id: Uuid,
    pub operation_key: String,
    pub source_commit: String,
    pub request_hash: String,
    pub task_id: Uuid,
    pub root_task_id: Uuid,
    pub assignment_id: Uuid,
    pub execution_id: Uuid,
    pub fencing_token: i64,
}
impl EvidenceQuery {
    fn source_query(&self) -> Result<RepositoryQuery, ApiError> {
        if [
            self.registry_instance_id,
            self.namespace_id,
            self.project_id,
            self.source_pipeline_id,
            self.task_id,
            self.root_task_id,
            self.assignment_id,
            self.execution_id,
        ]
        .iter()
        .any(Uuid::is_nil)
            || self.task_id != self.root_task_id
            || !(1..=9007199254740991).contains(&self.fencing_token)
            || self.operation_key.is_empty()
            || self.operation_key.len() > 128
            || !self
                .operation_key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            || !hex(&self.request_hash, 64)
            || !hex(&self.source_commit, 40)
        {
            return Err(ApiError::bad_request("invalid_devops_evidence_selector"));
        }
        let query = RepositoryQuery {
            registry_instance_id: self.registry_instance_id,
            namespace_id: self.namespace_id,
            base_ref: self.base_ref.clone(),
            base_commit: self.base_commit.clone(),
            delivery_configuration_name: Some(self.delivery_configuration_name.clone()),
        };
        query.source()?;
        Ok(query)
    }
    fn lookup(&self) -> WorkspaceOperationLookup {
        WorkspaceOperationLookup {
            request_hash: self.request_hash.clone(),
            task_id: self.task_id,
            root_task_id: self.root_task_id,
            assignment_id: self.assignment_id,
            execution_id: self.execution_id,
            fencing_token: self.fencing_token,
        }
    }
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && value.bytes().any(|b| b != b'0')
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevopsEvidence {
    pub schema: &'static str,
    pub source: VerifiedSource,
    pub candidate: CandidateEvidenceReadback,
    pub target: TargetEvidence,
    /// R proves observations, not Tracker acceptance, a current task lease or execution authority.
    pub dispatch_allowed: bool,
}

fn configuration_matches(source: &VerifiedSource, query: &EvidenceQuery, repository: Uuid) -> bool {
    source.repository_id == repository
        && source
            .delivery_configuration
            .as_ref()
            .is_some_and(|configuration| {
                configuration.id == query.project_id
                    && configuration.name == query.delivery_configuration_name
                    && configuration.repository_id == repository
                    && configuration.repository_name == source.public_name
            })
}

fn retained_matches(
    record: &TaskPipelineJournal,
    receipt: &WorkspaceOperationReceipt,
    query: &EvidenceQuery,
    repository: Uuid,
) -> Result<(), ApiError> {
    // Do not invoke a legacy SAT lookup or decrypt/introspect the historical Developer child.
    if record.service_account_id.is_some() || record.api_token_id.is_some() {
        return Err(ApiError::forbidden());
    }
    let stored = StoredCredential::from_value(
        &record
            .delegated_credential
            .as_ref()
            .ok_or_else(ApiError::forbidden)?
            .0,
    )
    .map_err(|_| unavailable())?;
    stored.context.validate().map_err(|_| unavailable())?;
    let intent = &record.intent.0;
    if record.pipeline_id != query.source_pipeline_id
        || record.project_id != query.project_id
        || stored.context.project_id != record.project_id
        || stored.context.intent != *intent
        || stored.context_hash != digest(&stored.context)?
        || intent.role != WorkspaceRole::Developer
        || intent.repository_id != repository
        || intent.source_commit != query.source_commit
        || intent.operation_key != query.operation_key
        || receipt.schema != "forge/workspace-operation-receipt/v1"
        || receipt.dispatch_allowed
        || receipt.project_id != record.project_id
        || receipt.request.repository_id != repository
        || receipt.request.role != WorkspaceRole::Developer
        || receipt.request.access != WorkspaceAccess::ReadWrite
        || receipt.request.operation_key != intent.operation_key
        || receipt.request.source_commit != intent.source_commit
        || receipt.request.binding != intent.binding
        || record.preparation_request.as_ref().map(|r| &r.0) != Some(&receipt.request)
        || receipt.request_hash
            != format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&receipt.request).map_err(|_| unavailable())?)
            )
        || receipt.request.validate().is_err()
    {
        return Err(ApiError::conflict("devops_retained_source_mismatch"));
    }
    sdlc_workspace::verify_lookup(receipt, &query.lookup())
}

fn candidate_matches(candidate: &CandidateEvidenceReadback, query: &EvidenceQuery) -> bool {
    candidate.pipeline_id == query.source_pipeline_id
        && candidate.source_commit == query.source_commit
        && candidate.candidate_observed
        && candidate
            .config_sha256
            .as_deref()
            .is_some_and(|v| hex(v, 64))
        && candidate.plan_sha256.as_deref().is_some_and(|v| hex(v, 64))
        && !candidate.artifacts.is_empty()
        && !candidate.dispatch_allowed
        && !candidate.deployment_verified
        && !candidate.acceptance_verified
}

#[utoipa::path(get, path="/api/v1/namespace-repositories/{id}/devops-evidence",
    tag="namespaces", params(("id"=Uuid, Path), EvidenceQuery),
    responses((status=200, body=DevopsEvidence), (status=400, description="Invalid exact selectors"),
        (status=401, description="Central reader authentication required"),
        (status=403, description="Registered namespace reader required"),
        (status=404, description="Original owner evidence missing"),
        (status=409, description="Source, configuration, candidate or target binding changed"),
        (status=503, description="Owner evidence unavailable; no dispatch")))]
pub async fn readback(
    State(state): State<Arc<AppState>>,
    Path(repository): Path<Uuid>,
    Query(query): Query<EvidenceQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    let source_query = query.source_query()?;
    let mut tx = crate::repository_catalog::catalog_write_lease(&state, repository).await?;
    let mapped: Option<Uuid> = sqlx::query_scalar(
        "SELECT p.id FROM projects p JOIN repository_catalog r ON r.id=p.repository_id \
         JOIN forge_namespace_bindings b ON b.resource_id=r.group_id \
         JOIN forge_workspace_projects w ON w.registry_instance_id=b.registry_instance_id AND w.namespace_id=b.namespace_id \
         WHERE p.id=$1 AND r.id=$2 AND p.name=$3 AND w.state='active' FOR SHARE OF p,w",
    ).bind(query.project_id).bind(repository).bind(&query.delivery_configuration_name)
        .fetch_optional(&mut *tx).await.map_err(|_| unavailable())?;
    if mapped.is_none() {
        return Err(ApiError::conflict("devops_project_mapping_mismatch"));
    }
    let source = crate::registered_source::observe_on(
        &state,
        &mut tx,
        repository,
        source_query.namespace(),
        source_query.source()?,
        Some(&query.delivery_configuration_name),
    )
    .await?;
    if !configuration_matches(&source, &query, repository) {
        return Err(ApiError::conflict("devops_configuration_mismatch"));
    }
    // Same row lock as current credential publication; no write or credential refresh.
    let selected: Option<Uuid> = sqlx::query_scalar(
        "SELECT pipeline_id FROM task_pipeline_intents WHERE pipeline_id=$1 AND project_id=$2 FOR SHARE",
    ).bind(query.source_pipeline_id).bind(query.project_id).fetch_optional(&mut *tx)
        .await.map_err(|_| unavailable())?;
    if selected.is_none() {
        return Err(ApiError::not_found());
    }
    let record = ledger::find_task_pipeline(&mut tx, query.source_pipeline_id, false)
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(ApiError::not_found)?;
    if record.service_account_id.is_some()
        || record.api_token_id.is_some()
        || record.delegated_credential.is_none()
    {
        return Err(ApiError::forbidden());
    }
    if !ledger::task_project_matches(&mut tx, query.project_id, &record.intent)
        .await
        .map_err(|_| unavailable())?
    {
        return Err(ApiError::conflict("devops_project_mapping_mismatch"));
    }
    let receipt = sdlc_workspace::task_pipeline::find_owned_receipt(&mut tx, &record)
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(ApiError::not_found)?;
    retained_matches(&record, &receipt, &query, repository)?;
    let candidate = sdlc_workspace::candidate_evidence_on(&state, &mut tx, receipt)
        .await
        .map_err(|_| unavailable())?;
    if !candidate_matches(&candidate, &query) {
        return Err(ApiError::conflict("devops_candidate_unverified"));
    }
    let target = crate::task_delivery::owner_evidence::observe(
        &state,
        &mut tx,
        query.project_id,
        repository,
        &candidate.operation_receipt.request.binding,
    )
    .await?;
    tx.commit().await.map_err(|_| unavailable())?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(DevopsEvidence {
            schema: "forge/namespace-devops-evidence/v1",
            source,
            candidate,
            target,
            dispatch_allowed: false,
        }),
    ))
}

fn unavailable() -> ApiError {
    ApiError::service_unavailable("devops_owner_evidence_unavailable")
}

/// Main merges this slice and registers its OpenAPI schema; auth cannot be omitted by that merge.
pub fn routes() -> Router<Arc<AppState>> {
    protect(Router::new().route(
        "/api/v1/namespace-repositories/{id}/devops-evidence",
        get(readback),
    ))
}
fn protect<S: Clone + Send + Sync + 'static>(router: Router<S>) -> Router<S> {
    router.route_layer(axum::middleware::from_fn(crate::namespace::reader_auth))
}

#[cfg(test)]
#[path = "registered_devops_evidence_tests.rs"]
mod tests;
