//! Synchronization use cases and ports. External credentials never enter job secrets.
use cicd_domain::repository_sync::{
    Connection, ProtectedBranchUpdates, SaveConnection, SaveSchedule, Schedule, ScheduleState,
    Settings,
};
use std::future::Future;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("Ресурс не найден")]
    NotFound,
    #[error("Недостаточно прав")]
    Forbidden,
    #[error("{0}")]
    Unavailable(&'static str),
}
#[derive(Clone, Copy)]
pub struct Access {
    pub actor_id: Uuid,
    pub project_id: Option<Uuid>,
    pub admin: bool,
    pub machine: bool,
    pub writable: bool,
}
impl Access {
    pub fn can_manage_protected_branch_updates(self) -> bool {
        self.admin && !self.machine
    }
    pub fn can_manage_schedule(self) -> bool {
        self.admin && self.writable && !self.machine && self.project_id.is_none()
    }
}
pub trait Crypto: Sync {
    fn seal(&self, value: &str) -> Result<String, Error>;
    fn open(&self, value: &str) -> Result<String, Error>;
    fn fingerprint(&self, value: &[u8]) -> Result<String, Error>;
}
pub trait Store: Sync {
    fn save(
        &self,
        repository_id: Uuid,
        operation: Uuid,
        access: Access,
        input: &SaveConnection,
        encrypted: Option<&str>,
        fingerprint: &str,
    ) -> impl Future<Output = Result<Connection, Error>> + Send;
    fn credential(
        &self,
        repository_id: Uuid,
    ) -> impl Future<Output = Result<Option<(Settings, String)>, Error>> + Send;
}
pub async fn save(
    store: &impl Store,
    crypto: &impl Crypto,
    repository_id: Uuid,
    operation: Uuid,
    access: Access,
    mut input: SaveConnection,
    origins: &[String],
) -> Result<Connection, Error> {
    if repository_id.is_nil() || operation.is_nil() || input.expected_version < 0 {
        return Err(Error::Invalid("Некорректная команда сохранения".into()));
    }
    if !access.writable
        || (input.settings.protected_branch_updates == ProtectedBranchUpdates::SourceFastForward
            && !access.can_manage_protected_branch_updates())
    {
        return Err(Error::Forbidden);
    }
    input.settings.validate(origins).map_err(Error::Invalid)?;
    if input.token.as_ref().is_some_and(|token| {
        token.trim().is_empty() || token.contains(['\n', '\r']) || token.len() > 8192
    }) {
        return Err(Error::Invalid("Некорректный токен доступа".into()));
    }
    let fingerprint = crypto.fingerprint(
        &serde_json::to_vec(&input).map_err(|_| Error::Invalid("Некорректные настройки".into()))?,
    )?;
    let encrypted = input
        .token
        .as_deref()
        .map(|token| crypto.seal(token))
        .transpose()?;
    store
        .save(
            repository_id,
            operation,
            access,
            &input,
            encrypted.as_deref(),
            &fingerprint,
        )
        .await
}
pub async fn credential(
    store: &impl Store,
    crypto: &impl Crypto,
    repository_id: Uuid,
    settings: &Settings,
    token: Option<&str>,
) -> Result<String, Error> {
    if let Some(token) = token {
        if token.trim().is_empty() || token.contains(['\n', '\r']) || token.len() > 8192 {
            return Err(Error::Invalid("Некорректный токен доступа".into()));
        }
        return Ok(token.to_owned());
    }
    let (old, encrypted) = store
        .credential(repository_id)
        .await?
        .ok_or_else(|| Error::Invalid("Укажите токен доступа".into()))?;
    if !old.same_endpoint(settings) {
        return Err(Error::Invalid(
            "Для нового провайдера или инстанса нужен новый токен".into(),
        ));
    }
    crypto.open(&encrypted)
}

pub trait ScheduleStore: Sync {
    fn read_schedule(
        &self,
        defaults: Schedule,
    ) -> impl Future<Output = Result<ScheduleState, Error>> + Send;
    fn save_schedule(
        &self,
        operation: Uuid,
        access: Access,
        input: &SaveSchedule,
        defaults: Schedule,
    ) -> impl Future<Output = Result<ScheduleState, Error>> + Send;
}

pub async fn save_schedule(
    store: &impl ScheduleStore,
    operation: Uuid,
    access: Access,
    input: SaveSchedule,
    defaults: Schedule,
) -> Result<ScheduleState, Error> {
    if !access.can_manage_schedule() {
        return Err(Error::Forbidden);
    }
    if operation.is_nil() || input.expected_version < 0 {
        return Err(Error::Invalid(
            "Некорректная команда сохранения расписания".into(),
        ));
    }
    input.settings.validate().map_err(Error::Invalid)?;
    store
        .save_schedule(operation, access, &input, defaults)
        .await
}
