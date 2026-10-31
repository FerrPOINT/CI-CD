//! Private bounded read-only custody observation for Base management.
//! Never invoke execution start/observe/reconcile/cleanup from this module.
use super::*;
use sqlx::{PgConnection, PgPool};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "forge/kubernetes-custody-observation/v1";
const SEAL_SCHEMA: &str = "forge/kubernetes-authored-spec/v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodRequest {
    pub name: String,
    pub uid: Uuid,
    pub resource_version: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub challenge: Uuid,
    pub installation_sha256: String,
    pub config_sha256: String,
    pub namespace: String,
    pub namespace_uid: String,
    pub workspace_claim: String,
    pub workspace_claim_uid: String,
    pub pods: Vec<PodRequest>,
}
impl Request {
    fn validate(&self, config: &Config, config_sha: &str) -> anyhow::Result<()> {
        ensure!(
            self.schema_version == 1 && !self.challenge.is_nil() && self.pods.len() <= 128,
            "invalid observation request"
        );
        ensure!(
            self.config_sha256 == config_sha
                && self.installation_sha256 == config.installation_sha256
                && self.namespace == config.namespace
                && self.namespace_uid == config.namespace_uid
                && self.workspace_claim == config.workspace_claim
                && self.workspace_claim_uid == config.workspace_claim_uid,
            "observation physical configuration mismatch"
        );
        let mut names = BTreeSet::new();
        let mut uids = BTreeSet::new();
        for pod in &self.pods {
            ensure!(
                dns(&pod.name)
                    && !pod.uid.is_nil()
                    && !pod.resource_version.is_empty()
                    && pod.resource_version.len() <= 128
                    && names.insert(&pod.name)
                    && uids.insert(pod.uid),
                "invalid/duplicate observation Pod"
            );
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Seal {
    schema: String,
    action_sha256: String,
    authored_spec_sha256: String,
    executor_kind: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pod_name: String,
    pod_uid: Uuid,
    resource_version: String,
    job_id: Uuid,
    attempt_id: Uuid,
    lease_id: Uuid,
    generation: i64,
    source_commit: String,
    runner_id: Option<Uuid>,
    executor_kind: String,
    authored_spec_sha256: String,
    phase: String,
}
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct Binding {
    job_status: String,
    attempt_status: String,
    lease_status: String,
    runner_id: Option<Uuid>,
    runner_name: String,
    runner_exists: bool,
    runner_acknowledged: bool,
    lease_current: bool,
    canceled: bool,
    completed: bool,
    completion_received: bool,
    terminal_status: Option<String>,
    queue_state: String,
    source_commit: Option<String>,
}

/// Exact existing reservation/attempt/lease authority. No token/hash is selected.
pub(crate) async fn db_binding(db: &mut PgConnection, scope: &Scope) -> anyhow::Result<Binding> {
    let row = sqlx::query_as::<_, Binding>(
        "SELECT j.status AS job_status,a.status AS attempt_status,l.lease_status,l.runner_id,l.runner_name, \
         (l.runner_id IS NULL OR (r.id IS NOT NULL AND r.name=l.runner_name)) AS runner_exists, \
         (l.acknowledged_at IS NOT NULL) AS runner_acknowledged,(l.lease_expires_at>now()) AS lease_current, \
         (l.cancel_requested_at IS NOT NULL) AS canceled,(l.completed_at IS NOT NULL) AS completed, \
         (l.completion_received_at IS NOT NULL) AS completion_received,l.terminal_status, \
         q.state AS queue_state,p.commit_sha AS source_commit \
         FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN execution_attempts a ON a.id=l.attempt_id AND a.job_id=j.id \
         JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id \
         JOIN job_queue q ON q.job_id=j.id AND q.attempt_id=a.id AND q.lease_id=l.id AND q.stage_id=s.id AND q.pipeline_id=p.id \
         LEFT JOIN runners r ON r.id=l.runner_id \
         WHERE l.id=$1 AND l.job_id=$2 AND l.attempt_id=$3 AND l.generation=$4 \
         AND l.generation=(SELECT max(x.generation) FROM job_leases x WHERE x.job_id=j.id) \
         AND NOT EXISTS(SELECT 1 FROM job_leases x WHERE x.job_id=j.id AND x.id<>l.id AND x.lease_status='active')"
    ).bind(scope.lease_id).bind(scope.job_id).bind(scope.attempt_id).bind(scope.generation)
        .fetch_optional(db).await?.context("exact reservation authority missing")?;
    ensure!(
        row.source_commit.as_deref() == Some(scope.source_commit.as_str())
            && row.runner_exists
            && ((row.runner_id.is_none() && row.runner_name == "embedded")
                || (row.runner_id.is_some() && row.runner_acknowledged)),
        "source or runner authority mismatch"
    );
    Ok(row)
}
fn phase(binding: &Binding, pod: &Value, terminal: bool) -> anyhow::Result<String> {
    if binding.job_status == "running"
        && binding.attempt_status == "running"
        && binding.lease_status == "active"
        && binding.lease_current
        && binding.queue_state == "leased"
    {
        return Ok(if terminal {
            "terminal-stopped"
        } else if binding.canceled {
            "canceling"
        } else if pod["status"]["phase"] == "Running" {
            "running"
        } else if pod["status"]["phase"] == "Pending" {
            "pending"
        } else {
            anyhow::bail!("unknown Pod phase")
        }
        .into());
    }
    let completed = matches!(
        binding.job_status.as_str(),
        "success" | "failed" | "canceled"
    ) && binding.attempt_status == binding.job_status
        && binding.terminal_status.as_deref() == Some(binding.job_status.as_str())
        && binding.completed
        && matches!(binding.queue_state.as_str(), "completed" | "canceled")
        && binding.lease_status
            == if binding.job_status == "canceled" {
                "canceled"
            } else {
                "completed"
            }
        && (binding.runner_id.is_none() || binding.completion_received);
    if completed && terminal {
        return Ok("cleanup-pending".into());
    }
    // Cancellation projection may precede the exact Pod stop; it is never stop proof.
    if binding.canceled
        && binding.job_status == "canceled"
        && binding.attempt_status == "canceled"
        && matches!(binding.lease_status.as_str(), "active" | "canceled")
        && binding.queue_state == "canceled"
        && !terminal
    {
        return Ok("canceling".into());
    }
    anyhow::bail!("execution DB state is unresolved")
}
fn default(value: &mut Value, key: &str, expected: Value) {
    if value.get(key) == Some(&expected) {
        value.as_object_mut().unwrap().remove(key);
    }
}
fn quantity(raw: &str) -> anyhow::Result<String> {
    ensure!(
        raw.len() <= 64 && !raw.is_empty(),
        "invalid resource quantity"
    );
    let split = raw
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(raw.len());
    let (number, suffix) = raw.split_at(split);
    let mut parts = number.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    ensure!(
        !whole.is_empty()
            && parts.next().is_none()
            && fraction.len() <= 9
            && whole.len() <= 18
            && whole
                .bytes()
                .chain(fraction.bytes())
                .all(|c| c.is_ascii_digit()),
        "unsupported resource quantity"
    );
    let scale = 10i128.pow(fraction.len() as u32);
    let mut numerator = whole
        .parse::<i128>()?
        .checked_mul(scale)
        .context("quantity overflow")?
        + if fraction.is_empty() {
            0
        } else {
            fraction.parse::<i128>()?
        };
    let (factor, divisor): (i128, i128) = match suffix {
        "" => (1, 1),
        "n" => (1, 1_000_000_000),
        "u" => (1, 1_000_000),
        "m" => (1, 1000),
        "k" | "K" => (1000, 1),
        "M" => (1000i128.pow(2), 1),
        "G" => (1000i128.pow(3), 1),
        "T" => (1000i128.pow(4), 1),
        "P" => (1000i128.pow(5), 1),
        "E" => (1000i128.pow(6), 1),
        "Ki" => (1024, 1),
        "Mi" => (1024i128.pow(2), 1),
        "Gi" => (1024i128.pow(3), 1),
        "Ti" => (1024i128.pow(4), 1),
        "Pi" => (1024i128.pow(5), 1),
        "Ei" => (1024i128.pow(6), 1),
        _ => anyhow::bail!("unsupported resource quantity suffix"),
    };
    numerator = numerator.checked_mul(factor).context("quantity overflow")?;
    let mut denominator = scale.checked_mul(divisor).context("quantity overflow")?;
    let (mut a, mut b) = (numerator, denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    if a != 0 {
        numerator /= a;
        denominator /= a;
    }
    Ok(format!("{numerator}/{denominator}"))
}
fn quantities(resources: &mut Value) -> anyhow::Result<()> {
    for section in ["requests", "limits"] {
        if let Some(rows) = resources.get_mut(section).and_then(Value::as_object_mut) {
            for (name, value) in rows {
                if matches!(name.as_str(), "cpu" | "memory" | "ephemeral-storage") {
                    *value = json!(quantity(
                        value.as_str().context("resource must be a string")?
                    )?);
                }
            }
        }
    }
    default(resources, "claims", json!([]));
    Ok(())
}
/// Only enumerated Kubernetes defaults/quantity encodings are normalized.
/// Every command, env value, mount, image, capability and unknown field remains hashed.
fn normalized_spec(spec: &Value) -> anyhow::Result<Value> {
    let mut v = spec.clone();
    ensure!(v.is_object(), "Pod spec missing");
    v.as_object_mut().unwrap().remove("nodeName"); // scheduler-owned; authored form forbids it
    for (key, value) in [
        ("dnsPolicy", json!("ClusterFirst")),
        ("schedulerName", json!("default-scheduler")),
        ("terminationGracePeriodSeconds", json!(30)),
        ("preemptionPolicy", json!("PreemptLowerPriority")),
        ("priority", json!(0)),
        ("hostNetwork", json!(false)),
        ("hostPID", json!(false)),
        ("hostIPC", json!(false)),
        ("hostUsers", json!(true)),
        ("shareProcessNamespace", json!(false)),
        ("setHostnameAsFQDN", json!(false)),
        ("imagePullSecrets", json!([])),
        ("initContainers", json!([])),
        ("ephemeralContainers", json!([])),
        ("readinessGates", json!([])),
        ("schedulingGates", json!([])),
        ("topologySpreadConstraints", json!([])),
        ("resourceClaims", json!([])),
        ("overhead", json!({})),
    ] {
        default(&mut v, key, value);
    }
    let sa = v["serviceAccountName"].clone();
    default(&mut v, "serviceAccount", sa);
    let known = [
        "node.kubernetes.io/not-ready",
        "node.kubernetes.io/unreachable",
    ];
    if let Some(rows) = v.get("tolerations").and_then(Value::as_array) {
        let keys: BTreeSet<_> = rows.iter().filter_map(|r| r["key"].as_str()).collect();
        if rows.len() <= 2 && keys.len() == rows.len() && rows.iter().all(|r|
            known.iter().any(|key| *r == json!({"key":key,"operator":"Exists","effect":"NoExecute","tolerationSeconds":300}))) {
            v.as_object_mut().unwrap().remove("tolerations");
        }
    }
    if let Some(sc) = v.get_mut("securityContext") {
        default(sc, "fsGroupChangePolicy", json!("Always"));
        default(sc, "supplementalGroupsPolicy", json!("Merge"));
        default(sc, "supplementalGroups", json!([]));
    }
    if let Some(volumes) = v.get_mut("volumes").and_then(Value::as_array_mut) {
        for volume in volumes {
            if let Some(pvc) = volume.get_mut("persistentVolumeClaim") {
                default(pvc, "readOnly", json!(false));
            }
            if let Some(dir) = volume.get_mut("emptyDir") {
                default(dir, "medium", json!(""));
                if let Some(size) = dir.get_mut("sizeLimit") {
                    *size = json!(quantity(
                        size.as_str().context("emptyDir quantity missing")?
                    )?);
                }
            }
            if let Some(secret) = volume.get_mut("secret") {
                default(secret, "defaultMode", json!(420));
                default(secret, "optional", json!(false));
            }
        }
    }
    let containers = v
        .get_mut("containers")
        .and_then(Value::as_array_mut)
        .context("containers missing")?;
    for c in containers {
        for (key, value) in [
            ("terminationMessagePath", json!("/dev/termination-log")),
            ("terminationMessagePolicy", json!("File")),
            ("command", json!([])),
            ("args", json!([])),
            ("env", json!([])),
            ("ports", json!([])),
            ("volumeDevices", json!([])),
            ("workingDir", json!("")),
            ("stdin", json!(false)),
            ("stdinOnce", json!(false)),
            ("tty", json!(false)),
        ] {
            default(c, key, value);
        }
        if let Some(env) = c.get_mut("env").and_then(Value::as_array_mut) {
            for item in env {
                default(item, "value", json!(""));
            }
        }
        if let Some(sc) = c.get_mut("securityContext") {
            default(sc, "privileged", json!(false));
            default(sc, "procMount", json!("Default"));
            if let Some(cap) = sc.get_mut("capabilities") {
                default(cap, "add", json!([]));
            }
        }
        if let Some(resources) = c.get_mut("resources") {
            quantities(resources)?;
        }
        if let Some(mounts) = c.get_mut("volumeMounts").and_then(Value::as_array_mut) {
            for mount in mounts {
                default(mount, "readOnly", json!(false));
                default(mount, "mountPropagation", json!("None"));
                default(mount, "recursiveReadOnly", json!("Disabled"));
                default(mount, "subPath", json!(""));
                default(mount, "subPathExpr", json!(""));
            }
        }
        for key in ["readinessProbe", "livenessProbe", "startupProbe"] {
            if let Some(probe) = c.get_mut(key) {
                for (field, value) in [
                    ("initialDelaySeconds", json!(0)),
                    ("timeoutSeconds", json!(1)),
                    ("periodSeconds", json!(10)),
                    ("successThreshold", json!(1)),
                    ("failureThreshold", json!(3)),
                ] {
                    default(probe, field, value);
                }
                if let Some(get) = probe.get_mut("httpGet") {
                    default(get, "scheme", json!("HTTP"));
                    default(get, "httpHeaders", json!([]));
                }
            }
        }
    }
    Ok(v)
}
impl Execution {
    pub(super) fn seal_authored_spec(&self) -> anyhow::Result<()> {
        ensure!(
            self.manifest["spec"].is_object() && self.manifest["spec"].get("nodeName").is_none(),
            "authored execution cannot select scheduler node"
        );
        let seal = Seal {
            schema: SEAL_SCHEMA.into(),
            action_sha256: self.intent.action_sha256.clone(),
            authored_spec_sha256: hash(&serde_json::to_vec(&normalized_spec(
                &self.manifest["spec"],
            )?)?),
            executor_kind: self.manifest["metadata"]["labels"]["pdlc.forge-executor"]
                .as_str()
                .context("executor kind missing")?
                .into(),
        };
        write_once(
            &self.directory.join("authored-spec.json"),
            &serde_json::to_vec(&seal)?,
        )
    }
    fn readonly_pod(&self, expected: &PodRequest, value: &Value) -> anyhow::Result<(Seal, bool)> {
        ensure!(
            self.directory.join("issued.json").exists()
                && self.directory.join("created.json").exists(),
            "issued/created authority missing"
        );
        let seal: Seal = read(&self.directory.join("authored-spec.json"))?;
        ensure!(
            seal.schema == SEAL_SCHEMA
                && seal.action_sha256 == self.intent.action_sha256
                && seal.executor_kind == "job",
            "execution feature is unqualified for management observation"
        );
        let uid = self.identity(value)?;
        ensure!(
            uid == expected.uid.to_string()
                && value["metadata"]["resourceVersion"] == expected.resource_version
                && self.intent.pod_name == expected.name
                && value["metadata"]["ownerReferences"]
                    .as_array()
                    .is_none_or(|v| v.is_empty())
                && value["metadata"]["labels"]["pdlc.installation"] == self.config.installation
                && value["metadata"]["labels"]["pdlc.forge-job"]
                    == self.intent.scope.job_id.to_string()
                && value["metadata"]["labels"]["pdlc.forge-attempt"]
                    == self.intent.scope.attempt_id.to_string()
                && value["metadata"]["labels"]["pdlc.forge-executor"] == "job",
            "observed Pod generation/ownership changed"
        );
        ensure!(
            hash(&serde_json::to_vec(&normalized_spec(&value["spec"])?)?)
                == seal.authored_spec_sha256,
            "observed Pod differs from sealed authored spec"
        );
        let terminal = super::terminal(value, &uid)?.is_some();
        if !terminal {
            let finalizers = value["metadata"]["finalizers"]
                .as_array()
                .context("custody finalizer missing")?;
            ensure!(
                finalizers.len() == 1 && finalizers[0] == FINALIZER,
                "unknown finalizer owner"
            );
        }
        Ok((seal, terminal))
    }
}
async fn empty_inventory_identities(config: &Config) -> anyhow::Result<()> {
    let (client, server, token_path) = transport(config)?;
    for (path, expected) in [
        (
            format!("/api/v1/namespaces/{}", config.namespace),
            &config.namespace_uid,
        ),
        (
            format!(
                "/api/v1/namespaces/{}/persistentvolumeclaims/{}",
                config.namespace, config.workspace_claim
            ),
            &config.workspace_claim_uid,
        ),
    ] {
        let mut request = client.get(format!("{server}{path}"));
        if let Some(token) = &token_path {
            request = request.bearer_auth(fs::read_to_string(token)?.trim());
        }
        let mut response = request.send().await?;
        ensure!(
            response.status().is_success(),
            "physical identity read unavailable"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= MAX_RESPONSE,
                "identity response bound exceeded"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            value["metadata"]["uid"].as_str() == Some(expected.as_str()),
            "physical identity replaced"
        );
        if path.contains("persistentvolumeclaims") {
            ensure!(
                value["status"]["phase"] == "Bound",
                "workspace claim not Bound"
            );
        }
    }
    Ok(())
}
fn scopes_for_observation(root: &Path) -> anyhow::Result<Vec<Scope>> {
    let control = root.join(".kubernetes-control");
    if !control.exists() {
        return Ok(Vec::new());
    }
    guarded(&control)?;
    let entries = fs::read_dir(control)?.collect::<Result<Vec<_>, _>>()?;
    ensure!(entries.len() <= 4096, "custody inventory exceeds bound");
    let mut scopes = Vec::new();
    for entry in entries {
        guarded(&entry.path())?;
        ensure!(entry.path().is_dir(), "unknown custody entry");
        let intent: Intent = read(&entry.path().join("intent.json"))?;
        intent.scope.validate()?;
        ensure!(
            entry.file_name().to_str() == Some(intent.scope.name().as_str()),
            "custody path/scope mismatch"
        );
        scopes.push(intent.scope);
    }
    Ok(scopes)
}
async fn inspect(
    pool: &PgPool,
    root: &Path,
    request: &Request,
    config: &Config,
) -> anyhow::Result<Vec<Observation>> {
    guarded(root)?;
    let scopes = scopes_for_observation(root)?;
    if request.pods.is_empty() {
        empty_inventory_identities(config).await?;
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await?;
    let workspaces = crate::runner_workspace::OwnedWorkspace::inventory(root)?;
    for workspace in &workspaces {
        let matches: Vec<_> = scopes
            .iter()
            .filter(|s| {
                s.attempt_id == workspace.attempt_id
                    && s.lease_id == workspace.lease_id
                    && s.generation == workspace.generation
            })
            .collect();
        ensure!(matches.len() <= 1, "multiple workspace custody");
        if let Some(scope) = matches.first() {
            let intent: Intent = read(
                &root
                    .join(".kubernetes-control")
                    .join(scope.name())
                    .join("intent.json"),
            )?;
            ensure!(
                intent.workspace == format!("{}/workspace", workspace.workspace_id),
                "workspace physical mapping mismatch"
            );
        } else {
            // Source checkout owns an exact lease before preparing any Pod intent.
            // This is known preparation, never permission to accept an API Pod.
            let row = sqlx::query_as::<_, (Uuid, Option<String>)>(
                "SELECT l.job_id,p.commit_sha FROM job_leases l JOIN jobs j ON j.id=l.job_id                  JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id                  WHERE l.id=$1 AND l.attempt_id=$2 AND l.generation=$3")
                .bind(workspace.lease_id).bind(workspace.attempt_id).bind(workspace.generation)
                .fetch_optional(&mut *tx).await?.context("unknown workspace lease")?;
            let scope = Scope {
                job_id: row.0,
                attempt_id: workspace.attempt_id,
                lease_id: workspace.lease_id,
                generation: workspace.generation,
                source_commit: row.1.context("workspace source pin missing")?,
            };
            scope.validate()?;
            ensure!(
                !request.pods.iter().any(|p| p.name == scope.name()),
                "API Pod lacks prepared custody"
            );
            let binding = db_binding(&mut tx, &scope).await?;
            ensure!(
                phase(&binding, &json!({"status":{"phase":"Pending"}}), false)? == "pending",
                "unprepared workspace is not a current source checkout"
            );
        }
    }
    for scope in &scopes {
        if request.pods.iter().any(|p| p.name == scope.name()) {
            continue;
        }
        let directory = root.join(".kubernetes-control").join(scope.name());
        if directory.join("cleaned.json").exists() {
            ensure!(
                read::<bool>(&directory.join("cleaned.json"))?,
                "cleanup record invalid"
            );
            let terminal: Terminal = read(&directory.join("terminal.json"))?;
            ensure!(
                read::<Terminal>(&directory.join("ack.json"))? == terminal
                    && read::<Terminal>(&directory.join("cleanup-intent.json"))? == terminal,
                "cleanup authority incomplete"
            );
        } else {
            ensure!(
                !directory.join("issued.json").exists(),
                "issued custody omitted from Pod census"
            );
            let binding = db_binding(&mut tx, scope).await?;
            phase(&binding, &json!({"status":{"phase":"Pending"}}), false)?;
        }
    }
    let mut observations = Vec::new();
    for expected in &request.pods {
        let matching: Vec<_> = scopes
            .iter()
            .filter(|s| s.name() == expected.name)
            .collect();
        ensure!(matching.len() == 1, "unknown/multiple Pod custody");
        let scope = matching[0];
        let execution = Execution::reopen(root, scope.attempt_id, scope.generation)?;
        ensure!(
            execution.intent.scope == *scope
                && execution.config.installation_sha256 == config.installation_sha256,
            "custody installation/scope changed"
        );
        execution.identities().await?;
        let before = db_binding(&mut tx, scope).await?;
        let value = execution
            .get(&execution.path())
            .await?
            .context("original Pod missing")?;
        let (seal, terminal) = execution.readonly_pod(expected, &value)?;
        let state = phase(&before, &value, terminal)?;
        ensure!(
            db_binding(&mut tx, scope).await? == before,
            "DB authority changed during observation"
        );
        observations.push(Observation {
            pod_name: expected.name.clone(),
            pod_uid: expected.uid,
            resource_version: expected.resource_version.clone(),
            job_id: scope.job_id,
            attempt_id: scope.attempt_id,
            lease_id: scope.lease_id,
            generation: scope.generation,
            source_commit: scope.source_commit.clone(),
            runner_id: before.runner_id,
            executor_kind: seal.executor_kind,
            authored_spec_sha256: seal.authored_spec_sha256,
            phase: state,
        });
    }
    tx.rollback().await?;
    Ok(observations)
}

/// The CLI prints only this bounded DTO; error context cannot leak env/credentials.
pub async fn command(bytes: &[u8], request_sha: &str) -> Value {
    let parsed = serde_json::from_slice::<Request>(bytes);
    let challenge = parsed.as_ref().ok().map(|r| r.challenge);
    let outcome = async {
        ensure!(bytes.len() <= 64 * 1024 && hash(bytes) == request_sha, "request hash/size mismatch");
        let request = parsed?;
        let path = std::env::var("CICD_KUBERNETES_RUNNER_CONFIG")?;
        let raw = fs::read(path)?;
        ensure!(raw.len() <= 64 * 1024, "config bound exceeded");
        let config: Config = serde_json::from_slice(&raw)?;
        config.validate()?;
        request.validate(&config, &hash(&raw))?;
        let runtime = crate::config::RuntimeConfig::from_env()?;
        let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1)
            .acquire_timeout(Duration::from_secs(3)).connect(&runtime.database.url).await?;
        let result = tokio::time::timeout(Duration::from_secs(15),
            inspect(&pool, Path::new("/workspaces"), &request, &config)).await;
        pool.close().await;
        let observations = result.context("observation deadline exceeded")??;
        Ok::<_, anyhow::Error>(json!({"schema":SCHEMA,"status":"VERIFIED","challenge":request.challenge,
            "requestSha256":request_sha,"installationSha256":request.installation_sha256,"configSha256":request.config_sha256,
            "namespace":request.namespace,"namespaceUid":request.namespace_uid,
            "workspaceClaim":request.workspace_claim,"workspaceClaimUid":request.workspace_claim_uid,
            "observations":observations,"lifecycleEffectsPerformed":false,"sourceWritersClosed":false}))
    }.await;
    outcome.unwrap_or_else(|_| json!({"schema":SCHEMA,"status":"HOLD","challenge":challenge,
        "requestSha256":request_sha,"reasonCode":"custody_unverified","lifecycleEffectsPerformed":false,
        "sourceWritersClosed":false}))
}

#[cfg(test)]
mod tests;
