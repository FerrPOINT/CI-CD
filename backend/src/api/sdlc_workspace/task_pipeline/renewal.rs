//! Explicit owner handoff only. No trigger, enqueue, claim, preparation or dispatch.
use super::*;
use crate::delegated_credentials::{custody, digest};
use axum::{
    http::HeaderMap,
    routing::{get, post},
};
use pdlc_auth_core::AuthContext;
use serde::Deserialize;

// Deliberately no Debug/Serialize: replacement bearer never enters a receipt/log.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenewalRequest {
    pub renewal_key: String,
    pub predecessor_fingerprint: String,
    pub credential_context: TaskCredentialContext,
    pub replacement_token: String,
}

impl RenewalRequest {
    fn validate(&self) -> Result<(), ApiError> {
        validate_key(&self.renewal_key)?;
        self.credential_context.validate()?;
        if self.predecessor_fingerprint.len() != 64
            || !self
                .predecessor_fingerprint
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || !pdlc_auth_core::is_personal_token(&self.replacement_token)
            || self.replacement_token.len() > 128
            || self
                .replacement_token
                .bytes()
                .any(|c| !c.is_ascii_graphic())
        {
            return Err(ApiError::bad_request("Invalid credential renewal input"));
        }
        Ok(())
    }

    fn fingerprint(&self, project: Uuid, pipeline: Uuid, owner: Uuid) -> Result<String, ApiError> {
        digest(&json!({"project":project,"pipeline":pipeline,"owner":owner,
            "key":self.renewal_key,"predecessor":self.predecessor_fingerprint,
            "context":self.credential_context,"replacement":crate::auth::hash_token(&self.replacement_token)}))
    }
}

fn validate_key(key: &str) -> Result<(), ApiError> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(&c))
    {
        return Err(ApiError::bad_request("Invalid credential renewal key"));
    }
    Ok(())
}

fn owner_identity(ctx: &AuthContext, config: &SdlcWorkspaceConfig) -> Result<Uuid, ApiError> {
    let subject = Uuid::parse_str(&ctx.user_id).map_err(|_| ApiError::forbidden())?;
    if subject.is_nil()
        || subject.to_string() != ctx.user_id
        || config.delegated_subject != Some(subject)
        || config.operation_subject == Some(subject)
        || crate::namespace::registered_machine(&ctx.user_id)
        || ctx.session_id.is_some()
        || ctx.role.is_some()
        || !pdlc_auth_core::is_personal_token(&ctx.token)
        || ctx.token.len() > 128
        || ctx.token.bytes().any(|c| !c.is_ascii_graphic())
        || ctx.scopes.len() != 2
        || !ctx.scopes.contains("ci-cd:read")
        || !ctx.scopes.contains("ci-cd:write")
    {
        return Err(ApiError::forbidden());
    }
    Ok(subject)
}

fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    let mut values = headers.get_all(axum::http::header::AUTHORIZATION).iter();
    let first = values.next().ok_or_else(ApiError::unauthorized)?;
    if values.next().is_some() {
        return Err(ApiError::unauthorized());
    }
    first
        .to_str()
        .ok()
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|token| pdlc_auth_core::is_personal_token(token))
        .ok_or_else(ApiError::unauthorized)
}

async fn owner(state: &AppState, headers: &HeaderMap) -> Result<Uuid, ApiError> {
    let ctx = crate::central_auth::try_central(bearer(headers)?)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    owner_identity(&ctx, &state.config.sdlc_workspace)
}

fn original(
    record: &ledger::TaskPipelineJournal,
    project: Uuid,
    owner: Uuid,
) -> Result<StoredCredential, ApiError> {
    if record.project_id != project
        || record.service_account_id.is_some()
        || record.api_token_id.is_some()
    {
        return Err(ApiError::forbidden());
    }
    let stored = StoredCredential::from_value(
        &record
            .delegated_credential
            .as_ref()
            .ok_or_else(ApiError::forbidden)?
            .0,
    )?;
    if stored.subject != owner
        || stored.context.project_id != project
        || stored.context.intent != *record.intent
    {
        return Err(ApiError::forbidden());
    }
    Ok(stored)
}

fn compare_predecessor(
    current: &StoredCredential,
    request: &RenewalRequest,
) -> Result<(), ApiError> {
    if current.fingerprint != request.predecessor_fingerprint {
        return Err(ApiError::conflict("Credential predecessor changed"));
    }
    if current.context != request.credential_context {
        return Err(ApiError::forbidden());
    }
    if crate::auth::hash_token(&request.replacement_token) == current.fingerprint {
        return Err(ApiError::conflict("Credential successor must differ"));
    }
    Ok(())
}

fn replacement(
    ctx: &AuthContext,
    config: &SdlcWorkspaceConfig,
    secrets: &SecretsConfig,
    original: &StoredCredential,
    context: TaskCredentialContext,
) -> Result<StoredCredential, ApiError> {
    let identity = Identity::verified(ctx, config)?;
    identity.bind(
        &context,
        original.context.project_id,
        &original.context.intent,
    )?;
    let replacement = identity.store(context, secrets)?;
    custody::same_context(original, &replacement)?;
    Ok(replacement)
}

async fn live_context(
    conn: &mut sqlx::PgConnection,
    state: &AppState,
    record: &ledger::TaskPipelineJournal,
    stored: &StoredCredential,
) -> Result<Instant, ApiError> {
    if !ledger::task_project_matches(conn, record.project_id, &record.intent)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::forbidden());
    }
    tracker_bound(
        &state.config.sdlc_workspace,
        &record.intent,
        Some(&stored.context),
    )
    .await
}

/// Mount on exact System-classified routes: these handlers perform their own P authentication.
/// Do not run the legacy User/SAT or NamespaceOwner credential adapters on this router.
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals", post(renew))
        .route("/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals/{renewal_key}", get(readback))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::map_response(|mut response: axum::response::Response| async move {
            response.headers_mut().insert("cache-control", axum::http::HeaderValue::from_static("no-store"));
            response
        }))
}

#[utoipa::path(post,path="/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals",
    tag="SDLC workspace",params(("project_id"=Uuid,Path),("pipeline_id"=Uuid,Path)),
    request_body=RenewalRequest,responses((status=200,body=custody::RenewalReadback),(status=401),(status=403),(status=409)))]
pub(crate) async fn renew(
    State(state): State<Arc<AppState>>,
    Path((project, pipeline)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<RenewalRequest>,
) -> ApiResult<custody::RenewalReadback> {
    request.validate()?;
    let subject = owner(&state, &headers).await?;
    let request_hash = request.fingerprint(project, pipeline, subject)?;
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    let record = ledger::find_task_pipeline(&mut tx, pipeline, true)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let original = original(&record, project, subject)?;
    if let Some(saved) = custody::find(&mut tx, pipeline, &request.renewal_key).await? {
        if saved.request_hash != request_hash {
            return Err(ApiError::conflict(
                "Renewal key already has different input",
            ));
        }
        let until = live_context(&mut tx, &state, &record, &original).await?;
        owner(&state, &headers).await?;
        if Instant::now() >= until {
            return Err(ApiError::forbidden());
        }
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(saved));
    }
    let current = custody::current(&mut tx, &record).await?;
    compare_predecessor(&current, &request)?;
    let (ctx, child_until) = crate::central_auth::try_central_child(&request.replacement_token)
        .await?
        .ok_or_else(ApiError::forbidden)?;
    let replacement = replacement(
        &ctx,
        &state.config.sdlc_workspace,
        &state.config.secrets,
        &original,
        request.credential_context,
    )?;
    if replacement.fingerprint == original.fingerprint {
        return Err(ApiError::conflict(
            "Original credential cannot be reinstalled",
        ));
    }
    let until = child_until.min(live_context(&mut tx, &state, &record, &replacement).await?);
    // Fresh root authentication after all lock/network waits. The expired predecessor
    // is data only, never the credential authenticating this handoff.
    if owner(&state, &headers).await? != subject || Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    let receipt = custody::append(
        &mut tx,
        pipeline,
        &request.renewal_key,
        &request_hash,
        &request.predecessor_fingerprint,
        &replacement,
    )
    .await?;
    if Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(receipt))
}

#[utoipa::path(get,path="/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals/{renewal_key}",
    tag="SDLC workspace",params(("project_id"=Uuid,Path),("pipeline_id"=Uuid,Path),("renewal_key"=String,Path)),
    responses((status=200,body=custody::RenewalReadback),(status=401),(status=403),(status=404)))]
pub(crate) async fn readback(
    State(state): State<Arc<AppState>>,
    Path((project, pipeline, key)): Path<(Uuid, Uuid, String)>,
    headers: HeaderMap,
) -> ApiResult<custody::RenewalReadback> {
    validate_key(&key)?;
    let subject = owner(&state, &headers).await?;
    let mut tx = pool(&state)?.begin().await.map_err(ApiError::internal)?;
    let record = ledger::find_task_pipeline(&mut tx, pipeline, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let original = original(&record, project, subject)?;
    // Share the intent lock so current=true remains a coherent read, not an old ACK.
    custody::current(&mut tx, &record).await?;
    let receipt = custody::find(&mut tx, pipeline, &key)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let until = live_context(&mut tx, &state, &record, &original).await?;
    if owner(&state, &headers).await? != subject || Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(receipt))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod pg_tests;
