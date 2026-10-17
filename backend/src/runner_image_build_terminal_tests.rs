//! Real PostgreSQL terminal-CAS tests only, not task admission or native build proof.
//! Registered by MAIN as a private runner child under cfg(test, feature = "integration").
use super::*;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{str::FromStr, time::Instant};

struct Fixture {
    pool: PgPool,
    worker: PgPool,
    admin: PgPool,
    schema: String,
    project: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("CICD_TEST_DATABASE_URL")
            .expect("explicit disposable CICD_TEST_DATABASE_URL required");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(
            parsed
                .path()
                .trim_start_matches('/')
                .starts_with("forge_test")
        );
        let schema = format!("it_image_terminal_{}", Uuid::new_v4().simple());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url).unwrap().options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "15000"),
        ]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.clone())
            .await
            .unwrap();
        crate::migrations_from_path(&Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"))
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
        let worker = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let project = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,$2,'https://example.invalid/terminal-fixture.git')")
            .bind(project).bind(format!("terminal-{project}")).execute(&pool).await.unwrap();
        Self {
            pool,
            worker,
            admin,
            schema,
            project,
        }
    }

    async fn cleanup(self) {
        self.worker.close().await;
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

struct Case {
    job: Uuid,
    lease: EmbeddedJobLease,
}

impl Case {
    async fn new(pool: &PgPool, project: Uuid) -> Self {
        let pipeline = Uuid::new_v4();
        let stage = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let lease = Uuid::new_v4();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','running')",
        )
        .bind(pipeline)
        .bind(project)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'package',0,'running')")
            .bind(stage).bind(pipeline).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status,started_at) VALUES($1,$2,'image','forge:backend-image-build','forge:image-build',0,'running',clock_timestamp())")
            .bind(job).bind(stage).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO execution_attempts(id,job_id,attempt_no,status,trigger,started_at) VALUES($1,$2,1,'running','runner',clock_timestamp())")
            .bind(attempt).bind(job).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO job_leases(id,job_id,attempt_id,runner_name,generation,lease_expires_at) VALUES($1,$2,$3,'embedded',1,clock_timestamp()+interval '1 hour')")
            .bind(lease).bind(job).bind(attempt).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO job_queue(id,job_id,attempt_id,pipeline_id,stage_id,state,lease_id,leased_at) VALUES($1,$2,$3,$4,$5,'leased',$6,clock_timestamp())")
            .bind(Uuid::new_v4()).bind(job).bind(attempt).bind(pipeline).bind(stage).bind(lease)
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        Self {
            job,
            lease: EmbeddedJobLease {
                id: lease,
                attempt_id: attempt,
                generation: 1,
            },
        }
    }

    async fn snapshot(&self, pool: &PgPool) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('job',to_jsonb(j),'attempt',to_jsonb(a),'lease',to_jsonb(l),'queue',to_jsonb(q)) FROM jobs j JOIN execution_attempts a ON a.job_id=j.id JOIN job_leases l ON l.attempt_id=a.id JOIN job_queue q ON q.lease_id=l.id WHERE j.id=$1 AND l.id=$2")
            .bind(self.job).bind(self.lease.id).fetch_one(pool).await.unwrap()
    }

    async fn finish(&self, pool: &PgPool, until: Instant) -> bool {
        finish_image_build_success(pool, self.job, &self.lease, Some(0), until)
            .await
            .unwrap()
    }

    fn spawn_finish(&self, worker: PgPool, until: Instant) -> tokio::task::JoinHandle<bool> {
        let job = self.job;
        let lease = EmbeddedJobLease {
            id: self.lease.id,
            attempt_id: self.lease.attempt_id,
            generation: self.lease.generation,
        };
        tokio::spawn(async move {
            finish_image_build_success(&worker, job, &lease, Some(0), until)
                .await
                .unwrap()
        })
    }
}

async fn worker_pid(worker: &PgPool) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(worker)
        .await
        .unwrap()
}

async fn wait_blocked(pool: &PgPool, pid: i32, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(pool)
                .await
                .unwrap();
            if blocked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("terminal writer must actually block at the selected SQL boundary");
}

#[tokio::test]
#[ignore = "requires disposable CICD_TEST_DATABASE_URL; no Git, Docker or native build"]
async fn image_build_terminal_pg_atomic_success_expiry_cancel_and_reconcile() {
    let fixture = Fixture::new().await;
    let pool = fixture.pool.clone();
    let worker = fixture.worker.clone();
    let project = fixture.project;
    // Catch assertion panics in the child so the owned schema is still removed.
    let assertions = tokio::spawn(async move {
        let live = || Instant::now() + Duration::from_secs(30);
        let success = Case::new(&pool, project).await;
        let (a, b) = tokio::join!(success.finish(&pool, live()), success.finish(&pool, live()));
        assert_ne!(a, b, "exactly one terminal CAS wins");
        let saved = success.snapshot(&pool).await;
        for row in ["job", "attempt"] {
            assert_eq!(saved[row]["status"], "success");
            assert!(saved[row]["finished_at"].is_string());
        }
        assert_eq!(saved["attempt"]["exit_code"], 0);
        assert_eq!(saved["lease"]["lease_status"], "completed");
        assert_eq!(saved["lease"]["terminal_status"], "success");
        assert!(saved["lease"]["completed_at"].is_string());
        assert_eq!(saved["queue"]["state"], "completed");
        assert!(saved["queue"]["completed_at"].is_string());
        assert!(!success.finish(&pool, live()).await);
        assert_eq!(
            success.snapshot(&pool).await,
            saved,
            "ACK replay is immutable"
        );

        // Force the finisher to wait at lease UPDATE, then expire that exact row.
        // The actual reconciler competes with its resumed UPDATE, not a mock writer.
        let race = Case::new(&pool, project).await;
        let pid = worker_pid(&worker).await;
        let mut lock = pool.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM job_leases WHERE id=$1 FOR UPDATE")
            .bind(race.lease.id)
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let finish = race.spawn_finish(worker.clone(), live());
        wait_blocked(&pool, pid, blocker).await;
        sqlx::query("UPDATE job_leases SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(race.lease.id).execute(&mut *lock).await.unwrap();
        lock.commit().await.unwrap();
        let (finished, reconciled) = tokio::join!(finish, reconcile_expired_leases(&pool));
        assert!(
            !finished.unwrap(),
            "expired original lease cannot acknowledge success"
        );
        assert_eq!(reconciled.unwrap(), 1);
        let state = race.snapshot(&pool).await;
        assert_eq!(state["job"]["status"], "failed");
        assert_eq!(state["attempt"]["status"], "failed");
        assert_eq!(state["lease"]["lease_status"], "expired");
        assert_eq!(state["lease"]["terminal_status"], "failed");
        assert_eq!(state["queue"]["state"], "completed");
        assert!(state["attempt"]["exit_code"].is_null());

        let expired = Case::new(&pool, project).await;
        sqlx::query("UPDATE job_leases SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(expired.lease.id).execute(&pool).await.unwrap();
        let before = expired.snapshot(&pool).await;
        assert!(!expired.finish(&pool, live()).await);
        assert_eq!(expired.snapshot(&pool).await, before);

        let cancelled = Case::new(&pool, project).await;
        sqlx::query("UPDATE job_leases SET cancel_requested_at=clock_timestamp() WHERE id=$1")
            .bind(cancelled.lease.id)
            .execute(&pool)
            .await
            .unwrap();
        let before = cancelled.snapshot(&pool).await;
        assert!(!cancelled.finish(&pool, live()).await);
        assert_eq!(cancelled.snapshot(&pool).await, before);

        let stale = Case::new(&pool, project).await;
        let before = stale.snapshot(&pool).await;
        let wrong = EmbeddedJobLease {
            id: stale.lease.id,
            attempt_id: stale.lease.attempt_id,
            generation: 2,
        };
        assert!(
            !finish_image_build_success(&pool, stale.job, &wrong, Some(0), live())
                .await
                .unwrap()
        );
        assert_eq!(stale.snapshot(&pool).await, before);
        assert!(!stale.finish(&pool, Instant::now()).await);
        assert_eq!(stale.snapshot(&pool).await, before);

        // A missing queue CAS is detected AFTER lease/attempt/job updates: all roll back.
        let no_queue = Case::new(&pool, project).await;
        sqlx::query("UPDATE job_queue SET state='canceled',completed_at=clock_timestamp() WHERE lease_id=$1")
            .bind(no_queue.lease.id).execute(&pool).await.unwrap();
        let before = no_queue.snapshot(&pool).await;
        assert!(!no_queue.finish(&pool, live()).await);
        assert_eq!(
            no_queue.snapshot(&pool).await,
            before,
            "zero-row late CAS rolls back earlier writes"
        );

        // Hold the final queue row: preceding writes must not be visible to other readers.
        let deadline = Case::new(&pool, project).await;
        let before = deadline.snapshot(&pool).await;
        let pid = worker_pid(&worker).await;
        let mut lock = pool.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM job_queue WHERE lease_id=$1 FOR UPDATE")
            .bind(deadline.lease.id)
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let finish = deadline.spawn_finish(worker.clone(), until);
        wait_blocked(&pool, pid, blocker).await;
        assert_eq!(
            deadline.snapshot(&pool).await,
            before,
            "uncommitted partial success is invisible"
        );
        tokio::time::sleep(
            until.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
        )
        .await;
        lock.commit().await.unwrap();
        assert!(
            !finish.await.unwrap(),
            "Tracker deadline is checked after the blocked terminal queue boundary"
        );
        assert_eq!(deadline.snapshot(&pool).await, before);

        let db_deadline = Case::new(&pool, project).await;
        let mut lock = pool.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM job_queue WHERE lease_id=$1 FOR UPDATE")
            .bind(db_deadline.lease.id)
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("UPDATE job_leases SET lease_expires_at=clock_timestamp()+interval '5 seconds' WHERE id=$1")
            .bind(db_deadline.lease.id).execute(&pool).await.unwrap();
        let before = db_deadline.snapshot(&pool).await;
        let pid = worker_pid(&worker).await;
        let finish = db_deadline.spawn_finish(worker.clone(), live());
        wait_blocked(&pool, pid, blocker).await;
        sqlx::query("SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM (lease_expires_at-clock_timestamp())),0)::double precision+0.01) FROM job_leases WHERE id=$1")
            .bind(db_deadline.lease.id).execute(&pool).await.unwrap();
        lock.commit().await.unwrap();
        assert!(
            !finish.await.unwrap(),
            "DB lease expiry is rechecked before commit"
        );
        assert_eq!(db_deadline.snapshot(&pool).await, before);
        let artifacts: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            artifacts, 0,
            "terminal helper does not fabricate descriptor artifacts"
        );
    });
    let result = assertions.await;
    fixture.cleanup().await;
    result.unwrap();
}
