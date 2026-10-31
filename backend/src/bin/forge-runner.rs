use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, bail, ensure};
use chrono::Utc;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::watch,
};
use uuid::Uuid;

const PROTOCOL_VERSION: i32 = 1;
const CONTROL_POLL_INTERVAL_SECONDS: u64 = 2;
const MAX_ARTIFACT_BYTES: u64 = 50 * 1024 * 1024;
const SERVER_POLL_WAIT_MAX_SECONDS: u64 = 30;
const RUNNER_HEARTBEAT_INTERVAL_SECONDS: u64 = 15;

#[derive(Debug, Parser)]
#[command(
    name = "forge-runner",
    about = "External Forge CI/CD runner for the /api/v1/runner protocol"
)]
struct Cli {
    #[arg(long, env = "CICD_API_URL", default_value = "http://127.0.0.1:22801")]
    api_url: String,
    #[arg(long, env = "CICD_RUNNER_NAME", default_value = "forge-runner")]
    name: String,
    #[arg(long, env = "CICD_RUNNER_CREDENTIAL")]
    credential: Option<String>,
    #[arg(long, env = "CICD_RUNNER_REGISTRATION_TOKEN")]
    registration_token: Option<String>,
    #[arg(
        long,
        env = "CICD_RUNNER_TAGS",
        value_delimiter = ',',
        default_value = "linux,host"
    )]
    tags: Vec<String>,
    #[arg(long, env = "CICD_RUNNER_TOTAL_SLOTS", default_value_t = 1)]
    total_slots: i32,
    #[arg(long, env = "CICD_RUNNER_WORK_DIR")]
    work_dir: Option<PathBuf>,
    #[arg(long, env = "CICD_RUNNER_POLL_INTERVAL_SECONDS", default_value_t = 5)]
    poll_interval_seconds: u64,
    #[arg(long, env = "CICD_RUNNER_ONCE", default_value_t = false)]
    once: bool,
    #[arg(long, env = "CICD_RUNNER_NO_CHECKOUT", default_value_t = false)]
    no_checkout: bool,
    #[arg(long, env = "CICD_RUNNER_KEEP_WORKSPACE", default_value_t = false)]
    keep_workspace: bool,
    #[arg(long, conflicts_with = "reconcile_workspaces")]
    inspect_workspaces: bool,
    #[arg(long)]
    reconcile_workspaces: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterResponse {
    runner_id: Uuid,
    credential: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaseOffer {
    lease_id: Uuid,
    lease_token: String,
    fencing_token: i64,
    attempt: AttemptSpec,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttemptSpec {
    id: Uuid,
    #[serde(default)]
    job_id: Uuid,
    #[serde(default = "shell_executor")]
    executor: String,
    #[serde(default)]
    image: String,
    job_key: String,
    git_ref: String,
    commit_sha: Option<String>,
    commands: Vec<String>,
    #[serde(default)]
    secrets: Vec<String>,
    #[serde(default)]
    artifacts: Vec<String>,
    timeout_seconds: i32,
    workspace: WorkspaceSpec,
    #[serde(default)]
    image_publish: Option<cicd::domain::image_publish::ImagePublishSpec>,
    #[serde(default)]
    helm_deploy: Option<cicd::domain::helm_deploy::DeploymentOffer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSpec {
    checkout: bool,
    checkout_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LeaseMutation<'a> {
    protocol_version: i32,
    lease_token: &'a str,
    fencing_token: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaseControlResponse {
    cancel_requested: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecretResolveRequest<'a> {
    protocol_version: i32,
    lease_token: &'a str,
    fencing_token: i64,
    secret_names: &'a [String],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretResolveResponse {
    items: Vec<SecretItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretItem {
    name: String,
    injection: String,
    value: String,
}

#[derive(Debug, Clone)]
struct LogContext {
    client: reqwest::Client,
    base: String,
    credential: String,
    lease_id: Uuid,
    lease_token: String,
    fencing_token: i64,
    attempt_id: Uuid,
    masks: Vec<String>,
}

#[derive(Debug, Clone)]
struct HeartbeatConfig {
    total_slots: i32,
    tags: Vec<String>,
}

impl HeartbeatConfig {
    fn from_cli(cli: &Cli) -> Self {
        Self {
            total_slots: cli.total_slots,
            tags: cli.tags.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerLogLine {
    stream: &'static str,
    message: String,
}

#[derive(Debug)]
struct ExecutionResult {
    outcome: &'static str,
    exit_code: Option<i32>,
    diagnostic: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompletionAck {
    protocol_version: i32,
    accepted: bool,
    terminal_status: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LeaseReceipt {
    protocol_version: i32,
    lease_id: Uuid,
    attempt_id: Uuid,
    fencing_token: i64,
    lease_status: String,
    terminal_status: Option<String>,
    completed_at: Option<chrono::DateTime<Utc>>,
    terminal_acknowledged: bool,
}

#[derive(Debug)]
enum CommandOutcome {
    Exited(std::process::ExitStatus),
    Canceled,
}

#[derive(Debug)]
struct UnconfirmedExecution;

impl std::fmt::Display for UnconfirmedExecution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "command termination is unconfirmed; workspace requires process reconciliation",
        )
    }
}

impl std::error::Error for UnconfirmedExecution {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("CICD_QUIESCENCE_DIR").is_none() {
        return run().await;
    }
    tokio::select! { result=run()=>result, result=cicd::quiescence::wait_for_exit()=>result }
}
async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    validate_cli(&cli)?;
    let root = workspace_root(&cli);
    cicd::quiescence::initialize(
        "external-runner",
        vec![cicd::quiescence::CustodyRoot::workspace(root.clone())],
    )?;
    cicd::quiescence::wait_for_enrollment().await?;
    if cli.inspect_workspaces {
        println!(
            "{}",
            serde_json::to_string(&cicd::runner_workspace::OwnedWorkspace::inventory(&root)?)?
        );
        return Ok(());
    }
    if !cli.reconcile_workspaces {
        ensure_no_unresolved_workspaces(&root)?;
    }
    let base = normalize_api_base(&cli.api_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .context("build runner HTTP client")?;
    let startup = cicd::quiescence::enter(true)?;
    let credential = ensure_credential(&client, &base, &cli).await?;
    drop(startup);

    if cli.reconcile_workspaces {
        reconcile_workspaces(&client, &base, &credential, &root, cli.keep_workspace).await?;
        return Ok(());
    }
    // A local ACK file is recovery metadata, not fresh server authority.
    reconcile_workspaces(&client, &base, &credential, &root, true).await?;

    loop {
        let Ok(_effect) = cicd::quiescence::enter(true) else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        heartbeat(&client, &base, &credential, &cli, 0, &[]).await?;
        let poll_started = tokio::time::Instant::now();
        let offer = match poll_work(&client, &base, &credential, &cli).await {
            Ok(offer) => offer,
            Err(_) if !cicd::quiescence::accepting() => {
                drop(_effect);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            Err(error) => return Err(error),
        };
        drop(_effect);
        match offer {
            Some(offer) => {
                run_offer(&client, &base, &credential, &cli, offer).await?;
                if cli.once {
                    return Ok(());
                }
            }
            None if cli.once => return Ok(()),
            None => sleep_after_empty_poll(&cli, poll_started.elapsed()).await,
        }
    }
}

fn workspace_root(cli: &Cli) -> PathBuf {
    cli.work_dir.clone().unwrap_or_else(|| {
        if kubernetes_mode() {
            PathBuf::from("/workspaces")
        } else {
            std::env::temp_dir().join("forge-runner")
        }
    })
}

fn ensure_no_unresolved_workspaces(root: &Path) -> anyhow::Result<()> {
    if cicd::runner_workspace::OwnedWorkspace::inventory(root)?
        .iter()
        .any(|workspace| !workspace.acknowledged)
    {
        bail!(
            "unresolved workspaces require owner readback; use --inspect-workspaces and --reconcile-workspaces before polling new work"
        );
    }
    Ok(())
}

async fn read_terminal_receipt(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    lease_id: Uuid,
    attempt_id: Uuid,
    generation: i64,
    outcome: &str,
) -> anyhow::Result<()> {
    let mut response = client
        .get(format!("{base}/api/v1/runner/leases/{lease_id}/receipt"))
        .bearer_auth(credential)
        .header("Accept-Encoding", "identity")
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("terminal readback unavailable"))?;
    if response.status() != reqwest::StatusCode::OK {
        bail!("terminal readback unavailable");
    }
    if response
        .headers()
        .get("content-encoding")
        .is_some_and(|value| value != "identity")
    {
        bail!("invalid terminal readback encoding");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("terminal readback unavailable"))?
    {
        if body.len() + chunk.len() > 4096 {
            bail!("terminal readback exceeds safety limit");
        }
        body.extend_from_slice(&chunk);
    }
    let receipt: LeaseReceipt =
        serde_json::from_slice(&body).map_err(|_| anyhow::anyhow!("invalid terminal readback"))?;
    if receipt.protocol_version != PROTOCOL_VERSION
        || receipt.lease_id != lease_id
        || receipt.attempt_id != attempt_id
        || receipt.fencing_token != generation
        || !receipt.terminal_acknowledged
        || receipt.completed_at.is_none()
        || receipt.terminal_status.as_deref() != Some(outcome)
        || receipt.lease_status
            != if outcome == "canceled" {
                "canceled"
            } else {
                "completed"
            }
    {
        bail!("terminal readback does not acknowledge this attempt result");
    }
    Ok(())
}

async fn reconcile_workspaces(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    root: &Path,
    keep_workspace: bool,
) -> anyhow::Result<()> {
    let _effect = cicd::quiescence::enter(false)?;
    let mut unresolved = 0;
    for record in cicd::runner_workspace::OwnedWorkspace::inventory(root)? {
        if kubernetes_mode()
            && !cicd::execution::kubernetes::external_marker(
                root,
                record.attempt_id,
                record.generation,
            )?
        {
            let ownership = client
                .get(format!(
                    "{base}/api/v1/runner/leases/{}/receipt",
                    record.lease_id
                ))
                .bearer_auth(credential)
                .send()
                .await?;
            // Only an authenticated foreign lease with no issued external marker may be skipped.
            if ownership.status() == reqwest::StatusCode::NOT_FOUND {
                continue;
            }
            ensure_success(ownership).await?;
        }
        if kubernetes_mode() {
            let known_unsubmitted = async {
                let workspace =
                    cicd::runner_workspace::OwnedWorkspace::reopen(root, &record.workspace_id)?;
                workspace.verify_identity(record.attempt_id, record.lease_id, record.generation)?;
                if cicd::execution::kubernetes::close_unprepared(
                    &cicd::execution::kubernetes::Config::from_env()?,
                    root,
                    record.attempt_id,
                    record.lease_id,
                    record.generation,
                    workspace.checkout(),
                )
                .await?
                .is_some()
                {
                    return Ok::<bool, anyhow::Error>(true);
                }
                cicd::execution::kubernetes::Execution::reopen(
                    root,
                    record.attempt_id,
                    record.generation,
                )?
                .close_not_submitted()
                .await
            }
            .await;
            match known_unsubmitted {
                Ok(false) => {} // The original issued/terminal path below is unchanged.
                Ok(true) => {
                    if recover_kubernetes(client, base, credential, root, &record, keep_workspace)
                        .await
                        .is_err()
                    {
                        unresolved += 1;
                    }
                    continue;
                }
                Err(_) => {
                    unresolved += 1;
                    continue;
                }
            }
        }
        let Some(outcome) = record.terminal_status.as_deref() else {
            if kubernetes_mode() {
                match recover_kubernetes(client, base, credential, root, &record, keep_workspace)
                    .await
                {
                    Ok(()) => continue,
                    Err(_) => {
                        unresolved += 1;
                        continue;
                    }
                }
            }
            unresolved += 1;
            continue;
        };
        if read_terminal_receipt(
            client,
            base,
            credential,
            record.lease_id,
            record.attempt_id,
            record.generation,
            outcome,
        )
        .await
        .is_err()
        {
            if kubernetes_mode() {
                match recover_kubernetes(client, base, credential, root, &record, keep_workspace)
                    .await
                {
                    Ok(()) => continue,
                    Err(_) => {
                        unresolved += 1;
                        continue;
                    }
                }
            }
            unresolved += 1;
            continue;
        }
        let workspace = cicd::runner_workspace::OwnedWorkspace::reopen(root, &record.workspace_id)?;
        workspace.verify_identity(record.attempt_id, record.lease_id, record.generation)?;
        if kubernetes_mode() {
            cicd::execution::kubernetes::Execution::reopen(
                root,
                record.attempt_id,
                record.generation,
            )?
            .acknowledge()
            .await?;
        }
        workspace.acknowledge_completion(outcome)?;
        if !keep_workspace {
            workspace.cleanup_after_ack().await?;
        }
        if kubernetes_mode() {
            cicd::execution::kubernetes::Execution::reopen(
                root,
                record.attempt_id,
                record.generation,
            )?
            .finish_workspace_cleanup()?;
        }
    }
    if unresolved > 0 {
        bail!("{unresolved} workspaces remain unresolved; no commands were re-executed");
    }
    Ok(())
}

fn validate_cli(cli: &Cli) -> anyhow::Result<()> {
    if cli.total_slots < 1 || cli.total_slots > 1024 {
        bail!("--total-slots must be between 1 and 1024");
    }
    if cli.poll_interval_seconds > 300 {
        bail!("--poll-interval-seconds must be at most 300");
    }
    Ok(())
}

fn normalize_api_base(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/api/v1")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

async fn ensure_credential(
    client: &reqwest::Client,
    base: &str,
    cli: &Cli,
) -> anyhow::Result<String> {
    if let Some(credential) = cli
        .credential
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(credential.to_owned());
    }
    let registration_token = cli
        .registration_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("CICD_RUNNER_CREDENTIAL or CICD_RUNNER_REGISTRATION_TOKEN is required")?;
    let response = client
        .post(format!("{base}/api/v1/runner/register"))
        .json(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "registrationToken": registration_token,
            "name": &cli.name,
            "tags": &cli.tags,
            "capabilities": runner_capabilities().await,
        }))
        .send()
        .await
        .context("register runner request failed")?;
    let registered: RegisterResponse = response_json(response).await?;
    eprintln!(
        "registered runner {}. Persist CICD_RUNNER_CREDENTIAL for the next start.",
        registered.runner_id
    );
    eprintln!("runner credential: {}", registered.credential);
    Ok(registered.credential)
}

async fn heartbeat(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    cli: &Cli,
    busy_slots: i32,
    active_lease_ids: &[Uuid],
) -> anyhow::Result<()> {
    heartbeat_with_config(
        client,
        base,
        credential,
        &HeartbeatConfig::from_cli(cli),
        busy_slots,
        active_lease_ids,
    )
    .await
}

async fn heartbeat_with_config(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    config: &HeartbeatConfig,
    busy_slots: i32,
    active_lease_ids: &[Uuid],
) -> anyhow::Result<()> {
    let response = client
        .post(format!("{base}/api/v1/runner/heartbeat"))
        .bearer_auth(credential)
        .json(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "status": "online",
            "draining": false,
            "capacity": {
                "totalSlots": config.total_slots,
                "busySlots": busy_slots,
            },
            "tags": &config.tags,
            "capabilities": runner_capabilities().await,
            "activeLeaseIds": active_lease_ids,
        }))
        .send()
        .await
        .context("heartbeat request failed")?;
    ensure_success(response).await.context("heartbeat failed")
}

async fn poll_work(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    cli: &Cli,
) -> anyhow::Result<Option<LeaseOffer>> {
    let response = client
        .post(format!("{base}/api/v1/runner/work:poll"))
        .bearer_auth(credential)
        .json(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capacity": {"freeSlots": cli.total_slots},
            "waitSeconds": runner_poll_wait_seconds(cli),
            "tags": &cli.tags,
        }))
        .send()
        .await
        .context("work poll request failed")?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    Ok(Some(response_json(response).await?))
}

fn runner_poll_wait_seconds(cli: &Cli) -> u64 {
    cli.poll_interval_seconds.min(SERVER_POLL_WAIT_MAX_SECONDS)
}

async fn sleep_after_empty_poll(cli: &Cli, elapsed: Duration) {
    if let Some(remaining) = empty_poll_sleep_duration(cli, elapsed) {
        tokio::time::sleep(remaining).await;
    }
}

fn empty_poll_sleep_duration(cli: &Cli, elapsed: Duration) -> Option<Duration> {
    Duration::from_secs(cli.poll_interval_seconds)
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
}

async fn run_offer(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    cli: &Cli,
    offer: LeaseOffer,
) -> anyhow::Result<()> {
    let _effect = cicd::quiescence::enter(false)?;
    eprintln!(
        "accepted offer lease={} attempt={} job={}",
        offer.lease_id, offer.attempt.id, offer.attempt.job_key
    );
    ack_lease(client, base, credential, &offer).await?;
    heartbeat(client, base, credential, cli, 1, &[offer.lease_id]).await?;

    let heartbeat_config = HeartbeatConfig::from_cli(cli);
    let active_lease_ids = vec![offer.lease_id];
    let (stop_tx, stop_rx) = watch::channel(false);
    let renew_task = tokio::spawn(renew_loop(
        client.clone(),
        base.to_owned(),
        credential.to_owned(),
        offer.lease_id,
        offer.lease_token.clone(),
        offer.fencing_token,
        stop_rx.clone(),
    ));
    let heartbeat_task = tokio::spawn(heartbeat_loop(
        client.clone(),
        base.to_owned(),
        credential.to_owned(),
        heartbeat_config,
        active_lease_ids,
        stop_rx,
    ));

    let secret_env = match resolve_lease_secrets(client, base, credential, &offer).await {
        Ok(secret_env) => secret_env,
        Err(error) => {
            stop_lease_background_tasks(&stop_tx, renew_task, heartbeat_task).await;
            let completion_result = complete_lease(
                client,
                base,
                credential,
                &offer,
                ExecutionResult {
                    outcome: "failed",
                    exit_code: None,
                    diagnostic: Some(truncate_diagnostic(&format!(
                        "secret resolve failed: {error:#}"
                    ))),
                },
            )
            .await;
            heartbeat(client, base, credential, cli, 0, &[]).await?;
            return completion_result;
        }
    };
    let masks: Vec<String> = secret_env
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(_, value)| value.clone())
        .collect();
    let log_context = LogContext {
        client: client.clone(),
        base: base.to_owned(),
        credential: credential.to_owned(),
        lease_id: offer.lease_id,
        lease_token: offer.lease_token.clone(),
        fencing_token: offer.fencing_token,
        attempt_id: offer.attempt.id,
        masks,
    };
    let result = execute_attempt(cli, &offer, Some(log_context), &secret_env).await;
    let completion_result =
        complete_attempt_result(client, base, credential, cli, &offer, result).await;
    stop_lease_background_tasks(&stop_tx, renew_task, heartbeat_task).await;
    completion_result?;
    heartbeat(client, base, credential, cli, 0, &[]).await?;
    Ok(())
}

async fn complete_attempt_result(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    cli: &Cli,
    offer: &LeaseOffer,
    result: anyhow::Result<(ExecutionResult, cicd::runner_workspace::OwnedWorkspace)>,
) -> anyhow::Result<()> {
    match result {
        Ok(executed) => {
            complete_executed_attempt(
                client,
                base,
                credential,
                offer,
                executed,
                cli.keep_workspace,
            )
            .await
        }
        Err(error) if error.is::<UnconfirmedExecution>() => Err(error),
        Err(error) => {
            complete_lease(
                client,
                base,
                credential,
                offer,
                ExecutionResult {
                    outcome: "failed",
                    exit_code: None,
                    diagnostic: Some(truncate_diagnostic(&format!("{error:#}"))),
                },
            )
            .await
        }
    }
}

async fn ack_lease(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    offer: &LeaseOffer,
) -> anyhow::Result<()> {
    let response = client
        .post(format!(
            "{base}/api/v1/runner/leases/{}/ack",
            offer.lease_id
        ))
        .bearer_auth(credential)
        .json(&LeaseMutation {
            protocol_version: PROTOCOL_VERSION,
            lease_token: &offer.lease_token,
            fencing_token: offer.fencing_token,
        })
        .send()
        .await
        .context("ack request failed")?;
    ensure_success(response).await.context("ack failed")
}

async fn stop_lease_background_tasks(
    stop_tx: &watch::Sender<bool>,
    renew_task: tokio::task::JoinHandle<()>,
    heartbeat_task: tokio::task::JoinHandle<()>,
) {
    let _ = stop_tx.send(true);
    renew_task.abort();
    heartbeat_task.abort();
    let _ = renew_task.await;
    let _ = heartbeat_task.await;
}

fn active_busy_slots(total_slots: i32, active_lease_ids: &[Uuid]) -> i32 {
    active_lease_ids.len().min(total_slots.max(0) as usize) as i32
}

async fn heartbeat_loop(
    client: reqwest::Client,
    base: String,
    credential: String,
    config: HeartbeatConfig,
    active_lease_ids: Vec<Uuid>,
    mut stop_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    return;
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(RUNNER_HEARTBEAT_INTERVAL_SECONDS)) => {
                let busy_slots = active_busy_slots(config.total_slots, &active_lease_ids);
                if let Err(error) = heartbeat_with_config(
                    &client,
                    &base,
                    &credential,
                    &config,
                    busy_slots,
                    &active_lease_ids,
                )
                .await
                {
                    eprintln!("active heartbeat failed: {error:#}");
                }
            }
        }
    }
}

async fn resolve_lease_secrets(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    offer: &LeaseOffer,
) -> anyhow::Result<Vec<(String, String)>> {
    if offer.attempt.secrets.is_empty() {
        return Ok(Vec::new());
    }
    let response = client
        .post(format!(
            "{base}/api/v1/runner/leases/{}/secrets:resolve",
            offer.lease_id
        ))
        .bearer_auth(credential)
        .json(&SecretResolveRequest {
            protocol_version: PROTOCOL_VERSION,
            lease_token: &offer.lease_token,
            fencing_token: offer.fencing_token,
            secret_names: &offer.attempt.secrets,
        })
        .send()
        .await
        .context("resolve secrets request failed")?;
    let resolved: SecretResolveResponse = response_json(response)
        .await
        .context("resolve secrets failed")?;
    let requested: BTreeSet<&str> = offer.attempt.secrets.iter().map(String::as_str).collect();
    let mut env = Vec::with_capacity(resolved.items.len());
    for item in resolved.items {
        if item.injection != "env" || !requested.contains(item.name.as_str()) {
            bail!("API returned an invalid secret bundle");
        }
        env.push((item.name, item.value));
    }
    if env.len() != requested.len() {
        bail!("API returned an incomplete secret bundle");
    }
    Ok(env)
}

async fn renew_loop(
    client: reqwest::Client,
    base: String,
    credential: String,
    lease_id: Uuid,
    lease_token: String,
    fencing_token: i64,
    mut stop_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    return;
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(40)) => {
                let response = client
                    .post(format!("{base}/api/v1/runner/leases/{lease_id}/renew"))
                    .bearer_auth(&credential)
                    .json(&LeaseMutation {
                        protocol_version: PROTOCOL_VERSION,
                        lease_token: &lease_token,
                        fencing_token,
                    })
                    .send()
                    .await;
                match response {
                    Ok(response) if response.status().is_success() => {}
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        eprintln!("lease renew failed: {status} {body}");
                    }
                    Err(error) => eprintln!("lease renew request failed: {error}"),
                }
            }
        }
    }
}

async fn complete_lease(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    offer: &LeaseOffer,
    result: ExecutionResult,
) -> anyhow::Result<()> {
    let deployment_observation = if kubernetes_mode() {
        cicd::execution::helm::completion_observation(
            Path::new("/workspaces"),
            offer.attempt.id,
            offer.fencing_token,
        )?
    } else {
        None
    };
    let response = client
        .post(format!(
            "{base}/api/v1/runner/leases/{}/complete",
            offer.lease_id
        ))
        .bearer_auth(credential)
        .json(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "leaseToken": &offer.lease_token,
            "fencingToken": offer.fencing_token,
            "attemptId": offer.attempt.id,
            "outcome": result.outcome,
            "finishedAt": Utc::now(),
            "exitCode": result.exit_code,
            "diagnostic": result.diagnostic,
            "deploymentObservation": deployment_observation,
            "terminationProof": if kubernetes_mode(){cicd::execution::kubernetes::Execution::reopen(Path::new("/workspaces"),offer.attempt.id,offer.fencing_token).ok().and_then(|e|e.termination_proof().ok())}else{None},
        }))
        .send()
        .await
        .context("complete request failed")?;
    let ack: CompletionAck = response_json(response).await.context("complete failed")?;
    if ack.protocol_version != PROTOCOL_VERSION
        || !ack.accepted
        || ack.terminal_status != result.outcome
    {
        bail!("terminal completion acknowledgement does not match this result");
    }
    Ok(())
}

async fn complete_executed_attempt(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    offer: &LeaseOffer,
    (result, workspace): (ExecutionResult, cicd::runner_workspace::OwnedWorkspace),
    keep_workspace: bool,
) -> anyhow::Result<()> {
    let outcome = result.outcome;
    let cleanup_root = workspace_root_for_checkout(workspace.checkout())?;
    let mut workspace_closed = true;
    let recovery_record = workspace.record_completion(outcome).is_err();
    if recovery_record && (!kubernetes_mode() || !matches!(outcome, "failed" | "canceled")) {
        bail!("workspace completion intention conflicts");
    }
    let completion = complete_lease(client, base, credential, offer, result).await;
    // Never repeat the POST: a lost reply may follow a committed completion.
    let completion = match completion {
        Ok(()) => Ok(()),
        Err(_) => {
            read_terminal_receipt(
                client,
                base,
                credential,
                offer.lease_id,
                offer.attempt.id,
                offer.fencing_token,
                outcome,
            )
            .await
        }
    };
    if completion.is_ok() {
        if recovery_record {
            workspace.record_recovered_termination(outcome)?;
        }
        if kubernetes_mode() {
            let execution = cicd::execution::kubernetes::Execution::reopen(
                &workspace_root_for_checkout(workspace.checkout())?,
                offer.attempt.id,
                offer.fencing_token,
            )?;
            if execution.not_submitted()? {
                read_terminal_receipt(
                    client,
                    base,
                    credential,
                    offer.lease_id,
                    offer.attempt.id,
                    offer.fencing_token,
                    outcome,
                )
                .await?;
            }
            execution.acknowledge().await?;
        }
        workspace.acknowledge_completion(outcome)?;
    }
    if completion.is_ok() && !keep_workspace {
        if let Err(error) = workspace.cleanup_after_ack().await {
            workspace_closed = false;
            eprintln!("acknowledged workspace retained: {error:#}");
        }
    }
    if completion.is_ok() && workspace_closed && kubernetes_mode() {
        cicd::execution::kubernetes::Execution::reopen(
            &cleanup_root,
            offer.attempt.id,
            offer.fencing_token,
        )?
        .finish_workspace_cleanup()?;
    }
    completion
}

async fn execute_attempt(
    cli: &Cli,
    offer: &LeaseOffer,
    log_context: Option<LogContext>,
    secret_env: &[(String, String)],
) -> anyhow::Result<(ExecutionResult, cicd::runner_workspace::OwnedWorkspace)> {
    let owned_workspace = prepare_workspace(cli, offer).await?;
    let workspace = owned_workspace.checkout();
    if kubernetes_mode() {
        return execute_kubernetes(cli, offer, log_context, secret_env, owned_workspace).await;
    }
    if offer.attempt.executor != "shell" {
        bail!("unsupported offered executor");
    }
    let mut last_exit_code = None;
    let mut result = ExecutionResult {
        outcome: "success",
        exit_code: None,
        diagnostic: None,
    };

    let mut processes = AttemptProcesses::new()?;
    for command in &offer.attempt.commands {
        let command_outcome = match run_shell_command_in_scope(
            command,
            workspace,
            offer.attempt.timeout_seconds,
            log_context.as_ref(),
            secret_env,
            &mut processes,
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
                if error.is::<UnconfirmedExecution>() {
                    let _ = processes.finish().await;
                    return Err(error);
                }
                result = ExecutionResult {
                    outcome: "failed",
                    exit_code: None,
                    diagnostic: Some(truncate_diagnostic(&format!("{error:#}"))),
                };
                break;
            }
        };
        let status = match command_outcome {
            CommandOutcome::Exited(status) => status,
            CommandOutcome::Canceled => {
                result = ExecutionResult {
                    outcome: "canceled",
                    exit_code: None,
                    diagnostic: Some("runner cancellation requested".to_string()),
                };
                break;
            }
        };
        last_exit_code = status.code();
        if !status.success() {
            result = ExecutionResult {
                outcome: "failed",
                exit_code: last_exit_code,
                diagnostic: Some(format!(
                    "command exited with status {}",
                    status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string())
                )),
            };
            break;
        }
    }

    // Background services belong to the attempt, including across successful steps.
    // Stop them and flush their logs before artifacts, terminal ACK or cleanup.
    if let Err(error) = processes.finish().await {
        if error.is::<UnconfirmedExecution>() {
            return Err(error);
        }
        result = ExecutionResult {
            outcome: "failed",
            exit_code: None,
            diagnostic: Some(truncate_diagnostic(&format!("{error:#}"))),
        };
    }

    let artifact_error = if result.outcome == "canceled" {
        None
    } else {
        match log_context.as_ref() {
            Some(context) => upload_declared_artifacts(context, offer, workspace)
                .await
                .err(),
            None if offer.attempt.artifacts.is_empty() => None,
            None => Some(anyhow::anyhow!(
                "artifact upload requires runner protocol log context"
            )),
        }
    };
    if result.outcome == "success" {
        if let Some(error) = artifact_error {
            result = ExecutionResult {
                outcome: "failed",
                exit_code: None,
                diagnostic: Some(truncate_diagnostic(&format!(
                    "artifact upload failed: {error:#}"
                ))),
            };
        }
    }

    if result.outcome == "success" {
        result.exit_code = last_exit_code.or(Some(0));
    }
    Ok((result, owned_workspace))
}

async fn prepare_workspace(
    cli: &Cli,
    offer: &LeaseOffer,
) -> anyhow::Result<cicd::runner_workspace::OwnedWorkspace> {
    let root = workspace_root(cli);
    let workspace = cicd::runner_workspace::OwnedWorkspace::create(
        &root,
        offer.attempt.id,
        offer.lease_id,
        offer.fencing_token,
    )?;
    if offer.attempt.commit_sha.is_some() && (!offer.attempt.workspace.checkout || cli.no_checkout)
    {
        bail!("pinned attempt requires checkout; no-checkout cannot bypass verification");
    }

    if offer.attempt.workspace.checkout && !cli.no_checkout {
        let url = offer
            .attempt
            .workspace
            .checkout_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("lease requires checkout but checkoutUrl is missing")?;
        if offer.attempt.image_publish.is_some() || offer.attempt.helm_deploy.is_some() {
            let pin = offer
                .attempt
                .commit_sha
                .as_deref()
                .context("typed delivery requires source pin")?;
            workspace.checkout_source_snapshot(url, pin).await?;
        } else {
            workspace
                .clone_checkout(
                    url,
                    offer.attempt.commit_sha.as_deref(),
                    &offer.attempt.git_ref,
                )
                .await?;
        }
    } else {
        workspace.create_empty_checkout()?;
    }
    Ok(workspace)
}

/// One live process-group leader pins ownership until the whole attempt ends.
/// Keeping the leader alive prevents a completed step's group ID being reused.
struct AttemptProcesses {
    #[cfg(unix)]
    anchor: tokio::process::Child,
    #[cfg(unix)]
    group: u32,
    stopped: bool,
    logs: Vec<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl AttemptProcesses {
    fn new() -> anyhow::Result<Self> {
        #[cfg(target_os = "linux")]
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) } != 0 {
            return Err(anyhow::Error::new(UnconfirmedExecution)
                .context("cannot establish descendant reaping"));
        }
        #[cfg(unix)]
        let (anchor, group) = {
            use std::os::unix::process::CommandExt;
            let mut command = shell("exec sleep 2147483647");
            command.as_std_mut().process_group(0);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            command.kill_on_drop(true);
            let child = command
                .spawn()
                .context("spawn attempt process-group anchor")?;
            let group = child.id().ok_or(UnconfirmedExecution)?;
            (child, group)
        };
        Ok(Self {
            #[cfg(unix)]
            anchor,
            #[cfg(unix)]
            group,
            stopped: false,
            logs: Vec::new(),
        })
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        if !self.stopped {
            #[cfg(unix)]
            terminate_command_group(&mut self.anchor, self.group).await?;
            self.stopped = true;
        }
        Ok(())
    }

    #[cfg(unix)]
    async fn stop_active(&mut self, child: &mut tokio::process::Child) -> anyhow::Result<()> {
        signal_command_group(self.group)?;
        // Reap Tokio's direct child before collecting adopted descendants.
        child.wait().await.map_err(|_| UnconfirmedExecution)?;
        self.stop().await
    }

    async fn flush_logs(&mut self) -> anyhow::Result<()> {
        let mut first_error = None;
        for task in std::mem::take(&mut self.logs) {
            if let Err(error) = await_log_task(Some(task)).await {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn finish(&mut self) -> anyhow::Result<()> {
        self.stop().await?;
        self.flush_logs().await
    }
}

impl Drop for AttemptProcesses {
    fn drop(&mut self) {
        // A failed execution cannot publish completion; still request scoped stop.
        #[cfg(unix)]
        if !self.stopped && self.anchor.id().is_some() {
            if let Ok(group) = i32::try_from(self.group) {
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
        }
    }
}

#[cfg(test)]
async fn run_shell_command(
    command: &str,
    cwd: &Path,
    timeout_seconds: i32,
    log_context: Option<&LogContext>,
    secret_env: &[(String, String)],
) -> anyhow::Result<CommandOutcome> {
    let mut processes = AttemptProcesses::new()?;
    let result = run_shell_command_in_scope(
        command,
        cwd,
        timeout_seconds,
        log_context,
        secret_env,
        &mut processes,
    )
    .await;
    processes.finish().await?;
    result
}

async fn run_shell_command_in_scope(
    command: &str,
    cwd: &Path,
    timeout_seconds: i32,
    log_context: Option<&LogContext>,
    secret_env: &[(String, String)],
    processes: &mut AttemptProcesses,
) -> anyhow::Result<CommandOutcome> {
    eprintln!("running: {command}");
    if let Some(context) = log_context {
        append_log_lines(
            context,
            vec![RunnerLogLine {
                stream: "system",
                message: protocol_log_message(&format!("running: {command}")),
            }],
        )
        .await?;
    }

    let mut command_process = shell(command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command_process
            .as_std_mut()
            .process_group(i32::try_from(processes.group).map_err(|_| UnconfirmedExecution)?);
    }
    command_process.current_dir(cwd).stdin(Stdio::null());
    for (name, value) in secret_env {
        command_process.env(name, value);
    }
    if log_context.is_some() {
        command_process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        command_process
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
    command_process.kill_on_drop(true);
    let mut child = command_process
        .spawn()
        .with_context(|| format!("spawn command in {}", cwd.display()))?;

    let stdout_task = match (log_context, child.stdout.take()) {
        (Some(context), Some(stdout)) => Some(tokio::spawn(stream_output_to_logs(
            context.clone(),
            "stdout",
            stdout,
        ))),
        _ => None,
    };
    let stderr_task = match (log_context, child.stderr.take()) {
        (Some(context), Some(stderr)) => Some(tokio::spawn(stream_output_to_logs(
            context.clone(),
            "stderr",
            stderr,
        ))),
        _ => None,
    };

    processes.logs.extend(stdout_task);
    processes.logs.extend(stderr_task);
    let timeout = tokio::time::sleep(Duration::from_secs(timeout_seconds.max(1) as u64));
    tokio::pin!(timeout);
    let control_delay = tokio::time::sleep(Duration::from_secs(CONTROL_POLL_INTERVAL_SECONDS));
    tokio::pin!(control_delay);

    let status = loop {
        tokio::select! {
            status = child.wait() => break status.map_err(|_| anyhow::Error::new(UnconfirmedExecution)),
            _ = &mut timeout => {
                #[cfg(unix)]
                processes.stop_active(&mut child).await?;
                #[cfg(not(unix))]
                {
                    child.kill().await.map_err(|_| UnconfirmedExecution)?;
                    child.wait().await.map_err(|_| UnconfirmedExecution)?;
                }
                bail!("command timed out after {timeout_seconds}s");
            }
            _ = &mut control_delay, if log_context.is_some() => {
                if let Some(context) = log_context {
                    match poll_lease_cancel_requested(context).await {
                        Ok(true) => {
                            eprintln!("runner cancellation requested, terminating command");
                            let _ = append_log_lines(
                                context,
                                vec![RunnerLogLine {
                                    stream: "system",
                                    message: protocol_log_message(
                                        "runner cancellation requested, terminating command",
                                    ),
                                }],
                            )
                            .await;
                            #[cfg(unix)]
                processes.stop_active(&mut child).await?;
                #[cfg(not(unix))]
                {
                    child.kill().await.map_err(|_| UnconfirmedExecution)?;
                    child.wait().await.map_err(|_| UnconfirmedExecution)?;
                }
                            return Ok(CommandOutcome::Canceled);
                        }
                        Ok(false) => {}
                        Err(error) => eprintln!("lease control poll failed: {error:#}"),
                    }
                }
                control_delay.as_mut().reset(
                    tokio::time::Instant::now()
                        + Duration::from_secs(CONTROL_POLL_INTERVAL_SECONDS),
                );
            }
        }
    }?;

    #[cfg(not(unix))]
    processes.flush_logs().await?;
    Ok(CommandOutcome::Exited(status))
}

#[cfg(unix)]
fn signal_command_group(process_id: u32) -> anyhow::Result<()> {
    let group = i32::try_from(process_id).map_err(|_| UnconfirmedExecution)?;
    if unsafe { libc::kill(-group, libc::SIGKILL) } != 0
        && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    {
        return Err(
            anyhow::Error::new(UnconfirmedExecution).context("cannot stop attempt process group")
        );
    }
    Ok(())
}

#[cfg(unix)]
async fn terminate_command_group(
    child: &mut tokio::process::Child,
    process_id: u32,
) -> anyhow::Result<()> {
    // The anchor pins ownership until all member processes have been signaled.
    signal_command_group(process_id)?;
    child.wait().await.map_err(|_| UnconfirmedExecution)?;
    #[cfg(unix)]
    {
        let group = i32::try_from(process_id).map_err(|_| UnconfirmedExecution)?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            // Linux subreaper/PID 1 adopts command descendants. Do not reap other groups.
            while unsafe { libc::waitpid(-group, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
            if unsafe { libc::kill(-group, 0) } != 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                    break;
                }
                return Err(anyhow::Error::new(UnconfirmedExecution));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::Error::new(UnconfirmedExecution)
                    .context("command descendants remain unconfirmed"));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

async fn poll_lease_cancel_requested(context: &LogContext) -> anyhow::Result<bool> {
    let response = context
        .client
        .get(format!(
            "{}/api/v1/runner/leases/{}/control",
            context.base, context.lease_id
        ))
        .bearer_auth(&context.credential)
        .header("X-Runner-Protocol-Version", PROTOCOL_VERSION.to_string())
        .header("X-Lease-Token", &context.lease_token)
        .header("X-Fencing-Token", context.fencing_token.to_string())
        .send()
        .await
        .context("poll lease control request failed")?;
    let body: LeaseControlResponse = response_json(response)
        .await
        .context("poll lease control failed")?;
    Ok(body.cancel_requested)
}

async fn upload_declared_artifacts(
    context: &LogContext,
    offer: &LeaseOffer,
    workspace: &Path,
) -> anyhow::Result<()> {
    upload_terminal_artifacts(context, offer, workspace, false).await
}

// Only a natively confirmed failed Kubernetes execution may omit an output it
// never produced. Successful jobs and the existing shell path remain strict.
async fn upload_terminal_artifacts(
    context: &LogContext,
    offer: &LeaseOffer,
    workspace: &Path,
    failed_kubernetes_exit: bool,
) -> anyhow::Result<()> {
    if offer.attempt.artifacts.is_empty() {
        return Ok(());
    }
    let names = cicd::domain::artifact_names::declared_artifact_names(&offer.attempt.artifacts)
        .map_err(anyhow::Error::msg)?;
    let workspace = tokio::fs::canonicalize(workspace)
        .await
        .with_context(|| format!("canonicalize workspace {}", workspace.display()))?;
    for (artifact_path, artifact_name) in offer.attempt.artifacts.iter().zip(names) {
        let Some(file) =
            terminal_artifact_path(&workspace, artifact_path, failed_kubernetes_exit).await?
        else {
            let message =
                format!("declared artifact {artifact_path} absent after failed execution");
            eprintln!("{message}");
            let _ = append_log_lines(
                context,
                vec![RunnerLogLine {
                    stream: "system",
                    message: protocol_log_message(&message),
                }],
            )
            .await;
            continue;
        };
        let metadata = tokio::fs::metadata(&file)
            .await
            .with_context(|| format!("stat artifact {}", file.display()))?;
        if metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
            bail!("artifact {artifact_path} must be between 1 byte and 50 MiB");
        }
        let bytes = tokio::fs::read(&file)
            .await
            .with_context(|| format!("read artifact {}", file.display()))?;
        let response = context
            .client
            .post(format!(
                "{}/api/v1/runner/leases/{}/artifacts",
                context.base, context.lease_id
            ))
            .bearer_auth(&context.credential)
            .header("X-Runner-Protocol-Version", PROTOCOL_VERSION.to_string())
            .header("X-Lease-Token", &context.lease_token)
            .header("X-Fencing-Token", context.fencing_token.to_string())
            .header("X-Attempt-Id", context.attempt_id.to_string())
            .header("X-Artifact-Path", artifact_path)
            .header("X-Artifact-Name", &artifact_name)
            .header("Content-Type", "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .context("upload runner artifact request failed")?;
        ensure_success(response)
            .await
            .with_context(|| format!("upload artifact {artifact_path} failed"))?;
        eprintln!("uploaded artifact {artifact_path}");
        let _ = append_log_lines(
            context,
            vec![RunnerLogLine {
                stream: "system",
                message: protocol_log_message(&format!("uploaded artifact {artifact_path}")),
            }],
        )
        .await;
    }
    Ok(())
}

async fn terminal_artifact_path(
    workspace: &Path,
    artifact_path: &str,
    failed_kubernetes_exit: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let error = match resolve_workspace_artifact(workspace, artifact_path).await {
        Ok(path) => return Ok(Some(path)),
        Err(error) => error,
    };
    if !failed_kubernetes_exit
        || error
            .downcast_ref::<std::io::Error>()
            .is_none_or(|e| e.kind() != std::io::ErrorKind::NotFound)
    {
        return Err(error);
    }
    // canonicalize(NotFound) can also mean an escaping/dangling symlink.
    // Check every existing prefix before classifying a genuinely absent tail.
    let mut current = workspace.to_path_buf();
    for part in Path::new(artifact_path).components() {
        match part {
            Component::Normal(name) => current.push(name),
            Component::CurDir => continue,
            _ => bail!("artifact path {artifact_path} must stay within workspace"),
        }
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                current = tokio::fs::canonicalize(&current).await?;
                ensure!(
                    current.starts_with(workspace),
                    "artifact path {artifact_path} must stay within workspace"
                );
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    }
    // A file appeared during the failed read; do not infer absent state.
    Err(error)
}

async fn resolve_workspace_artifact(
    workspace: &Path,
    artifact_path: &str,
) -> anyhow::Result<PathBuf> {
    let requested = Path::new(artifact_path);
    if requested.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        bail!("artifact path {artifact_path} must stay within workspace");
    }
    let file = tokio::fs::canonicalize(workspace.join(requested))
        .await
        .with_context(|| format!("declared artifact {artifact_path} does not exist"))?;
    if !file.starts_with(workspace) {
        bail!("artifact path {artifact_path} must stay within workspace");
    }
    let metadata = tokio::fs::metadata(&file)
        .await
        .with_context(|| format!("stat artifact {}", file.display()))?;
    if !metadata.is_file() {
        bail!("declared artifact {artifact_path} must be a file");
    }
    Ok(file)
}

async fn await_log_task(
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
) -> anyhow::Result<()> {
    if let Some(task) = task {
        task.await.context("log stream task panicked")??;
    }
    Ok(())
}

async fn stream_output_to_logs<R>(
    context: LogContext,
    stream: &'static str,
    output: R,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(output);
    let mut line = String::new();
    let mut upload_error = None;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .context("read process output")?;
        if bytes == 0 {
            break;
        }
        let raw_message = line.trim_end_matches(&['\r', '\n'][..]);
        let masked_message = mask_secrets(raw_message, &context.masks);
        let message = protocol_log_message(&masked_message);
        if stream == "stderr" {
            eprintln!("{masked_message}");
        } else {
            println!("{masked_message}");
        }
        if upload_error.is_none() {
            if let Err(error) =
                append_log_lines(&context, vec![RunnerLogLine { stream, message }]).await
            {
                upload_error = Some(error);
            }
        }
    }
    if let Some(error) = upload_error {
        return Err(error);
    }
    Ok(())
}

// Match the existing log protocol without increasing its server-side limits.
const LOG_BATCH_MAX_LINES: usize = 100;
const LOG_BATCH_MAX_BYTES: usize = 256 * 1024;
const LOG_APPEND_MAX_BYTES: usize = 1024 * 1024;
const LOG_APPEND_MAX_ATTEMPTS: usize = 3;
const LOG_APPEND_RETRY_BUDGET: Duration = Duration::from_secs(75);

#[derive(Default)]
struct RunnerLogBatch {
    lines: Vec<RunnerLogLine>,
    encoded_bytes: usize,
}

impl RunnerLogBatch {
    async fn push(&mut self, context: &LogContext, line: String) -> anyhow::Result<()> {
        let line = RunnerLogLine {
            stream: "stdout",
            message: protocol_log_message(&mask_secrets(&line, &context.masks)),
        };
        let bytes = serde_json::to_vec(&line)?.len() + 1;
        if !self.lines.is_empty()
            && (self.lines.len() == LOG_BATCH_MAX_LINES
                || self.encoded_bytes + bytes > LOG_BATCH_MAX_BYTES)
        {
            self.flush(context).await?;
        }
        ensure!(
            bytes <= LOG_BATCH_MAX_BYTES,
            "runner log line exceeds batch bound"
        );
        self.encoded_bytes += bytes;
        self.lines.push(line);
        if self.lines.len() == LOG_BATCH_MAX_LINES {
            self.flush(context).await?;
        }
        Ok(())
    }

    async fn flush(&mut self, context: &LogContext) -> anyhow::Result<()> {
        if !self.lines.is_empty() {
            append_log_batch(context, &self.lines, true).await?;
            self.lines.clear();
            self.encoded_bytes = 0;
        }
        Ok(())
    }
}

/// Terminal Pod output is a snapshot, not a live one-line HTTP stream.
/// Await each bounded batch before reading more; completion waits for the final flush.
async fn forward_kubernetes_logs(
    execution: &cicd::execution::kubernetes::Execution,
    context: &LogContext,
) -> anyhow::Result<()> {
    let sink = tokio::sync::Mutex::new(RunnerLogBatch::default());
    let batch = &sink;
    execution
        .stream_logs(move |line| async move { batch.lock().await.push(context, line).await })
        .await?;
    sink.into_inner().flush(context).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogAppendAck {
    protocol_version: i32,
    accepted: usize,
    next_after: Option<i32>,
}

fn log_retry_delay(
    value: Option<&reqwest::header::HeaderValue>,
    now: chrono::DateTime<Utc>,
) -> Duration {
    if let Some(value) = value.and_then(|value| value.to_str().ok()) {
        if let Ok(seconds) = value.parse::<u64>() {
            return Duration::from_secs(seconds);
        }
        if let Ok(date) = chrono::DateTime::parse_from_rfc2822(value) {
            return (date.with_timezone(&Utc) - now)
                .to_std()
                .unwrap_or_default();
        }
    }
    // The current shared runner-protocol bucket has a 60-second window.
    Duration::from_secs(60)
}

async fn append_log_lines(
    context: &LogContext,
    lines: impl AsRef<[RunnerLogLine]>,
) -> anyhow::Result<()> {
    // Existing live/shell and pre-cancel emitters remain fail-fast. In
    // particular, a log rate limit must not postpone killing a canceled child.
    append_log_batch(context, lines.as_ref(), false).await
}

async fn append_log_batch(
    context: &LogContext,
    lines: &[RunnerLogLine],
    retry_rate_limit: bool,
) -> anyhow::Result<()> {
    ensure!(
        !lines.is_empty() && lines.len() <= LOG_BATCH_MAX_LINES,
        "invalid runner log batch size"
    );
    let body = serde_json::to_vec(&json!({
        "protocolVersion": PROTOCOL_VERSION,
        "leaseToken": &context.lease_token,
        "fencingToken": context.fencing_token,
        "attemptId": context.attempt_id,
        "lines": lines,
    }))?;
    ensure!(
        body.len() <= LOG_APPEND_MAX_BYTES,
        "runner log request exceeds body limit"
    );
    let deadline = tokio::time::Instant::now() + LOG_APPEND_RETRY_BUDGET;
    for attempt in 1..=LOG_APPEND_MAX_ATTEMPTS {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        ensure!(
            !remaining.is_zero(),
            "runner log rate-limit retry budget exhausted"
        );
        let response = context
            .client
            .post(format!(
                "{}/api/v1/runner/leases/{}/logs",
                context.base, context.lease_id
            ))
            .bearer_auth(&context.credential)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone())
            .timeout(remaining.min(Duration::from_secs(45)))
            .send()
            .await
            .context("append runner logs request failed; append outcome is unknown")?;
        if retry_rate_limit && response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // Middleware rejects 429 before any append. Other errors (including
            // transport loss or 5xx after a partial append) are never replayed.
            let delay = log_retry_delay(
                response.headers().get(reqwest::header::RETRY_AFTER),
                Utc::now(),
            );
            drop(response);
            ensure!(
                attempt < LOG_APPEND_MAX_ATTEMPTS
                    && delay < deadline.saturating_duration_since(tokio::time::Instant::now()),
                "runner log rate-limit retry budget exhausted"
            );
            tokio::time::sleep(delay).await;
            continue;
        }
        let ack: LogAppendAck = response_json(response)
            .await
            .context("append runner logs failed; append outcome is unknown")?;
        ensure!(
            ack.protocol_version == PROTOCOL_VERSION
                && ack.accepted == lines.len()
                && ack.next_after.is_some_and(|sequence| sequence >= 0),
            "runner log acknowledgement does not match the full batch"
        );
        return Ok(());
    }
    unreachable!("bounded runner log append attempts")
}

fn truncate_log_message(value: &str) -> String {
    value.chars().take(8192).collect()
}

fn protocol_log_message(value: &str) -> String {
    truncate_log_message(&value.replace('\r', "\\r").replace('\n', "\\n"))
}

fn mask_secrets(value: &str, masks: &[String]) -> String {
    let mut masked = value.to_string();
    for secret in masks {
        if !secret.is_empty() {
            masked = masked.replace(secret, "***");
        }
    }
    masked
}

fn shell(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut process = Command::new("cmd");
        process.args(["/C", command]);
        process
    }
    #[cfg(not(windows))]
    {
        let mut process = Command::new("sh");
        process.args(["-lc", command]);
        process
    }
}

async fn response_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> anyhow::Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        bail!("API returned {status}: {body}");
    }
    serde_json::from_str(&body).with_context(|| format!("decode response body: {body}"))
}

async fn ensure_success(response: reqwest::Response) -> anyhow::Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    bail!("API returned {status}: {body}");
}

fn kubernetes_mode() -> bool {
    std::env::var("CICD_RUNNER_MODE").is_ok_and(|v| v == "kubernetes")
}
fn shell_executor() -> String {
    "shell".into()
}
fn workspace_root_for_checkout(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(path
        .parent()
        .and_then(Path::parent)
        .context("workspace root missing")?
        .to_path_buf())
}

async fn runner_capabilities() -> serde_json::Value {
    let config = if kubernetes_mode() {
        cicd::execution::kubernetes::Config::from_env().ok()
    } else {
        None
    };
    let publication = config.as_ref().is_some_and(|c| c.host_build.is_some())
        && Path::new("/run/forge-build-host/connection.json").is_file()
        && cicd::execution::publication::available().await;
    let target_root = std::env::var("CICD_HELM_TARGET_ROOT")
        .unwrap_or_else(|_| "/run/forge-deploy-targets".into());
    let targets: Vec<String> = config
        .as_ref()
        .map(|c| {
            c.helm_targets
                .iter()
                .filter(|grant| {
                    Path::new(&target_root)
                        .join(grant.environment_id.to_string())
                        .join("target.json")
                        .is_file()
                })
                .map(|grant| grant.environment_id.to_string())
                .collect()
        })
        .unwrap_or_default();
    let mut kinds = Vec::new();
    if publication {
        kinds.push("image-publish-v1");
    }
    if !targets.is_empty() {
        kinds.push("helm-deploy-v1");
    }
    json!({
        "jobKinds":kinds,"helmTargets":targets,
        "executorKinds":[if kubernetes_mode(){"kubernetes"}else{"shell"}],
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH
    })
}

fn truncate_diagnostic(value: &str) -> String {
    value.chars().take(4096).collect()
}

#[cfg(test)]
#[path = "tests/forge_runner.rs"]
mod tests;
