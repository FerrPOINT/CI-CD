//! Exact original task-pipeline read; no list, cross-task lookup or effect authority.
use super::*;
use crate::api::dto::PipelineDetail;

fn bind_read(
    identity: &Identity,
    pipeline: Uuid,
    record: &ledger::TaskPipelineJournal,
    stored: &StoredCredential,
) -> Result<(), ApiError> {
    if record.pipeline_id != pipeline
        || record.delegated_credential.is_none()
        || record.service_account_id.is_some()
        || record.api_token_id.is_some()
        || stored.subject != identity.subject
        || stored.fingerprint != identity.fingerprint()
    {
        return Err(ApiError::forbidden());
    }
    identity.bind(&stored.context, record.project_id, &record.intent)
}

fn bound_detail(
    detail: &PipelineDetail,
    record: &ledger::TaskPipelineJournal,
) -> Result<(), ApiError> {
    if detail.pipeline.id != record.pipeline_id
        || detail.pipeline.project_id != record.project_id
        || detail.plan.as_ref().is_some_and(|plan| {
            plan.pipeline_id != record.pipeline_id
                || plan.resolved_commit_sha.as_deref() != Some(record.intent.source_commit.as_str())
        })
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

pub(crate) async fn read(
    state: &AppState,
    identity: &Identity,
    pipeline: Uuid,
) -> ApiResult<PipelineDetail> {
    let mut tx = pool(state)?.begin().await.map_err(ApiError::internal)?;
    let record = ledger::find_task_pipeline(&mut tx, pipeline, false)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if record.delegated_credential.is_none()
        || record.service_account_id.is_some()
        || record.api_token_id.is_some()
    {
        return Err(ApiError::forbidden());
    }
    // Keep the existing intent SHARE lock through the read; renewal owns UPDATE.
    let stored = crate::delegated_credentials::custody::current(&mut tx, &record).await?;
    bind_read(identity, pipeline, &record, &stored)?;
    let until = authorize_saved(
        &mut tx,
        &state.config.sdlc_workspace,
        &record,
        &state.config.secrets,
    )
    .await?;
    let exact_source: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pipelines WHERE id=$1 AND project_id=$2 AND commit_sha=$3)",
    )
    .bind(pipeline)
    .bind(record.project_id)
    .bind(&record.intent.source_commit)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if !exact_source {
        return Err(ApiError::forbidden());
    }
    let detail = crate::api::pipeline_detail_on(&mut tx, pipeline).await?;
    bound_detail(&detail, &record)?;
    tx.commit().await.map_err(ApiError::internal)?;
    if Instant::now() >= until {
        return Err(ApiError::forbidden());
    }
    Ok(Json(detail))
}

#[cfg(test)]
mod tests;
