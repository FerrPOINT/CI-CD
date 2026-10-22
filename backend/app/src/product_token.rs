//! Existing product PAT + current central owner, without a new human identity/token.
use crate::auth::AccessClaims;
use chrono::{DateTime, Utc};
use pdlc_auth_core::{AuthContext, application_access::VerifiedApplicationAccess};
use sqlx::PgPool;
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ProductTokenError {
    #[error("Invalid product personal token")]
    Invalid,
    #[error("Current central owner admission required")]
    Denied,
    #[error("Product token storage unavailable")]
    Storage(#[from] sqlx::Error),
}
/// Verified local credential metadata; historical local role is deliberately not loaded.
pub struct LocalPersonalToken {
    token: Uuid,
    user: Uuid,
    owner: Uuid,
    project: Option<Uuid>,
    scopes: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
    started: Instant,
    wall_started: DateTime<Utc>,
}
#[derive(Clone)]
pub struct ConfirmedPersonalToken {
    claims: AccessClaims,
    owner: VerifiedApplicationAccess,
}
impl LocalPersonalToken {
    pub fn owner(&self) -> Uuid {
        self.owner
    }
    pub fn confirm(
        self,
        owner: VerifiedApplicationAccess,
    ) -> Result<ConfirmedPersonalToken, ProductTokenError> {
        if !owner.decision.enforced
            || owner.decision.subject != self.owner.to_string()
            || !owner.permits(&["ci-cd"], false)
        {
            return Err(ProductTokenError::Denied);
        }
        let owner = if let Some(until) = self.expires_at {
            let remaining = until
                .signed_duration_since(self.wall_started)
                .to_std()
                .map_err(|_| ProductTokenError::Denied)?;
            let deadline = self
                .started
                .checked_add(remaining)
                .ok_or(ProductTokenError::Denied)?;
            owner
                .restrict_lifetime(deadline, until)
                .ok_or(ProductTokenError::Denied)?
        } else {
            owner
        };
        if !owner.is_current() {
            return Err(ProductTokenError::Denied);
        }
        let claims = AccessClaims {
            sub: self.user,
            service_account_name: None,
            sid: None,
            token_id: Some(self.token),
            token_project_id: self.project,
            token_scopes: self.scopes,
            role: owner.decision.role.as_str().into(),
            ver: 0,
            iat: self.wall_started.timestamp(),
            exp: owner.decision.expires_at.timestamp(),
        };
        Ok(ConfirmedPersonalToken { claims, owner })
    }
}
impl ConfirmedPersonalToken {
    pub fn claims(&self) -> &AccessClaims {
        &self.claims
    }
    pub fn owner(&self) -> &VerifiedApplicationAccess {
        &self.owner
    }
    pub fn is_current(&self) -> bool {
        self.owner.permits(&["ci-cd"], false)
    }
    /// The server-bound owner and the product credential remain separate facts.
    /// No bearer/password is retained or minted by this adapter.
    pub fn context(&self) -> AuthContext {
        AuthContext {
            user_id: self.owner.decision.subject.clone(),
            role: Some(self.claims.role.clone()),
            email: None,
            scopes: self.claims.token_scopes.iter().cloned().collect(),
            session_id: None,
            token: String::new(),
        }
    }
}
pub async fn lookup(pool: &PgPool, token: &str) -> Result<LocalPersonalToken, ProductTokenError> {
    if !token.starts_with("cicd_") {
        return Err(ProductTokenError::Invalid);
    }
    let started = Instant::now();
    let wall_started = Utc::now();
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            Option<String>,
            Option<Uuid>,
            Vec<String>,
            Option<DateTime<Utc>>,
        ),
    >(
        "SELECT t.id,u.id,u.central_sub,t.project_id,t.scopes,t.expires_at
         FROM api_tokens t JOIN users u ON u.id=t.user_id
         WHERE t.token_hash=$1 AND t.principal_type='user' AND u.enabled
           AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at>clock_timestamp())",
    )
    .bind(crate::auth::hash_token(token))
    .fetch_optional(pool)
    .await?
    .ok_or(ProductTokenError::Invalid)?;
    let central = row.2.ok_or(ProductTokenError::Denied)?;
    let owner = Uuid::parse_str(&central).map_err(|_| ProductTokenError::Denied)?;
    if owner.is_nil() || owner.to_string() != central {
        return Err(ProductTokenError::Denied);
    }
    Ok(LocalPersonalToken {
        token: row.0,
        user: row.1,
        owner,
        project: row.3,
        scopes: row.4,
        expires_at: row.5,
        started,
        wall_started,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdlc_auth_core::application_access::{ApplicationAccessV1, HumanRole};
    fn record(expires: Option<DateTime<Utc>>) -> LocalPersonalToken {
        LocalPersonalToken {
            token: Uuid::from_u128(1),
            user: Uuid::from_u128(2),
            owner: Uuid::from_u128(3),
            project: Some(Uuid::from_u128(4)),
            scopes: vec!["git:read".into()],
            expires_at: expires,
            started: Instant::now(),
            wall_started: Utc::now(),
        }
    }
    fn proof(role: HumanRole, applications: &[&str]) -> VerifiedApplicationAccess {
        let now = Utc::now();
        VerifiedApplicationAccess::confirm(
            ApplicationAccessV1 {
                schema_version: 1,
                issuer: "https://owner.test".into(),
                subject: Uuid::from_u128(3).to_string(),
                role,
                applications: applications.iter().map(|key| key.to_string()).collect(),
                revision: 9,
                enforced: true,
                observed_at: now,
                expires_at: now + chrono::Duration::seconds(30),
            },
            "https://owner.test",
            &Uuid::from_u128(3).to_string(),
            Instant::now(),
            now,
        )
        .unwrap()
    }
    #[test]
    fn owner_role_and_existing_scope_project_intersect_without_minting_a_session() {
        for role in [HumanRole::User, HumanRole::Admin] {
            let value = record(None).confirm(proof(role, &["ci-cd"])).unwrap();
            assert!(value.is_current());
            assert_eq!(value.claims().role, role.as_str());
            assert_eq!(value.claims().token_scopes, vec!["git:read"]);
            assert_eq!(value.claims().token_project_id, Some(Uuid::from_u128(4)));
            assert_eq!(value.claims().sub, Uuid::from_u128(2));
            assert!(value.claims().sid.is_none());
            assert!(value.context().session_id.is_none());
            assert!(value.context().token.is_empty());
        }
    }
    #[test]
    fn absent_revoked_and_wrong_owner_application_cannot_be_replaced_by_product_history() {
        assert!(record(None).confirm(proof(HumanRole::User, &[])).is_err());
        let mut owner = proof(HumanRole::Admin, &["ci-cd"]);
        owner.decision.subject = Uuid::from_u128(99).to_string();
        assert!(record(None).confirm(owner).is_err());
        let mut owner = proof(HumanRole::Admin, &["ci-cd"]);
        owner.decision.enforced = false;
        assert!(record(None).confirm(owner).is_err());
    }
    #[test]
    fn local_credential_cannot_outlive_or_renew_the_original_owner_confirmation() {
        let owner = proof(HumanRole::User, &["ci-cd"]);
        let deadline = owner.monotonic_deadline();
        let until = Utc::now() + chrono::Duration::milliseconds(200);
        let value = record(Some(until)).confirm(owner).unwrap();
        assert!(value.owner().monotonic_deadline() <= deadline);
        assert!(value.owner().decision.expires_at <= until);
        assert!(
            record(Some(Utc::now() - chrono::Duration::seconds(1)))
                .confirm(proof(HumanRole::User, &["ci-cd"]))
                .is_err()
        );
        let mut expired = record(None);
        expired.started = Instant::now() - std::time::Duration::from_secs(100);
        expired.wall_started = Utc::now() - chrono::Duration::seconds(100);
        expired.expires_at = Some(expired.wall_started + chrono::Duration::seconds(1));
        assert!(expired.confirm(proof(HumanRole::User, &["ci-cd"])).is_err());
    }
}
