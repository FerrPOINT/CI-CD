//! Owner-policy static-artifact delivery. No scheduler, Git merge or shell executor.
pub mod delegated;
mod files;
pub mod oci;
pub(crate) mod owner_evidence;
pub mod postgres;
mod probes;
pub(crate) mod tracker;
use crate::{
    api::{AppState, sdlc_workspace},
    domain::{sdlc_workspace::*, task_delivery::*},
    store::sdlc_workspace as ledger,
};
use anyhow::{Context, ensure};
use axum::{
    Extension, Json,
    extract::{Path as ApiPath, Query, State},
};
use chrono::Utc;
use files::*;
pub use probes::DeliveryPolicy;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum TaskDeliveryReadback {
    Static(DeliveryReadback),
    Oci(oci::Readback),
}

enum Refresh<'a> {
    Tracker(&'a AppState),
    Child {
        state: &'a AppState,
        session: &'a delegated::Session,
        connection: &'a mut sqlx::PgConnection,
    },
}

impl Refresh<'_> {
    async fn read(
        &mut self,
        project: Uuid,
        policy: &probes::TaskExecutionPolicy,
        original: &WorkspaceOperationReceipt,
        command: &DeliveryCommand,
    ) -> anyhow::Result<tracker::Permit> {
        match self {
            Self::Child {
                state,
                session,
                connection,
            } => session
                .refresh(state, connection)
                .await
                .map_err(|_| anyhow::anyhow!("current child authority unavailable")),
            Self::Tracker(state) => {
                crate::repository_catalog::require_project_writable(
                    state.pool.as_ref().context("owner database absent")?,
                    project,
                )
                .await
                .map_err(|_| anyhow::anyhow!("project namespace became read-only"))?;
                tracker::read(
                    state,
                    policy,
                    original,
                    command.task_fence.as_ref().context("task fence absent")?,
                    true,
                )
                .await
                .map_err(|_| anyhow::anyhow!("Tracker authority changed before publication"))
            }
        }
    }
}

async fn task_environment(
    connection: &mut sqlx::PgConnection,
    project: Uuid,
    policy: &probes::TaskExecutionPolicy,
) -> Result<(), crate::api::ApiError> {
    use crate::api::ApiError;
    let environment = sqlx::query_as::<_, (Uuid, bool, i32)>(
        "SELECT project_id,protected,required_approvals FROM environments WHERE id=$1 FOR SHARE",
    )
    .bind(policy.environment_id)
    .fetch_optional(connection)
    .await
    .map_err(ApiError::internal)?;
    if environment != Some((project, false, 0)) {
        return Err(ApiError::conflict(
            "Task target чужой, защищён или требует approval",
        ));
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetOwner {
    schema: String,
    project_id: Uuid,
    policy_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    receipt: DeliveryReceipt,
    manifest: Option<DeliveryManifest>,
    policy_sha256: String,
}

struct Target {
    root: PathBuf,
    project_id: Uuid,
    policy: DeliveryPolicy,
    policy_sha256: String,
}

impl Target {
    fn open(
        root: &Path,
        project: Uuid,
        policy: DeliveryPolicy,
        create: bool,
    ) -> anyhow::Result<Self> {
        ensure!(
            root.is_absolute() && !project.is_nil(),
            "explicit absolute isolated target required"
        );
        if create {
            directory(root)?;
        }
        plain(root)?;
        let root = root.canonicalize()?;
        let policy_sha256 = policy.sha256()?;
        let owner = TargetOwner {
            schema: "forge/local-static-target/v1".into(),
            project_id: project,
            policy_sha256: policy_sha256.clone(),
        };
        let marker = root.join("owner.json");
        if create {
            let _lock = Lock::acquire(&root)?;
            if !marker.exists() {
                ensure!(
                    std::fs::read_dir(&root)?.all(|e| e.is_ok_and(|e| e.file_name() == ".lock")),
                    "refusing enrollment of existing target contents"
                );
                for name in ["operations", "manifests", "objects"] {
                    directory(&root.join(name))?;
                }
                immutable(&marker, &owner)?;
            }
            let observed: TargetOwner = read(&marker)?;
            ensure!(
                observed.schema == owner.schema
                    && observed.project_id == project
                    && observed.policy_sha256 == policy_sha256,
                "owner target or probe policy mismatch"
            );
            for name in ["operations", "manifests", "objects"] {
                let path = root.join(name);
                plain(&path)?;
                ensure!(
                    path.is_dir(),
                    "owner target directory missing; recovery required"
                );
            }
        } else {
            let observed: TargetOwner = read(&marker)?;
            ensure!(
                observed.schema == owner.schema
                    && observed.project_id == project
                    && observed.policy_sha256 == policy_sha256,
                "owner target or probe policy mismatch"
            );
        }
        Ok(Self {
            root,
            project_id: project,
            policy,
            policy_sha256,
        })
    }

    fn manifest(&self, hash: &str) -> anyhow::Result<DeliveryManifest> {
        ensure!(digest(hash), "invalid manifest reference");
        let bytes = read_bytes(
            &self.root.join("manifests").join(format!("{hash}.json")),
            128 * 1024,
        )?;
        ensure!(sha(&bytes) == hash, "manifest bytes mismatch");
        let value: DeliveryManifest = serde_json::from_slice(&bytes)?;
        ensure!(
            value.operation_receipt.project_id == self.project_id
                && value.operation_receipt.request.validate().is_ok()
                && value.operation_receipt.request_hash
                    == sha(&serde_json::to_vec(&value.operation_receipt.request)?),
            "manifest original owner identity mismatch"
        );
        ensure!(
            digest(&value.artifact_sha256)
                && digest(&value.config_sha256)
                && digest(&value.plan_sha256),
            "invalid pinned manifest digests"
        );
        ensure!(
            value.schema == "forge/local-static-manifest/v1"
                && value.target_policy_sha256 == self.policy_sha256,
            "manifest policy mismatch"
        );
        let bytes = read_bytes(
            &self.root.join("objects").join(&value.artifact_sha256),
            32 * 1024 * 1024,
        )?;
        ensure!(
            bytes.len() as i64 == value.artifact_size_bytes && sha(&bytes) == value.artifact_sha256,
            "published artifact bytes mismatch"
        );
        Ok(value)
    }

    fn readback(
        &self,
        command: &DeliveryCommand,
        original: &WorkspaceOperationReceipt,
    ) -> anyhow::Result<DeliveryReadback> {
        let dir = operation_dir(&self.root, &command.operation_key);
        let intent: Intent = read(&dir.join("intent.json"))?;
        ensure!(
            intent.policy_sha256 == self.policy_sha256
                && intent.receipt.command_sha256 == sha(&serde_json::to_vec(command)?)
                && serde_json::to_vec(&intent.receipt.original_operation)?
                    == serde_json::to_vec(original)?,
            "original delivery input mismatch"
        );
        if let Some(manifest) = &intent.manifest {
            ensure!(
                intent.receipt.manifest_sha256.as_ref()
                    == Some(&sha(&serde_json::to_vec(manifest)?))
                    && manifest.target_policy_sha256 == self.policy_sha256
                    && manifest.operation_receipt.project_id == self.project_id,
                "intent manifest identity mismatch"
            );
        } else {
            ensure!(
                intent.receipt.manifest_sha256.is_none(),
                "intent lacks manifest"
            );
        }
        let receipt = optional::<DeliveryReceipt>(&dir.join("result.json"))?
            .unwrap_or_else(|| intent.receipt.clone());
        let reconciled_receipt = optional::<DeliveryReceipt>(&dir.join("reconciled.json"))?;
        for observed in std::iter::once(&receipt).chain(reconciled_receipt.iter()) {
            ensure!(
                observed.schema == "forge/local-delivery-operation/v1"
                    && observed.scope
                        == if command.task_fence.is_some() {
                            "task_nonproduction_delivery"
                        } else {
                            "owner_local_verification"
                        }
                    && observed.dispatch_allowed == command.task_fence.is_some()
                    && !observed.sdlc_acceptance_verified
                    && observed.command_sha256 == sha(&serde_json::to_vec(command)?)
                    && observed.command_sha256 == sha(&serde_json::to_vec(&observed.command)?)
                    && serde_json::to_vec(&observed.original_operation)?
                        == serde_json::to_vec(original)?
                    && observed.manifest_sha256 == intent.receipt.manifest_sha256
                    && observed.previous_manifest_sha256 == intent.receipt.previous_manifest_sha256,
                "delivery result does not match immutable intent"
            );
            if observed.status == DeliveryStatus::Verified {
                let manifest = intent
                    .manifest
                    .as_ref()
                    .context("verified result lacks manifest")?;
                for (probe, expected) in [
                    (
                        &observed.version,
                        observed
                            .manifest_sha256
                            .as_ref()
                            .context("verified manifest reference missing")?,
                    ),
                    (&observed.served_artifact, &manifest.artifact_sha256),
                    (&observed.health, &self.policy.health_body_sha256),
                    (&observed.acceptance, &self.policy.acceptance_body_sha256),
                ] {
                    ensure!(
                        probe
                            .as_ref()
                            .is_some_and(|p| p.status == DeliveryStatus::Verified
                                && p.http_status == Some(200)
                                && p.body_sha256.as_ref() == Some(expected)),
                        "verified result lacks matching owner probe evidence"
                    );
                }
            }
        }
        let current_manifest_sha256 = pointer(&self.root, "current.json")?;
        let confirmed_manifest_sha256 = pointer(&self.root, "confirmed.json")?;
        let latest = reconciled_receipt.as_ref().unwrap_or(&receipt);
        Ok(DeliveryReadback {
            reconciliation_needed: latest.status == DeliveryStatus::Unknown,
            receipt,
            reconciled_receipt,
            current_manifest_sha256,
            confirmed_manifest_sha256,
        })
    }

    fn ensure_idle(&self, own_key: &str) -> anyhow::Result<()> {
        let own = operation_dir(&self.root, own_key);
        let mut count = 0;
        for entry in std::fs::read_dir(self.root.join("operations"))? {
            count += 1;
            ensure!(count <= 1024, "delivery inventory bound exceeded");
            let path = entry?.path();
            plain(&path)?;
            if path == own {
                continue;
            }
            let _: Intent = read(&path.join("intent.json"))?;
            let final_receipt = optional::<DeliveryReceipt>(&path.join("reconciled.json"))?
                .or(optional::<DeliveryReceipt>(&path.join("result.json"))?);
            ensure!(
                final_receipt.is_some_and(|r| r.status != DeliveryStatus::Unknown),
                "unreconciled owner operation holds target"
            );
        }
        Ok(())
    }

    async fn observe(
        &self,
        mut receipt: DeliveryReceipt,
        manifest: &DeliveryManifest,
    ) -> DeliveryReceipt {
        let hash = receipt.manifest_sha256.as_ref().expect("manifest intent");
        let version = probes::probe(&self.policy, "/.forge/version", hash, 128 * 1024).await;
        let served = probes::probe(
            &self.policy,
            "/",
            &manifest.artifact_sha256,
            32 * 1024 * 1024,
        )
        .await;
        let health = probes::probe(
            &self.policy,
            &self.policy.health_path,
            &self.policy.health_body_sha256,
            64 * 1024,
        )
        .await;
        let acceptance = probes::probe(
            &self.policy,
            &self.policy.acceptance_path,
            &self.policy.acceptance_body_sha256,
            64 * 1024,
        )
        .await;
        // Bracket checks with the actual served manifest, not a generic service liveness response.
        let last_version = probes::probe(&self.policy, "/.forge/version", hash, 128 * 1024).await;
        receipt.status = if version.status == DeliveryStatus::Unavailable
            || served.status == DeliveryStatus::Unavailable
            || last_version.status == DeliveryStatus::Unavailable
        {
            DeliveryStatus::Unknown
        } else if [
            version.status,
            served.status,
            last_version.status,
            health.status,
            acceptance.status,
        ]
        .contains(&DeliveryStatus::Failed)
        {
            DeliveryStatus::Failed
        } else if [health.status, acceptance.status].contains(&DeliveryStatus::Unavailable) {
            DeliveryStatus::Unavailable
        } else {
            DeliveryStatus::Verified
        };
        receipt.reason = match receipt.status {
            DeliveryStatus::Verified => "owner_policy_checks_verified",
            DeliveryStatus::Failed => "served_identity_or_application_check_failed",
            DeliveryStatus::Unknown => "served_outcome_unknown_reconciliation_required",
            DeliveryStatus::Unavailable => "application_check_unavailable",
        }
        .into();
        receipt.version = Some(last_version);
        receipt.served_artifact = Some(served);
        receipt.health = Some(health);
        receipt.acceptance = Some(acceptance);
        receipt.recorded_at = Utc::now();
        receipt
    }

    async fn execute(
        &self,
        command: &DeliveryCommand,
        original: WorkspaceOperationReceipt,
        candidate: Option<VerifiedCandidate>,
        reconcile: bool,
        mut permit: Option<tracker::Permit>,
        mut task_state: Option<Refresh<'_>>,
    ) -> anyhow::Result<DeliveryReadback> {
        ensure!(
            command.task_fence.is_some() == permit.is_some(),
            "task effect requires live owner authorization"
        );
        let _lock = Lock::acquire(&self.root)?;
        if let Some(refresh @ Refresh::Child { .. }) = task_state.as_mut() {
            permit = Some(
                refresh
                    .read(
                        self.project_id,
                        self.policy
                            .task_execution
                            .as_ref()
                            .context("task policy absent")?,
                        &original,
                        command,
                    )
                    .await?,
            );
        }
        let dir = operation_dir(&self.root, &command.operation_key);
        if dir.exists() {
            match task_state.as_ref() {
                Some(Refresh::Child { session, .. }) => session.check_journal(&dir)?,
                _ => ensure!(
                    !dir.join("task-child.json").exists(),
                    "task child custody requires its original credential"
                ),
            }
            let previous = self.readback(command, &original)?;
            if !reconcile || !previous.reconciliation_needed {
                return Ok(previous);
            }
            if let Some(permit) = permit {
                permit.check()?;
            }
            let intent: Intent = read(&dir.join("intent.json"))?;
            if let (Some(hash), Some(manifest)) =
                (intent.receipt.manifest_sha256.clone(), &intent.manifest)
            {
                // Lock acquisition proves the previous local writer exited. Recovery never republishes.
                if pointer(&self.root, "current.json")?.as_ref() == Some(&hash)
                    && self.manifest(&hash).is_ok()
                {
                    let receipt = self.observe(intent.receipt, manifest).await;
                    if let Some(refresh @ Refresh::Child { .. }) = task_state.as_mut() {
                        permit = Some(
                            refresh
                                .read(
                                    self.project_id,
                                    self.policy
                                        .task_execution
                                        .as_ref()
                                        .context("task policy absent")?,
                                    &original,
                                    command,
                                )
                                .await?,
                        );
                    }
                    if receipt.status != DeliveryStatus::Unknown {
                        if let Some(permit) = permit {
                            permit.check()?;
                        }
                        if receipt.status == DeliveryStatus::Verified {
                            if !dir.join("verified-checks.json").exists() {
                                immutable(&dir.join("verified-checks.json"), &receipt)?;
                            }
                            replace_pointer(&self.root, "confirmed.json", &hash)?;
                        }
                        immutable(&dir.join("reconciled.json"), &receipt)?;
                    }
                }
            }
            return self.readback(command, &original);
        }
        ensure!(!reconcile, "original delivery operation not found");
        if let Some(permit) = permit {
            permit.check()?;
        }
        self.ensure_idle(&command.operation_key)?;
        let previous = pointer(&self.root, "current.json")?;
        ensure!(
            previous == command.expected_manifest_sha256,
            "current manifest CAS mismatch"
        );
        let mut receipt = DeliveryReceipt {
            schema: "forge/local-delivery-operation/v1".into(),
            scope: if command.task_fence.is_some() {
                "task_nonproduction_delivery"
            } else {
                "owner_local_verification"
            }
            .into(),
            command: command.clone(),
            command_sha256: sha(&serde_json::to_vec(command)?),
            original_operation: original.clone(),
            manifest_sha256: None,
            previous_manifest_sha256: previous,
            status: DeliveryStatus::Unknown,
            reason: "local_effect_not_finalized".into(),
            version: None,
            served_artifact: None,
            health: None,
            acceptance: None,
            recorded_at: Utc::now(),
            dispatch_allowed: command.task_fence.is_some(),
            sdlc_acceptance_verified: false,
        };
        let manifest = match command.action {
            DeliveryAction::Deploy => candidate.as_ref().map(|c| c.manifest.clone()),
            DeliveryAction::Rollback => pointer(&self.root, "confirmed.json")?
                .map(|hash| self.manifest(&hash))
                .transpose()?,
        };
        if let Some(Refresh::Child { session, .. }) = task_state.as_ref()
            && command.action == DeliveryAction::Rollback
        {
            ensure!(
                manifest
                    .as_ref()
                    .map(|m| serde_json::to_vec(m).map(|v| sha(&v)))
                    .transpose()?
                    == session.binding.rollback_manifest_sha256,
                "committed rollback manifest changed under target lock"
            );
        }
        if let Some(manifest) = &manifest {
            receipt.manifest_sha256 = Some(sha(&serde_json::to_vec(manifest)?));
        } else {
            receipt.status = DeliveryStatus::Unavailable;
            receipt.reason = if command.action == DeliveryAction::Rollback {
                "last_confirmed_manifest_unavailable"
            } else {
                "verified_candidate_unavailable"
            }
            .into();
        }
        if let Some(permit) = permit {
            permit.check()?;
        }
        directory(&dir)?;
        let intent = Intent {
            receipt: receipt.clone(),
            manifest: manifest.clone(),
            policy_sha256: self.policy_sha256.clone(),
        };
        immutable(&dir.join("intent.json"), &intent)?;
        if let Some(Refresh::Child { session, .. }) = task_state.as_ref() {
            session.record_journal(&dir)?;
        }
        let Some(manifest) = manifest else {
            immutable(&dir.join("result.json"), &receipt)?;
            return self.readback(command, &original);
        };
        let hash = receipt.manifest_sha256.as_ref().unwrap().clone();
        // Durable active intent precedes every publication effect. Any IO error retains the hold.
        if let Some(permit) = permit {
            permit.check()?;
        }
        immutable(&dir.join("active.json"), &receipt.command_sha256)?;
        if let Some(candidate) = candidate {
            if let Some(permit) = permit {
                permit.check()?;
            }
            copy_artifact(
                &self.root,
                &candidate.storage_path,
                &manifest.artifact_sha256,
                manifest.artifact_size_bytes,
            )?;
        }
        if let Some(permit) = permit {
            permit.check()?;
        }
        immutable(
            &self.root.join("manifests").join(format!("{hash}.json")),
            &manifest,
        )?;
        self.manifest(&hash)?;
        if let Some(refresh) = task_state.as_mut() {
            // Re-read the existing lease after staging, immediately before changing the served pointer.
            permit = Some(
                refresh
                    .read(
                        self.project_id,
                        self.policy
                            .task_execution
                            .as_ref()
                            .context("task policy absent")?,
                        &original,
                        command,
                    )
                    .await?,
            );
        }
        if let Some(permit) = permit {
            permit.check()?;
        }
        replace_pointer(&self.root, "current.json", &hash)?;
        receipt = self.observe(receipt, &manifest).await;
        if let Some(refresh @ Refresh::Child { .. }) = task_state.as_mut() {
            permit = Some(
                refresh
                    .read(
                        self.project_id,
                        self.policy
                            .task_execution
                            .as_ref()
                            .context("task policy absent")?,
                        &original,
                        command,
                    )
                    .await?,
            );
        }
        if let Some(permit) = permit {
            permit.check()?;
        }
        if receipt.status == DeliveryStatus::Verified {
            immutable(&dir.join("verified-checks.json"), &receipt)?;
            replace_pointer(&self.root, "confirmed.json", &hash)?;
        }
        immutable(&dir.join("result.json"), &receipt)?;
        self.readback(command, &original)
    }
}

// No public constructor: candidate identity and storage path come only from owner DB/Git/bytes.
struct VerifiedCandidate {
    manifest: DeliveryManifest,
    storage_path: PathBuf,
}

async fn candidate(
    state: Arc<AppState>,
    claims: crate::auth::AccessClaims,
    project: Uuid,
    command: &DeliveryCommand,
    policy_sha256: String,
) -> anyhow::Result<Option<VerifiedCandidate>> {
    let evidence = sdlc_workspace::get_candidate_evidence(
        State(state.clone()),
        Some(Extension(claims)),
        None,
        ApiPath((project, command.workspace_operation_key.clone())),
        Query(command.original.clone()),
    )
    .await
    .map_err(|_| anyhow::anyhow!("owner candidate observation rejected"))?
    .0;
    let mut conn = state
        .pool
        .as_ref()
        .context("owner database unavailable")?
        .acquire()
        .await?;
    candidate_from_evidence(&state, &mut conn, command, policy_sha256, evidence).await
}

async fn candidate_from_evidence(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    command: &DeliveryCommand,
    policy_sha256: String,
    evidence: CandidateEvidenceReadback,
) -> anyhow::Result<Option<VerifiedCandidate>> {
    if !evidence.candidate_observed {
        return Ok(None);
    }
    let Some(artifact) = evidence
        .artifacts
        .into_iter()
        .find(|a| Some(a.artifact_id) == command.artifact_id)
    else {
        return Ok(None);
    };
    let rows = ledger::candidate_artifacts(conn, evidence.pipeline_id).await?;
    let row = rows
        .into_iter()
        .find(|r| r.artifact_id == artifact.artifact_id)
        .context("original artifact unavailable")?;
    ensure!(
        row.retained
            && row.sha256.as_deref() == Some(&artifact.sha256)
            && row.size_bytes == artifact.size_bytes
            && row.attempt_id == Some(artifact.attempt_id),
        "artifact identity or retention changed during observation"
    );
    let path = PathBuf::from(row.storage_path);
    plain(&path)?;
    ensure!(
        path.canonicalize()?
            .starts_with(state.config.artifacts.root.canonicalize()?),
        "artifact outside owner root"
    );
    Ok(Some(VerifiedCandidate {
        storage_path: path,
        manifest: DeliveryManifest {
            schema: "forge/local-static-manifest/v1".into(),
            operation_receipt: evidence.operation_receipt,
            pipeline_id: evidence.pipeline_id,
            artifact_id: artifact.artifact_id,
            artifact_attempt_id: artifact.attempt_id,
            artifact_sha256: artifact.sha256,
            artifact_size_bytes: artifact.size_bytes,
            config_sha256: evidence
                .config_sha256
                .context("config digest unavailable")?,
            plan_sha256: evidence.plan_sha256.context("plan digest unavailable")?,
            target_policy_sha256: policy_sha256,
        },
    }))
}

fn configured_target(state: &AppState, project: Uuid, create: bool) -> anyhow::Result<Target> {
    let root = state
        .config
        .sdlc_workspace
        .local_delivery_root
        .as_ref()
        .context("local delivery target unavailable")?;
    let path = state
        .config
        .sdlc_workspace
        .local_delivery_policy
        .as_ref()
        .context("owner probe policy unavailable")?;
    let policy: DeliveryPolicy = read(path)?;
    ensure!(
        root.is_absolute()
            && root.components().all(|c| !matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )),
        "normalized absolute owner root required"
    );
    let resolved = if root.exists() {
        plain(root)?;
        root.canonicalize()?
    } else {
        let parent = root.parent().context("owner parent missing")?;
        plain(parent)?;
        parent
            .canonicalize()?
            .join(root.file_name().context("owner root name missing")?)
    };
    for protected in [&state.git.root, &state.config.artifacts.root] {
        if let Ok(path) = protected.canonicalize() {
            ensure!(
                !resolved.starts_with(&path) && !path.starts_with(&resolved),
                "delivery target overlaps protected source/artifact root"
            );
        }
    }
    Target::open(root, project, policy, create)
}

/// Only the privileged local CLI calls this effect path. HTTP routes never call it.
pub async fn local_command(
    state: Arc<AppState>,
    project: Uuid,
    token: &str,
    command: DeliveryCommand,
    reconcile: bool,
    read_only: bool,
) -> anyhow::Result<DeliveryReadback> {
    if !read_only {
        ensure!(
            std::env::var("CICD_LOCAL_DELIVERY_MODE").ok().as_deref() == Some("local-verification"),
            "explicit privileged local verification mode required"
        );
    }
    command.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        command.task_fence.is_none(),
        "task delivery uses the authenticated owner API"
    );
    ensure!(
        token.starts_with("forge_sat_"),
        "existing project machine credential required"
    );
    let db = state.pool.as_ref().context("owner database unavailable")?;
    let secret = state
        .auth_secret
        .as_deref()
        .context("owner auth unavailable")?;
    let claims = crate::api::identity_for_bearer_token(db, secret, token)
        .await
        .map_err(|_| anyhow::anyhow!("owner credential rejected"))?;
    let mut tx = db.begin().await?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:read")
        .await
        .map_err(|_| anyhow::anyhow!("owner read authorization rejected"))?;
    if !read_only {
        sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write")
            .await
            .map_err(|_| anyhow::anyhow!("owner write authorization rejected"))?;
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
        .map_err(|_| anyhow::anyhow!("original operation binding mismatch"))?;
    let target = configured_target(&state, project, !read_only)?;
    let result = if read_only {
        target.readback(&command, &original)?
    } else {
        let existing = operation_dir(&target.root, &command.operation_key).exists();
        let candidate = if !existing && command.action == DeliveryAction::Deploy {
            candidate(
                state.clone(),
                claims.clone(),
                project,
                &command,
                target.policy_sha256.clone(),
            )
            .await?
        } else {
            None
        };
        sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write")
            .await
            .map_err(|_| anyhow::anyhow!("fresh owner write authorization rejected"))?;
        target
            .execute(&command, original, candidate, reconcile, None, None)
            .await?
    };
    tx.commit().await?;
    Ok(result)
}

/// Task-only nonproduction delivery. Generic deployment approval/rollback policy is unchanged.
#[utoipa::path(post,path="/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/delivery-operations",operation_id="reject_sdlc_delivery",tag="SDLC workspace",params(("project_id"=Uuid,Path),("operation_key"=String,Path)),request_body=DeliveryCommand,responses((status=200,body=TaskDeliveryReadback),(status=503,description="Owner task target or Tracker readback unavailable"),(status=400),(status=401),(status=403),(status=404),(status=409)))]
pub(crate) async fn execute_task_delivery(
    State(state): State<Arc<AppState>>,
    claims: Option<Extension<crate::auth::AccessClaims>>,
    child: Option<Extension<crate::delegated_credentials::Identity>>,
    ApiPath((project, key)): ApiPath<(Uuid, String)>,
    Json(command): Json<DeliveryCommand>,
) -> Result<Json<TaskDeliveryReadback>, crate::api::ApiError> {
    use crate::api::ApiError;
    let claims = match (claims, child) {
        (None, Some(Extension(identity))) => {
            return delegated::execute(state, &identity, project, &key, command).await;
        }
        (Some(Extension(claims)), None) => claims,
        _ => return Err(ApiError::forbidden()),
    };
    command.validate().map_err(ApiError::bad_request)?;
    if command.workspace_operation_key != key {
        return Err(ApiError::conflict("original workspace key mismatch"));
    }
    let mut tx = state
        .pool
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("owner database unavailable"))?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write").await?;
    let original = ledger::find_receipt(&mut tx, project, claims.sub, &key)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found_named("workspace operation"))?;
    sdlc_workspace::verify_lookup(&original, &command.original)?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:read").await?;
    let fence = command
        .task_fence
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("Нужен текущий task delivery fence"))?;
    if state.config.sdlc_workspace.local_delivery_kind == crate::config::LocalDeliveryKind::Oci {
        let result =
            oci::execute_task(state.clone(), &claims, &mut tx, project, &command, original).await?;
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(TaskDeliveryReadback::Oci(result)));
    }
    // No HTTP enrollment: the owner must already have published this target/policy.
    let target = configured_target(&state, project, false)
        .map_err(|_| ApiError::service_unavailable("Опубликованный owner target недоступен"))?;
    let policy = target.policy.task_execution.as_ref().ok_or_else(|| {
        ApiError::service_unavailable("Nonproduction task delivery policy недоступна")
    })?;
    task_environment(&mut tx, project, policy).await?;
    let existing = operation_dir(&target.root, &command.operation_key).exists();
    // Completed original-key replay needs no new lease and cannot repeat publication.
    if existing {
        let readback = target
            .readback(&command, &original)
            .map_err(|_| ApiError::conflict("Исходный delivery journal не совпадает"))?;
        if !readback.reconciliation_needed {
            tx.commit().await.map_err(ApiError::internal)?;
            return Ok(Json(TaskDeliveryReadback::Static(readback)));
        }
    }
    crate::repository_catalog::require_project_writable(
        state
            .pool
            .as_ref()
            .ok_or_else(|| ApiError::service_unavailable("owner database unavailable"))?,
        project,
    )
    .await?;
    let candidate = if !existing && command.action == DeliveryAction::Deploy {
        candidate(
            state.clone(),
            claims.clone(),
            project,
            &command,
            target.policy_sha256.clone(),
        )
        .await
        .map_err(|_| ApiError::conflict("Readback точного pipeline artifact отклонён"))?
    } else {
        None
    };
    let permit = tracker::read(&state, policy, &original, fence, existing).await?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:write").await?;
    let result = target
        .execute(
            &command,
            original,
            candidate,
            existing,
            Some(permit),
            Some(Refresh::Tracker(&state)),
        )
        .await
        .map_err(|_| {
            ApiError::conflict("Delivery удерживается; перед повтором прочитайте original key")
        })?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(TaskDeliveryReadback::Static(result)))
}

#[utoipa::path(get,path="/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/delivery-operations/{delivery_key}",tag="SDLC workspace",params(("project_id"=Uuid,Path),("operation_key"=String,Path),("delivery_key"=String,Path),("requestHash"=String,Query),("taskId"=Uuid,Query),("rootTaskId"=Uuid,Query),("assignmentId"=Uuid,Query),("executionId"=Uuid,Query),("fencingToken"=i64,Query)),responses((status=200,body=TaskDeliveryReadback),(status=400),(status=401),(status=403),(status=404),(status=409),(status=503)))]
pub(crate) async fn get_local_delivery(
    State(state): State<Arc<AppState>>,
    claims: Option<Extension<crate::auth::AccessClaims>>,
    child: Option<Extension<crate::delegated_credentials::Identity>>,
    ApiPath((project, key, delivery_key)): ApiPath<(Uuid, String, String)>,
    Query(expected): Query<WorkspaceOperationLookup>,
) -> Result<Json<TaskDeliveryReadback>, crate::api::ApiError> {
    use crate::api::ApiError;
    let claims = match (claims, child) {
        (None, Some(Extension(identity))) => {
            return delegated::readback(&state, &identity, project, &key, &delivery_key, &expected)
                .await;
        }
        (Some(Extension(claims)), None) => claims,
        _ => return Err(ApiError::forbidden()),
    };
    let mut tx = state
        .pool
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("owner database unavailable"))?
        .begin()
        .await
        .map_err(ApiError::internal)?;
    sdlc_workspace::authorize(&mut tx, &state, &claims, project, "api:read").await?;
    let original = ledger::find_receipt(&mut tx, project, claims.sub, &key)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found_named("workspace operation"))?;
    sdlc_workspace::verify_lookup(&original, &expected)?;
    if delivery_key.is_empty()
        || delivery_key.len() > 128
        || !delivery_key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err(ApiError::bad_request("invalid delivery key"));
    }
    if state.config.sdlc_workspace.local_delivery_kind == crate::config::LocalDeliveryKind::Oci {
        let result = oci::read_task(&state, project, &key, &delivery_key, &original)?;
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(TaskDeliveryReadback::Oci(result)));
    }
    let target = configured_target(&state, project, false)
        .map_err(|_| ApiError::service_unavailable("owner local delivery target unavailable"))?;
    let path = operation_dir(&target.root, &delivery_key).join("intent.json");
    let intent = optional::<Intent>(&path)
        .map_err(|_| ApiError::conflict("owner delivery journal unavailable or inconsistent"))?
        .ok_or_else(|| ApiError::not_found_named("delivery operation"))?;
    if intent.receipt.command.workspace_operation_key != key
        || intent.receipt.command.operation_key != delivery_key
    {
        return Err(ApiError::conflict("original workspace key mismatch"));
    }
    let result = target
        .readback(&intent.receipt.command, &original)
        .map_err(|_| ApiError::conflict("original delivery journal mismatch"))?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(TaskDeliveryReadback::Static(result)))
}
