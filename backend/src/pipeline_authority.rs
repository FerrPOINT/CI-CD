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
    admin: bool,
    clock: GitDeadline,
}
tokio::task_local! { static ORIGIN: Option<Origin>; }
pub(crate) fn current_clock() -> Option<GitDeadline> {
    ORIGIN
        .try_with(|origin| origin.map(|origin| origin.clock))
        .ok()
        .flatten()
}
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
            admin: identity.claims.role == "admin",
            clock: GitDeadline::native(until)?,
        }))
    }
    pub(crate) fn from_owner(
        owner: &pdlc_auth_core::application_access::VerifiedApplicationAccess,
    ) -> Result<Self, ApiError> {
        if !owner.decision.enforced || !owner.permits(&["ci-cd"], false) {
            return Err(ApiError::forbidden());
        }
        let subject =
            Uuid::parse_str(&owner.decision.subject).map_err(|_| ApiError::forbidden())?;
        let clock = GitDeadline::owner(owner);
        clock.require()?;
        Ok(Self {
            subject,
            admin: owner.decision.role == pdlc_auth_core::application_access::HumanRole::Admin,
            clock,
        })
    }
    pub(crate) fn subject(self) -> Uuid {
        self.subject
    }
    pub(crate) async fn bind(self, tx: &mut Transaction<'_, Postgres>) -> Result<(), ApiError> {
        self.clock.bind_commit_clock(tx).await
    }
    pub(crate) fn restrict(self, clock: GitDeadline) -> Self {
        Self {
            subject: self.subject,
            admin: self.admin,
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
        if requires_admin && !origin.admin {
            return Err(ApiError::forbidden());
        }
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
    attempt: Uuid,
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
    let row:Option<(Option<Uuid>,bool)>=sqlx::query_as(
        "SELECT COALESCE(o.owner_subject,CASE WHEN a.attempt_no=1 THEN p.owner_subject END),COALESCE(o.requires_admin,p.requires_admin,false) FROM execution_attempts a JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id LEFT JOIN execution_attempt_human_owners o ON o.attempt_id=a.id LEFT JOIN pipeline_human_owners p ON p.pipeline_id=s.pipeline_id WHERE s.pipeline_id=$1 AND a.id=$2",
    )
    .bind(pipeline).bind(attempt)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    ;
    let (subject, admin) = row
        .and_then(|(subject, admin)| subject.map(|subject| (subject, admin)))
        .ok_or_else(|| {
            ApiError::service_unavailable("Execution attempt central owner migration pending")
        })?;
    let owner = crate::product_tokens::current_owner(subject).await?;
    if !owner.decision.enforced || !owner.permits(&["ci-cd"], admin) {
        return Err(ApiError::forbidden());
    }
    let clock = GitDeadline::owner(&owner);
    clock.require()?;
    Ok(Some(clock))
}

/// An administrator-owned automation is a durable intent, not a durable role.
/// Its next fire reads the current owner and never selects a substitute admin.
pub(crate) async fn with_saved<T>(
    subject: Option<Uuid>,
    admin: bool,
    work: impl Future<Output = Result<T, ApiError>>,
) -> Result<T, ApiError> {
    if !crate::central_auth::application_policy_required()? {
        return work.await;
    }
    let owner = crate::product_tokens::current_owner(subject.ok_or_else(|| {
        ApiError::service_unavailable("Automation central owner migration pending")
    })?)
    .await?;
    if !owner.decision.enforced || !owner.permits(&["ci-cd"], admin) {
        return Err(ApiError::forbidden());
    }
    let id = Uuid::parse_str(&owner.decision.subject).map_err(|_| ApiError::forbidden())?;
    let clock = GitDeadline::owner(&owner);
    clock.require()?;
    with_origin(
        Some(Origin {
            subject: id,
            admin: owner.decision.role == pdlc_auth_core::application_access::HumanRole::Admin,
            clock,
        }),
        work,
    )
    .await
}

pub(crate) async fn retry_origin(
    tx: &mut Transaction<'_, Postgres>,
    pipeline: Uuid,
) -> Result<Option<(Origin, bool)>, ApiError> {
    let origin = require_creation(false)?;
    let Some(origin) = origin else {
        return Ok(None);
    };
    let task: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_pipeline_intents WHERE pipeline_id=$1)",
    )
    .bind(pipeline)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if task && crate::central_auth::application_policy_required()? {
        return Err(ApiError::forbidden());
    }
    let admin:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipeline_triggers WHERE pipeline_id=$1 AND source IN ('deployment-approval','deployment-rollback'))").bind(pipeline).fetch_one(&mut **tx).await.map_err(ApiError::internal)?;
    if admin && !origin.admin {
        return Err(ApiError::forbidden());
    }
    Ok(Some((origin, admin)))
}

pub(crate) async fn record_retry(
    tx: &mut Transaction<'_, Postgres>,
    pipeline: Uuid,
    attempt: Uuid,
) -> Result<(), ApiError> {
    let Some((origin, admin)) = retry_origin(tx, pipeline).await? else {
        return Ok(());
    };
    origin.bind(tx).await?;
    let inserted=sqlx::query("INSERT INTO execution_attempt_human_owners(attempt_id,owner_subject,requires_admin) SELECT a.id,$2,$3 FROM execution_attempts a JOIN jobs j ON j.id=a.job_id JOIN stages s ON s.id=j.stage_id WHERE a.id=$1 AND s.pipeline_id=$4").bind(attempt).bind(origin.subject).bind(admin).bind(pipeline).execute(&mut **tx).await.map_err(ApiError::internal)?;
    if inserted.rows_affected() != 1 {
        return Err(ApiError::conflict("Execution attempt scope changed"));
    }
    origin.clock.require()
}
