//! Embedded lifecycle integration using the same Kubernetes custody adapter as forge-runner.
use super::*;
use crate::execution::kubernetes::{Config, Execution, Scope};
use anyhow::{Context, ensure};
use serde_json::{Value, json};
#[path = "runner_kubernetes_deployment.rs"]
mod deployment;

static WORKERS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<Uuid>>> =
    std::sync::OnceLock::new();
struct Worker(Uuid);
impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(mut workers) = WORKERS.get_or_init(Default::default).lock() {
            workers.remove(&self.0);
        }
    }
}
pub(super) async fn run(
    pool: &PgPool,
    job: &JobRow,
    lease: &EmbeddedJobLease,
    running: &RunningJobs,
    config: &RuntimeRunnerConfig,
) -> Result<ExecutionOutcome, ApiError> {
    {
        let mut workers = WORKERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| ApiError::conflict("Kubernetes worker ownership unavailable"))?;
        if !workers.insert(job.id) {
            return Ok(ExecutionOutcome::Held);
        }
    }
    let _worker = Worker(job.id);
    run_inner(pool, job, lease, running, config).await
}
async fn run_inner(
    pool: &PgPool,
    job: &JobRow,
    lease: &EmbeddedJobLease,
    running: &RunningJobs,
    config: &RuntimeRunnerConfig,
) -> Result<ExecutionOutcome, ApiError> {
    let root = workspace_root(config);
    let retained = crate::execution::kubernetes::retry_unconfirmed(&root, lease.attempt_id)
        .map_err(|_| ApiError::conflict("Kubernetes custody unavailable"))?;
    if retained {
        running.lock().await.insert(job.id, u32::MAX);
    }
    let kube = Config::from_env()
        .map_err(|_| ApiError::conflict("Kubernetes operator config unavailable"))?;
    let input = image_build_input(pool, job, config).await?;
    if !retained && input.is_some() {
        kube.require_image_build()
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }
    let workspace = if retained {
        let records = crate::runner_workspace::OwnedWorkspace::inventory(&root)
            .map_err(|_| ApiError::conflict("Retained workspace inventory unavailable"))?;
        let matching: Vec<_> = records
            .iter()
            .filter(|w| {
                w.attempt_id == lease.attempt_id
                    && w.lease_id == lease.id
                    && w.generation == lease.generation
            })
            .collect();
        if matching.len() != 1 {
            return Ok(ExecutionOutcome::Held);
        }
        crate::runner_workspace::OwnedWorkspace::reopen(&root, &matching[0].workspace_id)
            .map_err(|_| ApiError::conflict("Original workspace unavailable"))?
    } else {
        prepare_workspace(pool, job, lease, config).await?
    };
    let secrets = if retained {
        Vec::new()
    } else {
        crate::platform::project_secret_pairs_for_names_with_config(
            pool,
            job.project_id,
            &job.required_secrets,
            &config.secrets,
        )
        .await?
    };
    let mut masks: Vec<String> = secrets
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(_, v)| v.clone())
        .collect();
    let timeout =
        sqlx::query_scalar::<_, Option<i32>>("SELECT timeout_seconds FROM jobs WHERE id=$1")
            .bind(job.id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?
            .unwrap_or(3600)
            .clamp(5, 86400) as u64;
    let scope = Scope {
        job_id: job.id,
        attempt_id: lease.attempt_id,
        lease_id: lease.id,
        generation: lease.generation,
        source_commit: job
            .commit_sha
            .clone()
            .ok_or_else(|| ApiError::conflict("Kubernetes job requires source pin"))?,
    };
    let mut env = vec![
        ("CICD_PIPELINE_ID".into(), job.pipeline_id.to_string()),
        ("CICD_JOB_ID".into(), job.id.to_string()),
        ("CICD_JOB_NAME".into(), job.name.clone()),
        ("CICD_STAGE_NAME".into(), job.stage_name.clone()),
        ("CICD_PROJECT_ID".into(), job.project_id.to_string()),
        ("CICD_PROJECT_NAME".into(), job.project_name.clone()),
        ("CICD_COMMIT_REF".into(), job.git_ref.clone()),
        ("CICD_COMMIT_SHA".into(), scope.source_commit.clone()),
        ("CICD_ARTIFACTS_DIR".into(), "/workspace/artifacts".into()),
        (
            "CICD_PIPELINE_ARTIFACTS_DIR".into(),
            "/workspace/pipeline-artifacts".into(),
        ),
    ];
    env.extend(secrets.iter().cloned());
    let vars =
        sqlx::query_scalar::<_, Option<Value>>("SELECT variables FROM pipelines WHERE id=$1")
            .bind(job.pipeline_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
    if let Some(Value::Object(vars)) = vars {
        for (key, value) in vars {
            env.push((
                format!("CICD_VAR_{}", key.to_uppercase().replace('-', "_")),
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or(value.to_string()),
            ));
        }
    }
    let pipeline = config
        .artifacts
        .root
        .join("pipelines")
        .join(job.pipeline_id.to_string());
    if !retained {
        std::fs::create_dir_all(workspace.checkout().join("artifacts"))
            .map_err(|_| ApiError::conflict("Artifact directory unavailable"))?;
        copy_artifact_tree(&pipeline, &workspace.checkout().join("pipeline-artifacts"))
            .map_err(|_| ApiError::conflict("Pipeline artifact staging unavailable"))?;
    }
    let registry = format!(
        "{}/attempt-{}:g{}",
        kube.registry_prefix,
        lease.attempt_id.simple(),
        lease.generation
    );
    let deployment = deployment::prepare(pool, job, lease, config, &kube)
        .await
        .map_err(|_| {
            ApiError::conflict("kubernetes_workspace_deploy_unsupported: exact configured recipe unavailable or unqualified; no launch performed")
        })?;
    let execution = if retained {
        Execution::reopen(&root, lease.attempt_id, lease.generation)
            .map_err(|_| ApiError::conflict("Original Kubernetes effect unavailable"))?
    } else {
        let (image, command) = if let Some((spec, source)) = &input {
            crate::runner_image_build::export_kubernetes_source(source, spec, workspace.checkout())
                .await
                .map_err(|_| ApiError::conflict("Exact build export unavailable"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::metadata(workspace.checkout().join("build-output"))
                    .map_err(|_| ApiError::conflict("Builder output permissions unavailable"))?;
                if meta.gid() != kube.security.fs_group || meta.mode() & 0o070 != 0o070 {
                    return Err(ApiError::conflict(
                        "Builder fsGroup cannot write its scoped output",
                    ));
                }
            }
            env = vec![
                (
                    "BUILDKITD_FLAGS".into(),
                    "--oci-worker-snapshotter=native".into(),
                ),
                ("XDG_RUNTIME_DIR".into(), "/tmp/buildkit-runtime".into()),
                ("TMPDIR".into(), "/tmp".into()),
            ];
            if kube.registry_secret.is_some() {
                env.push(("DOCKER_CONFIG".into(), "/registry-auth".into()));
            }
            let context = if spec.context == "." {
                "/workspace/source".into()
            } else {
                format!("/workspace/source/{}", spec.context)
            };
            let mut command = vec![
                "buildctl-daemonless.sh".into(),
                "build".into(),
                "--frontend".into(),
                "dockerfile.v0".into(),
                "--local".into(),
                format!("context={context}"),
                "--local".into(),
                format!("dockerfile={context}"),
                "--opt".into(),
                format!("filename={}", spec.dockerfile),
                "--opt".into(),
                format!(
                    "label:org.opencontainers.image.revision={}",
                    source.source_commit
                ),
                "--opt".into(),
                format!("label:io.forge.attempt={}", lease.attempt_id),
                "--output".into(),
                format!("type=image,name={registry},push=true"),
                "--metadata-file".into(),
                "/workspace/build-output/metadata.json".into(),
            ];
            if let Some(target) = &spec.target {
                command.extend(["--opt".into(), format!("target={target}")]);
            }
            (kube.builder_image.clone(), command)
        } else {
            (
                crate::execution::kubernetes::resolve_image(&job.image)
                    .await
                    .map_err(|_| ApiError::conflict("OCI image resolution unavailable"))?,
                vec![
                    "sh".into(),
                    "-lc".into(),
                    shell_capture_command(&job.command),
                ],
            )
        };
        let mut execution = Execution::prepare(
            kube.clone(),
            &root,
            scope,
            workspace.checkout(),
            &image,
            &command,
            &env,
            timeout,
        )
        .map_err(|_| ApiError::conflict("Kubernetes preparation unavailable"))?;
        if let Some(d) = &deployment {
            execution
                .deployment_template(&d.recipe, &d.images, &secrets)
                .map_err(|_| ApiError::conflict("Configured deployment unavailable"))?;
        }
        if input.is_some() {
            execution
                .build_template(&registry)
                .map_err(|_| ApiError::conflict("BuildKit security template unavailable"))?;
        }
        execution
    };
    if retained {
        masks = execution
            .recovery_masks(&job.required_secrets)
            .await
            .map_err(|_| ApiError::conflict("Original Pod secret masks unavailable"))?;
    }
    let submitted = retained
        && crate::api::sdlc_workspace::task_pipeline::already_submitted(
            pool,
            job.pipeline_id,
            lease.attempt_id,
        )
        .await?;
    let until = if submitted {
        Some(
            crate::api::sdlc_workspace::task_pipeline::verify_submitted_attempt(
                pool,
                job.pipeline_id,
                lease.attempt_id,
                &config.sdlc_workspace,
                &config.secrets,
            )
            .await
            .unwrap_or_else(|_| std::time::Instant::now()),
        )
    } else {
        match crate::api::sdlc_workspace::task_pipeline::before_spawn(
            pool,
            job.pipeline_id,
            lease.attempt_id,
            &config.sdlc_workspace,
            &config.secrets,
        )
        .await?
        {
            crate::api::sdlc_workspace::task_pipeline::Spawn::Held => {
                return Ok(ExecutionOutcome::Held);
            }
            crate::api::sdlc_workspace::task_pipeline::Spawn::Legacy => None,
            crate::api::sdlc_workspace::task_pipeline::Spawn::Permit(until) => Some(until),
        }
    };
    let mut guard = running.lock().await;
    if !retained
        && (!image_build_attempt_active(pool, lease).await?
            || until.is_some_and(|v| std::time::Instant::now() >= v))
    {
        return Ok(ExecutionOutcome::Held);
    }
    guard.insert(job.id, u32::MAX);
    drop(guard);
    let initial = if retained {
        execution.reconcile().await
    } else {
        execution.start().await.map(|_| None)
    };
    let mut terminal = match initial {
        Ok(value) => value,
        Err(_) => return Ok(ExecutionOutcome::Held),
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout);
    let mut timed_out = false;
    while terminal.is_none() {
        if deployment.is_some() {
            if execution.dependencies_ready().await.unwrap_or(false) {
                let gate = workspace.checkout().join(".forge-deployment-ready");
                if !gate.exists() {
                    crate::execution::kubernetes::write_once(&gate, b"ready")
                        .map_err(|_| ApiError::conflict("Readiness gate unavailable"))?;
                }
            }
            if execution.command_stopped().await.unwrap_or(false)
                && execution.finish_services().await.is_err()
            {
                return Ok(ExecutionOutcome::Held);
            }
        }
        if !image_build_attempt_active(pool, lease).await?
            || std::time::Instant::now() >= deadline
            || until.is_some_and(|v| std::time::Instant::now() >= v)
        {
            timed_out |= std::time::Instant::now() >= deadline;
            if execution.cancel().await.is_err() {
                return Ok(ExecutionOutcome::Held);
            }
        }
        terminal = match execution.observe().await {
            Ok(value) => value,
            Err(_) => return Ok(ExecutionOutcome::Held),
        };
        if terminal.is_none() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let terminal = terminal.expect("observed terminal");
    if execution
        .stream_logs(|line| {
            let pool = pool.clone();
            let masks = masks.clone();
            async move {
                append_attempt_log(
                    &pool,
                    job.id,
                    lease.attempt_id,
                    &mask_secrets(&line, &masks),
                )
                .await
                .map_err(|_| anyhow::anyhow!("masked log persistence unavailable"))
            }
        })
        .await
        .is_err()
    {
        return Ok(ExecutionOutcome::Held);
    }
    let mut status = if timed_out {
        "failed"
    } else if execution.cancelled() {
        "canceled"
    } else if terminal.exit_code == 0 {
        "success"
    } else {
        "failed"
    };
    let mut artifact_root = workspace.checkout().to_path_buf();
    if let Some((spec, _)) = &input {
        if status == "success" {
            if !image_build_attempt_active(pool, lease).await? {
                return Ok(ExecutionOutcome::Held);
            }
            artifact_root =
                match publish_build(&kube, job, lease, spec, workspace.checkout(), &registry).await
                {
                    Ok(root) => root,
                    Err(_) => return Ok(ExecutionOutcome::Held),
                };
        }
    }
    if let Some(d) = &deployment {
        if status == "success" && !image_build_attempt_active(pool, lease).await? {
            return Ok(ExecutionOutcome::Held);
        }
        let output = root
            .join(".kubernetes-control")
            .join(format!(
                "forge-{}-g{}",
                lease.attempt_id.simple(),
                lease.generation
            ))
            .join("deployment-output");
        std::fs::create_dir_all(&output)
            .map_err(|_| ApiError::conflict("Deployment output unavailable"))?;
        let path = output.join(&d.artifact);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| ApiError::conflict("Deployment output parent unavailable"))?;
        }
        let mut receipt = execution
            .deployment_receipt(&d.recipe.compose_sha256, &terminal)
            .map_err(|_| ApiError::conflict("Stopped receipt unavailable"))?;
        receipt["producerMapping"] = d.producers.clone();
        crate::execution::kubernetes::write_once(
            &path,
            &serde_json::to_vec(&receipt)
                .map_err(|_| ApiError::conflict("Receipt serialization unavailable"))?,
        )
        .map_err(|_| ApiError::conflict("Receipt persistence unavailable"))?;
        artifact_root = output;
    }
    let publish_allowed =
        until.is_none_or(|v| std::time::Instant::now() < v) && status != "canceled";
    if publish_allowed {
        crate::api::sdlc_workspace::task_pipeline::before_delegated_effect(
            pool,
            job.pipeline_id,
            lease.attempt_id,
            config,
        )
        .await?;
        if (input.is_none() || status == "success")
            && collect_declared_artifacts(
                pool,
                &config.artifacts,
                job.id,
                lease.attempt_id,
                &artifact_root,
                &job.artifact_paths,
            )
            .await
            .is_err()
        {
            if input.is_some() || deployment.is_some() {
                return Ok(ExecutionOutcome::Held);
            }
            status = "failed";
        }
        if input.is_none() {
            copy_artifact_tree(&workspace.checkout().join("pipeline-artifacts"), &pipeline)
                .map_err(|_| ApiError::conflict("Pipeline artifact return unavailable"))?;
            copy_artifact_tree(
                &workspace.checkout().join("artifacts"),
                &pipeline.join("jobs").join(job.id.to_string()),
            )
            .map_err(|_| ApiError::conflict("Job artifact return unavailable"))?;
        }
    } else if status == "success" {
        status = "failed";
    }
    if status == "success" && until.is_some() {
        let until = crate::api::sdlc_workspace::task_pipeline::verify_submitted_attempt(
            pool,
            job.pipeline_id,
            lease.attempt_id,
            &config.sdlc_workspace,
            &config.secrets,
        )
        .await?;
        let _guard = running.lock().await;
        if !finish_image_build_success(pool, job.id, lease, Some(terminal.exit_code), until).await?
        {
            return Ok(ExecutionOutcome::Held);
        }
    } else {
        let updated=sqlx::query("UPDATE jobs SET status=$2,finished_at=now() WHERE id=$1 AND status='running' AND EXISTS(SELECT 1 FROM execution_attempts WHERE id=$3 AND status='running')").bind(job.id).bind(status).bind(lease.attempt_id).execute(pool).await.map_err(ApiError::internal)?;
        if updated.rows_affected() == 0 {
            return Ok(ExecutionOutcome::Held);
        }
        sqlx::query("UPDATE execution_attempts SET status=$2,finished_at=now(),exit_code=$3 WHERE id=$1 AND status='running'").bind(lease.attempt_id).bind(status).bind(terminal.exit_code).execute(pool).await.map_err(ApiError::internal)?;
        complete_embedded_job_lease(pool, lease.id, status, None).await?;
    }
    let ack:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id WHERE l.id=$1 AND l.attempt_id=$2 AND l.generation=$3 AND l.completed_at IS NOT NULL AND l.lease_status IN ('completed','canceled') AND a.status=l.terminal_status AND a.finished_at IS NOT NULL)").bind(lease.id).bind(lease.attempt_id).bind(lease.generation).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !ack || execution.acknowledge().await.is_err() {
        return Ok(ExecutionOutcome::Held);
    }
    refresh_stage(pool.clone(), job.id).await?;
    let outcome =
        finish_workspace_retention(pool, lease, workspace, None, config.keep_workspace).await;
    if outcome == ExecutionOutcome::Finished && execution.finish_workspace_cleanup().is_err() {
        return Ok(ExecutionOutcome::Held);
    }
    Ok(outcome)
}

fn copy_artifact_tree(source: &Path, target: &Path) -> anyhow::Result<()> {
    ensure!(
        target.is_absolute(),
        "absolute pipeline artifact target required"
    );
    create_artifact_directory(target)?;
    if !source.exists() {
        return Ok(());
    }
    let mut pending = vec![(source.to_path_buf(), target.to_path_buf())];
    let mut count = 0usize;
    while let Some((from, to)) = pending.pop() {
        let meta = std::fs::symlink_metadata(&from)?;
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "linked artifact directory denied"
        );
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            count += 1;
            ensure!(count <= 4096, "pipeline artifact inventory exceeds bound");
            let meta = entry.file_type()?;
            ensure!(!meta.is_symlink(), "linked pipeline artifact denied");
            let dest = to.join(entry.file_name());
            if meta.is_dir() {
                if dest.exists() {
                    ensure!(
                        std::fs::symlink_metadata(&dest)?.is_dir(),
                        "artifact target type mismatch"
                    );
                } else {
                    std::fs::create_dir(&dest)?;
                }
                pending.push((entry.path(), dest));
            } else {
                ensure!(meta.is_file(), "unsupported artifact type");
                if dest.exists() {
                    let m = std::fs::symlink_metadata(&dest)?;
                    ensure!(
                        m.is_file() && !m.file_type().is_symlink(),
                        "linked artifact target denied"
                    );
                }
                std::fs::copy(entry.path(), dest)?;
            }
        }
    }
    Ok(())
}

async fn crane_json(kind: &str, image: &str) -> anyhow::Result<Value> {
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new("crane")
            .args([kind, "--", image])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(
        output.status.success() && output.stdout.len() <= 4 * 1024 * 1024,
        "registry readback unavailable"
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}
async fn publish_build(
    kube: &Config,
    job: &JobRow,
    lease: &EmbeddedJobLease,
    spec: &crate::domain::image_build::ImageBuildSpec,
    workspace: &Path,
    registry: &str,
) -> anyhow::Result<PathBuf> {
    let file = resolve_workspace_artifact(workspace, "build-output/metadata.json")
        .await
        .map_err(|_| anyhow::anyhow!("metadata missing"))?;
    let metadata: Value = serde_json::from_slice(&tokio::fs::read(file).await?)?;
    let digest = metadata["containerimage.digest"]
        .as_str()
        .context("builder digest missing")?;
    ensure!(
        crate::domain::image_build::valid_image_id(digest),
        "invalid registry digest"
    );
    let repository = registry.rsplit_once(':').context("registry tag missing")?.0;
    let image = format!("{repository}@{digest}");
    ensure!(
        crate::execution::kubernetes::resolve_image(&image).await? == image,
        "registry manifest changed"
    );
    let manifest = crane_json("manifest", &image).await?;
    let image_id = manifest["config"]["digest"]
        .as_str()
        .context("config digest missing")?;
    ensure!(
        metadata["containerimage.config.digest"] == image_id,
        "builder config digest mismatch"
    );
    let config = crane_json("config", &image).await?;
    ensure!(
        config["config"]["Labels"]["org.opencontainers.image.revision"]
            == job.commit_sha.as_deref().unwrap_or("")
            && config["config"]["Labels"]["io.forge.attempt"] == lease.attempt_id.to_string(),
        "registry provenance mismatch"
    );
    let descriptor = crate::domain::image_build::OciCandidateDescriptor::new(
        image_id.into(),
        job.commit_sha.clone().context("pin missing")?,
        spec.readable_schema_versions.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let root = Path::new("/workspaces/.kubernetes-control").join(format!(
        "forge-{}-g{}",
        lease.attempt_id.simple(),
        lease.generation
    ));
    let output = root.join("output");
    tokio::fs::create_dir_all(&output).await?;
    let path = output.join(&spec.descriptor_artifact);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    crate::execution::kubernetes::write_once(&path, &serde_json::to_vec(&descriptor)?)?;
    crate::execution::kubernetes::write_once(
        &root.join("registry-candidate.json"),
        &serde_json::to_vec(
            &json!({"schema":"forge/kubernetes-registry-candidate/v1","clusterCaSha256":kube.cluster_ca_sha256,"namespace":kube.namespace,"namespaceUid":kube.namespace_uid,"workspaceClaimUid":kube.workspace_claim_uid,"attemptId":lease.attempt_id,"leaseId":lease.id,"generation":lease.generation,"sourceCommit":job.commit_sha,"image":image,"descriptor":descriptor}),
        )?,
    )?;
    Ok(output)
}

/// Existing supervisor drives recovery; no new scheduler, process replay or guessed Pod adoption.
pub(super) async fn reconcile_terminal(
    pool: &PgPool,
    running: &RunningJobs,
    config: &RuntimeRunnerConfig,
) -> Result<(), sqlx::Error> {
    let scopes = crate::execution::kubernetes::inventory(&workspace_root(config))
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    static CURSOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let count = scopes.len().min(32);
    let start = if scopes.is_empty() {
        0
    } else {
        CURSOR.fetch_add(count, std::sync::atomic::Ordering::Relaxed) % scopes.len()
    };
    for scope in scopes.iter().cycle().skip(start).take(count).cloned() {
        if WORKERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| sqlx::Error::Protocol("worker lock poisoned".into()))?
            .contains(&scope.job_id)
        {
            continue;
        }
        let row=sqlx::query_as::<_,(String,String,String,bool)>("SELECT j.status,a.status,l.lease_status,l.completion_received_at IS NOT NULL FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN execution_attempts a ON a.id=l.attempt_id WHERE l.id=$1 AND l.job_id=$2 AND l.attempt_id=$3 AND l.generation=$4 AND l.runner_id IS NULL AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id)").bind(scope.lease_id).bind(scope.job_id).bind(scope.attempt_id).bind(scope.generation).fetch_optional(pool).await?;
        let Some((job_status, attempt_status, lease_status, ack)) = row else {
            continue;
        };
        if job_status == "running" && attempt_status == "running" && lease_status == "active" {
            running.lock().await.remove(&scope.job_id);
            let pool = pool.clone();
            let running = running.clone();
            let config = config.clone();
            let job = scope.job_id;
            tokio::spawn(async move {
                run_job_with_config(pool, job, running, config).await;
            });
            continue;
        }
        let Ok(execution) =
            Execution::reopen(&workspace_root(config), scope.attempt_id, scope.generation)
        else {
            continue;
        };
        match execution.reconcile().await {
            Ok(Some(_terminal))
                if job_status == attempt_status
                    && (matches!(job_status.as_str(), "failed" | "canceled")
                        || (job_status == "success" && ack)) =>
            {
                // Actual exact-UID stop readback, not expiry alone, acknowledges this owner execution.
                if !ack {
                    let result=sqlx::query("UPDATE job_leases l SET lease_status=CASE WHEN $5='canceled' THEN 'canceled' ELSE 'completed' END,terminal_status=$5,completed_at=COALESCE(completed_at,now()),acknowledged_at=COALESCE(acknowledged_at,now()),completion_received_at=now() WHERE l.id=$1 AND l.job_id=$2 AND l.attempt_id=$3 AND l.generation=$4 AND l.runner_id IS NULL AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=l.job_id) AND NOT EXISTS(SELECT 1 FROM job_leases other WHERE other.job_id=l.job_id AND other.id<>l.id AND other.lease_status='active')").bind(scope.lease_id).bind(scope.job_id).bind(scope.attempt_id).bind(scope.generation).bind(&job_status).execute(pool).await?;
                    if result.rows_affected() != 1 {
                        continue;
                    }
                }
                if execution.acknowledge().await.is_err() {
                    continue;
                }
                let Ok(records) =
                    crate::runner_workspace::OwnedWorkspace::inventory(&workspace_root(config))
                else {
                    continue;
                };
                let mut workspace_done = true;
                for record in records.iter().filter(|r| {
                    r.attempt_id == scope.attempt_id
                        && r.lease_id == scope.lease_id
                        && r.generation == scope.generation
                }) {
                    match crate::runner_workspace::OwnedWorkspace::reopen(
                        &workspace_root(config),
                        &record.workspace_id,
                    ) {
                        Ok(workspace) => {
                            if workspace
                                .record_completion(&job_status)
                                .or_else(|_| workspace.record_recovered_termination(&job_status))
                                .and_then(|_| workspace.acknowledge_completion(&job_status))
                                .is_err()
                            {
                                workspace_done = false;
                            } else if !config.keep_workspace
                                && workspace.cleanup_after_ack().await.is_err()
                            {
                                workspace_done = false;
                            }
                        }
                        Err(_) => workspace_done = false,
                    }
                }
                if !workspace_done || execution.finish_workspace_cleanup().is_err() {
                    continue;
                }
                running.lock().await.remove(&scope.job_id);
            }
            Ok(None) if matches!(job_status.as_str(), "failed" | "canceled") => {
                let _ = execution.cancel().await;
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipeline_artifacts_survive_two_distinct_job_workspaces() {
        let root = std::env::temp_dir().join(format!("forge-pipeline-copy-{}", Uuid::new_v4()));
        let shared = root.join("managed");
        let first = root.join("job-a");
        let second = root.join("job-b");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(first.join("output.txt"), b"first-stage").unwrap();
        copy_artifact_tree(&first, &shared).unwrap();
        copy_artifact_tree(&shared, &second).unwrap();
        assert_eq!(
            std::fs::read(second.join("output.txt")).unwrap(),
            b"first-stage"
        );
        std::fs::write(second.join("second.txt"), b"second-stage").unwrap();
        copy_artifact_tree(&second, &shared).unwrap();
        assert_eq!(
            std::fs::read(shared.join("output.txt")).unwrap(),
            b"first-stage"
        );
        assert_eq!(
            std::fs::read(shared.join("second.txt")).unwrap(),
            b"second-stage"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn create_artifact_directory(path: &Path) -> anyhow::Result<()> {
    let mut current = PathBuf::new();
    for part in path.components() {
        ensure!(
            !matches!(part, Component::ParentDir),
            "parent artifact path denied"
        );
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "linked artifact target denied"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&current)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
