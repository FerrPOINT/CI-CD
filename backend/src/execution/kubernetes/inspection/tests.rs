use super::*;
use std::fs;
struct Fixture {
    root: PathBuf,
    execution: Execution,
    expected: PodRequest,
    actual: Value,
}
impl Fixture {
    fn new() -> Self {
        let config = super::super::tests::config();
        let scope = Scope { job_id: Uuid::new_v4(), attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(), generation: 1, source_commit: "a".repeat(40) };
        let root = std::env::temp_dir().join(format!("forge-readonly-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let manifest = super::super::pod(&config, &scope, &scope.name(), "attempt-owned/workspace",
            &config.allowed_images[0], &["sh".into(), "-lc".into(), "echo scoped".into()],
            &[("DECLARED".into(), "test-only-secret-must-not-be-persisted".into()), ("EMPTY".into(), "".into())], 60);
        let action = hash(&serde_json::to_vec(&manifest).unwrap());
        let execution = Execution { config: config.clone(), client: Client::new(), server: "https://example.invalid".into(),
            token_path: None, directory: root.clone(), intent: Intent { config, scope: scope.clone(),
                pod_name: scope.name(), workspace: "attempt-owned/workspace".into(), image: manifest["spec"]["containers"][0]["image"].as_str().unwrap().into(),
                action_sha256: action.clone(), environment_sha256: "d".repeat(64) }, manifest: manifest.clone() };
        execution.seal_authored_spec().unwrap();
        let expected = PodRequest { name: scope.name(), uid: Uuid::new_v4(), resource_version: "100".into() };
        write_once(&root.join("issued.json"), b"true").unwrap();
        write_once(&root.join("created.json"), &serde_json::to_vec(&expected.uid.to_string()).unwrap()).unwrap();
        let mut actual = manifest;
        actual["metadata"]["uid"] = json!(expected.uid);
        actual["metadata"]["resourceVersion"] = json!(expected.resource_version);
        actual["metadata"]["annotations"]["forge.pdlc.acm-ai.ru/intent"] = json!(action);
        actual["spec"]["nodeName"] = json!("scheduled-node");
        actual["spec"]["dnsPolicy"] = json!("ClusterFirst");
        actual["spec"]["schedulerName"] = json!("default-scheduler");
        actual["spec"]["serviceAccount"] = actual["spec"]["serviceAccountName"].clone();
        actual["spec"]["tolerations"] = json!([
            {"key":"node.kubernetes.io/not-ready","operator":"Exists","effect":"NoExecute","tolerationSeconds":300},
            {"key":"node.kubernetes.io/unreachable","operator":"Exists","effect":"NoExecute","tolerationSeconds":300}]);
        actual["spec"]["containers"][0]["resources"]["requests"]["memory"] = json!("256Mi");
        actual["spec"]["containers"][0]["resources"]["limits"]["memory"] = json!("256Mi");
        actual["spec"]["containers"][0]["resources"]["requests"]["cpu"] = json!("0.5");
        actual["spec"]["containers"][0]["resources"]["limits"]["cpu"] = json!("0.5");
        actual["spec"]["containers"][0]["env"][1].as_object_mut().unwrap().remove("value");
        actual["spec"]["containers"][0]["terminationMessagePath"] = json!("/dev/termination-log");
        actual["spec"]["containers"][0]["terminationMessagePolicy"] = json!("File");
        actual["spec"]["volumes"][0]["persistentVolumeClaim"]["readOnly"] = json!(false);
        actual["status"] = json!({"phase":"Running","containerStatuses":[{"name":"execution","restartCount":0,"state":{"running":{"startedAt":"known"}}}]});
        Self { root, execution, expected, actual }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { fs::remove_dir_all(&self.root).unwrap(); }
}
fn binding() -> Binding {
    Binding { job_status:"running".into(), attempt_status:"running".into(), lease_status:"active".into(),
        runner_id:None, runner_name:"embedded".into(), runner_exists:true, runner_acknowledged:false,
        lease_current:true, canceled:false, completed:false, completion_received:false, terminal_status:None,
        queue_state:"leased".into(), source_commit:Some("a".repeat(40)) }
}
#[test]
fn readonly_accepts_exact_authored_spec_api_defaults_and_persists_no_secrets() {
    let f = Fixture::new();
    let before = fs::read(f.root.join("authored-spec.json")).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains("test-only-secret"));
    assert!(!String::from_utf8_lossy(&before).contains("echo scoped"));
    let (_, terminal) = f.execution.readonly_pod(&f.expected, &f.actual).unwrap();
    assert!(!terminal);
    assert_eq!(fs::read(f.root.join("authored-spec.json")).unwrap(), before);
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 3);
    assert_eq!(phase(&binding(), &f.actual, false).unwrap(), "running");
}
#[test]
fn readonly_rejects_command_env_security_mount_image_uid_and_unknown_default_drift() {
    for target in ["command","env","security","mount","image","uid","resourceVersion","unknown","toleration"] {
        let mut f = Fixture::new();
        match target {
            "command" => f.actual["spec"]["containers"][0]["command"] = json!(["sh","-c","foreign"]),
            "env" => f.actual["spec"]["containers"][0]["env"][0]["value"] = json!("rotated-or-foreign"),
            "security" => f.actual["spec"]["containers"][0]["securityContext"]["allowPrivilegeEscalation"] = json!(true),
            "mount" => f.actual["spec"]["containers"][0]["volumeMounts"][0]["subPath"] = json!("foreign/workspace"),
            "image" => f.actual["spec"]["containers"][0]["image"] = json!("foreign:latest"),
            "uid" => f.actual["metadata"]["uid"] = json!(Uuid::new_v4()),
            "resourceVersion" => f.actual["metadata"]["resourceVersion"] = json!("101"),
            "unknown" => f.actual["spec"]["hostAliases"] = json!([{"ip":"127.0.0.1","hostnames":["foreign"]}]),
            _ => f.actual["spec"]["tolerations"][0]["tolerationSeconds"] = json!(301),
        }
        assert!(f.execution.readonly_pod(&f.expected, &f.actual).is_err(), "{target}");
        assert!(!f.root.join("terminal.json").exists());
        assert!(!f.root.join("cleaned.json").exists());
    }
}
#[test]
fn historical_missing_seal_and_unknown_created_reply_never_get_blessed() {
    for missing in ["authored-spec.json","created.json","issued.json"] {
        let f = Fixture::new();
        fs::remove_file(f.root.join(missing)).unwrap();
        assert!(f.execution.readonly_pod(&f.expected, &f.actual).is_err(), "{missing}");
        assert!(!f.root.join(missing).exists());
    }
}
#[test]
fn readonly_is_not_terminal_ack_and_expiry_or_projection_conflict_is_hold() {
    let f = Fixture::new();
    let mut b = binding();
    b.lease_current = false;
    assert!(phase(&b, &f.actual, false).is_err());
    b = binding(); b.job_status = "success".into();
    assert!(phase(&b, &f.actual, true).is_err());
    b = binding(); b.canceled = true;
    assert_eq!(phase(&b, &f.actual, false).unwrap(), "canceling");
    b.job_status="canceled".into(); b.attempt_status="canceled".into();
    b.lease_status="canceled".into(); b.queue_state="canceled".into();
    assert_eq!(phase(&b, &f.actual, false).unwrap(), "canceling");
    assert!(phase(&b, &f.actual, true).is_err());
    b.completed=true; b.terminal_status=Some("canceled".into());
    assert_eq!(phase(&b, &f.actual, true).unwrap(), "cleanup-pending");
    b.runner_id=Some(Uuid::new_v4());
    assert!(phase(&b, &f.actual, true).is_err());
    b.completion_received=true;
    assert_eq!(phase(&b, &f.actual, true).unwrap(), "cleanup-pending");
}
#[test]
fn terminal_requires_complete_container_stop_and_no_writes() {
    let mut f = Fixture::new();
    f.actual["status"] = json!({"phase":"Succeeded","containerStatuses":[{"name":"execution","restartCount":0,
        "state":{"terminated":{"exitCode":0,"reason":"Completed","finishedAt":"known"}}}]});
    let (_, terminal) = f.execution.readonly_pod(&f.expected, &f.actual).unwrap();
    assert!(terminal);
    assert!(!f.root.join("terminal.json").exists());
    f.actual["status"]["containerStatuses"][0]["restartCount"] = json!(1);
    assert!(f.execution.readonly_pod(&f.expected, &f.actual).is_err());
}
#[test]
fn exact_resource_normalization_never_rounds_or_ignores_unknown_quantities() {
    for (a,b) in [("256Mi","268435456"),("1000m","1"),("0.5","500m"),("1Gi","1024Mi")] {
        assert_eq!(quantity(a).unwrap(),quantity(b).unwrap());
    }
    for value in ["-1","1e3","1.0000000001","NaN","1.2.3","1foreign"] { assert!(quantity(value).is_err()); }
    assert_ne!(quantity("256Mi").unwrap(),quantity("256M").unwrap());
}
#[test]
fn request_is_exact_challenged_namespace_config_and_complete_unique_pod_set() {
    let f = Fixture::new();
    let mut r = Request { schema_version:1, challenge:Uuid::new_v4(),
        installation_sha256:f.execution.config.installation_sha256.clone(),config_sha256:"d".repeat(64),
        namespace:f.execution.config.namespace.clone(),namespace_uid:f.execution.config.namespace_uid.clone(),
        workspace_claim:f.execution.config.workspace_claim.clone(),workspace_claim_uid:f.execution.config.workspace_claim_uid.clone(),
        pods:vec![f.expected.clone()] };
    r.validate(&f.execution.config,&"d".repeat(64)).unwrap();
    r.pods.push(r.pods[0].clone());
    assert!(r.validate(&f.execution.config,&"d".repeat(64)).is_err());
    r.pods.pop(); r.namespace_uid=Uuid::new_v4().to_string();
    assert!(r.validate(&f.execution.config,&"d".repeat(64)).is_err());
}
#[tokio::test]
async fn malformed_or_unbound_request_returns_only_sanitized_hold() {
    let raw = br#"{"secret":"must-not-appear-in-output"}"#;
    let value = command(raw, &"0".repeat(64)).await;
    assert_eq!(value["status"], "HOLD");
    assert_eq!(value["lifecycleEffectsPerformed"], false);
    assert!(!value.to_string().contains("must-not-appear"));
}

#[test]
fn catalog_does_not_silently_skip_unknown_or_reanchored_control_entries() {
    let f = Fixture::new();
    assert!(scopes_for_observation(&f.root).unwrap().is_empty());
    let control=f.root.join(".kubernetes-control");
    fs::create_dir(&control).unwrap();
    let foreign=control.join("foreign");
    fs::create_dir(&foreign).unwrap();
    assert!(scopes_for_observation(&f.root).is_err());
    write_once(&foreign.join("intent.json"),&serde_json::to_vec(&f.execution.intent).unwrap()).unwrap();
    assert!(scopes_for_observation(&f.root).is_err());
    fs::rename(&foreign,control.join(f.execution.intent.scope.name())).unwrap();
    assert_eq!(scopes_for_observation(&f.root).unwrap(),vec![f.execution.intent.scope.clone()]);
}
