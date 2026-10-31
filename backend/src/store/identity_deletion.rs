//! Durable owner fences. Every owned reference writer is guarded by migration 0105.
use pdlc_shared::identity_lifecycle::{
    DeletionRequest, OwnerFreezeReceipt, OwnerReferenceSummary, OwnerReleaseReceipt,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

const OWNER: &str = "ci-cd";
const SOURCES: &[&str] = &[
    "users",
    "user_credentials",
    "sessions",
    "api_tokens",
    "project_memberships",
    "tenant_memberships",
    "notification_preferences",
    "forge_confirmed_user_roles",
    "service_accounts",
    "tenants",
    "pull_task_links",
    "repository_attach_operations",
    "delivery_configuration_operations",
    "repository_sync_operations",
    "repository_sync_connections",
    "repository_sync_runs",
    "pull_requests",
    "audit_log",
    "releases",
    "deployment_approvals",
    "forge_space_owner_operations",
    "task_pipeline_intents",
    "task_credential_successors",
    "sdlc_workspace_operations",
    "pipeline_human_owners",
    "execution_attempt_human_owners",
    "schedules",
    "schedule_fires",
    "outbox_messages",
    "pull_request_merge_attempts",
    "development_workspaces",
    "development_workspace_sessions",
    "development_workspace_operations",
];

#[derive(Debug, thiserror::Error)]
pub enum OwnerError {
    #[error("invalid_identity_deletion_request")]
    Invalid,
    #[error("identity_deletion_operation_conflict")]
    Conflict,
    #[error("identity_legacy_issuer_unbound")]
    Unbound,
    #[error("identity_legacy_reference_unresolved")]
    Unresolved,
    #[error("identity_deletion_authority_expired")]
    Expired,
    #[error("identity_deletion_storage_unavailable")]
    Storage,
}
fn storage(error: sqlx::Error) -> OwnerError {
    if error.as_database_error().is_some_and(|error| {
        error.code().as_deref() == Some("42501")
            && error.message() == "forge_request_authority_expired"
    }) {
        OwnerError::Expired
    } else {
        OwnerError::Storage
    }
}

/// Original machine-request freshness only; this does not grant human or native access.
#[derive(Clone, Copy)]
pub struct IdentityDeletionWindow {
    clock: crate::git_host::GitDeadline,
}
impl IdentityDeletionWindow {
    pub fn restricted(deadline: std::time::Instant, valid_until: i64) -> Result<Self, OwnerError> {
        Ok(Self {
            clock: crate::git_host::GitDeadline::native(deadline)
                .map_err(|_| OwnerError::Expired)?
                .cap_until(valid_until),
        })
    }
    pub(crate) fn confirmed(
        source: &pdlc_auth_core::service_bridge::VerifiedCentralToken,
    ) -> Result<Self, OwnerError> {
        let original = crate::git_host::GitDeadline::identity(source);
        let cap = crate::git_host::GitDeadline::native(source.lifetime.monotonic_deadline())
            .map_err(|_| OwnerError::Expired)?;
        Ok(Self {
            clock: original.intersect(cap),
        })
    }
    pub fn is_current(&self) -> bool {
        self.clock.current()
    }
    async fn bind(&self, tx: &mut Transaction<'_, Postgres>) -> Result<(), OwnerError> {
        self.clock.bind_commit_clock(tx).await.map_err(|_| {
            if self.is_current() {
                OwnerError::Storage
            } else {
                OwnerError::Expired
            }
        })
    }
}

#[derive(Clone)]
pub struct IdentityDeletionStore {
    pool: PgPool,
}
impl IdentityDeletionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// An explicit deployment assertion, never inferred from email or UUID similarity.
    pub async fn bind_legacy_issuer(&self, issuer: &str) -> Result<(), OwnerError> {
        let url = reqwest::Url::parse(issuer).map_err(|_| OwnerError::Invalid)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(OwnerError::Invalid);
        }
        let mut tx = self.pool.begin().await.map_err(storage)?;
        lock(&mut tx).await?;
        let bound: Option<String> =
            sqlx::query_scalar("SELECT issuer FROM identity_legacy_issuer WHERE singleton")
                .fetch_optional(&mut *tx)
                .await
                .map_err(storage)?;
        match bound {
            Some(bound) if bound != issuer => return Err(OwnerError::Conflict),
            Some(_) => (),
            None => {
                sqlx::query("INSERT INTO identity_legacy_issuer(singleton,issuer) VALUES(true,$1)")
                    .bind(issuer)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage)?;
            }
        }
        sqlx::query("SELECT forge_identity_enroll('local_user_id',id::text,$1,central_sub),forge_identity_enroll('local_principal_id',id::text,$1,central_sub),forge_identity_enroll('username',username,$1,central_sub) FROM users WHERE central_sub IS NOT NULL AND central_sub<>''")
            .bind(issuer).execute(&mut *tx).await.map_err(storage)?;
        sqlx::query("SELECT forge_identity_enroll_machine(id::text) FROM service_accounts")
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        tx.commit().await.map_err(storage)
    }

    async fn begin(
        &self,
        request: &DeletionRequest,
        window: &IdentityDeletionWindow,
    ) -> Result<Transaction<'_, Postgres>, OwnerError> {
        if !request.valid() {
            return Err(OwnerError::Invalid);
        }
        let mut tx = self.pool.begin().await.map_err(storage)?;
        sqlx::query(
            "SELECT set_config('lock_timeout','8s',true),set_config('statement_timeout','8s',true)",
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        window.bind(&mut tx).await?;
        lock(&mut tx).await?;
        let bound: Option<String> =
            sqlx::query_scalar("SELECT issuer FROM identity_legacy_issuer WHERE singleton")
                .fetch_optional(&mut *tx)
                .await
                .map_err(storage)?;
        if bound.as_deref() != Some(request.identity.issuer.as_str()) {
            return Err(OwnerError::Unbound);
        }
        Ok(tx)
    }

    pub async fn check(
        &self,
        request: &DeletionRequest,
        window: &IdentityDeletionWindow,
    ) -> Result<OwnerReferenceSummary, OwnerError> {
        let mut tx = self.begin(request, window).await?;
        if !window.is_current() {
            return Err(OwnerError::Expired);
        }
        verify_operation(&mut tx, request).await?;
        let reference_count = count(&mut tx, request).await?;
        let revision = revision(&mut tx).await?;
        if !window.is_current() {
            return Err(OwnerError::Expired);
        }
        tx.commit().await.map_err(storage)?;
        Ok(OwnerReferenceSummary {
            request: request.clone(),
            owner: OWNER.into(),
            revision,
            reference_count,
            fencing_supported: true,
        })
    }

    pub async fn freeze(
        &self,
        request: &DeletionRequest,
        window: &IdentityDeletionWindow,
    ) -> Result<OwnerFreezeReceipt, OwnerError> {
        let mut tx = self.begin(request, window).await?;
        if !window.is_current() {
            return Err(OwnerError::Expired);
        }
        if let Some(row) = verify_operation(&mut tx, request).await? {
            if row.try_get::<String, _>("state").map_err(storage)? != "frozen" {
                return Err(OwnerError::Conflict);
            }
            let result = OwnerFreezeReceipt {
                request: request.clone(),
                owner: OWNER.into(),
                fence: row.try_get("fence").map_err(storage)?,
                revision: row.try_get::<i64, _>("revision").map_err(storage)? as u64,
                reference_count: row.try_get::<i64, _>("reference_count").map_err(storage)? as u64,
            };
            if !window.is_current() {
                return Err(OwnerError::Expired);
            }
            tx.commit().await.map_err(storage)?;
            return Ok(result);
        }
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity_deletion_operations WHERE issuer=$1 AND subject=$2 AND state='frozen')")
            .bind(&request.identity.issuer).bind(&request.identity.subject).fetch_one(&mut *tx).await.map_err(storage)?;
        if active {
            return Err(OwnerError::Conflict);
        }
        let reference_count = count(&mut tx, request).await?;
        let revision = revision(&mut tx).await?;
        let fence = Uuid::new_v4();
        sqlx::query("INSERT INTO identity_deletion_operations(operation_id,issuer,subject,inventory_revision,request,state,fence,revision,reference_count) VALUES($1,$2,$3,$4,$5,'frozen',$6,$7,$8)")
            .bind(request.operation_id).bind(&request.identity.issuer).bind(&request.identity.subject)
            .bind(request.inventory_revision).bind(serde_json::to_value(request).map_err(|_|OwnerError::Invalid)?)
            .bind(fence).bind(revision as i64).bind(reference_count as i64).execute(&mut *tx).await.map_err(storage)?;
        if !window.is_current() {
            return Err(OwnerError::Expired);
        }
        tx.commit().await.map_err(storage)?;
        Ok(OwnerFreezeReceipt {
            request: request.clone(),
            owner: OWNER.into(),
            fence,
            revision,
            reference_count,
        })
    }

    pub async fn release(
        &self,
        request: &DeletionRequest,
        window: &IdentityDeletionWindow,
    ) -> Result<OwnerReleaseReceipt, OwnerError> {
        let mut tx = self.begin(request, window).await?;
        if !window.is_current() {
            return Err(OwnerError::Expired);
        }
        // The trusted coordinator serializes cancellation against finalization.
        // A release receipt is durable even if the original freeze never arrived.
        verify_operation(&mut tx, request).await?;
        let revision = revision(&mut tx).await?;
        sqlx::query("INSERT INTO identity_deletion_operations(operation_id,issuer,subject,inventory_revision,request,state,fence,revision,reference_count) VALUES($1,$2,$3,$4,$5,'released',NULL,$6,0) ON CONFLICT(operation_id) DO UPDATE SET state='released',fence=NULL")
            .bind(request.operation_id).bind(&request.identity.issuer).bind(&request.identity.subject)
            .bind(request.inventory_revision).bind(serde_json::to_value(request).map_err(|_|OwnerError::Invalid)?)
            .bind(revision as i64).execute(&mut *tx).await.map_err(storage)?;
        if !window.is_current() {
            return Err(OwnerError::Expired);
        }
        tx.commit().await.map_err(storage)?;
        Ok(OwnerReleaseReceipt {
            request: request.clone(),
            owner: OWNER.into(),
            cancelled: true,
        })
    }
}
async fn lock(tx: &mut Transaction<'_, Postgres>) -> Result<(), OwnerError> {
    sqlx::query(
        "SELECT set_config('lock_timeout','8s',true),set_config('statement_timeout','8s',true)",
    )
    .execute(&mut **tx)
    .await
    .map_err(storage)?;
    sqlx::query("SELECT pg_advisory_xact_lock(714402216002::bigint)")
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
    let result = sqlx::query("UPDATE identity_fence_epoch SET revision=revision+1 WHERE singleton")
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
    if result.rows_affected() != 1 {
        return Err(OwnerError::Storage);
    }
    Ok(())
}
async fn verify_operation(
    tx: &mut Transaction<'_, Postgres>,
    request: &DeletionRequest,
) -> Result<Option<sqlx::postgres::PgRow>, OwnerError> {
    let row=sqlx::query("SELECT request,state,fence,revision,reference_count FROM identity_deletion_operations WHERE operation_id=$1")
        .bind(request.operation_id).fetch_optional(&mut **tx).await.map_err(storage)?;
    if let Some(row) = &row {
        let value: Value = row.try_get("request").map_err(storage)?;
        if value != serde_json::to_value(request).map_err(|_| OwnerError::Invalid)? {
            return Err(OwnerError::Conflict);
        }
    }
    Ok(row)
}
async fn revision(tx: &mut Transaction<'_, Postgres>) -> Result<u64, OwnerError> {
    let value: i64 = sqlx::query_scalar("SELECT nextval('identity_reference_revision')")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage)?;
    Ok(value as u64)
}
async fn count(
    tx: &mut Transaction<'_, Postgres>,
    request: &DeletionRequest,
) -> Result<u64, OwnerError> {
    let mut count = 0_u64;
    for source in SOURCES {
        let sql = format!(
            "SELECT COUNT(*) FROM {source} t WHERE EXISTS(SELECT 1 FROM forge_identity_row_refs('{source}',to_jsonb(t)) r WHERE r.issuer=$1 AND r.subject=$2)"
        );
        let unknown_sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {source} t CROSS JOIN LATERAL forge_identity_row_refs('{source}',to_jsonb(t)) r WHERE r.unresolved)"
        );
        let unresolved: bool = sqlx::query_scalar(&unknown_sql)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage)?;
        if unresolved {
            return Err(OwnerError::Unresolved);
        }
        let value: i64 = sqlx::query_scalar(&sql)
            .bind(&request.identity.issuer)
            .bind(&request.identity.subject)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage)?;
        count = count.checked_add(value as u64).ok_or(OwnerError::Storage)?;
    }
    Ok(count)
}
