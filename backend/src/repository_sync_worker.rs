//! Existing worker-role composition. Git effects are journaled before subprocess execution.
use super::*;
use cicd_app::repository_sync::{Crypto, Store};
use cicd_domain::repository_sync::{RefPlan, plan_ref};
use sqlx::PgPool;
use std::{collections::BTreeSet, sync::atomic::Ordering, time::Duration};

struct Heartbeat(tokio::task::JoinHandle<()>);
impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn supervisor(pool: PgPool, config: crate::config::RuntimeConfig) {
    let store = PgSyncStore {
        pool: pool.clone(),
        worker_enabled: config.git_sync.enabled,
        interval_seconds: config.git_sync.interval_seconds,
    };
    let admission = pool
        .options()
        .clone()
        .max_connections(config.git_sync.concurrency as u32)
        .min_connections(0)
        .connect_lazy_with((*pool.connect_options()).clone());
    let mut tasks = tokio::task::JoinSet::new();
    let mut inflight = BTreeSet::new();
    loop {
        while let Some(result) = tasks.try_join_next() {
            if let Ok(id) = result {
                inflight.remove(&id);
            } else {
                // Running leases and the OS guard still prevent overlap after a task panic.
                inflight.clear();
            }
        }
        if let Err(error) = deliver_events(&store, &config).await {
            tracing::warn!(%error,"sync pipeline delivery pending");
        }
        if config.git_sync.enabled {
            if let Err(error) = store.schedule().await {
                tracing::warn!(%error,"sync scheduling unavailable");
            }
        }
        if let Ok(candidates) = store.candidates().await {
            for (id, repository, storage) in candidates {
                if tasks.len() >= config.git_sync.concurrency {
                    break;
                }
                if inflight.contains(&repository) {
                    continue;
                }
                // Probe without holding DB/admission locks or using worker capacity.
                // execute still acquires namespace admission before its lasting guard.
                match MutationGuard::try_acquire(&config.git.root, repository) {
                    Ok(Some(probe)) => drop(probe),
                    _ => continue,
                }
                if !config.git_sync.enabled
                    && store
                        .run(repository, id)
                        .await
                        .is_ok_and(|run| run.state == "queued")
                {
                    continue;
                }
                let store = store.clone();
                let config = config.clone();
                let admission = admission.clone();
                inflight.insert(repository);
                tasks.spawn(async move {
                    if let Err(error) =
                        execute(&store, &admission, &config, id, repository, &storage).await
                    {
                        tracing::warn!(%id,%error,"sync run will be reconciled");
                    }
                    repository
                });
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn all_refs(store: &PgSyncStore, run: Uuid) -> Result<Vec<RefResult>, Error> {
    let mut result = Vec::new();
    let mut offset = 0;
    loop {
        let (page, total) = store.refs(run, 1000, offset).await?;
        offset += page.len() as i64;
        result.extend(page);
        if offset >= total {
            break;
        }
    }
    Ok(result)
}

async fn admission_healthy(lease: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query("SELECT 1").execute(&mut **lease),
        )
        .await,
        Ok(Ok(_))
    )
}

async fn execute(
    store: &PgSyncStore,
    admission: &PgPool,
    config: &crate::config::RuntimeConfig,
    id: Uuid,
    repository: Uuid,
    storage: &str,
) -> Result<(), Error> {
    let storage = crate::git_host::validate_repo_name(storage).map_err(Error::Invalid)?;
    // Namespace admission always precedes the OS mutation guard, as in receive-pack/merge.
    let mut lease =
        match crate::repository_catalog::catalog_write_lease_on_pool(admission, repository).await {
            Ok(lease) => Some(lease),
            Err(error) if error.status == StatusCode::CONFLICT => None,
            Err(_) => return Err(Error::Unavailable("Namespace admission недоступен")),
        };
    if lease.is_none() && store.run(repository, id).await?.state == "queued" {
        // An archive can win after candidate selection. Keep queued work paused;
        // only already-started work needs read-only outcome reconciliation.
        return Ok(());
    }
    let mut guard = MutationGuard::acquire(&config.git.root, repository).await?;
    // PostgreSQL slot loss must not release real Git capacity while the old
    // process is alive. The shared-storage slot is inherited with this guard.
    if !guard.try_reserve_sync_capacity(&config.git.root, config.git_sync.concurrency)? {
        return Ok(());
    }
    let guard = Arc::new(guard);
    let Some(_slot) = store.slot(config.git_sync.concurrency).await? else {
        return Ok(());
    };
    GitSession::clean_recovered_scratch(&config.git.root, id)?;
    let owner = Uuid::new_v4();
    let Some((run, generation)) = store.claim(id, owner).await? else {
        return Ok(());
    };
    let canceled = Arc::new(AtomicBool::new(false));
    let heartbeat_store = store.clone();
    let heartbeat_cancel = canceled.clone();
    let initially_canceled = run.cancel_requested;
    let _heartbeat = Heartbeat(tokio::spawn(async move {
        loop {
            match heartbeat_store.heartbeat(id, owner, generation).await {
                Ok(cancel) if !cancel || initially_canceled => {}
                _ => {
                    heartbeat_cancel.store(true, Ordering::SeqCst);
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }));
    let namespace_writable = lease.is_some();
    let initially_healthy = match lease.as_mut() {
        Some(lease) => admission_healthy(lease).await,
        None => true,
    };
    let (outcome, mut admission_lost) = if initially_healthy {
        let action = execute_claimed(
            store,
            config,
            &run,
            owner,
            generation,
            &storage,
            guard.clone(),
            canceled.clone(),
            namespace_writable,
        );
        tokio::pin!(action);
        if let Some(lease) = lease.as_mut() {
            // The store heartbeat uses a different connection and cannot prove
            // that this transaction still holds namespace admission. Borrow the
            // actual lease; never replace it with a newly acquired connection.
            let watch = async {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if !admission_healthy(lease).await {
                        break;
                    }
                }
            };
            tokio::pin!(watch);
            tokio::select! {
                outcome = &mut action => (outcome, false),
                _ = &mut watch => {
                    canceled.store(true, Ordering::SeqCst);
                    // GitSession must terminate and drain its owned process
                    // group before its unknown effect can be reconciled. Do not
                    // drop the execution future at the cancellation boundary.
                    (action.await, true)
                }
            }
        } else {
            // An archived namespace can still reconcile previously accepted
            // effects, using the existing read-only recovery path.
            (action.await, false)
        }
    } else {
        (Err(Error::Unavailable("namespace_admission_lost")), true)
    };
    if !admission_lost && let Some(lease) = lease.as_mut() {
        // Also catch a short execution that completes between periodic probes.
        // A failed lease never produces a claimed terminal success.
        admission_lost = !admission_healthy(lease).await;
    }
    let outcome = if admission_lost {
        // Even an empty journal needs recovery after admission loss. A fresh
        // owner must observe refs before interpreting any interrupted effect.
        Ok((
            "reconciling".to_owned(),
            Some("namespace_admission_lost".to_owned()),
        ))
    } else {
        outcome
    };
    let result = match outcome {
        Ok((state, message)) => {
            store
                .finish(id, owner, generation, &state, message.as_deref())
                .await
        }
        Err(error) => {
            let refs = all_refs(store, id).await?;
            let unknown = refs
                .iter()
                .any(|r| matches!(r.state.as_str(), "planned" | "unknown"));
            let state = if unknown {
                "reconciling"
            } else if store.run(repository, id).await?.cancel_requested {
                "canceled"
            } else {
                "failed"
            };
            store
                .finish(id, owner, generation, state, Some(&error.to_string()))
                .await
        }
    };
    // Keep the original guard through subprocess drain and final journal write.
    drop(guard);
    result
}
#[allow(clippy::too_many_arguments)]
async fn execute_claimed(
    store: &PgSyncStore,
    config: &crate::config::RuntimeConfig,
    run: &SyncRun,
    owner: Uuid,
    generation: i64,
    storage: &str,
    guard: Arc<MutationGuard>,
    canceled: Arc<AtomicBool>,
    namespace_writable: bool,
) -> Result<(String, Option<String>), Error> {
    let (_, encrypted) = store
        .scoped(run.connection_id)
        .credential(run.repository_id)
        .await?
        .ok_or(Error::NotFound)?;
    let token = Secrets(config.secrets.key).open(&encrypted)?;
    let mut settings = run.settings.clone();
    settings
        .validate(&config.git_sync.allowed_gitlab_origins)
        .map_err(Error::Invalid)?;
    let git = GitSession::new_with_guard(
        &config.git.root,
        config.git.root.join(format!("{storage}.git")),
        settings,
        token,
        run.id,
        generation,
        canceled.clone(),
        Some(guard),
    )
    .await?;
    let local = git.refs(false).await?;
    let remote = git.refs(true).await?;
    let mut refs = all_refs(store, run.id).await?;
    // Fetch never touches the live repository's refs. Recovered plans retain original SHA intent.
    git.fetch().await?;
    if refs.is_empty() {
        let protected = store.protected_branches(run.repository_id).await?;
        let names: BTreeSet<_> = local.keys().chain(remote.keys()).cloned().collect();
        let mut planned = Vec::new();
        for name in names {
            let a = local.get(&name);
            let b = remote.get(&name);
            let tag = name.starts_with("refs/tags/");
            let (a_ancestor, b_ancestor) = if let (Some(a), Some(b)) = (a, b) {
                if !tag && a != b {
                    (git.ancestor(a, b).await, git.ancestor(b, a).await)
                } else {
                    (false, false)
                }
            } else {
                (false, false)
            };
            let protected = name
                .strip_prefix("refs/heads/")
                .is_some_and(|branch| protected.iter().any(|p| p == branch));
            let plan = plan_ref(
                run.settings.direction,
                tag,
                protected,
                a.map(String::as_str),
                b.map(String::as_str),
                a_ancestor,
                b_ancestor,
            );
            let (direction, old, new, state, reason) = match plan {
                RefPlan::Pull => ("pull", a, b, "planned", None),
                RefPlan::Push => ("push", b, a, "planned", None),
                RefPlan::Protected => ("pull", a, b, "blocked", Some("protected_branch")),
                RefPlan::Conflict => ("none", a, b, "conflict", Some("history_or_tag_conflict")),
                RefPlan::NoChange => ("none", a, a.or(b), "unchanged", None),
            };
            planned.push(RefResult {
                ref_name: name,
                direction: direction.into(),
                old_sha: old.cloned(),
                new_sha: new.cloned(),
                state: state.into(),
                reason: reason.map(str::to_owned),
                event_delivered: direction != "pull",
            });
        }
        store.record_plan(run.id, &planned).await?;
        refs = all_refs(store, run.id).await?;
    }
    for reference in &mut refs {
        if !matches!(reference.state.as_str(), "planned" | "unknown") {
            continue;
        }
        let outgoing = reference.direction == "push";
        let current = git.refs(outgoing).await?;
        let actual = current.get(&reference.ref_name).map(String::as_str);
        let wanted = reference
            .new_sha
            .as_deref()
            .ok_or(Error::Unavailable("Некорректное Git intent"))?;
        if actual == Some(wanted) {
            reference.state = "applied".into();
            store
                .ref_state(
                    run.id,
                    &reference.ref_name,
                    "applied",
                    None,
                    owner,
                    generation,
                )
                .await?;
            continue;
        }
        if actual != reference.old_sha.as_deref() {
            reference.state = "conflict".into();
            store
                .ref_state(
                    run.id,
                    &reference.ref_name,
                    "conflict",
                    Some("destination_changed"),
                    owner,
                    generation,
                )
                .await?;
            continue;
        }
        if !namespace_writable
            || !config.git_sync.enabled
            || !store.allowed_to_apply(run, owner, generation).await?
        {
            reference.state = "canceled".into();
            store
                .ref_state(
                    run.id,
                    &reference.ref_name,
                    "canceled",
                    Some("sync_stopped"),
                    owner,
                    generation,
                )
                .await?;
            continue;
        }
        if !outgoing {
            let protected = store.protected_branches(run.repository_id).await?;
            if reference
                .ref_name
                .strip_prefix("refs/heads/")
                .is_some_and(|branch| protected.iter().any(|p| p == branch))
            {
                reference.state = "blocked".into();
                store
                    .ref_state(
                        run.id,
                        &reference.ref_name,
                        "blocked",
                        Some("protected_branch"),
                        owner,
                        generation,
                    )
                    .await?;
                continue;
            }
        }
        match git
            .apply(
                outgoing,
                &reference.ref_name,
                reference.old_sha.as_deref(),
                wanted,
            )
            .await
        {
            Ok(()) => {
                reference.state = "applied".into();
                store
                    .ref_state(
                        run.id,
                        &reference.ref_name,
                        "applied",
                        None,
                        owner,
                        generation,
                    )
                    .await?;
            }
            Err(error) => {
                // The provider can accept a push and lose the response. Observe before deciding.
                match git.refs(outgoing).await {
                    Ok(current)
                        if current.get(&reference.ref_name).map(String::as_str) == Some(wanted) =>
                    {
                        reference.state = "applied".into();
                        store
                            .ref_state(
                                run.id,
                                &reference.ref_name,
                                "applied",
                                None,
                                owner,
                                generation,
                            )
                            .await?;
                    }
                    Ok(current)
                        if current.get(&reference.ref_name).map(String::as_str)
                            == reference.old_sha.as_deref() =>
                    {
                        let (state, reason) = match error {
                            Error::Conflict("destination_changed" | "non_fast_forward") => {
                                ("conflict", "destination_changed")
                            }
                            Error::Unavailable("git_outcome_unknown") => {
                                ("planned", "safe_retry_after_observation")
                            }
                            Error::Unavailable("git_access_denied") => ("failed", "access_denied"),
                            Error::Unavailable("git_protected_branch") => {
                                ("blocked", "provider_protected_branch")
                            }
                            _ => ("failed", "provider_rejected_or_source_missing"),
                        };
                        reference.state = state.into();
                        store
                            .ref_state(
                                run.id,
                                &reference.ref_name,
                                state,
                                Some(reason),
                                owner,
                                generation,
                            )
                            .await?;
                    }
                    Ok(_) => {
                        reference.state = "conflict".into();
                        store
                            .ref_state(
                                run.id,
                                &reference.ref_name,
                                "conflict",
                                Some("destination_changed"),
                                owner,
                                generation,
                            )
                            .await?;
                    }
                    Err(_) => {
                        reference.state = "unknown".into();
                        store
                            .ref_state(
                                run.id,
                                &reference.ref_name,
                                "unknown",
                                Some("outcome_unknown"),
                                owner,
                                generation,
                            )
                            .await?;
                    }
                }
            }
        }
    }
    let applied = refs.iter().any(|r| r.state == "applied");
    let blocked = refs
        .iter()
        .any(|r| matches!(r.state.as_str(), "conflict" | "blocked"));
    let failed = refs.iter().any(|r| r.state == "failed");
    let canceled = refs.iter().any(|r| r.state == "canceled") || run.cancel_requested;
    let state = if refs
        .iter()
        .any(|r| matches!(r.state.as_str(), "unknown" | "planned"))
    {
        "reconciling"
    } else if applied && (blocked || canceled || failed) {
        "partial"
    } else if blocked {
        "conflict"
    } else if failed {
        "failed"
    } else if canceled {
        "canceled"
    } else {
        "success"
    };
    Ok((state.into(), None))
}

async fn deliver_events(
    store: &PgSyncStore,
    config: &crate::config::RuntimeConfig,
) -> Result<(), Error> {
    for (run, repository, reference) in store.pending_events().await? {
        if let Err(error) = deliver_event(store, config, run, repository, &reference).await {
            store.event_retry(run, &reference.ref_name).await?;
            tracing::warn!(%run,%error,"sync CI event delivery pending");
        }
    }
    Ok(())
}
async fn deliver_event(
    store: &PgSyncStore,
    config: &crate::config::RuntimeConfig,
    run: Uuid,
    repository: Uuid,
    reference: &RefResult,
) -> Result<(), Error> {
    // An accepted inbound update is also a local change for other automatic
    // connections. Durable dirty flags coalesce retries; equal refs end the loop.
    store.signal(repository).await?;
    let sha = reference
        .new_sha
        .as_deref()
        .ok_or(Error::Unavailable("CI event SHA missing"))?;
    let storage = crate::repository_catalog::storage_by_id(&store.pool, repository)
        .await
        .map_err(|_| Error::Unavailable("Git storage недоступен"))?;
    let key = crate::git_host::git_push_idempotency_key(&storage, &reference.ref_name, sha);
    let project = crate::delivery_configs::push_decision(&store.pool, repository, Some(&key))
        .await
        .map_err(|_| Error::Unavailable("Не удалось прочитать push policy"))?;
    if let Some(project) = project {
        // Git tags may legally point to a blob/tree. Transfer them, but there is no
        // commit to build; persist that terminal CI decision instead of retrying forever.
        if reference.ref_name.starts_with("refs/tags/") {
            if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Unavailable("Invalid CI event SHA"));
            }
            let storage = crate::git_host::validate_repo_name(&storage).map_err(Error::Invalid)?;
            let mut command = tokio::process::Command::new("git");
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .arg(format!(
                    "--git-dir={}",
                    config.git.root.join(format!("{storage}.git")).display()
                ))
                .args(["cat-file", "-t", &format!("{sha}^{{}}")])
                .kill_on_drop(true);
            let object = tokio::time::timeout(Duration::from_secs(5), command.output())
                .await
                .map_err(|_| Error::Unavailable("CI object observation timed out"))?
                .map_err(|_| Error::Unavailable("CI object observation failed"))?;
            if !object.status.success() {
                return Err(Error::Unavailable("CI event object unavailable"));
            }
            if object.stdout != b"commit\n" {
                store.event_non_commit_tag(run, &reference.ref_name).await?;
                return Ok(());
            }
        }
        let short = reference
            .ref_name
            .strip_prefix("refs/heads/")
            .or_else(|| reference.ref_name.strip_prefix("refs/tags/"))
            .ok_or(Error::Unavailable("Некорректный ref события"))?;
        crate::pipeline_authority::with_saved(
            store.event_owner(run).await?,
            false,
            crate::api::pipelines_routes::create_pipeline_at_commit(
                &store.pool,
                project,
                short.into(),
                "git-push",
                &key,
                &config.git.root,
                sha,
            ),
        )
        .await
        .map_err(|_| Error::Unavailable("CI-событие сохранено, доставка ожидает повтора"))?;
    }
    store.event_delivered(run, &reference.ref_name).await?;
    Ok(())
}
