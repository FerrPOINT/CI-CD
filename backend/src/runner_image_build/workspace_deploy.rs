//! Unconnected owner-only readback unit. No enqueue, create, start, stop or cleanup.
//! Register only inside runner_image_build, whose private custody checks are reused.
use super::*;
use crate::{api::ApiError, runner_docker::RuntimeImageFacts};
use cicd_infra::sdlc_workspace as ledger;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

/// Constructed only from the current owner plan and verified Git objects, not a job report.
pub(crate) struct VerifiedComposeSource {
    consumer: ImageBuildScope,
    repository: Uuid,
    source_commit: String,
    plan_sha256: String,
    path: String,
    blob_oid: String,
    bytes: Vec<u8>,
}

impl VerifiedComposeSource {
    pub(crate) fn owner_input(
        &self,
        scope: &ImageBuildScope,
        source: &PreparationSource,
    ) -> anyhow::Result<(serde_json::Value, &[u8])> {
        ensure!(
            self.consumer == *scope
                && scope.project_id == source.project_id
                && self.repository == source.repository_id
                && self.source_commit == source.source_commit,
            "Compose source consumer binding mismatch"
        );
        Ok((
            serde_json::json!({
                "job_id": scope.job_id, "attempt_id": scope.attempt_id,
                "source_commit": self.source_commit, "compose_path": self.path,
                "blob_oid": self.blob_oid, "sha256": digest(&self.bytes),
                "plan_sha256": self.plan_sha256,
            }),
            &self.bytes,
        ))
    }
}

fn compose_path(plan: &serde_json::Value, current: &str) -> anyhow::Result<String> {
    // Also validates unique keys and the current job's dependency graph.
    upstream_jobs(plan, current)?;
    let job = plan["jobs"]
        .as_array()
        .context("missing jobs")?
        .iter()
        .find(|job| job["key"].as_str() == Some(current))
        .context("missing deployment job")?;
    let spec = job["workspace_deploy"]
        .as_object()
        .context("missing sealed deployment spec")?;
    ensure!(spec.len() == 2, "unknown deployment spec fields");
    let compose = spec
        .get("compose")
        .and_then(serde_json::Value::as_str)
        .context("missing Compose path")?;
    let images = spec
        .get("images_manifest")
        .and_then(serde_json::Value::as_str)
        .context("missing images manifest path")?;
    ensure!(
        valid_relative_path(compose, false)
            && valid_relative_path(images, false)
            && compose != images
            && !excluded_source_path(compose),
        "unsafe sealed deployment path"
    );
    Ok(compose.into())
}

fn compose_entry(tree: &[u8], path: &str) -> anyhow::Result<TreeEntry> {
    let mut entries = tree_entries(tree)?;
    ensure!(
        entries.len() == 1 && entries[0].path == path,
        "Compose must be one exact regular Git blob"
    );
    Ok(entries.remove(0))
}

/// Reads no mutable checkout. Caller must still obtain fresh execution admission before effects.
pub(crate) async fn read_owner_compose_source(
    pool: &PgPool,
    source: &PreparationSource,
    scope: &ImageBuildScope,
    deadline: std::time::Instant,
) -> anyhow::Result<VerifiedComposeSource> {
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
        let (plan, current) = current_plan(pool, source, scope).await?;
        let path = compose_path(&plan, &current)?;
        let literal_path = format!(":(literal){path}");
        let tree = git_read(
            &source.repository,
            &[
                "ls-tree",
                "-r",
                "-z",
                "--full-tree",
                &source.source_commit,
                "--",
                &literal_path,
            ],
            MAX_TREE,
        )
        .await?;
        let entry = compose_entry(&tree, &path)?;
        let bytes = git_read(
            &source.repository,
            &["cat-file", "blob", &entry.oid],
            256 * 1024,
        )
        .await?;
        ensure!(!bytes.is_empty(), "empty Compose blob");
        verify_blob(&bytes, &entry.oid)?;
        ensure!(
            current_plan(pool, source, scope).await? == (plan.clone(), current),
            "owner attempt/source/plan changed during Git read"
        );
        Ok(VerifiedComposeSource {
            consumer: scope.clone(),
            repository: source.repository_id,
            source_commit: source.source_commit.clone(),
            plan_sha256: digest(&serde_json::to_vec(&plan)?),
            path,
            blob_oid: entry.oid,
            bytes,
        })
    })
    .await
    .context("owner Compose source deadline")?
}

pub(crate) struct VerifiedImageRegistry {
    images: BTreeMap<String, VerifiedImage>,
    dependencies: BTreeMap<String, String>,
    facts: BTreeMap<String, RuntimeImageFacts>,
    consumer: ImageBuildScope,
    repository: Uuid,
    source_commit: String,
    daemon: String,
}

#[derive(Clone)]
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
        let mut ids: BTreeMap<_, _> = self
            .images
            .iter()
            .map(|(name, image)| (name.clone(), image.descriptor.image_id.clone()))
            .collect();
        for (service, image) in &self.dependencies {
            ensure!(
                ids.insert(service.clone(), image.clone()).is_none(),
                "duplicate image origin"
            );
        }
        Ok(ids)
    }

    fn owner_facts_for(
        &self,
        scope: &ImageBuildScope,
        source: &PreparationSource,
    ) -> anyhow::Result<serde_json::Value> {
        let ids = self.image_ids_for(scope, source, &self.daemon)?;
        ensure!(
            ids.len() == self.facts.len() && !ids.is_empty(),
            "incomplete actual image Config inventory"
        );
        let facts = serde_json::to_value(&self.facts)?;
        for (service, image) in ids {
            ensure!(
                facts[&service]["image_id"] == image,
                "image Config inventory binding changed"
            );
        }
        Ok(facts)
    }
}

fn recipe_image_selectors(bytes: &[u8]) -> anyhow::Result<BTreeMap<String, String>> {
    let recipe: serde_json::Value = serde_json::from_slice(bytes)?;
    let services = recipe["services"]
        .as_object()
        .context("missing recipe services")?;
    ensure!(
        !services.is_empty() && services.len() <= 8,
        "service inventory bound"
    );
    services
        .iter()
        .map(|(name, spec)| {
            ensure!(
                !name.is_empty()
                    && name.len() <= 63
                    && name.bytes().enumerate().all(|(i, c)| c.is_ascii_lowercase()
                        || (i > 0 && (c.is_ascii_digit() || matches!(c, b'_' | b'-')))),
                "invalid inventory service"
            );
            let image = spec["image"].as_str().context("missing image selector")?;
            ensure!(
                crate::domain::image_build::valid_image_id(image),
                "dependency selector is not immutable"
            );
            Ok((name.clone(), image.into()))
        })
        .collect()
}

/// Selections come only from verified Git bytes. Inventory dependencies never acquire
/// build/source labels; matching upstream build receipts retain their distinct provenance.
async fn read_recipe_registry(
    pool: &PgPool,
    source: &PreparationSource,
    scope: &ImageBuildScope,
    roots: (&Path, &Path),
    compose: &VerifiedComposeSource,
    deadline: std::time::Instant,
) -> anyhow::Result<VerifiedImageRegistry> {
    let (_, bytes) = compose.owner_input(scope, source)?;
    let selected = recipe_image_selectors(bytes)?;
    let (plan, current) = current_plan(pool, source, scope).await?;
    ensure!(
        digest(&serde_json::to_vec(&plan)?) == compose.plan_sha256,
        "Compose inventory plan drift"
    );
    let upstream = upstream_jobs(&plan, &current)?;
    let mut builds = BTreeMap::new();
    for job in plan["jobs"].as_array().context("missing jobs")? {
        let key = job["key"].as_str().context("missing job key")?;
        if !upstream.contains(key) || job["image_build"].is_null() {
            continue;
        }
        ensure!(builds.len() < 8, "upstream image receipt bound");
        let spec: ImageBuildSpec = serde_json::from_value(job["image_build"].clone())?;
        spec.validate().map_err(anyhow::Error::msg)?;
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT ar.id FROM artifacts ar JOIN jobs j ON j.id=ar.job_id JOIN stages s ON s.id=j.stage_id JOIN job_leases l ON l.job_id=j.id AND l.attempt_id=ar.attempt_id WHERE s.pipeline_id=$1 AND j.name=$2 AND ar.name=$3 AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id) LIMIT 2")
            .bind(scope.pipeline_id).bind(key).bind(&spec.descriptor_artifact).fetch_all(pool).await?;
        ensure!(ids.len() == 1, "unique upstream image artifact unavailable");
        builds.insert(format!("build{}", builds.len()), ids[0]);
    }
    let client = Client::configured_verified().await?;
    let daemon = client.image_build_daemon_id().to_owned();
    let mut registry = if builds.is_empty() {
        VerifiedImageRegistry {
            images: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            facts: BTreeMap::new(),
            consumer: scope.clone(),
            repository: source.repository_id,
            source_commit: source.source_commit.clone(),
            daemon: daemon.clone(),
        }
    } else {
        read_owner_image_registry(pool, source, scope, roots, &daemon, &builds, deadline).await?
    };
    let built = std::mem::take(&mut registry.images);
    let mut observed: BTreeMap<String, RuntimeImageFacts> = BTreeMap::new();
    for (service, id) in &selected {
        let witnesses: Vec<_> = built
            .values()
            .filter(|image| image.descriptor.image_id == *id)
            .collect();
        ensure!(witnesses.len() <= 1, "ambiguous build image identity");
        if let Some(image) = witnesses.first() {
            registry.images.insert(service.clone(), (**image).clone());
        } else {
            registry.dependencies.insert(service.clone(), id.clone());
        }
        let facts = if let Some(facts) = observed.get(id) {
            facts.clone()
        } else {
            let facts = client.runtime_image(id).await?;
            observed.insert(id.clone(), facts.clone());
            facts
        };
        registry.facts.insert(service.clone(), facts);
    }
    for (id, facts) in observed {
        ensure!(
            client.runtime_image(&id).await? == facts,
            "dependency Config changed during inventory"
        );
    }
    ensure!(
        current_plan(pool, source, scope).await? == (plan, current),
        "inventory attempt/source changed"
    );
    ensure!(
        registry.image_ids_for(scope, source, &daemon)? == selected,
        "inventory service identity differs"
    );
    Ok(registry)
}

pub(crate) struct InventoryHold {
    reason: &'static str,
}

impl InventoryHold {
    pub(crate) fn message(&self) -> &'static str {
        self.reason
    }
}

fn requested(plan: &serde_json::Value, key: &str) -> bool {
    plan["jobs"].as_array().is_some_and(|jobs| {
        jobs.iter().any(|job| {
            job["key"].as_str() == Some(key)
                && job
                    .as_object()
                    .is_some_and(|v| v.contains_key("workspace_deploy"))
        })
    })
}

fn bridge_hold(value: &serde_json::Value) -> anyhow::Result<InventoryHold> {
    ensure!(
        value.as_object().is_some_and(|v| v.len() == 2) && value["status"] == "held",
        "inventory validator must not issue execution admission"
    );
    let reason = match value["reason"].as_str() {
        Some("owner_volume_user_preparation_unavailable") => {
            "runner: deployment held; owner volume/user preparation unavailable"
        }
        Some("project_secret_readback_unavailable") => {
            "runner: deployment held; project secret readback unavailable"
        }
        Some("owner_network_lifecycle_unavailable") => {
            "runner: deployment held; owner network lifecycle unavailable"
        }
        Some("recipe_rejected") => "runner: deployment held; closed recipe validation failed",
        _ => anyhow::bail!("unknown inventory validator result"),
    };
    Ok(InventoryHold { reason })
}

async fn inventory_bridge(input: serde_json::Value) -> anyhow::Result<InventoryHold> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "workspace_deploy_admission": include_str!("../../../scripts/workspace_deploy_admission.py"),
        "workspace_deploy_adapter": include_str!("../../../scripts/workspace_deploy_adapter.py"),
        "input": input,
    }))?;
    ensure!(bytes.len() <= 1024 * 1024, "owner inventory input bound");
    let mut command = Command::new("python3");
    command
        .args([
            "-I",
            "-c",
            include_str!("../../../scripts/workspace_deploy_bridge.py"),
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let mut child = command.spawn()?;
    let mut input = child.stdin.take().context("inventory input missing")?;
    input.write_all(&bytes).await?;
    input.shutdown().await?;
    drop(input);
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .context("inventory output missing")?
        .take(4097)
        .read_to_end(&mut output)
        .await?;
    ensure!(
        output.len() <= 4096 && child.wait().await?.success(),
        "bounded inventory validation failed"
    );
    bridge_hold(&serde_json::from_slice(&output)?)
}

async fn inspect_recipe(
    pool: &PgPool,
    scope: &ImageBuildScope,
    config: &crate::runner::RuntimeRunnerConfig,
    control_root: &Path,
    deadline: std::time::Instant,
) -> anyhow::Result<InventoryHold> {
    ensure!(
        config.mode == crate::config::RunnerMode::Docker,
        "Docker owner inventory required"
    );
    let mut connection = pool.acquire().await?;
    let task = ledger::find_task_pipeline(&mut connection, scope.pipeline_id, false)
        .await?
        .context("task intent unavailable")?;
    drop(connection);
    let storage = crate::repository_catalog::local_storage_for_project(pool, scope.project_id)
        .await
        .map_err(|_| anyhow::anyhow!("hosted source unavailable"))?
        .context("hosted source missing")?;
    let storage = crate::git_host::validate_repo_name(&storage).map_err(anyhow::Error::msg)?;
    let source = PreparationSource {
        project_id: scope.project_id,
        repository_id: task.intent.repository_id,
        repository: config.git_root.join(format!("{storage}.git")),
        source_commit: task.intent.source_commit,
    };
    let compose = read_owner_compose_source(pool, &source, scope, deadline).await?;
    let registry = read_recipe_registry(
        pool,
        &source,
        scope,
        (control_root, &config.artifacts.root),
        &compose,
        deadline,
    )
    .await?;
    let ids = registry.image_ids_for(scope, &source, &registry.daemon)?;
    let manifest = serde_json::to_vec(&serde_json::json!({"schema":"forge/workspace-images/v1",
        "sourceCommit":source.source_commit,"images":ids}))?;
    let (source_projection, bytes) = compose.owner_input(scope, &source)?;
    let recipe: serde_json::Value = serde_json::from_slice(bytes)?;
    let (plan, current) = current_plan(pool, &source, scope).await?;
    ensure!(
        digest(&serde_json::to_vec(&plan)?) == compose.plan_sha256,
        "bridge plan changed"
    );
    let job = plan["jobs"]
        .as_array()
        .context("missing jobs")?
        .iter()
        .find(|job| job["key"].as_str() == Some(&current))
        .context("missing current job")?;
    let secrets = job["required_secrets"]
        .as_array()
        .context("missing declared project secret names")?;
    ensure!(
        secrets.len() <= 32 && secrets.iter().all(serde_json::Value::is_string),
        "secret-name bound"
    );
    let environment: BTreeMap<_, Vec<_>> = recipe["services"]
        .as_object()
        .context("missing services")?
        .iter()
        .map(|(name, service)| {
            (
                name,
                service["environment"]
                    .as_object()
                    .map(|env| env.keys().collect())
                    .unwrap_or_default(),
            )
        })
        .collect();
    inventory_bridge(
        serde_json::json!({"job_id":scope.job_id,"attempt_id":scope.attempt_id,
        "compose":std::str::from_utf8(bytes)?,"manifest":std::str::from_utf8(&manifest)?,
        "source":source_projection,"image_facts":registry.owner_facts_for(scope,&source)?,
        "seal":{"source_commit":source.source_commit,"compose_sha256":digest(bytes),
            "images_manifest_sha256":digest(&manifest),"images":ids,
            "environment":environment,"secret_names":secrets}}),
    )
    .await
}

/// Existing runner hook: no task submission claim, secret resolution, workspace creation
/// or lifecycle effect. A requested recipe can return only HOLD, never command fallback.
pub(crate) async fn inspect_requested(
    pool: &PgPool,
    scope: ImageBuildScope,
    config: &crate::runner::RuntimeRunnerConfig,
    control_root: &Path,
) -> Result<Option<InventoryHold>, ApiError> {
    let row = sqlx::query("SELECT pp.plan,j.name FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id JOIN pipeline_plans pp ON pp.pipeline_id=p.id WHERE j.id=$1 AND p.id=$2 AND p.project_id=$3")
        .bind(scope.job_id).bind(scope.pipeline_id).bind(scope.project_id).fetch_optional(pool).await
        .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let plan: serde_json::Value = row.try_get("plan").map_err(ApiError::internal)?;
    let name: String = row.try_get("name").map_err(ApiError::internal)?;
    if !requested(&plan, &name) {
        return Ok(None);
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let hold = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        inspect_recipe(pool, &scope, config, control_root, deadline),
    )
    .await;
    Ok(Some(match hold {
        Ok(Ok(hold)) => hold,
        _ => InventoryHold {
            reason: "runner: deployment held; owner inventory validation unavailable",
        },
    }))
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
    let row = sqlx::query("SELECT p.commit_sha,pp.config_source,pp.resolved_commit_sha,pp.raw_config,pp.config_sha256,pp.plan,pp.plan_sha256,j.name FROM pipelines p JOIN projects pr ON pr.id=p.project_id AND pr.repository_id=$7 JOIN pipeline_plans pp ON pp.pipeline_id=p.id JOIN stages s ON s.pipeline_id=p.id JOIN jobs j ON j.stage_id=s.id JOIN execution_attempts a ON a.job_id=j.id JOIN job_leases l ON l.job_id=j.id AND l.attempt_id=a.id WHERE p.id=$1 AND p.project_id=$2 AND j.id=$3 AND a.id=$4 AND l.id=$5 AND l.generation=$6 AND j.status='running' AND a.status='running' AND l.lease_status='active' AND l.runner_id IS NULL AND l.runner_name='embedded' AND l.lease_expires_at>clock_timestamp() AND l.cancel_requested_at IS NULL AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id)")
        .bind(scope.pipeline_id).bind(scope.project_id).bind(scope.job_id)
        .bind(scope.attempt_id).bind(scope.lease_id).bind(scope.generation).bind(source.repository_id)
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
        dependencies: BTreeMap::new(),
        facts: BTreeMap::new(),
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
    fn inventory_never_accepts_repo_verified_flags_or_mutable_image_selectors() {
        let id = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            recipe_image_selectors(
                &serde_json::to_vec(&json!({"services":{
            "database":{"image":id}}}))
                .unwrap()
            )
            .unwrap()["database"],
            id
        );
        for value in [
            json!({"services":{}}),
            json!({"services":{"database":{"image":"postgres:17"}}}),
            json!({"services":{"database":{"image":{"verified":true,"image_id":id}}}}),
            json!({"services":{"../database":{"image":id}}}),
        ] {
            assert!(recipe_image_selectors(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }

    #[test]
    fn explicit_null_requested_recipe_cannot_fall_back_to_ordinary_commands() {
        let plan = json!({"jobs":[{"key":"deploy","workspace_deploy":null},
            {"key":"legacy","command":"echo legacy"}]});
        assert!(requested(&plan, "deploy"));
        assert!(!requested(&plan, "legacy"));
        assert!(!requested(&plan, "missing"));
    }

    #[test]
    fn bridge_can_return_only_fixed_safe_hold_diagnostics_never_execution_permission() {
        for reason in [
            "owner_volume_user_preparation_unavailable",
            "project_secret_readback_unavailable",
            "owner_network_lifecycle_unavailable",
            "recipe_rejected",
        ] {
            assert!(bridge_hold(&json!({"status":"held","reason":reason})).is_ok());
        }
        for value in [
            json!({"status":"ready","reason":"owner_network_lifecycle_unavailable"}),
            json!({"status":"held","reason":"raw-private-value"}),
            json!({"status":"held","reason":"recipe_rejected","credential":"private"}),
        ] {
            assert!(bridge_hold(&value).is_err());
        }
    }

    #[test]
    fn compose_path_comes_only_from_exact_unique_sealed_job() {
        let plan = json!({"jobs":[{"key":"deploy","needs":[],"workspace_deploy":{
            "compose":"deploy/task.compose.json","images_manifest":"reports/images.json"}}]});
        assert_eq!(
            compose_path(&plan, "deploy").unwrap(),
            "deploy/task.compose.json"
        );
        for spec in [
            json!(null),
            json!({"compose":"a","images_manifest":"a"}),
            json!({"compose":"../a","images_manifest":"b"}),
            json!({"compose":"a","images_manifest":"b","verified":true}),
        ] {
            let mut changed = plan.clone();
            changed["jobs"][0]["workspace_deploy"] = spec;
            assert!(compose_path(&changed, "deploy").is_err());
        }
        assert!(compose_path(&plan, "foreign").is_err());
    }

    #[test]
    fn compose_git_entry_rejects_symlink_gitlink_and_path_substitution() {
        let oid = "a".repeat(40);
        let regular = format!("100644 blob {oid}\tdeploy/task.compose.json\0");
        assert_eq!(
            compose_entry(regular.as_bytes(), "deploy/task.compose.json")
                .unwrap()
                .oid,
            oid
        );
        assert!(compose_entry(regular.as_bytes(), "other.json").is_err());
        assert!(
            compose_entry(
                format!("{regular}{regular}").as_bytes(),
                "deploy/task.compose.json"
            )
            .is_err()
        );
        assert!(
            compose_entry(
                format!("100755 blob {oid}\tdeploy/task.compose.json\0").as_bytes(),
                "deploy/task.compose.json"
            )
            .is_ok()
        );
        for header in ["120000 blob", "160000 commit"] {
            assert!(
                compose_entry(
                    format!("{header} {oid}\tdeploy/task.compose.json\0").as_bytes(),
                    "deploy/task.compose.json"
                )
                .is_err()
            );
        }
        assert!(compose_entry(b"", "deploy/task.compose.json").is_err());
    }

    #[test]
    fn compose_owner_projection_is_bound_to_repository_commit_and_attempt() {
        let scope = ImageBuildScope {
            project_id: Uuid::new_v4(),
            pipeline_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            lease_id: Uuid::new_v4(),
            generation: 1,
        };
        let source = PreparationSource {
            project_id: scope.project_id,
            repository_id: Uuid::new_v4(),
            repository: PathBuf::from("/not-opened"),
            source_commit: "a".repeat(40),
        };
        let verified = VerifiedComposeSource {
            consumer: scope.clone(),
            repository: source.repository_id,
            source_commit: source.source_commit.clone(),
            plan_sha256: "b".repeat(64),
            path: "compose.json".into(),
            blob_oid: "c".repeat(40),
            bytes: b"{}".to_vec(),
        };
        let (projection, bytes) = verified.owner_input(&scope, &source).unwrap();
        assert_eq!(bytes, b"{}");
        assert_eq!(projection["sha256"], digest(bytes));
        let mut next = scope.clone();
        next.generation += 1;
        assert!(verified.owner_input(&next, &source).is_err());
        let other = PreparationSource {
            repository_id: Uuid::new_v4(),
            ..source
        };
        assert!(verified.owner_input(&scope, &other).is_err());
        let other = PreparationSource {
            repository_id: verified.repository,
            source_commit: "d".repeat(40),
            ..other
        };
        assert!(verified.owner_input(&scope, &other).is_err());
    }

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
            dependencies: BTreeMap::new(),
            facts: BTreeMap::new(),
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
