//! Owner-only native HTTP canary: actual main, real PG and the production Kubernetes runner.
#![cfg(all(unix, feature = "integration"))]
use anyhow::{Context, Result, ensure};
use cicd::{
    execution::kubernetes::{self, Config},
    quiescence::{Coordinator, CustodyRoot, Gate, Identity},
    runner_workspace::OwnedWorkspace,
};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::{sleep, timeout},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    path: PathBuf,
    sha256: String,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum RunnerProfile {
    #[default]
    Embedded,
    External,
}
struct RunnerProcess {
    child: Child,
    pid: u32,
    start: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    #[serde(default)]
    runner_profile: RunnerProfile,
    #[serde(default)]
    external_runner: Option<Reference>,
    schema_version: u32,
    kind: String,
    operation_id: Uuid,
    expected_source_head: String,
    expected_executor_sha256: String,
    main_binary_sha256: String,
    image: String,
    api_host: String,
    api_port: u16,
    admission_file: PathBuf,
    migrations_directory: PathBuf,
    migrations_sha256: String,
    database: Reference,
    runner_config: Reference,
    expected_config: Config,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn private(path: &Path) -> Result<Vec<u8>> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        path.is_absolute()
            && path.canonicalize()? == path
            && m.is_file()
            && m.nlink() == 1
            && m.uid() == unsafe { libc::geteuid() }
            && m.mode() & 0o077 == 0
            && m.len() <= 65536,
        "private canonical fixture file required"
    );
    Ok(fs::read(path)?)
}
fn pinned(r: &Reference) -> Result<Vec<u8>> {
    let b = private(&r.path)?;
    ensure!(hash(&b) == r.sha256, "fixture file changed");
    Ok(b)
}
fn evidence(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(name))?;
    f.write_all(&serde_json::to_vec(value)?)?;
    f.sync_all()?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}
fn process_start(pid: u32) -> Result<String> {
    Ok(fs::read_to_string(format!("/proc/{pid}/stat"))?
        .rsplit_once(") ")
        .context("process identity")?
        .1
        .split_whitespace()
        .nth(19)
        .context("process start")?
        .into())
}
fn migrations_digest(root: &Path) -> Result<String> {
    ensure!(
        root.is_absolute() && root.canonicalize()? == root,
        "migration directory alias"
    );
    let mut files = fs::read_dir(root)?
        .map(|r| r.map(|r| r.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    ensure!(
        !files.is_empty() && files.len() <= 512,
        "migration catalog bound"
    );
    let mut h = Sha256::new();
    for p in files {
        let m = fs::symlink_metadata(&p)?;
        ensure!(
            m.is_file()
                && m.nlink() == 1
                && m.len() <= 1024 * 1024
                && p.extension().is_some_and(|e| e == "sql"),
            "migration file invalid"
        );
        h.update(
            p.file_name()
                .context("migration filename")?
                .as_encoded_bytes(),
        );
        h.update([0]);
        h.update(fs::read(p)?);
    }
    Ok(format!("{:x}", h.finalize()))
}
struct Fixture {
    input: Input,
    root: PathBuf,
    pool: PgPool,
    http: Client,
    child: Mutex<Child>,
    external: Mutex<Option<RunnerProcess>>,
    unknown_probe: Mutex<Option<Child>>,
    runner_id: Mutex<Option<Uuid>>,
    registration_token: String,
    pid: u32,
    start: Option<String>,
    success: String,
    cancel: String,
}
impl Fixture {
    async fn request(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> Result<reqwest::Response> {
        let mut r = self
            .http
            .request(method, format!("http://127.0.0.1:22801{path}"));
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        Ok(r.header("Idempotency-Key", Uuid::new_v4().to_string())
            .json(&body)
            .send()
            .await?)
    }
    async fn json(&self, method: Method, path: &str, token: &str, body: Value) -> Result<Value> {
        let r = self.request(method, path, Some(token), body).await?;
        ensure!(
            r.status() == StatusCode::OK,
            "canary HTTP request rejected ({})",
            r.status()
        );
        let b = r.bytes().await?;
        ensure!(b.len() <= 262144, "HTTP response bound");
        Ok(serde_json::from_slice(&b)?)
    }
    async fn pipeline(&self, id: Uuid, token: &str, expected: &str) -> Result<Value> {
        timeout(Duration::from_secs(150), async {
            loop {
                let v = self
                    .json(
                        Method::GET,
                        &format!("/api/v1/pipelines/{id}"),
                        token,
                        Value::Null,
                    )
                    .await?;
                if v["pipeline"]["status"] == expected {
                    return Ok(v);
                }
                ensure!(
                    !["failed", "canceled", "success"]
                        .iter()
                        .any(|s| v["pipeline"]["status"] == *s),
                    "unexpected terminal pipeline"
                );
                sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .context("pipeline observation deadline")?
    }
    async fn completed(&self, job: Uuid, expected: &str) -> Result<Value> {
        timeout(Duration::from_secs(150),async {
            loop {
                let row: Option<(Uuid,Uuid,i64)>=sqlx::query_as(
                    "SELECT l.id,l.attempt_id,l.generation FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=l.job_id WHERE l.job_id=$1 AND (($3::uuid IS NULL AND l.runner_id IS NULL AND l.runner_name='embedded') OR ($3::uuid IS NOT NULL AND l.runner_id=$3 AND l.completion_received_at IS NOT NULL)) AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=$1) AND l.completed_at IS NOT NULL AND l.lease_status IN ('completed','canceled') AND l.terminal_status=$2 AND a.status=$2 AND a.finished_at IS NOT NULL AND j.status=$2")
                    .bind(job).bind(expected).bind(*self.runner_id.lock().await).fetch_optional(&self.pool).await?;
                if let Some((lease,attempt,generation))=row {
                    let root=Path::new("/workspaces");
                    if !kubernetes::inventory(root)?.iter().any(|s|s.job_id==job)
                        && !OwnedWorkspace::inventory(root)?.iter().any(|w|w.attempt_id==attempt) {
                        return Ok(json!({"leaseId":lease,"attemptId":attempt,"generation":generation,
                            "databaseTerminalAndExecutorCleanupConfirmed":true,"runnerId":*self.runner_id.lock().await}));
                    }
                }
                sleep(Duration::from_millis(250)).await;
            }
        }).await.context("bound lease ACK/workspace cleanup deadline")?
    }
    async fn start_external(&self) -> Result<()> {
        if self.input.runner_profile != RunnerProfile::External {
            return Ok(());
        }
        let reference = self
            .input
            .external_runner
            .as_ref()
            .context("external binary absent")?;
        ensure!(
            self.external.lock().await.is_none(),
            "runner spawn already issued; no repeat"
        );
        let stdout = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.root.join("runner.stdout.private.log"))?;
        let stderr = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.root.join("runner.stderr.private.log"))?;
        evidence(
            &self.root,
            "runner-planned.json",
            &json!({"operationId":self.input.operation_id,
            "binary":reference.path,"binarySha256":reference.sha256,"spawnIssued":true,
            "participant":"canary-external","workspace":"/workspaces"}),
        )?;
        let child = Command::new(&reference.path)
            .env_clear()
            .envs([
                ("PATH", "/usr/local/bin:/usr/bin:/bin".to_owned()),
                ("HOME", self.root.join("home").display().to_string()),
                ("TMPDIR", self.root.join("tmp").display().to_string()),
                ("CICD_API_URL", "http://127.0.0.1:22801".into()),
                (
                    "CICD_RUNNER_NAME",
                    format!("external-canary-{}", self.input.operation_id),
                ),
                (
                    "CICD_RUNNER_REGISTRATION_TOKEN",
                    self.registration_token.clone(),
                ),
                ("CICD_RUNNER_TAGS", "linux,kubernetes".into()),
                ("CICD_RUNNER_TOTAL_SLOTS", "1".into()),
                ("CICD_RUNNER_POLL_INTERVAL_SECONDS", "1".into()),
                ("CICD_RUNNER_WORK_DIR", "/workspaces".into()),
                ("CICD_RUNNER_MODE", "kubernetes".into()),
                (
                    "CICD_KUBERNETES_RUNNER_CONFIG",
                    self.input.runner_config.path.display().to_string(),
                ),
                (
                    "CICD_KUBERNETES_ADMISSION_FILE",
                    self.input.admission_file.display().to_string(),
                ),
                ("KUBERNETES_SERVICE_HOST", self.input.api_host.clone()),
                (
                    "KUBERNETES_SERVICE_PORT_HTTPS",
                    self.input.api_port.to_string(),
                ),
                (
                    "CICD_QUIESCENCE_DIR",
                    self.root.join("control").display().to_string(),
                ),
                ("CICD_QUIESCENCE_PARTICIPANT", "canary-external".into()),
            ])
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(false)
            .spawn()?;
        // No fallible operation may lose this child after spawn.
        let pid = child.id().unwrap_or(0);
        let start = process_start(pid).ok();
        *self.external.lock().await = Some(RunnerProcess { child, pid, start });
        let start = self
            .external
            .lock()
            .await
            .as_ref()
            .and_then(|p| p.start.clone())
            .context("runner process identity unavailable")?;
        evidence(
            &self.root,
            "runner-child.json",
            &json!({"operationId":self.input.operation_id,
            "pid":pid,"processStart":start,"binarySha256":reference.sha256}),
        )?;
        timeout(Duration::from_secs(60),async {
            loop {
                ensure!(self.external.lock().await.as_mut().context("runner holder absent")?
                    .child.try_wait()?.is_none(),"actual runner exited before qualification");
                let rows:Vec<(Uuid,String,Value)>=sqlx::query_as(
                    "SELECT id,credential_hash,heartbeat_payload FROM runners WHERE name=$1 AND status='online' AND capacity_busy_slots=0 AND last_seen_at>now()-interval '10 seconds'")
                    .bind(format!("external-canary-{}",self.input.operation_id)).fetch_all(&self.pool).await?;
                if rows.len()==1 {
                    let (id,digest,payload)=&rows[0];
                    let log=fs::read(self.root.join("runner.stderr.private.log"))?;
                    ensure!(log.len()<=1024*1024,"registration log bound");
                    let text=std::str::from_utf8(&log)?;
                    let credentials:Vec<_>=text.lines().filter_map(|l|l.strip_prefix("runner credential: ")).collect();
                    if credentials.len()==1 && hash(credentials[0].as_bytes())==*digest
                        && payload["capacity"]["busySlots"]==0 && payload["activeLeaseIds"]==json!([])
                        && payload["capabilities"]["executorKinds"]==json!(["kubernetes"]) {
                        let count:i64=sqlx::query_scalar("SELECT count(*) FROM runners").fetch_one(&self.pool).await?;
                        ensure!(count==1,"unexpected fixture runner registration");
                        *self.runner_id.lock().await=Some(*id);
                        evidence(&self.root,"runner-registration.json",&json!({"operationId":self.input.operation_id,
                            "runnerId":id,"credentialSha256":digest,"actualCliRegistration":true,
                            "heartbeatObserved":true,"kubernetesCapability":true,"pid":pid,"processStart":start}))?;
                        return Ok::<_,anyhow::Error>(());
                    }
                } else {
                    ensure!(rows.is_empty(),"ambiguous runner registration");
                }
                sleep(Duration::from_millis(100)).await;
            }
        }).await.context("actual runner registration/heartbeat unknown; no repeat")?
    }

    async fn unknown_workspace_neighbor(&self) -> Result<()> {
        if self.input.runner_profile != RunnerProfile::External {
            return Ok(());
        }
        let path = self.root.join("unknown-workspace");
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        let local = OwnedWorkspace::create(&path, Uuid::new_v4(), Uuid::new_v4(), 1)?;
        local.record_completion("success")?;
        let before = serde_json::to_value(OwnedWorkspace::inventory(&path)?)?;
        let before_bytes = unknown_workspace_bytes(&path)?;
        let leases: i64 = sqlx::query_scalar("SELECT count(*) FROM job_leases")
            .fetch_one(&self.pool)
            .await?;
        let runners: i64 = sqlx::query_scalar("SELECT count(*) FROM runners")
            .fetch_one(&self.pool)
            .await?;
        ensure!(
            kubernetes::inventory(Path::new("/workspaces"))?.is_empty(),
            "job custody remains before negative"
        );
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.root.join("unknown-runner.stderr.private.log"))?;
        let child = Command::new(
            &self
                .input
                .external_runner
                .as_ref()
                .context("binary absent")?
                .path,
        )
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("CICD_RUNNER_MODE", "kubernetes")
        .args([
            "--api-url",
            "http://127.0.0.1:22801",
            "--credential",
            "fixture-not-used",
            "--once",
            "--work-dir",
        ])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .kill_on_drop(false)
        .spawn()?;
        let pid = child.id().unwrap_or(0);
        let start = process_start(pid).ok();
        *self.unknown_probe.lock().await = Some(child);
        let status = timeout(Duration::from_secs(15), async {
            self.unknown_probe
                .lock()
                .await
                .as_mut()
                .context("negative child custody absent")?
                .wait()
                .await
                .context("negative runner wait")
        })
        .await
        .context("negative runner outcome unknown; child retained")??;
        ensure!(
            status.code().is_some_and(|code| code != 0),
            "unknown local completion did not reject by normal startup exit"
        );
        ensure!(
            before == serde_json::to_value(OwnedWorkspace::inventory(&path)?)?
                && before_bytes == unknown_workspace_bytes(&path)?,
            "unresolved workspace changed; no synthetic cleanup permitted"
        );
        ensure!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_leases")
                .fetch_one(&self.pool)
                .await?
                == leases
                && sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runners")
                    .fetch_one(&self.pool)
                    .await?
                    == runners
                && kubernetes::inventory(Path::new("/workspaces"))?.is_empty(),
            "unknown neighbor issued new work"
        );
        let message = fs::read_to_string(self.root.join("unknown-runner.stderr.private.log"))?;
        ensure!(
            message.contains("unresolved workspaces require owner readback"),
            "negative failed outside owning startup boundary"
        );
        evidence(
            &self.root,
            "unknown-workspace.json",
            &json!({"operationId":self.input.operation_id,
            "case":"startup-unknown-custody-no-repeat","pid":pid,"processStart":start,
            "exitCode":status.code(),"workspaceRetained":true,"noNewLease":true,"noNewRegistration":true,
            "noExecutorCustody":true,"networkReplyLossQualified":false}),
        )?;
        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        let gate = Gate::open(&self.root.join("control"))?;
        let identity: Identity = serde_json::from_slice(&private(
            &self.root.join("control/participants/canary.json"),
        )?)?;
        ensure!(
            identity.process_id == self.pid
                && Some(&identity.process_start) == self.start.as_ref()
                && identity.process_start == process_start(self.pid)?
                && identity.participant == "canary"
                && identity.kind == "server"
                && identity.custody_roots
                    == vec![
                        CustodyRoot::workspace(PathBuf::from("/workspaces")),
                        CustodyRoot::git(self.root.join("git"))
                    ],
            "actual main participant does not bind retained child"
        );
        ensure!(
            self.child.lock().await.try_wait()?.is_none(),
            "main exited before normal-stop proof"
        );
        let mut expected = vec![identity];
        if self.input.runner_profile == RunnerProfile::External {
            let participant: Identity = serde_json::from_slice(&private(
                &self.root.join("control/participants/canary-external.json"),
            )?)?;
            let mut holder = self.external.lock().await;
            let native = holder
                .as_mut()
                .context("actual runner spawn/identity unknown")?;
            ensure!(
                native.child.try_wait()?.is_none()
                    && participant.process_id == native.pid
                    && Some(&participant.process_start) == native.start.as_ref()
                    && participant.process_start == process_start(native.pid)?
                    && participant.participant == "canary-external"
                    && participant.kind == "external-runner"
                    && participant.custody_roots
                        == vec![CustodyRoot::workspace(PathBuf::from("/workspaces"))],
                "external runner participant differs from exact retained process"
            );
            if let Some(probe) = self.unknown_probe.lock().await.as_mut() {
                ensure!(
                    probe.try_wait()?.is_some(),
                    "negative runner child still unresolved"
                );
            }
            expected.push(participant);
        }
        let owner = Coordinator::request(gate, self.input.operation_id)?;
        let fence = timeout(Duration::from_secs(180), async {
            loop {
                owner.gate().verify_participants(&expected)?;
                if let Some(fence) = owner.try_fence()? {
                    owner.gate().verify_no_effect_records(&fence)?;
                    if matches!(
                        timeout(
                            Duration::from_secs(5),
                            cicd::quiescence::verify_recoverable_idle(
                                &fence, &self.pool, &expected
                            )
                        )
                        .await,
                        Ok(Ok(()))
                    ) {
                        return Ok::<_, anyhow::Error>(fence);
                    }
                    drop(fence);
                }
                sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .context("normal-stop idle remains unknown")??;
        self.pool.close().await;
        owner.publish_held(&fence, &expected)?;
        evidence(
            &self.root,
            "held.json",
            &json!({"operationId":self.input.operation_id,"participants":expected,"recoverableIdle":true}),
        )?;
        owner.request_exit(&fence, &expected)?;
        let mut child = self.child.lock().await;
        let status = timeout(Duration::from_secs(15), child.wait())
            .await
            .context("normal main exit unknown")??;
        drop(child);
        if self.input.runner_profile == RunnerProfile::External {
            let mut holder = self.external.lock().await;
            let native = holder.as_mut().context("runner holder missing")?;
            let status = timeout(Duration::from_secs(15), native.child.wait())
                .await
                .context("normal runner exit unknown")??;
            ensure!(
                status.success(),
                "actual external runner did not exit normally"
            );
            evidence(
                &self.root,
                "runner-exited.json",
                &json!({"operationId":self.input.operation_id,
                "pid":native.pid,"processStart":native.start,"normalExit":true}),
            )?;
        }
        ensure!(
            status.success() && owner.gate().all_exited(&expected)?,
            "normal main exit not qualified"
        );
        evidence(
            &self.root,
            "exited.json",
            &json!({"operationId":self.input.operation_id,"pid":self.pid,"participants":expected,"normalExit":true}),
        )?;
        Ok(())
    }
}
fn unknown_workspace_bytes(root: &Path) -> Result<Vec<(PathBuf, u64, u32, String)>> {
    let mut pending = vec![root.to_path_buf()];
    let mut result = Vec::new();
    while let Some(path) = pending.pop() {
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            path.canonicalize()? == path && result.len() < 4096,
            "unknown fixture tree redirected/unbounded"
        );
        let digest = if meta.is_dir() {
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
            String::new()
        } else {
            ensure!(
                meta.is_file() && meta.nlink() == 1 && meta.len() <= 65536,
                "unknown fixture leaf redirected/unbounded"
            );
            hash(&fs::read(&path)?)
        };
        result.push((
            path.strip_prefix(root)?.to_path_buf(),
            meta.ino(),
            meta.mode(),
            digest,
        ));
    }
    result.sort();
    Ok(result)
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()?;
    ensure!(out.status.success(), "owned Git fixture command failed");
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
fn source(root: &Path, image: &str, op: Uuid) -> Result<(String, String)> {
    let work = root.join("source");
    fs::create_dir(&work)?;
    git(&work, &["init", "--initial-branch=success"])?;
    fs::write(work.join("source.txt"), op.to_string())?;
    let mut pins = Vec::new();
    for branch in ["success", "cancel"] {
        if branch == "cancel" {
            git(&work, &["checkout", "-b", "cancel"])?;
        }
        let command = if branch == "success" {
            format!(
                "test \"$(cat source.txt)\" = '{op}'; mkdir -p artifacts; printf '%s' '{op}' > artifacts/product.txt; printf 'main-canary:%s\\n' '{op}'"
            )
        } else {
            format!(
                "test \"$(cat source.txt)\" = '{op}'; mkdir -p artifacts; printf '%s' '{op}' > artifacts/started; exec sleep 90"
            )
        };
        let mut job = json!({"image":image,"commands":[command],"timeout":"120s"});
        if branch == "success" {
            job["artifacts"] = json!({"paths":["artifacts/product.txt"]});
        }
        fs::write(
            work.join(".forge-ci.yml"),
            serde_json::to_vec(&json!({"version":1,"jobs":{"ordinary":job}}))?,
        )?;
        git(&work, &["add", "."])?;
        git(
            &work,
            &[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "commit",
                "-m",
                branch,
            ],
        )?;
        pins.push(git(&work, &["rev-parse", "HEAD"])?);
    }
    git(
        root,
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            "source",
            "git/canary.git",
        ],
    )?;
    Ok((pins.remove(0), pins.remove(0)))
}
async fn exercise(f: Arc<Fixture>) -> Result<Value> {
    evidence(
        &f.root,
        "child.json",
        &json!({"pid":f.pid,"processStart":f.start.as_ref().context("initial child start unavailable")?,
        "mainBinarySha256":f.input.main_binary_sha256,"sourceHead":f.input.expected_source_head}),
    )?;
    timeout(Duration::from_secs(60), async {
        loop {
            ensure!(
                f.child.lock().await.try_wait()?.is_none(),
                "main exited during startup"
            );
            if let Ok(r) = f
                .http
                .get("http://127.0.0.1:22801/api/v1/readiness")
                .send()
                .await
            {
                if r.status() == StatusCode::OK {
                    break Ok::<_, anyhow::Error>(());
                }
            }
            sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .context("main readiness deadline")??;
    f.start_external().await?;
    let user = Uuid::new_v4();
    let password = Uuid::new_v4().to_string();
    let username = format!("canary-{user}");
    sqlx::query("INSERT INTO users(id,username,role) VALUES($1,$2,'admin')")
        .bind(user)
        .bind(&username)
        .execute(&f.pool)
        .await?;
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES($1,$2)")
        .bind(user)
        .bind(cicd::auth::hash_password(&password)?)
        .execute(&f.pool)
        .await?;
    ensure!(
        f.request(Method::GET, "/api/v1/projects", None, Value::Null)
            .await?
            .status()
            == StatusCode::UNAUTHORIZED,
        "authentication bypass"
    );
    let login = f
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            json!({"username":username,"password":password}),
        )
        .await?;
    ensure!(
        login.status() == StatusCode::OK,
        "real local-auth login failed"
    );
    let pair: Value = login.json().await?;
    let token = pair["access_token"].as_str().context("access JWT absent")?;
    ensure!(
        f.request(
            Method::GET,
            "/api/v1/projects",
            Some("invalid-fixture"),
            Value::Null
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "invalid credential accepted"
    );
    let repo = Uuid::new_v4();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,'canary')")
        .bind(repo)
        .execute(&f.pool)
        .await?;
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,'native-canary','http://127.0.0.1:22801/git/canary.git',$2)")
        .bind(project).bind(repo).execute(&f.pool).await?;
    let mut receipts = Vec::new();
    for (branch, pin) in [("success", &f.success), ("cancel", &f.cancel)] {
        let created = f
            .json(
                Method::POST,
                &format!("/api/v1/projects/{project}/pipelines"),
                token,
                json!({"git_ref":branch}),
            )
            .await?;
        let pipeline: Uuid = serde_json::from_value(created["pipeline"]["id"].clone())?;
        ensure!(
            created["plan"]["config_source"] == "repository"
                && created["plan"]["resolved_commit_sha"] == pin.as_str(),
            "source pin/plan mismatch"
        );
        let job: Uuid = serde_json::from_value(created["stages"][0]["jobs"][0]["id"].clone())?;
        if branch == "cancel" {
            timeout(Duration::from_secs(90),async {
                loop {
                    let lease: Option<(Uuid,Uuid,i64)>=sqlx::query_as("SELECT attempt_id,id,generation FROM job_leases WHERE job_id=$1 ORDER BY generation DESC LIMIT 1").bind(job).fetch_optional(&f.pool).await?;
                    if let Some((attempt,id,generation))=lease {
                        for w in OwnedWorkspace::inventory(Path::new("/workspaces"))? {
                            if (w.attempt_id,w.lease_id,w.generation)==(attempt,id,generation) {
                                let p=Path::new("/workspaces").join(&w.workspace_id).join("workspace/artifacts/started");
                                if let Ok(meta)=fs::symlink_metadata(&p) {
                                    ensure!(p.canonicalize()?==p && meta.is_file() && meta.nlink()==1 && meta.len()<=128,"cancel marker alias");
                                    ensure!(fs::read(p)?==f.input.operation_id.to_string().as_bytes(),"foreign cancel marker");
                                    let v=f.json(Method::GET,&format!("/api/v1/pipelines/{pipeline}"),token,Value::Null).await?;
                                    ensure!(v["pipeline"]["status"]=="running" && v["stages"][0]["jobs"][0]["status"]=="running","cancel execution already stopped");
                                    if let Some(runner)=*f.runner_id.lock().await {
                                        let count:i64=sqlx::query_scalar("SELECT count(*) FROM runners r JOIN job_leases l ON l.runner_id=r.id WHERE r.id=$1 AND l.id=$2 AND l.acknowledged_at IS NOT NULL AND r.capacity_busy_slots=1 AND r.heartbeat_payload->'activeLeaseIds' @> $3::jsonb")
                                            .bind(runner).bind(id).bind(json!([id])).fetch_one(&f.pool).await?;
                                        ensure!(count==1,"actual busy heartbeat/lease ACK not observed");
                                    }
                                    evidence(&f.root,"cancel-marker.json",&json!({"attemptId":attempt,"leaseId":id,"generation":generation,"workspaceId":w.workspace_id,"actualJobMarker":true}))?;
                                    return Ok::<_,anyhow::Error>(());
                                }
                            }
                        }
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            }).await.context("actual cancel start marker deadline")??;
            f.json(
                Method::POST,
                &format!("/api/v1/pipelines/{pipeline}/cancel"),
                token,
                json!({}),
            )
            .await?;
        }
        let done = f
            .pipeline(
                pipeline,
                token,
                if branch == "success" {
                    "success"
                } else {
                    "canceled"
                },
            )
            .await?;
        if branch == "success" {
            let logs = f
                .json(
                    Method::GET,
                    &format!("/api/v1/jobs/{job}/logs"),
                    token,
                    Value::Null,
                )
                .await?;
            ensure!(
                logs.to_string()
                    .contains(&format!("main-canary:{}", f.input.operation_id)),
                "actual job log missing"
            );
            let artifacts = f
                .json(
                    Method::GET,
                    &format!("/api/v1/jobs/{job}/artifacts"),
                    token,
                    Value::Null,
                )
                .await?;
            let rows = artifacts.as_array().context("artifact list")?;
            ensure!(rows.len() == 1, "artifact inventory");
            let response = f
                .request(
                    Method::GET,
                    &format!(
                        "/api/v1/artifacts/{}/download",
                        rows[0]["id"].as_str().context("artifact id")?
                    ),
                    Some(token),
                    Value::Null,
                )
                .await?;
            ensure!(
                response.status() == StatusCode::OK
                    && response.bytes().await?.as_ref()
                        == f.input.operation_id.to_string().as_bytes(),
                "actual artifact/source bytes mismatch"
            );
        }
        let custody = f
            .completed(
                job,
                if branch == "success" {
                    "success"
                } else {
                    "canceled"
                },
            )
            .await?;
        receipts
            .push(json!({"branch":branch,"pipeline":done,"sourceCommit":pin,"custody":custody}));
    }
    evidence(&f.root, "http-cases.json", &json!(receipts))?;
    f.unknown_workspace_neighbor().await?;
    Ok(
        json!({"status":"PASS","localAuthenticationQualified":true,"centralSsoQualified":false,
        "actualMain":true,"ordinaryPipelineCases":["success","cancel"],"externalRunnerProtocolQualified":f.input.runner_profile==RunnerProfile::External}),
    )
}

fn validate_scope(i: &Input, built: Option<&str>) -> Result<()> {
    ensure!(
        i.schema_version == 1
            && i.kind == "forge/native-main-canary/v1"
            && !i.operation_id.is_nil(),
        "unsupported canary ABI"
    );
    ensure!(
        built == Some(i.expected_source_head.as_str())
            && i.expected_source_head.len() == 40
            && i.expected_source_head
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            && i.expected_executor_sha256 == hash(include_bytes!("../src/execution/kubernetes.rs")),
        "compiled source differs from owner pin"
    );
    ensure!(
        match (&i.runner_profile, &i.external_runner) {
            (RunnerProfile::Embedded, None) => true,
            (RunnerProfile::External, Some(r)) =>
                r.path.is_absolute()
                    && r.path.to_str() == Some(env!("CARGO_BIN_EXE_forge-runner"))
                    && r.sha256.len() == 64
                    && r.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            _ => false,
        },
        "explicit runner profile/binary scope differs"
    );
    i.expected_config.validate()?;
    ensure!(
        !i.expected_config.builder_enabled && i.expected_config.deployment_recipes.is_empty(),
        "ordinary CI canary only"
    );
    ensure!(
        i.image
            .rsplit_once("@sha256:")
            .is_some_and(|(name, d)| !name.is_empty()
                && d.len() == 64
                && d.bytes().all(|b| b.is_ascii_hexdigit()))
            && !i.image.chars().any(char::is_whitespace),
        "immutable qualified image required"
    );
    ensure!(
        i.api_host.parse::<std::net::Ipv4Addr>().is_ok() && i.api_port > 0,
        "qualified literal API endpoint required"
    );
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owner-pinned main binary, isolated real PG, SA/CA, private fixture and actual Kubernetes"]
async fn actual_main_http_current_ci_canary() -> Result<()> {
    let input_path = PathBuf::from(std::env::var("FORGE_MAIN_CANARY_INPUT")?);
    let bytes = private(&input_path)?;
    ensure!(
        hash(&bytes) == std::env::var("FORGE_MAIN_CANARY_INPUT_SHA256")?,
        "canary input hash changed"
    );
    let input: Input = serde_json::from_slice(&bytes)?;
    validate_scope(&input, option_env!("FORGE_MAIN_CANARY_BUILD_HEAD"))?;
    ensure!(
        serde_json::from_slice::<Config>(&pinned(&input.runner_config)?)? == input.expected_config,
        "runner config pin differs"
    );
    ensure!(
        migrations_digest(&input.migrations_directory)? == input.migrations_sha256,
        "migration catalog changed"
    );
    private(&input.admission_file)?;
    let binary = Path::new(env!("CARGO_BIN_EXE_cicd-server"));
    let meta = fs::symlink_metadata(binary)?;
    ensure!(
        binary.canonicalize()? == binary
            && meta.is_file()
            && meta.nlink() == 1
            && meta.len() <= 256 * 1024 * 1024,
        "main binary alias/bound"
    );
    let mut h = Sha256::new();
    let mut file = fs::File::open(binary)?;
    let mut chunk = [0u8; 65536];
    loop {
        let n = std::io::Read::read(&mut file, &mut chunk)?;
        if n == 0 {
            break;
        }
        h.update(&chunk[..n]);
    }
    ensure!(
        format!("{:x}", h.finalize()) == input.main_binary_sha256,
        "actual compiled main binary changed"
    );
    if let Some(reference) = &input.external_runner {
        let meta = fs::symlink_metadata(&reference.path)?;
        ensure!(
            reference.path.canonicalize()? == reference.path
                && meta.is_file()
                && meta.nlink() == 1
                && meta.len() <= 256 * 1024 * 1024,
            "external binary alias/bound"
        );
        let mut h = Sha256::new();
        let mut file = fs::File::open(&reference.path)?;
        let mut chunk = [0u8; 65536];
        loop {
            let n = std::io::Read::read(&mut file, &mut chunk)?;
            if n == 0 {
                break;
            }
            h.update(&chunk[..n]);
        }
        ensure!(
            format!("{:x}", h.finalize()) == reference.sha256,
            "actual external runner binary changed"
        );
    }
    let dsn = String::from_utf8(pinned(&input.database)?)?;
    let url = reqwest::Url::parse(dsn.trim())?;
    ensure!(
        matches!(url.scheme(), "postgres" | "postgresql")
            && url.path().starts_with("/forge_test_")
            && url.query().is_none()
            && url.fragment().is_none(),
        "new disposable whole DB only; no search_path override"
    );
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(dsn.trim())
        .await?;
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema='public'",
    )
    .fetch_one(&pool)
    .await?;
    ensure!(
        tables == 0,
        "canary DB must be new/empty before actual main migrations"
    );
    let workspace = Path::new("/workspaces");
    ensure!(
        workspace.canonicalize()? == workspace
            && OwnedWorkspace::inventory(workspace)?.is_empty()
            && kubernetes::inventory(workspace)?.is_empty(),
        "workspace/execution custody already present"
    );
    let c = &input.expected_config;
    ensure!(
        unsafe { libc::geteuid() } == c.security.run_as_user
            && unsafe { libc::getegid() } == c.security.run_as_group,
        "controller/job UID differs"
    );
    ensure!(
        workspace.metadata()?.gid() == c.security.fs_group
            && workspace.metadata()?.mode() & 0o2070 == 0o2070,
        "workspace fsGroup root not prepared"
    );
    let root = PathBuf::from(format!("/qualification/main-canary-{}", input.operation_id));
    ensure!(
        root.parent().context("fixture parent")?.canonicalize()? == Path::new("/qualification"),
        "private fixture parent alias"
    );
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    for name in ["git", "artifacts", "control", "home", "tmp"] {
        fs::create_dir(root.join(name))?;
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700))?;
    }
    let (success, cancel) = source(&root, &input.image, input.operation_id)?;
    let reserve = std::net::TcpListener::bind("127.0.0.1:22801")?;
    drop(reserve);
    let secret = Uuid::new_v4().to_string();
    let registration_token = Uuid::new_v4().to_string();
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("main.stdout.private.log"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("main.stderr.private.log"))?;
    evidence(
        &root,
        "planned.json",
        &json!({"operationId":input.operation_id,"sourceHead":input.expected_source_head,
        "mainBinary":binary,"mainBinarySha256":input.main_binary_sha256,"workspace":workspace,"pidUnknownUntilSpawn":true}),
    )?;
    let http = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(5))
        .build()?;
    let child = Command::new(binary)
        .env_clear()
        .envs([
            ("PATH", "/usr/local/bin:/usr/bin:/bin".to_owned()),
            ("HOME", root.join("home").display().to_string()),
            ("TMPDIR", root.join("tmp").display().to_string()),
            ("CICD_DATABASE_URL", dsn.trim().to_owned()),
            (
                "CICD_MIGRATIONS_DIR",
                input.migrations_directory.display().to_string(),
            ),
            ("CICD_BIND", "127.0.0.1:22801".into()),
            ("CICD_ROLES", "api,runner".into()),
            ("CICD_AUTH_SECRET", secret),
            ("CICD_GIT_ROOT", root.join("git").display().to_string()),
            ("CICD_GIT_INTERNAL_TOKEN", Uuid::new_v4().to_string()),
            (
                "CICD_ARTIFACTS_DIR",
                root.join("artifacts").display().to_string(),
            ),
            ("CICD_RUNNER_MODE", "kubernetes".into()),
            (
                "CICD_EMBEDDED_RUNNER_ENABLED",
                if input.runner_profile == RunnerProfile::External {
                    "false".into()
                } else {
                    "true".into()
                },
            ),
            (
                "CICD_RUNNER_REGISTRATION_TOKEN",
                if input.runner_profile == RunnerProfile::External {
                    registration_token.clone()
                } else {
                    String::new()
                },
            ),
            ("CICD_RUNNER_KEEP_WORKSPACE", "false".into()),
            ("CICD_RUNNER_QUEUE_TIMEOUT_SECONDS", "120".into()),
            (
                "CICD_KUBERNETES_RUNNER_CONFIG",
                input.runner_config.path.display().to_string(),
            ),
            (
                "CICD_KUBERNETES_ADMISSION_FILE",
                input.admission_file.display().to_string(),
            ),
            ("KUBERNETES_SERVICE_HOST", input.api_host.clone()),
            ("KUBERNETES_SERVICE_PORT_HTTPS", input.api_port.to_string()),
            (
                "CICD_QUIESCENCE_DIR",
                root.join("control").display().to_string(),
            ),
            ("CICD_QUIESCENCE_PARTICIPANT", "canary".into()),
        ])
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .kill_on_drop(false)
        .spawn()?;
    // All fallible work after spawn is inside caught tasks. The outer owner retains Child.
    let pid = child.id().unwrap_or(0);
    let start = process_start(pid).ok();
    let f = Arc::new(Fixture {
        input,
        root,
        pool,
        http,
        child: Mutex::new(child),
        external: Mutex::new(None),
        unknown_probe: Mutex::new(None),
        runner_id: Mutex::new(None),
        registration_token,
        pid,
        start,
        success,
        cancel,
    });
    let case = {
        let f = f.clone();
        tokio::spawn(async move {
            timeout(Duration::from_secs(420), exercise(f))
                .await
                .context("HTTP case deadline")?
        })
        .await
    };
    let stopped = {
        let f = f.clone();
        tokio::spawn(async move {
            timeout(Duration::from_secs(240), f.shutdown())
                .await
                .context("normal cleanup deadline")?
        })
        .await
    };
    if !matches!(&stopped, Ok(Ok(()))) {
        let _ = evidence(
            &f.root,
            "HOLD.json",
            &json!({"status":"HOLD","pid":f.pid,"operationId":f.input.operation_id,
            "processStart":f.start,"normalExitProven":false,"ownerMustRetainWorker":true,"reason":"case/cleanup failed; unknown custody is retained"}),
        );
        eprintln!(
            "FORGE_MAIN_CANARY_HOLD operation={} pid={} evidence={}",
            f.input.operation_id,
            f.pid,
            f.root.display()
        );
        loop {
            sleep(Duration::from_secs(30)).await;
        }
    }
    let result = case.context("canary task panicked; main has now exited normally")??;
    evidence(&f.root, "result.json", &result)?;
    Ok(())
}

#[test]
fn canary_scope_requires_ordinary_feature_and_exact_build() {
    let head = "a".repeat(40);
    let mut v = json!({"schemaVersion":1,"kind":"forge/native-main-canary/v1","operationId":Uuid::new_v4(),
        "expectedSourceHead":head,"expectedExecutorSha256":hash(include_bytes!("../src/execution/kubernetes.rs")),
        "mainBinarySha256":"b".repeat(64),"image":format!("docker.io/library/postgres@sha256:{}","c".repeat(64)),
        "apiHost":"10.0.0.1","apiPort":6443,"admissionFile":"/qualification/admission.json",
        "migrationsDirectory":"/qualification/migrations","migrationsSha256":"d".repeat(64),
        "database":{"path":"/qualification/db.private","sha256":"e".repeat(64)},
        "runnerConfig":{"path":"/qualification/runner.json","sha256":"f".repeat(64)},
        "expectedConfig":{"schemaVersion":1,"installationSha256":"1".repeat(64),"installation":"qa",
            "clusterCaSha256":"2".repeat(64),"namespace":"forge-native-qa","namespaceUid":Uuid::new_v4(),
            "workspaceClaim":"workspace","workspaceClaimUid":Uuid::new_v4(),"jobServiceAccount":"job",
            "builderEnabled":false,"resources":{"cpuMillis":100,"memoryBytes":134217728,"ephemeralStorageBytes":134217728},
            "security":{"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001}}});
    let i: Input = serde_json::from_value(v.clone()).unwrap();
    validate_scope(&i, Some(&head)).expect("valid command-only neighbor");
    assert!(validate_scope(&i, Some(&"0".repeat(40))).is_err());
    v["expectedConfig"]["builderEnabled"] = json!(true);
    assert!(validate_scope(&serde_json::from_value(v.clone()).unwrap(), Some(&head)).is_err());
    v["expectedConfig"]["builderEnabled"] = json!(false);
    v["image"] = json!("postgres:17.6");
    assert!(validate_scope(&serde_json::from_value(v).unwrap(), Some(&head)).is_err());
}

#[test]
fn external_profile_requires_exact_cli_binary_and_explicit_selection() {
    let v = json!({"schemaVersion":1,"kind":"forge/native-main-canary/v1","operationId":Uuid::new_v4(),
        "runnerProfile":"external","externalRunner":{"path":env!("CARGO_BIN_EXE_forge-runner"),"sha256":"b".repeat(64)},
        "expectedSourceHead":"a".repeat(40),"expectedExecutorSha256":hash(include_bytes!("../src/execution/kubernetes.rs")),
        "mainBinarySha256":"b".repeat(64),"image":format!("docker.io/library/postgres@sha256:{}","c".repeat(64)),
        "apiHost":"10.0.0.1","apiPort":6443,"admissionFile":"/qualification/admission.json",
        "migrationsDirectory":"/qualification/migrations","migrationsSha256":"d".repeat(64),
        "database":{"path":"/qualification/db.private","sha256":"e".repeat(64)},
        "runnerConfig":{"path":"/qualification/runner.json","sha256":"f".repeat(64)},
        "expectedConfig":{"schemaVersion":1,"installationSha256":"1".repeat(64),"installation":"qa",
            "clusterCaSha256":"2".repeat(64),"namespace":"forge-native-qa","namespaceUid":Uuid::new_v4(),
            "workspaceClaim":"workspace","workspaceClaimUid":Uuid::new_v4(),"jobServiceAccount":"job",
            "builderEnabled":false,"resources":{"cpuMillis":100,"memoryBytes":134217728,"ephemeralStorageBytes":134217728},
            "security":{"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001}}});
    validate_scope(
        &serde_json::from_value(v.clone()).unwrap(),
        Some(&"a".repeat(40)),
    )
    .unwrap();
    for field in ["runnerProfile", "externalRunner"] {
        let mut changed = v.clone();
        changed.as_object_mut().unwrap().remove(field);
        assert!(
            validate_scope(
                &serde_json::from_value(changed).unwrap(),
                Some(&"a".repeat(40))
            )
            .is_err()
        );
    }
    let mut wrong = v;
    wrong["externalRunner"]["path"] = json!("/usr/local/bin/forge-runner");
    assert!(
        validate_scope(
            &serde_json::from_value(wrong).unwrap(),
            Some(&"a".repeat(40))
        )
        .is_err()
    );
}
