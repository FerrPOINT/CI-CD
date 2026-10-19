//! Central child custody for the existing task pipeline, never a local SAT or human identity.
use crate::{
    api::ApiError,
    config::{SdlcWorkspaceConfig, SecretsConfig},
    domain::sdlc_workspace::TaskWorkspaceIntent,
};
use pdlc_auth_core::AuthContext;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const GRANT_PREFIX: &str = "ci-cd:sdlc:task:v1:";

pub(crate) mod custody;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCredentialContext {
    pub version: u32,
    pub project_id: Uuid,
    pub intent: TaskWorkspaceIntent,
    pub run_id: Uuid,
    pub chat_id: Uuid,
    pub machine_subject: String,
    pub trigger_id: Uuid,
    pub variables_sha256: String,
}

pub(crate) fn digest(value: &impl Serialize) -> Result<String, ApiError> {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Object(v) => Value::Object(
                v.into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(v) => Value::Array(v.into_iter().map(canonical).collect()),
            v => v,
        }
    }
    let value = serde_json::to_value(value)
        .map_err(|_| ApiError::bad_request("invalid task credential context"))?;
    let bytes = serde_json::to_vec(&canonical(value))
        .map_err(|_| ApiError::bad_request("invalid task credential context"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl TaskCredentialContext {
    pub(crate) fn bind_trigger(
        &self,
        trigger_id: &str,
        git_ref: Option<&str>,
        variables: &impl Serialize,
    ) -> Result<(), ApiError> {
        if trigger_id != self.trigger_id.to_string()
            || git_ref != Some(self.intent.source_commit.as_str())
            || digest(variables)? != self.variables_sha256
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), ApiError> {
        self.intent.validate().map_err(ApiError::bad_request)?;
        if self.version != 1
            || [self.project_id, self.run_id, self.chat_id, self.trigger_id]
                .iter()
                .any(Uuid::is_nil)
            || !hex_digest(&self.variables_sha256)
            || self.machine_subject.is_empty()
            || self.machine_subject.len() > 256
            || self.machine_subject.trim() != self.machine_subject
            || self.machine_subject.chars().any(char::is_control)
        {
            return Err(ApiError::bad_request("invalid task credential context"));
        }
        Ok(())
    }

    pub(crate) fn grant(&self) -> Result<String, ApiError> {
        self.validate()?;
        Ok(format!("{GRANT_PREFIX}{}", digest(self)?))
    }

    pub(crate) fn native_matches(&self, value: &Value) -> bool {
        value["receipt"]["binding"]["run_id"] == json!(self.run_id)
            && value["receipt"]["binding"]["chat_id"] == json!(self.chat_id)
            && value["receipt"]["binding"]["machine_subject"] == json!(self.machine_subject)
    }
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn has_grant(ctx: &AuthContext) -> bool {
    ctx.scopes
        .iter()
        .any(|scope| scope.starts_with("ci-cd:sdlc:"))
}

pub(crate) fn configured_subject(subject: &str) -> bool {
    std::env::var("CICD_SDLC_DELEGATED_SUBJECT").is_ok_and(|value| value == subject)
}

// Deliberately no Debug/Serialize: the verified bearer must not enter logs or responses.
#[derive(Clone)]
pub(crate) struct Identity {
    pub subject: Uuid,
    pub grant_hash: String,
    token: String,
}

impl Identity {
    pub(crate) fn fingerprint(&self) -> String {
        crate::auth::hash_token(&self.token)
    }

    pub(crate) fn verified(
        ctx: &AuthContext,
        config: &SdlcWorkspaceConfig,
    ) -> Result<Self, ApiError> {
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
            || ctx.token.chars().any(char::is_whitespace)
        {
            return Err(ApiError::forbidden());
        }
        let grants: Vec<_> = ctx
            .scopes
            .iter()
            .filter(|s| s.starts_with("ci-cd:sdlc:"))
            .collect();
        let grant_hash = match grants.as_slice() {
            [grant] => grant
                .strip_prefix(GRANT_PREFIX)
                .filter(|s| hex_digest(s))
                .ok_or_else(ApiError::forbidden)?,
            _ => return Err(ApiError::forbidden()),
        };
        // No cross-service, admin, git or unrelated product scopes on this child.
        if !ctx.scopes.contains("ci-cd:read")
            || !ctx.scopes.contains("ci-cd:write")
            || ctx
                .scopes
                .iter()
                .any(|s| s != "ci-cd:read" && s != "ci-cd:write" && !s.starts_with(GRANT_PREFIX))
        {
            return Err(ApiError::forbidden());
        }
        Ok(Self {
            subject,
            grant_hash: grant_hash.into(),
            token: ctx.token.clone(),
        })
    }

    pub(crate) fn route(&self, method: &str, path: &str) -> Result<(), ApiError> {
        let parts: Vec<_> = path.split('/').collect();
        let id = |s: &str| Uuid::parse_str(s).is_ok_and(|id| !id.is_nil() && id.to_string() == s);
        let key = |s: &str| {
            // Existing CLI URL-encodes ':' in otherwise unreserved operation keys.
            // Accept only that canonical escape, never a general encoded path separator.
            if s.len() > 384 {
                return false;
            }
            let decoded = s.replace("%3A", ":");
            !decoded.is_empty()
                && decoded.len() <= 128
                && !matches!(decoded.as_str(), "." | "..")
                && decoded
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        };
        let allowed = match (method, parts.as_slice()) {
            ("POST", ["", "api", "v1", "projects", project, "pipelines"]) => id(project),
            ("GET", ["", "api", "v1", "pipelines", pipeline]) => id(pipeline),
            (
                "GET",
                [
                    "",
                    "api",
                    "v1",
                    "projects",
                    project,
                    "pipelines",
                    pipeline,
                    "task-workspace",
                ],
            ) => id(project) && id(pipeline),
            (
                "GET",
                [
                    "",
                    "api",
                    "v1",
                    "projects",
                    project,
                    "sdlc",
                    "workspace-operations",
                    operation,
                ],
            ) => id(project) && key(operation),
            (
                "GET",
                [
                    "",
                    "api",
                    "v1",
                    "projects",
                    project,
                    "sdlc",
                    "workspace-operations",
                    operation,
                    "candidate-evidence",
                ],
            )
            | (
                "POST",
                [
                    "",
                    "api",
                    "v1",
                    "projects",
                    project,
                    "sdlc",
                    "workspace-operations",
                    operation,
                    "delivery-operations",
                ],
            ) => id(project) && key(operation),
            (
                "GET",
                [
                    "",
                    "api",
                    "v1",
                    "projects",
                    project,
                    "sdlc",
                    "workspace-operations",
                    operation,
                    "delivery-operations",
                    delivery,
                ],
            ) => id(project) && key(operation) && key(delivery),
            _ => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }

    pub(crate) fn bind(
        &self,
        context: &TaskCredentialContext,
        project: Uuid,
        intent: &TaskWorkspaceIntent,
    ) -> Result<(), ApiError> {
        context.validate()?;
        if context.project_id != project
            || context.intent != *intent
            || digest(context)? != self.grant_hash
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }

    pub(crate) fn store(
        &self,
        context: TaskCredentialContext,
        secrets: &SecretsConfig,
    ) -> Result<StoredCredential, ApiError> {
        self.bind(&context, context.project_id, &context.intent)?;
        let fingerprint = crate::auth::hash_token(&self.token);
        let plain = serde_json::to_string(
            &json!({"subject":self.subject,"context":context,"token":self.token}),
        )
        .map_err(|_| ApiError::forbidden())?;
        let encrypted_child = crate::platform::encrypt_secret(&plain, secrets)
            .map_err(|_| ApiError::service_unavailable("task credential encryption unavailable"))?;
        Ok(StoredCredential {
            subject: self.subject,
            fingerprint,
            context_hash: digest(&context)?,
            context,
            encrypted_child,
        })
    }
}

// Serialized only into the private DB custody column; never a response or a Debug field.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredCredential {
    pub subject: Uuid,
    pub fingerprint: String,
    pub context_hash: String,
    pub context: TaskCredentialContext,
    encrypted_child: String,
}

pub(crate) fn trigger_fingerprint(
    legacy: String,
    credential: Option<&StoredCredential>,
) -> Result<String, ApiError> {
    match credential {
        Some(stored) => digest(
            &json!({"trigger":legacy,"child":stored.fingerprint,"context":stored.context_hash}),
        ),
        None => Ok(legacy),
    }
}

impl StoredCredential {
    pub(crate) fn from_value(value: &Value) -> Result<Self, ApiError> {
        serde_json::from_value(value.clone()).map_err(|_| ApiError::forbidden())
    }

    pub(crate) fn value(&self) -> Result<Value, ApiError> {
        serde_json::to_value(self).map_err(|_| ApiError::forbidden())
    }

    pub(crate) async fn verify(
        &self,
        config: &SdlcWorkspaceConfig,
        secrets: &SecretsConfig,
        project: Uuid,
        intent: &TaskWorkspaceIntent,
    ) -> Result<std::time::Instant, ApiError> {
        self.verify_with(config, secrets, project, intent, |token| async move {
            crate::central_auth::try_central_child(&token).await
        })
        .await
    }

    async fn verify_with<F, Fut>(
        &self,
        config: &SdlcWorkspaceConfig,
        secrets: &SecretsConfig,
        project: Uuid,
        intent: &TaskWorkspaceIntent,
        introspect: F,
    ) -> Result<std::time::Instant, ApiError>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<
                Output = Result<Option<(AuthContext, std::time::Instant)>, ApiError>,
            >,
    {
        let token = self.open(secrets)?;
        let (central, until) = introspect(token).await?.ok_or_else(ApiError::forbidden)?;
        let identity = Identity::verified(&central, config)?;
        if identity.subject != self.subject
            || crate::auth::hash_token(&identity.token) != self.fingerprint
        {
            return Err(ApiError::forbidden());
        }
        identity.bind(&self.context, project, intent)?;
        if std::time::Instant::now() >= until {
            return Err(ApiError::forbidden());
        }
        Ok(until)
    }

    fn open(&self, secrets: &SecretsConfig) -> Result<String, ApiError> {
        use base64::Engine;
        let parts: Vec<_> = self.encrypted_child.split(':').collect();
        if self.encrypted_child.len() > 65536
            || parts.len() != 3
            || parts[0] != "v1"
            || base64::engine::general_purpose::STANDARD
                .decode(parts[1])
                .map_or(true, |b| b.len() != 12)
        {
            return Err(ApiError::forbidden());
        }
        let plain = crate::platform::decrypt_secret(&self.encrypted_child, secrets)
            .map_err(|_| ApiError::forbidden())?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            subject: Uuid,
            context: TaskCredentialContext,
            token: String,
        }
        let envelope: Envelope = serde_json::from_str(&plain).map_err(|_| ApiError::forbidden())?;
        if envelope.subject != self.subject
            || envelope.context != self.context
            || digest(&self.context)? != self.context_hash
            || crate::auth::hash_token(&envelope.token) != self.fingerprint
        {
            return Err(ApiError::forbidden());
        }
        Ok(envelope.token)
    }
}

/// Server-side producer request for Base's EXISTING endpoint; never a CLI mint action.
pub fn delegation_request(
    context: &TaskCredentialContext,
    key: &str,
    ttl: i64,
) -> Result<Value, ApiError> {
    if !(1..=1800).contains(&ttl)
        || key.is_empty()
        || key.len() > 128
        || !key.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(ApiError::bad_request(
            "invalid task credential delegation bounds",
        ));
    }
    Ok(json!({"service":"ci-cd","label":"Forge task pipeline",
        "scopes":["ci-cd:read","ci-cd:write",context.grant()?],
        "idempotency_key":key,"expires_in_seconds":ttl}))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod devops_boundary_tests;
