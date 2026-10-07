use super::*;
use std::os::unix::fs::PermissionsExt;

struct Oci {
    t: ActualDelivery,
    root: PathBuf,
    data: PathBuf,
    policy: PathBuf,
    project: String,
}
impl Drop for Oci {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(std::env::var("CICD_TEST_OCI_COMPOSE_ROOT").unwrap())
        {
            let mut files = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.exists())
                .collect::<Vec<_>>();
            files.sort_by_key(|p| std::fs::metadata(p).unwrap().modified().unwrap());
            if let Some(file) = files.last() {
                std::fs::copy(file, "/output/oci-compose.json").unwrap();
                let status = std::process::Command::new("/usr/local/bin/docker-compose")
                    .args(["-p", &self.project, "-f"])
                    .arg(file)
                    .args(["down", "--remove-orphans"])
                    .env("DOCKER_HOST", "unix:///var/run/docker.sock")
                    .status()
                    .unwrap();
                assert!(status.success(), "owned OCI cleanup failed");
            }
        }
    }
}
impl Oci {
    async fn new() -> Self {
        let t = ActualDelivery::new().await;
        let root = PathBuf::from("/delivery-qa/oci-root");
        let data = PathBuf::from("/delivery-qa/oci-data.json");
        std::fs::write(
            &data,
            br#"{"schemaVersion":1,"records":[{"id":"task-1","state":"queued"}]}"#,
        )
        .unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o444)).unwrap();
        let project = std::env::var("CICD_TEST_OCI_PROJECT").unwrap();
        let id = std::process::Command::new("docker")
            .args(["info", "--format", "{{json .ID}}"])
            .output()
            .unwrap();
        assert!(id.status.success());
        let daemon: serde_json::Value = serde_json::from_slice(&id.stdout).unwrap();
        let policy = t.temp.join("oci-policy.json");
        std::fs::write(&policy,serde_json::to_vec(&serde_json::json!({"projectName":project,"networkName":std::env::var("CICD_TEST_OCI_NETWORK").unwrap(),"daemonId":daemon,"dockerBin":"/usr/local/bin/docker","composeBin":"/usr/local/bin/docker-compose","composeRoot":std::env::var("CICD_TEST_OCI_COMPOSE_ROOT").unwrap(),"volumeName":std::env::var("CICD_TEST_OCI_VOLUME").unwrap(),"volumeRoot":"/delivery-qa","dataFile":data,"dataSha256":hash(&std::fs::read(&data).unwrap()),"checks":{"origin":"http://127.0.0.1:8000","healthPath":"/health","healthBodySha256":hash(b"ok\n"),"acceptancePath":"/acceptance","acceptanceBodySha256":hash(b"accepted:task-1:queued\n")}})).unwrap()).unwrap();
        Self {
            t,
            root,
            data,
            policy,
            project,
        }
    }
    async fn build(
        &mut self,
        version: &str,
        schemas: &[u32],
        migrations: &[&str],
    ) -> DeliveryCommand {
        std::fs::write(
            self.t.source.join("app.py"),
            include_bytes!("../helpers/oci_application.py"),
        )
        .unwrap();
        std::fs::write(self.t.source.join("app-version"), version).unwrap();
        std::fs::write(self.t.source.join("Dockerfile"),"FROM python:3.12-bookworm@sha256:e91fec3d1ac69f04e4eddcd29c327e630ce34658cf31075bfa7e8b0e052bafea\nCOPY app.py /app.py\nCOPY app-version /app-version\nEXPOSE 8000\nENTRYPOINT [\"python\",\"-B\",\"/app.py\"]\n").unwrap();
        let schemas = serde_json::to_string(schemas).unwrap();
        let migrations = serde_json::to_string(migrations).unwrap();
        let label = if version == "wrong-image-commit" {
            "invalid"
        } else {
            "$commit"
        };
        let script = format!(
            "commit=$(git rev-parse HEAD); docker build --pull=false --label org.opencontainers.image.revision=\"{label}\" --iidfile image-id .; image=$(cat image-id); printf '{{\"schema\":\"forge/oci-candidate/v1\",\"imageId\":\"%s\",\"sourceCommit\":\"%s\",\"dataProtocol\":\"readonly_snapshot_v1\",\"readableSchemaVersions\":{schemas},\"migrations\":{migrations}}}' \"$image\" \"$commit\" > product.txt"
        );
        self.t.build_script(version, &script).await.0
    }
    fn process(&self, command: &DeliveryCommand) -> tokio::process::Command {
        let mut child = self.t.process(command);
        child
            .arg("--oci")
            .env("CICD_LOCAL_OCI_ROOT", &self.root)
            .env("CICD_LOCAL_OCI_POLICY", &self.policy);
        child
    }
    async fn execute(
        &self,
        command: &DeliveryCommand,
        mode: Option<&str>,
    ) -> (i32, Option<serde_json::Value>) {
        let mut child = self.process(command);
        if let Some(mode) = mode {
            child.arg(mode);
        }
        let output = tokio::time::timeout(Duration::from_secs(50), child.output())
            .await
            .unwrap()
            .unwrap();
        if !output.status.success() {
            eprintln!("OCI_CLI_EXIT={:?}", output.status.code());
        }
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stdout).ok(),
        )
    }
}

#[tokio::test]
async fn sdlc_oci_delivery_actual_image_data_acceptance_rollback_and_unknown() {
    let mut t = Oci::new().await;
    let data = std::fs::read(&t.data).unwrap();
    let a = t.build("A", &[1], &[]).await;
    let (code, first) = t.execute(&a, None).await;
    assert_eq!(code, 0);
    let first = first.unwrap();
    assert_eq!(latest(&first)["compatibility"]["status"], "verified");
    assert_eq!(latest(&first)["dataSha256"], hash(&data));
    let manifest = first["confirmedManifestSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let container = latest(&first)["containerId"].clone();
    assert_eq!(t.execute(&a, None).await.1.unwrap(), first);
    let mut b = t.build("B", &[1], &[]).await;
    b.expected_manifest_sha256 = Some(manifest.clone());
    let failed = t.execute(&b, None).await.1.unwrap();
    assert_eq!(latest(&failed)["status"], "failed");
    assert_eq!(latest(&failed)["health"]["httpStatus"], 503);
    assert_eq!(failed["confirmedManifestSha256"], manifest);
    let mut rollback = b.clone();
    rollback.operation_key = "oci:rollback:B".into();
    rollback.action = DeliveryAction::Rollback;
    rollback.artifact_id = None;
    rollback.expected_manifest_sha256 = failed["currentManifestSha256"].as_str().map(str::to_owned);
    let restored = t.execute(&rollback, None).await.1.unwrap();
    assert_eq!(latest(&restored)["status"], "verified");
    assert_eq!(restored["currentManifestSha256"], manifest);
    assert_eq!(latest(&restored)["imageId"], latest(&first)["imageId"]);
    assert_ne!(latest(&restored)["containerId"], container);
    assert_eq!(std::fs::read(&t.data).unwrap(), data);
    let mut c = t.build("C", &[1], &[]).await;
    c.expected_manifest_sha256 = Some(manifest.clone());
    let failed = t.execute(&c, None).await.1.unwrap();
    assert_eq!(latest(&failed)["acceptance"]["httpStatus"], 422);
    assert_eq!(failed["confirmedManifestSha256"], manifest);
    rollback.operation_key = "oci:rollback:C".into();
    rollback.workspace_operation_key = c.workspace_operation_key.clone();
    rollback.original = c.original.clone();
    rollback.expected_manifest_sha256 = failed["currentManifestSha256"].as_str().map(str::to_owned);
    assert_eq!(t.execute(&rollback, None).await.0, 0);
    let mut wrong = t.build("schema2", &[2], &[]).await;
    wrong.expected_manifest_sha256 = Some(manifest.clone());
    let blocked = t.execute(&wrong, None).await;
    assert_eq!(blocked.0, 1);
    assert_eq!(
        blocked.1.unwrap()["reason"],
        "actual_data_schema_incompatible"
    );
    assert_eq!(
        t.execute(&a, Some("--readback")).await.1.unwrap()["currentManifestSha256"],
        manifest
    );
    let mut migration = t.build("migration", &[1], &["DROP TABLE tasks"]).await;
    migration.expected_manifest_sha256 = Some(manifest.clone());
    let blocked = t.execute(&migration, None).await;
    assert_eq!(blocked.0, 1);
    assert_eq!(
        blocked.1.unwrap()["reason"],
        "unsupported_mutable_data_or_migration"
    );
    let mut wrong_image = t.build("wrong-image-commit", &[1], &[]).await;
    wrong_image.expected_manifest_sha256 = Some(manifest.clone());
    assert_eq!(t.execute(&wrong_image, None).await.0, 1);
    let mut d = t.build("D", &[1], &[]).await;
    d.expected_manifest_sha256 = Some(manifest.clone());
    let mut child = t
        .process(&d)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let dir = t
        .root
        .join("operations")
        .join(hash(d.operation_key.as_bytes()));
    let deadline = Instant::now() + Duration::from_secs(25);
    while !dir.join("child-stopped.json").exists() {
        assert!(Instant::now() < deadline, "Compose effect deadline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let name = format!("{}-application-1", t.project);
    loop {
        let logs = std::process::Command::new("docker")
            .args(["logs", "--tail", "20", &name])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&logs.stdout).contains("HEALTH_DELAY_ENTERED") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual application health entry deadline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let actual_id = std::process::Command::new("docker")
        .args(["container", "inspect", "--format", "{{.Id}}", &name])
        .output()
        .unwrap();
    child.start_kill().unwrap();
    assert!(!child.wait().await.unwrap().success());
    let unknown = t.execute(&d, Some("--readback")).await.1.unwrap();
    assert_eq!(latest(&unknown)["status"], "unknown");
    let pointer_time = std::fs::metadata(t.root.join("current.json"))
        .unwrap()
        .modified()
        .unwrap();
    let mut other = d.clone();
    other.operation_key = "oci:held".into();
    other.expected_manifest_sha256 = unknown["currentManifestSha256"].as_str().map(str::to_owned);
    assert_eq!(t.execute(&other, None).await.0, 1);
    let recovered = t.execute(&d, Some("--reconcile")).await.1.unwrap();
    assert_eq!(latest(&recovered)["status"], "verified");
    assert_eq!(
        latest(&recovered)["containerId"],
        String::from_utf8(actual_id.stdout).unwrap().trim()
    );
    assert_eq!(
        std::fs::metadata(t.root.join("current.json"))
            .unwrap()
            .modified()
            .unwrap(),
        pointer_time
    );
    assert_eq!(recovered["receipt"], unknown["receipt"]);
    assert_eq!(std::fs::read(&t.data).unwrap(), data);
    // Owner-side drift is not silently repaired or accepted for rollback.
    std::fs::set_permissions(&t.data, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(&t.data, br#"{"schemaVersion":2,"records":[]}"#).unwrap();
    rollback.operation_key = "oci:rollback:incompatible".into();
    rollback.expected_manifest_sha256 = recovered["currentManifestSha256"]
        .as_str()
        .map(str::to_owned);
    let blocked = t.execute(&rollback, None).await;
    assert_eq!(blocked.0, 1);
    assert_eq!(blocked.1.unwrap()["reason"], "owner_data_snapshot_drift");
    std::fs::write(&t.data, &data).unwrap();
    std::fs::set_permissions(&t.data, std::fs::Permissions::from_mode(0o444)).unwrap();
    println!(
        "ACTUAL_OCI_SCENARIOS=real-runner-build,image-commit,data-readback,health503,acceptance422,rollback-exact-image,readonly-data-preserved,unknown-SIGKILL,reconcile-no-recreate,incompatible-schema-blocked,migration-blocked,data-drift-blocked"
    );
}
