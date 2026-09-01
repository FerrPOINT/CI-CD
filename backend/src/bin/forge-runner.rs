use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, bail};
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

#[derive(Debug)]
enum CommandOutcome {
    Exited(std::process::ExitStatus),
    Canceled,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    validate_cli(&cli)?;
    let base = normalize_api_base(&cli.api_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("build runner HTTP client")?;
    let credential = ensure_credential(&client, &base, &cli).await?;

    loop {
        heartbeat(&client, &base, &credential, &cli, 0, &[]).await?;
        match poll_work(&client, &base, &credential, &cli).await? {
            Some(offer) => {
                run_offer(&client, &base, &credential, &cli, offer).await?;
                if cli.once {
                    return Ok(());
                }
            }
            None if cli.once => return Ok(()),
            None => tokio::time::sleep(Duration::from_secs(cli.poll_interval_seconds)).await,
        }
    }
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
            "capabilities": runner_capabilities(),
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
    let response = client
        .post(format!("{base}/api/v1/runner/heartbeat"))
        .bearer_auth(credential)
        .json(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "status": "online",
            "draining": false,
            "capacity": {
                "totalSlots": cli.total_slots,
                "busySlots": busy_slots,
            },
            "tags": &cli.tags,
            "capabilities": runner_capabilities(),
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

async fn run_offer(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    cli: &Cli,
    offer: LeaseOffer,
) -> anyhow::Result<()> {
    eprintln!(
        "accepted offer lease={} attempt={} job={}",
        offer.lease_id, offer.attempt.id, offer.attempt.job_key
    );
    ack_lease(client, base, credential, &offer).await?;
    let secret_env = match resolve_lease_secrets(client, base, credential, &offer).await {
        Ok(secret_env) => secret_env,
        Err(error) => {
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
    heartbeat(client, base, credential, cli, 1, &[offer.lease_id]).await?;

    let (stop_tx, stop_rx) = watch::channel(false);
    let renew_task = tokio::spawn(renew_loop(
        client.clone(),
        base.to_owned(),
        credential.to_owned(),
        offer.lease_id,
        offer.lease_token.clone(),
        offer.fencing_token,
        stop_rx,
    ));
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
    let _ = stop_tx.send(true);
    let _ = renew_task.await;

    let completion_result = match result {
        Ok(result) => complete_lease(client, base, credential, &offer, result).await,
        Err(error) => {
            complete_lease(
                client,
                base,
                credential,
                &offer,
                ExecutionResult {
                    outcome: "failed",
                    exit_code: None,
                    diagnostic: Some(truncate_diagnostic(&format!("{error:#}"))),
                },
            )
            .await
        }
    };
    heartbeat(client, base, credential, cli, 0, &[]).await?;
    completion_result
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
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
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
        }))
        .send()
        .await
        .context("complete request failed")?;
    ensure_success(response).await.context("complete failed")
}

async fn execute_attempt(
    cli: &Cli,
    offer: &LeaseOffer,
    log_context: Option<LogContext>,
    secret_env: &[(String, String)],
) -> anyhow::Result<ExecutionResult> {
    let workspace = prepare_workspace(cli, offer).await?;
    let cleanup_path = workspace.clone();
    let mut last_exit_code = None;
    let mut result = ExecutionResult {
        outcome: "success",
        exit_code: None,
        diagnostic: None,
    };

    for command in &offer.attempt.commands {
        let command_outcome = match run_shell_command(
            command,
            &workspace,
            offer.attempt.timeout_seconds,
            log_context.as_ref(),
            secret_env,
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
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

    let artifact_error = if result.outcome == "canceled" {
        None
    } else {
        match log_context.as_ref() {
            Some(context) => upload_declared_artifacts(context, offer, &workspace)
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

    if !cli.keep_workspace {
        cleanup_workspace(&cleanup_path, cli.work_dir.as_deref())?;
    }
    if result.outcome == "success" {
        result.exit_code = last_exit_code.or(Some(0));
    }
    Ok(result)
}

async fn prepare_workspace(cli: &Cli, offer: &LeaseOffer) -> anyhow::Result<PathBuf> {
    let root = cli
        .work_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("forge-runner"));
    std::fs::create_dir_all(&root)
        .with_context(|| format!("create runner work root {}", root.display()))?;
    let workspace = root.join(format!(
        "attempt-{}-{}",
        offer.attempt.id,
        Uuid::new_v4().simple()
    ));

    if offer.attempt.workspace.checkout && !cli.no_checkout {
        let url = offer
            .attempt
            .workspace
            .checkout_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("lease requires checkout but checkoutUrl is missing")?;
        let workspace_arg = workspace.to_string_lossy().into_owned();
        if let Err(error) = run_git(["clone", "--quiet", url, workspace_arg.as_str()], &root).await
        {
            cleanup_workspace(&workspace, Some(&root))?;
            return Err(error).with_context(|| format!("clone {url}"));
        }
        let checkout_target = offer
            .attempt
            .commit_sha
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&offer.attempt.git_ref);
        if !checkout_target.trim().is_empty() {
            if let Err(error) = run_git(["checkout", "--quiet", checkout_target], &workspace).await
            {
                cleanup_workspace(&workspace, Some(&root))?;
                return Err(error).with_context(|| format!("checkout {checkout_target}"));
            }
        }
    } else {
        std::fs::create_dir_all(&workspace)
            .with_context(|| format!("create attempt workspace {}", workspace.display()))?;
    }
    Ok(workspace)
}

async fn run_git<const N: usize>(args: [&str; N], cwd: &Path) -> anyhow::Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .status()
        .await
        .context("spawn git")?;
    if status.success() {
        Ok(())
    } else {
        bail!("git exited with {status}");
    }
}

async fn run_shell_command(
    command: &str,
    cwd: &Path,
    timeout_seconds: i32,
    log_context: Option<&LogContext>,
    secret_env: &[(String, String)],
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

    let timeout = tokio::time::sleep(Duration::from_secs(timeout_seconds.max(1) as u64));
    tokio::pin!(timeout);
    let control_delay = tokio::time::sleep(Duration::from_secs(CONTROL_POLL_INTERVAL_SECONDS));
    tokio::pin!(control_delay);

    let status = loop {
        tokio::select! {
            status = child.wait() => break status.context("wait for command"),
            _ = &mut timeout => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = await_log_task(stdout_task).await;
                let _ = await_log_task(stderr_task).await;
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
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                            let _ = await_log_task(stdout_task).await;
                            let _ = await_log_task(stderr_task).await;
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

    await_log_task(stdout_task).await?;
    await_log_task(stderr_task).await?;
    Ok(CommandOutcome::Exited(status))
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
    if offer.attempt.artifacts.is_empty() {
        return Ok(());
    }
    let workspace = tokio::fs::canonicalize(workspace)
        .await
        .with_context(|| format!("canonicalize workspace {}", workspace.display()))?;
    for artifact_path in &offer.attempt.artifacts {
        let file = resolve_workspace_artifact(&workspace, artifact_path).await?;
        let metadata = tokio::fs::metadata(&file)
            .await
            .with_context(|| format!("stat artifact {}", file.display()))?;
        if metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
            bail!("artifact {artifact_path} must be between 1 byte and 50 MiB");
        }
        let bytes = tokio::fs::read(&file)
            .await
            .with_context(|| format!("read artifact {}", file.display()))?;
        let artifact_name = artifact_name_from_declared_path(artifact_path);
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

fn artifact_name_from_declared_path(path: &str) -> String {
    path.replace(['/', '\\'], "__")
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

async fn append_log_lines(context: &LogContext, lines: Vec<RunnerLogLine>) -> anyhow::Result<()> {
    let response = context
        .client
        .post(format!(
            "{}/api/v1/runner/leases/{}/logs",
            context.base, context.lease_id
        ))
        .bearer_auth(&context.credential)
        .json(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "leaseToken": &context.lease_token,
            "fencingToken": context.fencing_token,
            "attemptId": context.attempt_id,
            "lines": lines,
        }))
        .send()
        .await
        .context("append runner logs request failed")?;
    ensure_success(response)
        .await
        .context("append runner logs failed")
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

fn cleanup_workspace(path: &Path, configured_root: Option<&Path>) -> anyhow::Result<()> {
    let root = configured_root
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("forge-runner"));
    let root = root.canonicalize().unwrap_or(root);
    let candidate = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if candidate.starts_with(&root) && candidate != root && candidate.exists() {
        std::fs::remove_dir_all(&candidate)
            .with_context(|| format!("cleanup workspace {}", candidate.display()))?;
    }
    Ok(())
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

fn runner_capabilities() -> serde_json::Value {
    json!({
        "executorKinds": ["shell"],
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

fn truncate_diagnostic(value: &str) -> String {
    value.chars().take(4096).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cli(work_dir: PathBuf) -> Cli {
        Cli {
            api_url: "http://127.0.0.1:22801".to_string(),
            name: "test-runner".to_string(),
            credential: Some("credential".to_string()),
            registration_token: None,
            tags: vec!["linux".to_string()],
            total_slots: 1,
            work_dir: Some(work_dir),
            poll_interval_seconds: 1,
            once: true,
            no_checkout: true,
            keep_workspace: false,
        }
    }

    fn test_offer(command: &str, timeout_seconds: i32) -> LeaseOffer {
        LeaseOffer {
            lease_id: Uuid::new_v4(),
            lease_token: "lease-token".to_string(),
            fencing_token: 1,
            attempt: AttemptSpec {
                id: Uuid::new_v4(),
                job_key: "test".to_string(),
                git_ref: "main".to_string(),
                commit_sha: None,
                commands: vec![command.to_string()],
                secrets: Vec::new(),
                artifacts: Vec::new(),
                timeout_seconds,
                workspace: WorkspaceSpec {
                    checkout: false,
                    checkout_url: None,
                },
            },
        }
    }

    #[test]
    fn api_base_accepts_root_or_v1_url() {
        assert_eq!(
            normalize_api_base("http://127.0.0.1:22801"),
            "http://127.0.0.1:22801"
        );
        assert_eq!(
            normalize_api_base("http://127.0.0.1:22801/api/v1/"),
            "http://127.0.0.1:22801"
        );
    }

    #[test]
    fn diagnostic_is_bounded() {
        let input = "x".repeat(5000);
        assert_eq!(truncate_diagnostic(&input).len(), 4096);
    }

    #[test]
    fn protocol_log_message_escapes_internal_line_breaks() {
        assert_eq!(protocol_log_message("one\rtwo\nthree"), "one\\rtwo\\nthree");
    }

    #[test]
    fn masks_secret_values_before_log_upload() {
        assert_eq!(
            mask_secrets(
                "deploy token=super-secret",
                &["super-secret".to_string(), "".to_string()]
            ),
            "deploy token=***"
        );
    }

    #[test]
    fn declared_artifact_name_preserves_relative_context() {
        assert_eq!(
            artifact_name_from_declared_path("target/release/app.tar.gz"),
            "target__release__app.tar.gz"
        );
    }

    #[tokio::test]
    async fn declared_artifact_path_cannot_escape_workspace() {
        let work_dir =
            std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
        let workspace = work_dir.join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::write(workspace.join("report.txt"), b"ok").expect("write artifact");

        let workspace = tokio::fs::canonicalize(&workspace)
            .await
            .expect("canonical workspace");
        assert!(
            resolve_workspace_artifact(&workspace, "report.txt")
                .await
                .is_ok()
        );
        assert!(
            resolve_workspace_artifact(&workspace, "../outside.txt")
                .await
                .is_err()
        );

        let _ = std::fs::remove_dir_all(work_dir);
    }

    #[tokio::test]
    async fn failed_command_reports_process_exit_code() {
        let work_dir =
            std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
        let cli = test_cli(work_dir.clone());
        let offer = test_offer("exit 7", 5);

        let result = execute_attempt(&cli, &offer, None, &[]).await.unwrap();

        assert_eq!(result.outcome, "failed");
        assert_eq!(result.exit_code, Some(7));
        let _ = std::fs::remove_dir_all(work_dir);
    }

    #[tokio::test]
    async fn secret_env_is_available_to_commands() {
        let work_dir =
            std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
        let cli = test_cli(work_dir.clone());
        let command = if cfg!(windows) {
            "if \"%DEPLOY_TOKEN%\"==\"super-secret\" (exit /b 0) else (exit /b 9)"
        } else {
            "test \"$DEPLOY_TOKEN\" = \"super-secret\""
        };
        let mut offer = test_offer(command, 5);
        offer.attempt.secrets = vec!["DEPLOY_TOKEN".to_string()];

        let result = execute_attempt(
            &cli,
            &offer,
            None,
            &[("DEPLOY_TOKEN".to_string(), "super-secret".to_string())],
        )
        .await
        .unwrap();

        assert_eq!(result.outcome, "success");
        let _ = std::fs::remove_dir_all(work_dir);
    }

    #[tokio::test]
    async fn timeout_failure_has_no_success_exit_code() {
        let work_dir =
            std::env::temp_dir().join(format!("forge-runner-test-{}", Uuid::new_v4().simple()));
        let cli = test_cli(work_dir.clone());
        let command = if cfg!(windows) {
            "ping -n 3 127.0.0.1 > nul"
        } else {
            "sleep 2"
        };
        let offer = test_offer(command, 1);

        let result = execute_attempt(&cli, &offer, None, &[]).await.unwrap();

        assert_eq!(result.outcome, "failed");
        assert_eq!(result.exit_code, None);
        assert!(
            result
                .diagnostic
                .as_deref()
                .is_some_and(|value| value.contains("timed out"))
        );
        let _ = std::fs::remove_dir_all(work_dir);
    }
}
