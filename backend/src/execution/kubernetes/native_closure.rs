//! Failure-only use of a privileged operator's native negative observation.
//! The API's missing finishedAt is never converted into a fabricated timestamp.
use super::*;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const PATH: &str = "/run/forge-native-closures/closures.json";
const KIND: &str = "forge/native-execution-closures/v1";
const UNADMITTED_PATH: &str = "/run/forge-native-closures/unadmitted.json";
const UNADMITTED_KIND: &str = "forge/native-unadmitted-attempt/v1";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observation {
    schema_version: u32,
    kind: String,
    installation_sha256: String,
    cluster_ca_sha256: String,
    namespace_uid: String,
    workspace_claim_uid: String,
    node_uid: String,
    node_generation: Vec<Value>,
    observed_epoch: i64,
    pods: Vec<Closure>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Closure {
    pod_uid: String,
    resource_version: String,
    name: String,
    node_name: String,
    reason: String,
    native_closed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UnadmittedAttempt {
    schema_version: u32,
    kind: String,
    installation_sha256: String,
    cluster_ca_sha256: String,
    namespace_uid: String,
    workspace_claim_uid: String,
    scope: Scope,
    pod_name: String,
    intent_sha256: String,
    issued_epoch: i64,
    observed_epoch: i64,
    storage_history_sha256: String,
}

fn enabled() -> bool {
    std::env::var("CICD_KUBERNETES_NATIVE_CLOSURES")
        .ok()
        .as_deref()
        == Some("true")
}

pub(super) fn observe_unadmitted(execution: &Execution) -> anyhow::Result<Option<Value>> {
    if !enabled() || !Path::new(UNADMITTED_PATH).try_exists()? {
        return Ok(None);
    }
    let proof: UnadmittedAttempt =
        serde_json::from_slice(&read_operator_observation(UNADMITTED_PATH)?)?;
    if proof.scope != execution.intent.scope {
        return Ok(None);
    }
    let issued = execution.directory.join("issued.json");
    guarded(&issued)?;
    let issued_epoch: i64 = fs::metadata(issued)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .try_into()?;
    qualify_unadmitted(
        &execution.intent,
        &proof,
        issued_epoch,
        chrono::Utc::now().timestamp(),
    )
    .map(Some)
}

fn qualify_unadmitted(
    intent: &Intent,
    proof: &UnadmittedAttempt,
    issued_epoch: i64,
    now: i64,
) -> anyhow::Result<Value> {
    let config = &intent.config;
    ensure!(
        proof.schema_version == 1
            && proof.kind == UNADMITTED_KIND
            && proof.installation_sha256 == config.installation_sha256
            && proof.cluster_ca_sha256 == config.cluster_ca_sha256
            && proof.namespace_uid == config.namespace_uid
            && proof.workspace_claim_uid == config.workspace_claim_uid
            && proof.scope == intent.scope
            && proof.pod_name == intent.pod_name
            && proof.intent_sha256 == hash(&serde_json::to_vec(intent)?)
            && proof.issued_epoch == issued_epoch
            && issued_epoch > 0
            && proof.observed_epoch > issued_epoch
            && (now.saturating_sub(90)..=now).contains(&proof.observed_epoch)
            && sha256(&proof.storage_history_sha256),
        "non-admission observation is stale or differs from the original issued attempt"
    );
    Ok(json!({"kind":"operator-storage-history","sha256":proof.storage_history_sha256}))
}

pub(super) fn observe(pod: &Value, uid: &str, config: &Config) -> anyhow::Result<Option<Terminal>> {
    if !enabled() {
        return Ok(None);
    }
    let bytes = read_operator_observation(PATH)?;
    let proof: Observation = serde_json::from_slice(&bytes)?;
    qualify(pod, uid, config, &proof, chrono::Utc::now().timestamp()).map(Some)
}

#[cfg(unix)]
fn read_operator_observation(path: &str) -> anyhow::Result<Vec<u8>> {
    // A runner workspace or repository must never supply its own privileged proof.
    let mounts = fs::read_to_string("/proc/self/mountinfo")?;
    ensure!(
        mounts.lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields.len() > 6
                && fields[4] == "/run/forge-native-closures"
                && fields[5].split(',').any(|option| option == "ro")
        }),
        "native closure observation requires its separate read-only operator mount"
    );
    let metadata = fs::metadata(path)?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.len() <= 65536,
        "native closure observation is not operator-owned"
    );
    let bytes = fs::read(path)?;
    Ok(bytes)
}
#[cfg(not(unix))]
fn read_operator_observation(_path: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::bail!("native closure observation requires a Unix operator mount")
}

fn qualify(
    pod: &Value,
    uid: &str,
    config: &Config,
    proof: &Observation,
    now: i64,
) -> anyhow::Result<Terminal> {
    ensure!(
        proof.schema_version == 1
            && proof.kind == KIND
            && proof.installation_sha256 == config.installation_sha256
            && proof.cluster_ca_sha256 == config.cluster_ca_sha256
            && proof.namespace_uid == config.namespace_uid
            && proof.workspace_claim_uid == config.workspace_claim_uid
            && Uuid::parse_str(&proof.node_uid).is_ok()
            && proof.node_generation.len() == 3
            && proof.observed_epoch > 0
            && proof.observed_epoch <= now + 5
            && now - proof.observed_epoch <= 90
            && proof.pods.len() <= 64,
        "native closure context is stale or foreign"
    );
    let matches: Vec<_> = proof
        .pods
        .iter()
        .filter(|entry| entry.pod_uid == uid)
        .collect();
    ensure!(
        matches.len() == 1,
        "native closure needs exactly one matching Pod"
    );
    let entry = matches[0];
    ensure!(
        entry.native_closed
            && entry.reason == "container-not-created"
            && pod["metadata"]["uid"] == uid
            && pod["metadata"]["resourceVersion"] == entry.resource_version
            && pod["metadata"]["name"] == entry.name
            && pod["metadata"]["namespace"] == config.namespace
            && pod["spec"]["nodeName"] == entry.node_name
            && !pod["metadata"]["deletionTimestamp"].is_null()
            && pod["spec"]["restartPolicy"] == "Never"
            && pod["status"]["phase"] == "Failed",
        "native closure differs from its actual terminal Pod"
    );
    for key in ["hostPID", "hostIPC", "hostNetwork"] {
        ensure!(
            pod["spec"][key].is_null() || pod["spec"][key] == false,
            "host execution cannot use unstarted closure"
        );
    }
    for key in ["initContainers", "ephemeralContainers"] {
        ensure!(
            pod["spec"][key].is_null()
                || pod["spec"][key]
                    .as_array()
                    .is_some_and(|rows| rows.is_empty()),
            "additional containers cannot use unstarted closure"
        );
    }
    let expected = pod["spec"]["containers"]
        .as_array()
        .context("execution container missing")?;
    let rows = pod["status"]["containerStatuses"]
        .as_array()
        .context("execution status missing")?;
    ensure!(
        expected.len() == 1
            && rows.len() == 1
            && expected[0]["name"] == "execution"
            && rows[0]["name"] == "execution",
        "unstarted closure requires the complete single-container inventory"
    );
    let row = &rows[0];
    let absent = |value: &Value| value.is_null() || value.as_str() == Some("");
    let state = &row["state"]["terminated"];
    ensure!(
        absent(&row["containerID"])
            && absent(&row["imageID"])
            && row["restartCount"] == 0
            && row["started"] == false
            && row["lastState"]
                .as_object()
                .is_some_and(|value| value.is_empty())
            && state["reason"] == "ContainerStatusUnknown"
            && state["exitCode"] == 137
            && absent(&state["startedAt"])
            && absent(&state["finishedAt"])
            && pod["status"]["conditions"]
                .as_array()
                .is_some_and(|rows| rows
                    .iter()
                    .any(|c| c["type"] == "PodReadyToStartContainers" && c["status"] == "False")),
        "native closure cannot replace an unknown started or live execution"
    );
    // This is a failure sentinel, not an invented process exit code or finish time.
    Ok(Terminal {
        pod_uid: uid.into(),
        exit_code: 137,
        reason: "NativeVerifiedNotStarted".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Config, Observation) {
        let config: Config = serde_json::from_value(json!({
            "schemaVersion":1, "installationSha256":"a".repeat(64), "installation":"qualified",
            "clusterCaSha256":"b".repeat(64), "namespace":"qualified",
            "namespaceUid":"11111111-1111-4111-8111-111111111111",
            "workspaceClaim":"work", "workspaceClaimUid":"22222222-2222-4222-8222-222222222222",
            "jobServiceAccount":"jobs",
            "resources":{"cpuMillis":200,"memoryBytes":402653184,"ephemeralStorageBytes":268435456},
            "security":{"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001}
        }))
        .unwrap();
        let pod = json!({"metadata":{"name":"execution","namespace":"qualified","uid":"pod-uid",
            "resourceVersion":"123","deletionTimestamp":"2026-11-03T13:04:01Z"},
            "spec":{"nodeName":"qa","restartPolicy":"Never","containers":[{"name":"execution"}]},
            "status":{"phase":"Failed","conditions":[{"type":"PodReadyToStartContainers","status":"False"}],
                "containerStatuses":[{"name":"execution","imageID":"","restartCount":0,"started":false,
                    "lastState":{},"state":{"terminated":{"reason":"ContainerStatusUnknown","exitCode":137}}}]}});
        let proof: Observation = serde_json::from_value(json!({
            "schemaVersion":1,"kind":KIND,"installationSha256":config.installation_sha256,
            "clusterCaSha256":config.cluster_ca_sha256,"namespaceUid":config.namespace_uid,
            "workspaceClaimUid":config.workspace_claim_uid,
            "nodeUid":"33333333-3333-4333-8333-333333333333",
            "nodeGeneration":["k3s.service","invocation",1234],"observedEpoch":1000,
            "pods":[{"podUid":"pod-uid","resourceVersion":"123","name":"execution","nodeName":"qa",
                "reason":"container-not-created","nativeClosed":true}]}))
        .unwrap();
        (pod, config, proof)
    }
    #[test]
    fn exact_fresh_native_closure_is_failure_without_fabricating_a_timestamp() {
        let (pod, config, proof) = fixture();
        let terminal = qualify(&pod, "pod-uid", &config, &proof, 1001).unwrap();
        assert_eq!(terminal.exit_code, 137);
        assert_eq!(terminal.reason, "NativeVerifiedNotStarted");
        assert!(
            pod["status"]["containerStatuses"][0]["state"]["terminated"]["finishedAt"].is_null()
        );
    }
    #[test]
    fn stale_foreign_or_unconfirmed_native_observation_is_rejected() {
        for change in 0..7 {
            let (pod, config, mut proof) = fixture();
            match change {
                0 => proof.observed_epoch = 900,
                1 => proof.namespace_uid = "foreign".into(),
                2 => proof.workspace_claim_uid = "foreign".into(),
                3 => proof.pods[0].resource_version = "old".into(),
                4 => proof.pods[0].native_closed = false,
                5 => proof.pods[0].pod_uid = "foreign".into(),
                _ => proof.observed_epoch = i64::MIN,
            }
            assert!(qualify(&pod, "pod-uid", &config, &proof, 1001).is_err());
        }
    }
    #[test]
    fn operator_history_must_bind_the_exact_issued_intent_and_fresh_observation() {
        let (_, config, _) = fixture();
        let scope = Scope {
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 2,
            source_commit: "a".repeat(40),
        };
        let intent = Intent {
            config,
            pod_name: scope.name(),
            scope,
            workspace: "attempt-fixture/workspace".into(),
            image: "unused".into(),
            action_sha256: "a".repeat(64),
            environment_sha256: "b".repeat(64),
        };
        let proof = json!({
            "schemaVersion":1,"kind":UNADMITTED_KIND,
            "installationSha256":intent.config.installation_sha256,
            "clusterCaSha256":intent.config.cluster_ca_sha256,
            "namespaceUid":intent.config.namespace_uid,
            "workspaceClaimUid":intent.config.workspace_claim_uid,
            "scope":intent.scope,"podName":intent.pod_name,
            "intentSha256":hash(&serde_json::to_vec(&intent).unwrap()),
            "issuedEpoch":800,"observedEpoch":1000,
            "storageHistorySha256":"c".repeat(64)
        });
        let valid: UnadmittedAttempt = serde_json::from_value(proof.clone()).unwrap();
        assert_eq!(
            qualify_unadmitted(&intent, &valid, 800, 1001).unwrap(),
            json!({"kind":"operator-storage-history","sha256":"c".repeat(64)})
        );
        assert!(qualify_unadmitted(&intent, &valid, 801, 1001).is_err());
        assert!(qualify_unadmitted(&intent, &valid, 800, 1091).is_err());
        for (field, value) in [
            ("podName", json!("another-pod")),
            ("namespaceUid", json!("other-namespace")),
            ("intentSha256", json!("e".repeat(64))),
            ("storageHistorySha256", json!("incomplete")),
        ] {
            let mut foreign = proof.clone();
            foreign[field] = value;
            let foreign = serde_json::from_value(foreign).unwrap();
            assert!(qualify_unadmitted(&intent, &foreign, 800, 1001).is_err());
        }
        let mut stale = proof;
        stale["scope"]["generation"] = json!(1);
        let stale = serde_json::from_value(stale).unwrap();
        assert!(qualify_unadmitted(&intent, &stale, 800, 1001).is_err());
    }
    #[test]
    fn neighboring_started_running_or_successful_status_cannot_use_negative_proof() {
        for change in 0..6 {
            let (mut pod, config, proof) = fixture();
            match change {
                0 => {
                    pod["status"]["containerStatuses"][0]["containerID"] =
                        json!("containerd://known")
                }
                1 => {
                    pod["status"]["containerStatuses"][0]["state"]["terminated"]["startedAt"] =
                        json!("known")
                }
                2 => pod["status"]["phase"] = json!("Running"),
                3 => {
                    pod["status"]["containerStatuses"][0]["state"]["terminated"]["exitCode"] =
                        json!(0)
                }
                4 => pod["status"]["containerStatuses"][0]["restartCount"] = json!(1),
                _ => pod["metadata"]["deletionTimestamp"] = Value::Null,
            }
            assert!(qualify(&pod, "pod-uid", &config, &proof, 1001).is_err());
        }
    }
}
