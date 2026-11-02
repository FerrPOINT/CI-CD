//! Global sync scheduling policy. The audit owner retains identity attribution.
use crate::repository_sync::PgSyncStore;
use cicd_app::repository_sync::{Access, Error, ScheduleStore};
use cicd_domain::repository_sync::{SaveSchedule, Schedule, ScheduleState};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

fn db(_: sqlx::Error) -> Error {
    Error::Unavailable("Расписание синхронизации недоступно")
}

pub(crate) async fn schedule_state(
    connection: &mut PgConnection,
    defaults: Schedule,
) -> Result<ScheduleState, Error> {
    let row = sqlx::query("SELECT version,interval_seconds,on_change FROM repository_sync_schedule ORDER BY version DESC LIMIT 1")
        .fetch_optional(connection).await.map_err(db)?;
    Ok(row
        .map(|row| ScheduleState {
            version: row.get("version"),
            settings: Schedule {
                interval_seconds: row.get("interval_seconds"),
                on_change: row.get("on_change"),
            },
        })
        .unwrap_or(ScheduleState {
            version: 0,
            settings: defaults,
        }))
}

impl ScheduleStore for PgSyncStore {
    async fn read_schedule(&self, defaults: Schedule) -> Result<ScheduleState, Error> {
        let mut connection = self.pool.acquire().await.map_err(db)?;
        schedule_state(&mut connection, defaults).await
    }
    async fn save_schedule(
        &self,
        operation: Uuid,
        access: Access,
        input: &SaveSchedule,
        defaults: Schedule,
    ) -> Result<ScheduleState, Error> {
        if !access.can_manage_schedule() {
            return Err(Error::Forbidden);
        }
        input.settings.validate().map_err(Error::Invalid)?;
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('repository_sync_schedule',96))",
        )
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        // Revalidate the confirmed role under a lock after any policy wait.
        let actor = sqlx::query("SELECT role,enabled FROM users WHERE id=$1 FOR SHARE")
            .bind(access.actor_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
        if !actor.is_some_and(|row| {
            row.get::<String, _>("role") == "admin" && row.get::<bool, _>("enabled")
        }) {
            return Err(Error::Forbidden);
        }
        if let Some(row) = sqlx::query("SELECT p.version,p.interval_seconds,p.on_change,a.actor FROM repository_sync_schedule p LEFT JOIN audit_log a ON a.resource_id=p.id AND a.resource_type='repository_sync_schedule' AND a.action='repository_sync.schedule_updated' WHERE p.id=$1")
            .bind(operation).fetch_optional(&mut *tx).await.map_err(db)? {
            if Some(row.get::<i64,_>("version")) != input.expected_version.checked_add(1)
                || row.get::<i64,_>("interval_seconds") != input.settings.interval_seconds
                || row.get::<bool,_>("on_change") != input.settings.on_change
                || row.get::<Option<String>,_>("actor").as_deref() != Some(access.actor_id.to_string().as_str()) {
                return Err(Error::Conflict("idempotency_conflict"));
            }
            let original = ScheduleState { version: row.get("version"), settings: input.settings };
            tx.commit().await.map_err(db)?;
            return Ok(original);
        }
        let current = schedule_state(&mut tx, defaults).await?;
        if current.version != input.expected_version {
            return Err(Error::Conflict("sync_schedule_version_conflict"));
        }
        let version = current
            .version
            .checked_add(1)
            .ok_or(Error::Conflict("sync_schedule_version_exhausted"))?;
        sqlx::query("INSERT INTO repository_sync_schedule(id,version,interval_seconds,on_change) VALUES($1,$2,$3,$4)")
            .bind(operation).bind(version).bind(input.settings.interval_seconds).bind(input.settings.on_change)
            .execute(&mut *tx).await.map_err(db)?;
        sqlx::query("INSERT INTO audit_log(action,resource_type,resource_id,actor,identity_refs) VALUES('repository_sync.schedule_updated','repository_sync_schedule',$1,$2,$3)")
            .bind(operation).bind(access.actor_id.to_string())
            .bind(serde_json::json!([{"kind":"local_principal_id","alias":access.actor_id}]))
            .execute(&mut *tx).await.map_err(db)?;
        // Keep manual runs independent. A period change reuses each connection's last automatic clock.
        sqlx::query("UPDATE repository_sync_connections c SET next_check_at=COALESCE((SELECT max(r.created_at) FROM repository_sync_runs r WHERE r.connection_id=c.id AND r.source IN ('schedule','git_change'))+make_interval(secs=>$1::double precision),now()) WHERE (c.settings->>'automatic')::boolean")
            .bind(input.settings.interval_seconds as f64).execute(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(ScheduleState {
            version,
            settings: input.settings,
        })
    }
}
