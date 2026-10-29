//! PostgreSQL synchronization journal and compare-and-swap operations.
use cicd_app::repository_sync::{Access, Error, Store};
use cicd_domain::repository_sync::{
    Connection, Direction, RefResult, SaveConnection, Settings, SyncRun,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Clone)]
pub struct PgSyncStore {
    pub pool: PgPool,
    pub worker_enabled: bool,
    pub interval_seconds: i64,
}
fn db(_: sqlx::Error) -> Error {
    Error::Unavailable("Хранилище синхронизаций недоступно")
}
fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, Error> {
    serde_json::from_value(value)
        .map_err(|_| Error::Unavailable("Повреждённый журнал синхронизации"))
}
fn json<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, Error> {
    serde_json::to_value(value).map_err(|_| Error::Unavailable("Ошибка сериализации синхронизации"))
}
const CONNECTION_SELECT: &str = r#"
SELECT jsonb_build_object('id',c.id,'repository_id',r.id,'repository_name',COALESCE(g.slug||'/'||r.slug,r.slug),
'namespace',CASE WHEN b.resource_id IS NOT NULL THEN jsonb_build_object('registry_instance_id',b.registry_instance_id,'namespace_id',b.namespace_id) ELSE NULL END,
'version',c.version,'settings',c.settings,'has_credential',true,
'writable',($3 AND r.ready AND COALESCE(b.state,'active')='active'),
'paused_reason',CASE WHEN NOT r.ready THEN 'storage_pending' WHEN b.state IS NOT NULL AND b.state<>'active' THEN 'namespace_read_only' ELSE NULL END,
'worker_enabled',$4,'next_check_at',c.next_check_at,'active_run',to_jsonb(a),'last_run',to_jsonb(l))
FROM repository_sync_connections c JOIN repository_catalog r ON r.id=c.repository_id
LEFT JOIN git_groups g ON g.id=r.group_id LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id
LEFT JOIN LATERAL (SELECT * FROM repository_sync_runs WHERE connection_id=c.id AND state IN ('queued','running','stopping','reconciling') ORDER BY created_at DESC LIMIT 1) a ON true
LEFT JOIN LATERAL (SELECT * FROM repository_sync_runs WHERE connection_id=c.id AND finished_at IS NOT NULL AND settings->>'provider'=c.settings->>'provider' AND settings->>'instance_url'=c.settings->>'instance_url' AND settings->>'repository_path'=c.settings->>'repository_path' ORDER BY created_at DESC,id DESC LIMIT 1) l ON true
WHERE ($1::uuid IS NULL OR r.id=$1) AND ($2::uuid IS NULL OR EXISTS(SELECT 1 FROM projects p WHERE p.id=$2 AND p.repository_id=r.id))
"#;
impl PgSyncStore {
    /// A detached connection releases its advisory slot on drop, never into the pool.
    pub async fn slot(&self, limit: usize) -> Result<Option<sqlx::PgConnection>, Error> {
        let mut connection = self.pool.acquire().await.map_err(db)?.detach();
        for slot in 0..limit {
            let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(93,$1)")
                .bind(slot as i32)
                .fetch_one(&mut connection)
                .await
                .map_err(db)?;
            if acquired {
                return Ok(Some(connection));
            }
        }
        Ok(None)
    }
    pub async fn authorize(&self, id: Uuid, access: Access, write: bool) -> Result<(), Error> {
        if access.machine && access.project_id.is_none() {
            return Err(Error::Forbidden);
        }
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM repository_catalog r WHERE r.id=$1 AND r.kind='hosted' AND ($2::uuid IS NULL OR EXISTS(SELECT 1 FROM projects p WHERE p.id=$2 AND p.repository_id=r.id)) AND ($3 OR $4 OR (NOT $5 AND EXISTS(SELECT 1 FROM forge_namespace_bindings b WHERE b.resource_id=r.group_id)) OR EXISTS(SELECT 1 FROM projects p JOIN project_memberships m ON m.project_id=p.id WHERE p.repository_id=r.id AND m.user_id=$6 AND (NOT $7 OR m.role IN ('developer','maintainer','admin')))))")
            .bind(id).bind(access.project_id).bind(access.admin).bind(access.machine && access.project_id.is_some()).bind(access.machine).bind(access.actor_id).bind(write).fetch_one(&self.pool).await.map_err(db)?;
        if !allowed || (write && !access.writable) {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
    pub async fn connection(&self, id: Uuid, access: Access) -> Result<Option<Connection>, Error> {
        self.connection_at(id, id, access).await
    }
    pub async fn connection_at(
        &self,
        id: Uuid,
        connection: Uuid,
        access: Access,
    ) -> Result<Option<Connection>, Error> {
        let value: Option<serde_json::Value> =
            sqlx::query_scalar(&format!("{CONNECTION_SELECT} AND c.id=$5"))
                .bind(Some(id))
                .bind(access.project_id)
                .bind(access.writable)
                .bind(self.worker_enabled)
                .bind(connection)
                .fetch_optional(&self.pool)
                .await
                .map_err(db)?;
        value.map(decode).transpose()
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn list(
        &self,
        access: Access,
        search: &str,
        provider: &str,
        state: &str,
        group: Option<Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Connection>, i64), Error> {
        self.list_at(access, search, provider, state, group, limit, offset, None)
            .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn list_at(
        &self,
        access: Access,
        search: &str,
        provider: &str,
        state: &str,
        group: Option<Uuid>,
        limit: i64,
        offset: i64,
        repository: Option<Uuid>,
    ) -> Result<(Vec<Connection>, i64), Error> {
        if access.machine && access.project_id.is_none() {
            return Err(Error::Forbidden);
        }
        let predicate = r#" AND ($5='' OR position(lower($5) in lower(COALESCE(g.slug||'/'||r.slug,r.slug)||' '||(c.settings->>'instance_url')||'/'||(c.settings->>'repository_path')))>0)
            AND ($6='' OR c.settings->>'provider'=$6) AND ($7::uuid IS NULL OR r.group_id=$7)
            AND ($8 OR $9 OR (NOT $10 AND b.resource_id IS NOT NULL) OR EXISTS(SELECT 1 FROM projects p JOIN project_memberships m ON m.project_id=p.id WHERE p.repository_id=r.id AND m.user_id=$11))
            AND ($12='' OR ($12='disabled' AND c.settings->>'direction'='disabled') OR ($12='enabled' AND c.settings->>'direction'<>'disabled') OR COALESCE(a.state,l.state)=$12) "#;
        let query = format!("{CONNECTION_SELECT}{predicate}");
        let count_query = format!("SELECT count(*) FROM ({query}) counted");
        macro_rules! bound {
            ($q:expr) => {
                sqlx::query_scalar($q)
                    .bind(repository)
                    .bind(access.project_id)
                    .bind(access.writable)
                    .bind(self.worker_enabled)
                    .bind(search)
                    .bind(provider)
                    .bind(group)
                    .bind(access.admin)
                    .bind(access.machine && access.project_id.is_some())
                    .bind(access.machine)
                    .bind(access.actor_id)
                    .bind(state)
            };
        }
        let total: i64 = bound!(&count_query)
            .fetch_one(&self.pool)
            .await
            .map_err(db)?;
        let items: Vec<serde_json::Value> = bound!(&format!(
            "{query} ORDER BY g.slug NULLS LAST,r.slug,r.id,c.id LIMIT $13 OFFSET $14"
        ))
        .bind(limit.clamp(1, 100))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        Ok((
            items.into_iter().map(decode).collect::<Result<_, _>>()?,
            total,
        ))
    }
    pub async fn run(&self, repository: Uuid, id: Uuid) -> Result<SyncRun, Error> {
        let value: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT to_jsonb(r) FROM repository_sync_runs r WHERE repository_id=$1 AND id=$2",
        )
        .bind(repository)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db)?;
        decode(value.ok_or(Error::NotFound)?)
    }
    pub async fn runs(
        &self,
        repository: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<SyncRun>, i64), Error> {
        self.runs_at(repository, repository, limit, offset).await
    }
    pub async fn runs_at(
        &self,
        repository: Uuid,
        connection: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<SyncRun>, i64), Error> {
        let total = sqlx::query_scalar(
            "SELECT count(*) FROM repository_sync_runs WHERE repository_id=$1 AND connection_id=$2",
        )
        .bind(repository)
        .bind(connection)
        .fetch_one(&self.pool)
        .await
        .map_err(db)?;
        let values:Vec<serde_json::Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM repository_sync_runs r WHERE repository_id=$1 AND connection_id=$4 ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3").bind(repository).bind(limit.clamp(1,100)).bind(offset.max(0)).bind(connection).fetch_all(&self.pool).await.map_err(db)?;
        Ok((
            values.into_iter().map(decode).collect::<Result<_, _>>()?,
            total,
        ))
    }
    pub async fn refs(
        &self,
        run: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<RefResult>, i64), Error> {
        let total = sqlx::query_scalar("SELECT count(*) FROM repository_sync_refs WHERE run_id=$1")
            .bind(run)
            .fetch_one(&self.pool)
            .await
            .map_err(db)?;
        let values:Vec<serde_json::Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM repository_sync_refs r WHERE run_id=$1 ORDER BY ref_name LIMIT $2 OFFSET $3").bind(run).bind(limit.clamp(1,1000)).bind(offset.max(0)).fetch_all(&self.pool).await.map_err(db)?;
        Ok((
            values.into_iter().map(decode).collect::<Result<_, _>>()?,
            total,
        ))
    }
    pub async fn enqueue(
        &self,
        repository: Uuid,
        source: &str,
        operation: Option<(Uuid, Access)>,
    ) -> Result<SyncRun, Error> {
        self.enqueue_at(repository, repository, source, operation)
            .await
    }
    pub async fn enqueue_at(
        &self,
        repository: Uuid,
        connection: Uuid,
        source: &str,
        operation: Option<(Uuid, Access)>,
    ) -> Result<SyncRun, Error> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        if let Some((key, access)) = operation {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,93))")
                .bind(key.to_string())
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            if let Some(row)=sqlx::query("SELECT connection_id,repository_id,actor_id,kind,readback FROM repository_sync_operations WHERE operation_id=$1").bind(key).fetch_optional(&mut *tx).await.map_err(db)? {
                if row.get::<Uuid,_>("connection_id")!=connection || row.get::<Uuid,_>("repository_id")!=repository || row.get::<Uuid,_>("actor_id")!=access.actor_id || row.get::<String,_>("kind")!="run" {return Err(Error::Conflict("idempotency_conflict"));}
                return decode(row.get("readback"));
            }
        }
        let row=sqlx::query("SELECT version,settings FROM repository_sync_connections WHERE repository_id=$1 AND id=$2 FOR UPDATE").bind(repository).bind(connection).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Error::NotFound)?;
        let settings: Settings = decode(row.get("settings"))?;
        if settings.direction == Direction::Disabled {
            return Err(Error::Conflict("sync_disabled"));
        }
        let old:Option<serde_json::Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM repository_sync_runs r WHERE repository_id=$1 AND connection_id=$2 AND state IN ('queued','running','stopping','reconciling')").bind(repository).bind(connection).fetch_optional(&mut *tx).await.map_err(db)?;
        let value = if let Some(old) = old {
            old
        } else {
            sqlx::query_scalar("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state,connection_id) VALUES($1,$2,$3,$4,$5,'queued',$6) RETURNING to_jsonb(repository_sync_runs)")
                .bind(Uuid::new_v4()).bind(repository).bind(row.get::<i64,_>("version")).bind(json(&settings)?).bind(source).bind(connection).fetch_one(&mut *tx).await.map_err(db)?
        };
        if let Some((key, access)) = operation {
            sqlx::query("INSERT INTO repository_sync_operations(operation_id,repository_id,actor_id,kind,fingerprint,readback,connection_id) VALUES($1,$2,$3,'run','manual',$4,$5)").bind(key).bind(repository).bind(access.actor_id).bind(&value).bind(connection).execute(&mut *tx).await.map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        decode(value)
    }
    pub async fn signal(&self, repository: Uuid) -> Result<(), Error> {
        sqlx::query("UPDATE repository_sync_connections SET dirty=true WHERE repository_id=$1 AND (settings->>'automatic')::boolean AND settings->>'direction'<>'disabled'").bind(repository).execute(&self.pool).await.map_err(db)?;
        Ok(())
    }
    pub async fn schedule(&self) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        let rows=sqlx::query("SELECT c.id,c.repository_id,c.settings,c.version,c.dirty FROM repository_sync_connections c JOIN repository_catalog r ON r.id=c.repository_id LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE (c.settings->>'automatic')::boolean AND c.settings->>'direction'<>'disabled' AND (c.dirty OR c.next_check_at<=now()) AND r.ready AND COALESCE(b.state,'active')='active' AND NOT EXISTS(SELECT 1 FROM repository_sync_runs run WHERE run.connection_id=c.id AND run.state IN ('queued','running','stopping','reconciling')) ORDER BY c.next_check_at NULLS FIRST LIMIT 100 FOR UPDATE OF c SKIP LOCKED").fetch_all(&mut *tx).await.map_err(db)?;
        for row in rows {
            let id: Uuid = row.get("repository_id");
            let connection: Uuid = row.get("id");
            sqlx::query("INSERT INTO repository_sync_runs(id,repository_id,connection_version,settings,source,state,connection_id) VALUES($1,$2,$3,$4,$5,'queued',$6)").bind(Uuid::new_v4()).bind(id).bind(row.get::<i64,_>("version")).bind(row.get::<serde_json::Value,_>("settings")).bind(if row.get::<bool,_>("dirty"){"git_change"}else{"schedule"}).bind(connection).execute(&mut *tx).await.map_err(db)?;
            sqlx::query("UPDATE repository_sync_connections SET dirty=false,next_check_at=now()+make_interval(secs=>$2::double precision) WHERE id=$1").bind(connection).bind(self.interval_seconds as f64).execute(&mut *tx).await.map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        Ok(())
    }
    pub async fn candidates(&self) -> Result<Vec<(Uuid, Uuid, String)>, Error> {
        // A queued sibling must never hide the repository's unresolved writer:
        // supervisor deduplicates repositories, while claim correctly refuses it.
        // Deduplicate before LIMIT so a large connection backlog cannot consume
        // the whole batch and starve independent repositories.
        sqlx::query_as(r#"
            WITH candidates AS (
                SELECT DISTINCT ON (run.repository_id) run.id,run.repository_id,r.storage_name,run.created_at
                FROM repository_sync_runs run JOIN repository_catalog r ON r.id=run.repository_id
                LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id
                WHERE (run.state='queued' AND r.ready AND COALESCE(b.state,'active')='active'
                    AND NOT EXISTS(SELECT 1 FROM repository_sync_runs writer
                        WHERE writer.repository_id=run.repository_id
                        AND writer.state IN ('running','stopping','reconciling')))
                    OR (run.state IN ('running','stopping','reconciling') AND run.lease_until<now())
                ORDER BY run.repository_id,run.created_at,run.id
            )
            SELECT id,repository_id,storage_name FROM candidates ORDER BY created_at,id LIMIT 100
        "#).fetch_all(&self.pool).await.map_err(db)
    }
    pub async fn claim(&self, id: Uuid, owner: Uuid) -> Result<Option<(SyncRun, i64)>, Error> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        let repository: Option<Uuid> =
            sqlx::query_scalar("SELECT repository_id FROM repository_sync_runs WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        let Some(repository) = repository else {
            return Ok(None);
        };
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,94))")
            .bind(repository.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let row=sqlx::query("UPDATE repository_sync_runs SET state=CASE WHEN state='queued' THEN 'running' ELSE 'reconciling' END,owner_id=$2,generation=generation+1,lease_until=now()+interval '60 seconds',started_at=COALESCE(started_at,now()) WHERE id=$1 AND NOT EXISTS(SELECT 1 FROM repository_sync_runs other WHERE other.repository_id=repository_sync_runs.repository_id AND other.id<>$1 AND other.state IN ('running','stopping','reconciling')) AND (state='queued' OR (state IN ('running','stopping','reconciling') AND lease_until<now())) RETURNING to_jsonb(repository_sync_runs) AS value,generation").bind(id).bind(owner).fetch_optional(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)?;
        row.map(|row| Ok((decode(row.get("value"))?, row.get("generation"))))
            .transpose()
    }
    pub async fn heartbeat(&self, run: Uuid, owner: Uuid, generation: i64) -> Result<bool, Error> {
        let value:Option<bool>=sqlx::query_scalar("UPDATE repository_sync_runs SET lease_until=now()+interval '60 seconds' WHERE id=$1 AND owner_id=$2 AND generation=$3 AND state IN ('running','stopping','reconciling') RETURNING cancel_requested").bind(run).bind(owner).bind(generation).fetch_optional(&self.pool).await.map_err(db)?;
        value.ok_or(Error::Conflict("sync_fence_lost"))
    }
    pub async fn allowed_to_apply(
        &self,
        run: &SyncRun,
        owner: Uuid,
        generation: i64,
    ) -> Result<bool, Error> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM repository_sync_runs run JOIN repository_sync_connections c ON c.id=run.connection_id AND c.repository_id=run.repository_id JOIN repository_catalog r ON r.id=c.repository_id LEFT JOIN forge_namespace_bindings b ON b.resource_id=r.group_id WHERE run.id=$1 AND run.owner_id=$2 AND run.generation=$3 AND run.lease_until>now() AND NOT run.cancel_requested AND c.version=run.connection_version AND c.settings->>'direction'<>'disabled' AND COALESCE(b.state,'active')='active')").bind(run.id).bind(owner).bind(generation).fetch_one(&self.pool).await.map_err(db)
    }
    pub async fn record_plan(&self, run: Uuid, refs: &[RefResult]) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        for r in refs {
            sqlx::query("INSERT INTO repository_sync_refs(run_id,ref_name,direction,old_sha,new_sha,state,reason,event_delivered) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING").bind(run).bind(&r.ref_name).bind(&r.direction).bind(&r.old_sha).bind(&r.new_sha).bind(&r.state).bind(&r.reason).bind(r.event_delivered).execute(&mut *tx).await.map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        Ok(())
    }
    pub async fn ref_state(
        &self,
        run: Uuid,
        reference: &str,
        state: &str,
        reason: Option<&str>,
        owner: Uuid,
        generation: i64,
    ) -> Result<(), Error> {
        let rows=sqlx::query("UPDATE repository_sync_refs SET state=$3,reason=$4 WHERE run_id=$1 AND ref_name=$2 AND EXISTS(SELECT 1 FROM repository_sync_runs WHERE id=$1 AND owner_id=$5 AND generation=$6 AND lease_until>now())").bind(run).bind(reference).bind(state).bind(reason).bind(owner).bind(generation).execute(&self.pool).await.map_err(db)?.rows_affected();
        if rows == 0 {
            return Err(Error::Conflict("sync_fence_lost"));
        }
        Ok(())
    }
    pub async fn finish(
        &self,
        run: Uuid,
        owner: Uuid,
        generation: i64,
        state: &str,
        message: Option<&str>,
    ) -> Result<(), Error> {
        let terminal = !matches!(state, "reconciling" | "stopping");
        sqlx::query("UPDATE repository_sync_runs SET state=$4,message=$5,finished_at=CASE WHEN $6 THEN now() ELSE NULL END,lease_until=now()+interval '30 seconds' WHERE id=$1 AND owner_id=$2 AND generation=$3").bind(run).bind(owner).bind(generation).bind(state).bind(message).bind(terminal).execute(&self.pool).await.map_err(db)?;
        Ok(())
    }
    pub async fn protected_branches(&self, repository: Uuid) -> Result<Vec<String>, Error> {
        sqlx::query_scalar(
            "SELECT DISTINCT unnest(array_prepend(default_branch, protected_branches)) \
             FROM projects WHERE repository_id=$1",
        )
        .bind(repository)
        .fetch_all(&self.pool)
        .await
        .map_err(db)
    }
    pub async fn pending_events(&self) -> Result<Vec<(Uuid, Uuid, RefResult)>, Error> {
        let rows=sqlx::query("SELECT r.run_id,run.repository_id,to_jsonb(r) AS value FROM repository_sync_refs r JOIN repository_sync_runs run ON run.id=r.run_id WHERE r.direction='pull' AND r.state='applied' AND NOT r.event_delivered AND r.event_next_attempt_at<=now() ORDER BY run.created_at LIMIT 100").fetch_all(&self.pool).await.map_err(db)?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.get("run_id"),
                    row.get("repository_id"),
                    decode(row.get("value"))?,
                ))
            })
            .collect()
    }
    pub async fn event_delivered(&self, run: Uuid, reference: &str) -> Result<(), Error> {
        sqlx::query(
            "UPDATE repository_sync_refs SET event_delivered=true WHERE run_id=$1 AND ref_name=$2",
        )
        .bind(run)
        .bind(reference)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        Ok(())
    }
    pub async fn event_retry(&self, run: Uuid, reference: &str) -> Result<(), Error> {
        sqlx::query("UPDATE repository_sync_refs SET event_next_attempt_at=now()+interval '60 seconds' WHERE run_id=$1 AND ref_name=$2 AND NOT event_delivered").bind(run).bind(reference).execute(&self.pool).await.map_err(db)?;
        Ok(())
    }
    pub async fn event_non_commit_tag(&self, run: Uuid, reference: &str) -> Result<(), Error> {
        sqlx::query("UPDATE repository_sync_refs SET event_delivered=true,reason='ci_non_commit_tag' WHERE run_id=$1 AND ref_name=$2 AND state='applied'").bind(run).bind(reference).execute(&self.pool).await.map_err(db)?;
        Ok(())
    }
}
pub struct ConnectionStore<'a> {
    store: &'a PgSyncStore,
    id: Uuid,
}
impl std::ops::Deref for ConnectionStore<'_> {
    type Target = PgSyncStore;
    fn deref(&self) -> &Self::Target {
        self.store
    }
}
impl PgSyncStore {
    pub fn scoped(&self, connection: Uuid) -> ConnectionStore<'_> {
        ConnectionStore {
            store: self,
            id: connection,
        }
    }
}
impl Store for PgSyncStore {
    async fn credential(&self, repository: Uuid) -> Result<Option<(Settings, String)>, Error> {
        self.scoped(repository).credential(repository).await
    }
    async fn save(
        &self,
        repository: Uuid,
        operation: Uuid,
        access: Access,
        input: &SaveConnection,
        encrypted: Option<&str>,
        fingerprint: &str,
    ) -> Result<Connection, Error> {
        self.scoped(repository)
            .save(repository, operation, access, input, encrypted, fingerprint)
            .await
    }
}
impl Store for ConnectionStore<'_> {
    async fn credential(&self, repository: Uuid) -> Result<Option<(Settings, String)>, Error> {
        let row=sqlx::query("SELECT settings,encrypted_credential FROM repository_sync_connections WHERE repository_id=$1 AND id=$2").bind(repository).bind(self.id).fetch_optional(&self.pool).await.map_err(db)?;
        row.map(|row| {
            Ok((
                decode(row.get("settings"))?,
                row.get("encrypted_credential"),
            ))
        })
        .transpose()
    }
    async fn save(
        &self,
        repository: Uuid,
        operation: Uuid,
        access: Access,
        input: &SaveConnection,
        encrypted: Option<&str>,
        fingerprint: &str,
    ) -> Result<Connection, Error> {
        if self.id.is_nil() {
            return Err(Error::Invalid("Некорректный UUID подключения".into()));
        }
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,93))")
            .bind(operation.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if let Some(row)=sqlx::query("SELECT connection_id,repository_id,actor_id,kind,fingerprint,readback FROM repository_sync_operations WHERE operation_id=$1").bind(operation).fetch_optional(&mut *tx).await.map_err(db)? {
            if row.get::<Uuid,_>("connection_id")!=self.id || row.get::<Uuid,_>("repository_id")!=repository || row.get::<Uuid,_>("actor_id")!=access.actor_id || row.get::<String,_>("kind")!="save" || row.get::<String,_>("fingerprint")!=fingerprint {return Err(Error::Conflict("idempotency_conflict"));}
            return decode(row.get("readback"));
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,94))")
            .bind(repository.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,95))")
            .bind(self.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let row=sqlx::query("SELECT version,settings,encrypted_credential FROM repository_sync_connections WHERE repository_id=$1 AND id=$2 FOR UPDATE").bind(repository).bind(self.id).fetch_optional(&mut *tx).await.map_err(db)?;
        let foreign: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM repository_sync_connections WHERE id=$1 AND repository_id<>$2)").bind(self.id).bind(repository).fetch_one(&mut *tx).await.map_err(db)?;
        if foreign {
            return Err(Error::NotFound);
        }
        let version = row
            .as_ref()
            .map(|r| r.get::<i64, _>("version"))
            .unwrap_or(0);
        if input.expected_version != version {
            return Err(Error::Conflict("sync_version_conflict"));
        }
        let old = row
            .as_ref()
            .map(|r| decode::<Settings>(r.get("settings")))
            .transpose()?;
        let credential = match encrypted {
            Some(value) => value.to_owned(),
            None => {
                if old
                    .as_ref()
                    .is_none_or(|old| !old.same_endpoint(&input.settings))
                {
                    return Err(Error::Invalid("Укажите новый токен доступа".into()));
                }
                row.as_ref()
                    .ok_or(Error::NotFound)?
                    .get("encrypted_credential")
            }
        };
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM repository_sync_runs WHERE repository_id=$1 AND connection_id=$2 AND state IN ('queued','running','stopping','reconciling'))").bind(repository).bind(self.id).fetch_one(&mut *tx).await.map_err(db)?;
        if active {
            let mut disabling = old.ok_or(Error::NotFound)?;
            disabling.direction = Direction::Disabled;
            if input.settings != disabling || encrypted.is_some() {
                return Err(Error::Conflict("sync_running"));
            }
        }
        sqlx::query("INSERT INTO repository_sync_connections(repository_id,id,version,settings,encrypted_credential,next_check_at) VALUES($1,$6,1,$2,$3,CASE WHEN $4 THEN now()+make_interval(secs=>$5::double precision) ELSE NULL END) ON CONFLICT(id) DO UPDATE SET version=repository_sync_connections.version+1,settings=EXCLUDED.settings,encrypted_credential=EXCLUDED.encrypted_credential,next_check_at=EXCLUDED.next_check_at,dirty=false,updated_at=now()")
            .bind(repository).bind(json(&input.settings)?).bind(credential).bind(input.settings.automatic && input.settings.direction!=Direction::Disabled).bind(self.interval_seconds as f64).bind(self.id).execute(&mut *tx).await.map_err(db)?;
        if input.settings.direction == Direction::Disabled {
            sqlx::query("UPDATE repository_sync_runs SET cancel_requested=true,state=CASE WHEN state='queued' THEN 'canceled' ELSE 'stopping' END,finished_at=CASE WHEN state='queued' THEN now() ELSE finished_at END WHERE repository_id=$1 AND connection_id=$2 AND state IN ('queued','running','stopping','reconciling')").bind(repository).bind(self.id).execute(&mut *tx).await.map_err(db)?;
        }
        let value: serde_json::Value =
            sqlx::query_scalar(&format!("{CONNECTION_SELECT} AND c.id=$5"))
                .bind(Some(repository))
                .bind(access.project_id)
                .bind(access.writable)
                .bind(self.worker_enabled)
                .bind(self.id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        sqlx::query("INSERT INTO repository_sync_operations(operation_id,repository_id,actor_id,kind,fingerprint,readback,connection_id) VALUES($1,$2,$3,'save',$4,$5,$6)").bind(operation).bind(repository).bind(access.actor_id).bind(fingerprint).bind(&value).bind(self.id).execute(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)?;
        decode(value)
    }
}
