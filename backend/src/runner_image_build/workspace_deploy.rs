//! Unconnected owner-only readback unit. No enqueue, create, start, stop or cleanup.
//! Register only inside runner_image_build, whose private custody checks are reused.
use super::*;
use cicd_infra::sdlc_workspace as ledger;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

pub(crate) struct VerifiedImageRegistry {
    images: BTreeMap<String, VerifiedImage>,
    consumer: ImageBuildScope,
    repository: Uuid,
    source_commit: String,
    daemon: String,
}

struct VerifiedImage {
    descriptor: OciCandidateDescriptor,
    artifact_id: Uuid,
    artifact_sha256: String,
    artifact_size: i64,
    artifact_path: String,
    scope: ImageBuildScope,
}

impl VerifiedImageRegistry {
    pub(crate) fn image_ids_for(
        &self,
        scope: &ImageBuildScope,
        source: &PreparationSource,
        daemon: &str,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        ensure!(
            self.consumer == *scope
                && self.repository == source.repository_id
                && scope.project_id == source.project_id
                && self.source_commit == source.source_commit
                && self.daemon == daemon,
            "registry consumer binding mismatch"
        );
        Ok(self
            .images
            .iter()
            .map(|(name, image)| (name.clone(), image.descriptor.image_id.clone()))
            .collect())
    }
}

#[derive(PartialEq, Eq)]
struct RetainedImage {
    intent: Intent,
    descriptor: OciCandidateDescriptor,
    request_sha256: String,
}

fn check_intent(
    intent: &Intent,
    expected: &ImageBuildScope,
    repository: Uuid,
    source: &str,
    daemon: &str,
) -> anyhow::Result<()> {
    intent.spec.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        intent.schema == "forge/backend-image-build/v1"
            && intent.scope == *expected
            && intent.repository_id == repository
            && valid_commit(source)
            && intent.source_commit == source
            && !daemon.is_empty()
            && intent.daemon_id == daemon,
        "retained image differs from owner source/attempt"
    );
    Ok(())
}

fn read_retained(
    root: &Path,
    expected: &ImageBuildScope,
    repository: Uuid,
    source: &str,
    daemon: &str,
) -> anyhow::Result<RetainedImage> {
    ensure!(cfg!(unix), "private build custody requires Unix");
    expected.validate()?;
    private_directory(root)?;
    ensure!(
        root.is_absolute()
            && root
                .file_name()
                .is_some_and(|name| name == ".compose-control")
            && root.canonicalize()? == root,
        "noncanonical backend control root"
    );
    let directory = root.join(format!("image-build-{}", expected.attempt_id.simple()));
    private_directory(&directory)?;
    ensure!(
        directory.canonicalize()? == directory,
        "linked build custody"
    );
    let bytes = read_private(&directory.join("intent.json"), 8192)?;
    let intent: Intent = serde_json::from_slice(&bytes)?;
    check_intent(&intent, expected, repository, source, daemon)?;
    ensure!(
        serde_json::to_vec(&intent)? == bytes,
        "noncanonical retained intent"
    );
    let request_sha256 = digest(&bytes);
    ensure!(
        cleanup_is_complete(&directory, &intent, &request_sha256)?,
        "build completion/cleanup acknowledgement unavailable"
    );
    let completion: Completion =
        serde_json::from_slice(&read_private(&directory.join("completed.json"), 8192)?)?;
    let config = directory.join("docker-config");
    private_directory(&config)?;
    ensure!(
        config.canonicalize()? == config
            && read_private(&config.join("config.json"), 1024)? == b"{\"auths\":{}}",
        "unexpected build readback credential configuration"
    );
    Ok(RetainedImage {
        intent,
        descriptor: completion.descriptor,
        request_sha256,
    })
}

async fn read_completed_image(
    root: &Path,
    source: &PreparationSource,
    scope: &ImageBuildScope,
    daemon: &str,
) -> anyhow::Result<RetainedImage> {
    let observe = || {
        let (root, scope, repository, commit, daemon) = (
            root.to_owned(),
            scope.clone(),
            source.repository_id,
            source.source_commit.clone(),
            daemon.to_owned(),
        );
        tokio::task::spawn_blocking(move || {
            read_retained(&root, &scope, repository, &commit, &daemon)
        })
    };
    let before = observe().await??;
    let config = root
        .join(format!("image-build-{}", scope.attempt_id.simple()))
        .join("docker-config");
    let client = Client::for_image_build(&config).await?;
    ensure!(
        client.image_build_daemon_id() == daemon,
        "image daemon changed"
    );
    client
        .inspect_built_image(
            &config,
            &before.descriptor.image_id,
            &source.source_commit,
            scope.attempt_id,
            &before.request_sha256,
        )
        .await?;
    ensure!(
        before == observe().await??,
        "retained image changed during readback"
    );
    Ok(before)
}

fn upstream_jobs(plan: &serde_json::Value, current: &str) -> anyhow::Result<BTreeSet<String>> {
    let rows = plan["jobs"]
        .as_array()
        .context("missing immutable DAG jobs")?;
    ensure!(!rows.is_empty() && rows.len() <= 500, "DAG job bound");
    let mut jobs = BTreeMap::new();
    for row in rows {
        let key = row["key"].as_str().context("missing DAG key")?;
        ensure!(jobs.insert(key, row).is_none(), "duplicate DAG key");
    }
    ensure!(jobs.contains_key(current), "current job absent from plan");
    let mut pending = vec![current.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(key) = pending.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }
        let row = jobs.get(key.as_str()).context("unknown DAG dependency")?;
        let needs = row["needs"].as_array().context("missing DAG needs")?;
        ensure!(needs.len() <= 64, "DAG dependency bound");
        for need in needs {
            let name = need.as_str().context("invalid DAG dependency")?;
            ensure!(name != current, "cyclic current deployment dependency");
            pending.push(name.to_owned());
        }
    }
    let mut remaining = visited.clone();
    while !remaining.is_empty() {
        let ready: Vec<_> = remaining
            .iter()
            .filter(|key| {
                jobs[key.as_str()]["needs"].as_array().is_some_and(|needs| {
                    needs
                        .iter()
                        .all(|need| need.as_str().is_some_and(|name| !remaining.contains(name)))
                })
            })
            .cloned()
            .collect();
        ensure!(!ready.is_empty(), "cyclic upstream dependency");
        for key in ready {
            remaining.remove(&key);
        }
    }
    visited.remove(current);
    Ok(visited)
}

async fn current_plan(
    pool: &PgPool,
    source: &PreparationSource,
    scope: &ImageBuildScope,
) -> anyhow::Result<(serde_json::Value, String)> {
    scope.validate()?;
    ensure!(
        scope.project_id == source.project_id,
        "owner source project mismatch"
    );
    let row = sqlx::query("SELECT p.commit_sha,pp.config_source,pp.resolved_commit_sha,pp.raw_config,pp.config_sha256,pp.plan,pp.plan_sha256,j.name FROM pipelines p JOIN pipeline_plans pp ON pp.pipeline_id=p.id JOIN stages s ON s.pipeline_id=p.id JOIN jobs j ON j.stage_id=s.id JOIN execution_attempts a ON a.job_id=j.id JOIN job_leases l ON l.job_id=j.id AND l.attempt_id=a.id WHERE p.id=$1 AND p.project_id=$2 AND j.id=$3 AND a.id=$4 AND l.id=$5 AND l.generation=$6 AND j.status='running' AND a.status='running' AND l.lease_status='active' AND l.runner_id IS NULL AND l.runner_name='embedded' AND l.lease_expires_at>clock_timestamp() AND l.cancel_requested_at IS NULL AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id)")
        .bind(scope.pipeline_id).bind(scope.project_id).bind(scope.job_id)
        .bind(scope.attempt_id).bind(scope.lease_id).bind(scope.generation)
        .fetch_optional(pool).await?.context("current owner attempt unavailable")?;
    let plan: serde_json::Value = row.try_get("plan")?;
    let bytes =
        preparation::repository_pipeline_config(&source.repository, &source.source_commit).await?;
    ensure!(
        row.try_get::<String, _>("commit_sha")? == source.source_commit
            && row.try_get::<String, _>("resolved_commit_sha")? == source.source_commit
            && row.try_get::<String, _>("config_source")? == "repository"
            && row.try_get::<String, _>("raw_config")?.as_bytes() == bytes
            && row.try_get::<String, _>("config_sha256")? == digest(&bytes)
            && row.try_get::<String, _>("plan_sha256")? == digest(&serde_json::to_vec(&plan)?),
        "owner pipeline source/plan mismatch"
    );
    let mut connection = pool.acquire().await?;
    let task = ledger::find_task_pipeline(&mut connection, scope.pipeline_id, false)
        .await?
        .context("task intent unavailable")?;
    task.intent.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        task.project_id == source.project_id
            && task.intent.repository_id == source.repository_id
            && task.intent.source_commit == source.source_commit
            && task.intent.role == crate::domain::sdlc_workspace::WorkspaceRole::Devops,
        "registry requires the actual DevOps task intent"
    );
    Ok((plan, row.try_get("name")?))
}

/// Observes actual owner records; does not grant task authority or publish a manifest.
/// Dependency images without retained backend build custody remain unsupported/held.
pub(crate) async fn read_owner_image_registry(
    pool: &PgPool,
    source: &PreparationSource,
    scope: &ImageBuildScope,
    roots: (&Path, &Path),
    daemon: &str,
    selection: &BTreeMap<String, Uuid>,
    deadline: std::time::Instant,
) -> anyhow::Result<VerifiedImageRegistry> {
    tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        read_registry(pool, source, scope, roots, daemon, selection),
    )
    .await
    .context("owner registry readback deadline")?
}

async fn read_registry(
    pool: &PgPool,
    source: &PreparationSource,
    scope: &ImageBuildScope,
    roots: (&Path, &Path),
    daemon: &str,
    selection: &BTreeMap<String, Uuid>,
) -> anyhow::Result<VerifiedImageRegistry> {
    ensure!(
        1 <= selection.len() && selection.len() <= 8,
        "service selection bound"
    );
    let (control_root, artifact_root) = roots;
    let (plan, current) = current_plan(pool, source, scope).await?;
    let upstream = upstream_jobs(&plan, &current)?;
    let mut images = BTreeMap::new();
    for (service, artifact_id) in selection {
        ensure!(
            !service.is_empty()
                && service.len() <= 63
                && service
                    .bytes()
                    .enumerate()
                    .all(|(i, c)| c.is_ascii_lowercase()
                        || (i > 0 && (c.is_ascii_digit() || matches!(c, b'_' | b'-')))),
            "invalid selected service"
        );
        let mut connection = pool.acquire().await?;
        let jobs = ledger::candidate_jobs(&mut connection, scope.pipeline_id).await?;
        let artifacts = ledger::candidate_artifacts(&mut connection, scope.pipeline_id).await?;
        ensure!(
            jobs.len() <= 1000 && artifacts.len() <= 32,
            "owner observation bound"
        );
        let artifact = artifacts
            .iter()
            .find(|item| item.artifact_id == *artifact_id)
            .context("selected pipeline artifact unavailable")?;
        let attempt = artifact
            .attempt_id
            .context("artifact attempt unavailable")?;
        ensure!(
            artifact.retained
                && jobs.iter().any(|job| job.job_id == artifact.job_id
                    && job.attempt_id == Some(attempt)
                    && job.terminal_acknowledged
                    && job.artifacts_complete),
            "upstream terminal acknowledgement unavailable"
        );
        let row = sqlx::query("SELECT l.id,l.generation,j.name AS job_name,ar.name AS artifact_name FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN artifacts ar ON ar.job_id=j.id AND ar.attempt_id=l.attempt_id WHERE ar.id=$1 AND l.attempt_id=$2 AND l.lease_status='completed' AND l.terminal_status='success' AND l.completed_at IS NOT NULL AND l.completion_received_at IS NOT NULL AND l.acknowledged_at IS NOT NULL AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id)")
            .bind(artifact_id).bind(attempt).fetch_one(&mut *connection).await?;
        let job_name: String = row.try_get("job_name")?;
        ensure!(
            upstream.contains(&job_name),
            "artifact is not a declared upstream dependency"
        );
        let build_scope = ImageBuildScope {
            project_id: scope.project_id,
            pipeline_id: scope.pipeline_id,
            job_id: artifact.job_id,
            attempt_id: attempt,
            lease_id: row.try_get("id")?,
            generation: row.try_get("generation")?,
        };
        drop(connection);
        let retained = read_completed_image(control_root, source, &build_scope, daemon).await?;
        let planned = plan["jobs"]
            .as_array()
            .context("missing immutable jobs")?
            .iter()
            .find(|job| job["key"].as_str() == Some(&job_name))
            .context("build plan unavailable")?;
        let spec: ImageBuildSpec = serde_json::from_value(planned["image_build"].clone())?;
        let descriptor = serde_json::to_vec(&retained.descriptor)?;
        let sha256 = artifact
            .sha256
            .as_ref()
            .context("artifact hash unavailable")?;
        ensure!(
            retained.intent.spec == spec
                && row.try_get::<String, _>("artifact_name")? == spec.descriptor_artifact
                && artifact.size_bytes == descriptor.len() as i64
                && *sha256 == digest(&descriptor),
            "artifact differs from retained backend image completion"
        );
        crate::candidate_evidence::verify_artifact(
            artifact_root,
            Path::new(&artifact.storage_path),
            sha256,
            artifact.size_bytes,
        )
        .await?;
        images.insert(
            service.clone(),
            VerifiedImage {
                descriptor: retained.descriptor,
                artifact_id: *artifact_id,
                artifact_sha256: sha256.clone(),
                artifact_size: artifact.size_bytes,
                artifact_path: artifact.storage_path.clone(),
                scope: build_scope,
            },
        );
    }
    // Use a fresh observation after filesystem/Engine IO, not a retained transaction snapshot.
    ensure!(
        current_plan(pool, source, scope).await? == (plan, current),
        "owner attempt/plan changed"
    );
    let mut connection = pool.acquire().await?;
    let jobs = ledger::candidate_jobs(&mut connection, scope.pipeline_id).await?;
    let artifacts = ledger::candidate_artifacts(&mut connection, scope.pipeline_id).await?;
    for image in images.values() {
        ensure!(
            jobs.iter().any(|job| job.job_id == image.scope.job_id
                && job.attempt_id == Some(image.scope.attempt_id)
                && job.terminal_acknowledged
                && job.artifacts_complete)
                && artifacts
                    .iter()
                    .any(|artifact| artifact.artifact_id == image.artifact_id
                        && artifact.retained
                        && artifact.attempt_id == Some(image.scope.attempt_id)
                        && artifact.job_id == image.scope.job_id
                        && artifact.size_bytes == image.artifact_size
                        && artifact.storage_path == image.artifact_path
                        && artifact.sha256.as_ref() == Some(&image.artifact_sha256)),
            "upstream completion/artifact changed during readback"
        );
    }
    Ok(VerifiedImageRegistry {
        images,
        consumer: scope.clone(),
        repository: source.repository_id,
        source_commit: source.source_commit.clone(),
        daemon: daemon.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_transitive_declared_upstream_jobs_are_selected() {
        let plan = json!({"jobs":[{"key":"build","needs":[]},
            {"key":"test","needs":["build"]},{"key":"deploy","needs":["test"]},
            {"key":"foreign","needs":[]}]});
        assert_eq!(
            upstream_jobs(&plan, "deploy").unwrap(),
            BTreeSet::from(["build".into(), "test".into()])
        );
    }

    #[test]
    fn missing_duplicate_and_cyclic_current_dependencies_are_held() {
        for plan in [
            json!({"jobs":[]}),
            json!({"jobs":[{"key":"deploy","needs":["missing"]}]}),
            json!({"jobs":[{"key":"deploy","needs":[]},{"key":"deploy","needs":[]}]}),
            json!({"jobs":[{"key":"deploy","needs":["build"]},{"key":"build","needs":["deploy"]}]}),
            json!({"jobs":[{"key":"deploy","needs":["build"]},{"key":"build","needs":["other"]},
                {"key":"other","needs":["build"]}]}),
        ] {
            assert!(upstream_jobs(&plan, "deploy").is_err());
        }
    }

    #[test]
    fn retained_intent_cannot_move_between_source_scope_or_daemon() {
        let scope = ImageBuildScope {
            project_id: Uuid::new_v4(),
            pipeline_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 1,
        };
        let intent = Intent {
            schema: "forge/backend-image-build/v1".into(),
            scope: scope.clone(),
            repository_id: Uuid::new_v4(),
            source_commit: "a".repeat(40),
            daemon_id: "owner".into(),
            spec: ImageBuildSpec {
                context: ".".into(),
                dockerfile: "Dockerfile".into(),
                target: None,
                descriptor_artifact: "image.json".into(),
                readable_schema_versions: vec![1],
            },
        };
        assert!(
            check_intent(
                &intent,
                &scope,
                intent.repository_id,
                &intent.source_commit,
                "owner"
            )
            .is_ok()
        );
        for key in [
            "project_id",
            "pipeline_id",
            "job_id",
            "attempt_id",
            "lease_id",
        ] {
            let mut value = serde_json::to_value(&scope).unwrap();
            value[key] = json!(Uuid::new_v4());
            let other = serde_json::from_value(value).unwrap();
            assert!(
                check_intent(
                    &intent,
                    &other,
                    intent.repository_id,
                    &intent.source_commit,
                    "owner"
                )
                .is_err()
            );
        }
        assert!(
            check_intent(
                &intent,
                &scope,
                Uuid::new_v4(),
                &intent.source_commit,
                "owner"
            )
            .is_err()
        );
        assert!(
            check_intent(
                &intent,
                &scope,
                intent.repository_id,
                &"b".repeat(40),
                "owner"
            )
            .is_err()
        );
        assert!(
            check_intent(
                &intent,
                &scope,
                intent.repository_id,
                &intent.source_commit,
                "foreign"
            )
            .is_err()
        );
    }

    #[test]
    fn registry_observation_cannot_rebind_a_different_consumer() {
        let consumer = ImageBuildScope {
            project_id: Uuid::new_v4(),
            pipeline_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 1,
        };
        let source = PreparationSource {
            project_id: consumer.project_id,
            repository_id: Uuid::new_v4(),
            repository: PathBuf::from("/not-opened"),
            source_commit: "a".repeat(40),
        };
        let registry = VerifiedImageRegistry {
            images: BTreeMap::new(),
            consumer: consumer.clone(),
            repository: source.repository_id,
            source_commit: source.source_commit.clone(),
            daemon: "owner".into(),
        };
        assert!(
            registry
                .image_ids_for(&consumer, &source, "owner")
                .unwrap()
                .is_empty()
        );
        let mut other = consumer.clone();
        other.generation += 1;
        assert!(registry.image_ids_for(&other, &source, "owner").is_err());
        assert!(
            registry
                .image_ids_for(&consumer, &source, "foreign")
                .is_err()
        );
        let other_source = PreparationSource {
            source_commit: "b".repeat(40),
            ..source
        };
        assert!(
            registry
                .image_ids_for(&consumer, &other_source, "owner")
                .is_err()
        );
    }
}
