//! Privileged isolated PostgreSQL coordinator. No production/SDLC write admission.
use super::*;
use sqlx::{Connection, Row};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub const GUARD_KEY: i64 = 0x464f524745504744;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schema {
    pub version: i64,
    pub catalog_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Migration {
    pub version: i64,
    pub description: String,
    pub sql: String,
    pub sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    pub schema: String,
    pub data_protocol: String,
    pub image_id: String,
    pub source_commit: String,
    pub source_schema: Schema,
    pub target_schema: Schema,
    pub readable_schema_versions: Vec<i64>,
    pub migrations: Vec<Migration>,
}

impl Descriptor {
    fn validate(&self, commit: &str) -> anyhow::Result<()> {
        ensure!(
            self.schema == "forge/postgres-candidate/v1"
                && self.data_protocol == "isolated_postgres_shadow_v1",
            "unsupported PostgreSQL protocol"
        );
        ensure!(
            self.image_id.strip_prefix("sha256:").is_some_and(digest)
                && self.source_commit == commit,
            "exact image/source required"
        );
        ensure!(
            self.source_schema.version > 0
                && self.target_schema.version >= self.source_schema.version
                && self.target_schema.version <= self.source_schema.version + 1
                && digest(&self.source_schema.catalog_sha256)
                && digest(&self.target_schema.catalog_sha256)
                && self
                    .readable_schema_versions
                    .contains(&self.target_schema.version)
                && self.readable_schema_versions.len() <= 16,
            "unknown schema compatibility"
        );
        ensure!(
            !self.migrations.is_empty() && self.migrations.len() <= 16,
            "bounded complete migration catalog required"
        );
        let mut previous = 0;
        for migration in &self.migrations {
            ensure!(
                migration.version > previous
                    && migration.version <= self.target_schema.version
                    && migration.sql.len() <= 16 * 1024
                    && !migration.sql.is_empty()
                    && migration
                        .description
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b == b'_')
                    && !migration.description.is_empty()
                    && migration.description.len() <= 64
                    && sha(migration.sql.as_bytes()) == migration.sha256,
                "immutable migration catalog mismatch"
            );
            // Only an additive nullable scalar column is supported for pending SQL.
            // Historical bytes are verified against SQLx before any migration effect.
            if migration.version > self.source_schema.version {
                let words = migration
                    .sql
                    .trim()
                    .trim_end_matches(';')
                    .split_whitespace()
                    .collect::<Vec<_>>();
                let identifier = |word: &str| {
                    !word.is_empty()
                        && word.len() <= 63
                        && word
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                };
                ensure!(
                    words.len() == 8
                        && words[..2] == ["ALTER", "TABLE"]
                        && words[2].strip_prefix("public.").is_some_and(identifier)
                        && words[3..5] == ["ADD", "COLUMN"]
                        && identifier(words[5])
                        && matches!(words[6], "text" | "integer" | "bigint" | "boolean")
                        && words[7] == "NULL",
                    "only reviewed additive nullable-column migration supported"
                );
            }
            previous = migration.version;
        }
        ensure!(
            previous == self.target_schema.version,
            "target migration catalog incomplete"
        );
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub project_name: String,
    pub daemon_id: String,
    pub system_identifier: String,
    pub controller_ip: String,
    pub hba_file: PathBuf,
    pub compose_file: PathBuf,
    pub compose_root: PathBuf,
    pub postgres_image_id: String,
    pub postgres_volume_name: String,
    pub network_name: String,
    pub volume_name: String,
    pub volume_root: PathBuf,
    pub initial_database: String,
    pub base_root: PathBuf,
    pub python_bin: PathBuf,
    pub executor: PathBuf,
    pub migration_bin: PathBuf,
    pub lease_seconds: u64,
    pub checks: DeliveryPolicy,
}

pub async fn local_command(
    state: Arc<AppState>,
    project: Uuid,
    token: &str,
    command: DeliveryCommand,
    reconcile: bool,
    read_only: bool,
) -> anyhow::Result<serde_json::Value> {
    ensure!(
        cfg!(target_os = "linux"),
        "Linux process/target guard semantics required"
    );
    command.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        token.starts_with("forge_sat_"),
        "owner machine credential required"
    );
    let db = state.pool.as_ref().context("owner database unavailable")?;
    let claims = crate::api::identity_for_bearer_token(
        db,
        state
            .auth_secret
            .as_deref()
            .context("owner auth unavailable")?,
        token,
    )
    .await
    .map_err(|_| anyhow::anyhow!("owner credential rejected"))?;
    let mut tx = db.begin().await?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:read")
        .await
        .map_err(|_| anyhow::anyhow!("owner read rejected"))?;
    if !read_only {
        ensure!(
            std::env::var("CICD_LOCAL_DELIVERY_MODE").ok().as_deref() == Some("local-verification"),
            "explicit local mode required"
        );
        sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write")
            .await
            .map_err(|_| anyhow::anyhow!("owner write rejected"))?;
    }
    let original = ledger::find_receipt(
        &mut tx,
        project,
        claims.sub,
        &command.workspace_operation_key,
    )
    .await?
    .context("original workspace operation unavailable")?;
    sdlc_workspace::verify_lookup(&original, &command.original)
        .map_err(|_| anyhow::anyhow!("original binding mismatch"))?;
    let root = PathBuf::from(std::env::var("CICD_LOCAL_PG_ROOT")?);
    ensure!(root.is_absolute(), "isolated absolute root required");
    let resolved = if root.exists() {
        plain(&root)?;
        root.canonicalize()?
    } else {
        plain(root.parent().context("root parent missing")?)?;
        root.parent()
            .unwrap()
            .canonicalize()?
            .join(root.file_name().context("root name missing")?)
    };
    for protected in [&state.git.root, &state.config.artifacts.root] {
        if let Ok(path) = protected.canonicalize() {
            ensure!(
                !resolved.starts_with(&path) && !path.starts_with(&resolved),
                "target overlaps protected files"
            );
        }
    }
    let policy: Policy = read(&PathBuf::from(std::env::var("CICD_LOCAL_PG_POLICY")?))?;
    ensure!(
        policy.schema == "forge/isolated-postgres-policy/v1"
            && (5..=300).contains(&policy.lease_seconds),
        "bounded owner policy required"
    );
    policy.checks.validate()?;
    for path in [
        &policy.compose_file,
        &policy.hba_file,
        &policy.compose_root,
        &policy.base_root,
        &policy.python_bin,
        &policy.executor,
        &policy.migration_bin,
    ] {
        ensure!(path.is_absolute(), "absolute trusted paths required");
        plain(path)?;
    }
    ensure!(
        read_bytes(&policy.executor, 128 * 1024)?
            == include_bytes!("../../../scripts/postgres-delivery.py"),
        "coordinator source differs from compiled owner implementation"
    );
    for (name, bytes) in [
        (
            "platform_backup.py",
            include_bytes!("../../../../services-base/scripts/platform_backup.py").as_slice(),
        ),
        (
            "platform_postgres.py",
            include_bytes!("../../../../services-base/scripts/platform_postgres.py").as_slice(),
        ),
        (
            "compose_helpers.py",
            include_bytes!("../../../../services-base/scripts/compose_helpers.py").as_slice(),
        ),
    ] {
        ensure!(
            read_bytes(&policy.base_root.join("scripts").join(name), 256 * 1024)? == bytes,
            "pinned Base utility differs"
        );
    }
    let policy_hash = sha(&serde_json::to_vec(&policy)?);
    let candidate = if !read_only
        && !reconcile
        && command.action == DeliveryAction::Deploy
        && !operation_dir(&resolved, &command.operation_key).exists()
    {
        let candidate = super::candidate(
            state.clone(),
            claims.clone(),
            project,
            &command,
            policy_hash.clone(),
        )
        .await?
        .context("sealed candidate unavailable")?;
        let bytes = read_bytes(&candidate.storage_path, 64 * 1024)?;
        ensure!(
            sha(&bytes) == candidate.manifest.artifact_sha256,
            "actual artifact bytes changed"
        );
        let descriptor: Descriptor = serde_json::from_slice(&bytes)?;
        descriptor.validate(&original.request.source_commit)?;
        Some(serde_json::json!({"candidate":candidate.manifest,"descriptor":descriptor}))
    } else {
        None
    };
    let mut guard = None;
    let mut guard_pid = None;
    if !read_only {
        sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write")
            .await
            .map_err(|_| anyhow::anyhow!("fresh owner write rejected"))?;
        let url = std::env::var("CICD_LOCAL_PG_GUARD_URL")?;
        let options = url
            .parse::<sqlx::postgres::PgConnectOptions>()?
            .application_name(&format!("forge_pg_guard:{project}"))
            .options([("statement_timeout", "8000"), ("lock_timeout", "3000")]);
        let mut conn = tokio::time::timeout(
            Duration::from_secs(5),
            sqlx::PgConnection::connect_with(&options),
        )
        .await
        .context("target guard connection deadline")??;
        let actual=sqlx::query("SELECT current_database() AS database,system_identifier::text,pg_backend_pid() AS pid FROM pg_control_system()")
            .fetch_one(&mut conn).await?;
        ensure!(
            actual.try_get::<String, _>("database")? == "postgres"
                && actual.try_get::<String, _>("system_identifier")? == policy.system_identifier,
            "guard target identity mismatch"
        );
        ensure!(
            sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
                .bind(GUARD_KEY)
                .fetch_one(&mut conn)
                .await?,
            "previous target guard alive"
        );
        guard_pid = Some(actual.try_get::<i32, _>("pid")?);
        guard = Some(conn);
    }
    let packet = serde_json::json!({"projectId":project,"root":resolved,"policy":policy,"policySha256":policy_hash,
        "command":command,"originalOperation":original,"sealedCandidate":candidate,
        "readOnly":read_only,"reconcile":reconcile,"guardPid":guard_pid,"guardKey":GUARD_KEY});
    let mut process = Command::new(&policy.python_bin);
    process
        .arg("-B")
        .arg(&policy.executor)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("DOCKER_HOST", "unix:///var/run/docker.sock")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for name in [
        "CICD_LOCAL_PG_RUNTIME_PASSWORD",
        "CICD_TEST_PG_PAUSE_AT",
        "CICD_TEST_PG_RESUME_FILE",
    ] {
        if let Ok(value) = std::env::var(name) {
            process.env(name, value);
        }
    }
    #[cfg(target_os = "linux")]
    unsafe {
        process.pre_exec(|| {
            let parent = libc::getppid();
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 || libc::getppid() != parent
            {
                return Err(std::io::Error::other("owner unavailable"));
            }
            Ok(())
        });
    }
    let mut child = process.spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&packet)?)
        .await?;
    let mut output = Vec::new();
    let stdout = child.stdout.take().unwrap();
    let task = async {
        let mut bounded_stdout = stdout.take(1024 * 1024 + 1);
        let (status, _) = tokio::try_join!(child.wait(), bounded_stdout.read_to_end(&mut output))?;
        ensure!(
            status.success() && output.len() <= 1024 * 1024,
            "coordinator outcome requires original-key readback"
        );
        Ok::<_, anyhow::Error>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(policy.lease_seconds + 15), task).await;
    if !matches!(&result, Ok(Ok(_))) {
        let _ = child.start_kill();
        child.wait().await?;
    }
    result.context("coordinator deadline; retain unknown hold")??;
    drop(guard);
    let value: serde_json::Value = serde_json::from_slice(&output)?;
    ensure!(
        value["dispatchAllowed"] == false && value["sdlcAcceptanceVerified"] == false,
        "local result cannot grant SDLC authority"
    );
    tx.commit().await?;
    Ok(value)
}
