//! Immutable central owner of human-created pipelines, separate from runner credentials.
use crate::{
    api::{ApiError, authz_mw::VerifiedRequestIdentity},
    git_host::GitDeadline,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::future::Future;
use uuid::Uuid;
#[derive(Clone, Copy)]
pub(crate) struct Origin {
    subject: Uuid,
    clock: GitDeadline,
}
tokio::task_local! { static ORIGIN: Option<Origin>; }
impl Origin {
    pub(crate) fn from_identity(
        identity: &VerifiedRequestIdentity,
    ) -> Result<Option<Self>, ApiError> {
        if identity.claims.role == "service_account"
            || identity.claims.service_account_name.is_some()
        {
            return Ok(None);
        }
        let Some(until) = identity.original_deadline()? else {
            return Ok(None);
        };
        let subject = identity.space_identity()?.subject;
        let id = Uuid::parse_str(&subject).map_err(|_| ApiError::forbidden())?;
        if id.is_nil() || id.to_string() != subject {
            return Err(ApiError::forbidden());
        }
        Ok(Some(Self {
            subject: id,
            clock: GitDeadline::native(until)?,
        }))
    }
    pub(crate) fn restrict(self, clock: GitDeadline) -> Self {
        Self {
            subject: self.subject,
            clock: self.clock.intersect(clock),
        }
    }
}
pub(crate) async fn with_origin<T>(origin: Option<Origin>, work: impl Future<Output = T>) -> T {
    ORIGIN.scope(origin, work).await
}
pub(crate) fn current() -> Result<Option<Origin>, ApiError> {
    let origin = ORIGIN.try_with(|value| *value).unwrap_or(None);
    if let Some(origin) = origin {
        origin.clock.require()?;
    }
    Ok(origin)
}
pub(crate) fn require_creation(is_task: bool) -> Result<Option<Origin>, ApiError> {
    let origin = current()?;
    if crate::central_auth::application_policy_required()? && !is_task && origin.is_none() {
        return Err(ApiError::forbidden());
    }
    Ok(origin)
}
pub(crate) async fn record(
    tx: &mut Transaction<'_, Postgres>,
    pipeline: Uuid,
    origin: Option<Origin>,
    requires_admin: bool,
) -> Result<(), ApiError> {
    if let Some(origin) = origin {
        origin.clock.bind_commit_clock(tx).await?;
        sqlx::query("INSERT INTO pipeline_human_owners(pipeline_id,owner_subject,requires_admin) VALUES($1,$2,$3)")
            .bind(pipeline).bind(origin.subject).bind(requires_admin).execute(&mut **tx).await.map_err(ApiError::internal)?;
        origin.clock.require()?;
    }
    Ok(())
}
pub(crate) async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    pipeline: Uuid,
    origin: Option<Origin>,
) -> Result<(), ApiError> {
    if !crate::central_auth::application_policy_required()? {
        return Ok(());
    }
    if let Some(origin) = origin {
        let subject: Option<Uuid> = sqlx::query_scalar(
            "SELECT owner_subject FROM pipeline_human_owners WHERE pipeline_id=$1",
        )
        .bind(pipeline)
        .fetch_optional(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        if subject != Some(origin.subject) {
            return Err(ApiError::conflict("Pipeline original owner differs"));
        }
        origin.clock.bind_commit_clock(tx).await?;
    }
    Ok(())
}
/// New preparation/spawn/runner stages confirm the immutable owner again.
/// Task pipelines retain their stricter native Tracker/child/lease adapter.
pub(crate) async fn confirm(
    pool: &PgPool,
    pipeline: Uuid,
) -> Result<Option<GitDeadline>, ApiError> {
    if !crate::central_auth::application_policy_required()? {
        return Ok(None);
    }
    let task: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_pipeline_intents WHERE pipeline_id=$1)",
    )
    .bind(pipeline)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if task {
        return Ok(None);
    }
    let (subject, admin): (Uuid, bool) = sqlx::query_as(
        "SELECT owner_subject,requires_admin FROM pipeline_human_owners WHERE pipeline_id=$1",
    )
    .bind(pipeline)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::service_unavailable("Pipeline central owner migration pending"))?;
    let owner = crate::product_tokens::current_owner(subject).await?;
    if !owner.decision.enforced || !owner.permits(&["ci-cd"], admin) {
        return Err(ApiError::forbidden());
    }
    let clock = GitDeadline::owner(&owner);
    clock.require()?;
    Ok(Some(clock))
}
