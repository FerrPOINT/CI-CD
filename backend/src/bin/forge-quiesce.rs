//! Private operator control. The live foreground process owns the backup fence and release authority.
use anyhow::{Context, ensure};
use cicd::quiescence::{Coordinator, Gate, Identity};
use clap::{Parser, Subcommand};
use std::{
    io::{Read, Write},
    path::PathBuf,
};
use uuid::Uuid;
#[derive(Parser)]
struct Args {
    #[arg(long, env = "CICD_QUIESCENCE_DIR")]
    directory: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Read-only, challenged management observation. Never opens the quiescence gate.
    InspectKubernetesCustody {
        #[arg(long)]
        request_sha256: String,
    },
    /// Read-only predecessor hashes; never a native retirement or held acknowledgement.
    AdoptionStatus,
    /// Explicit owner handoff; native source/destination proof is verified by Base.
    AdoptPrepare {
        #[arg(long)]
        operation: Uuid,
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long)]
        handoff_sha256: String,
    },
    AdoptCommit {
        #[arg(long)]
        operation: Uuid,
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long)]
        handoff_sha256: String,
    },
    /// Persistent request; no idle or backup acknowledgement.
    Request {
        #[arg(long)]
        operation: Uuid,
    },
    /// Hold the actual fence. Requires the owner's complete, runtime-pinned identity manifest.
    Hold {
        #[arg(long)]
        operation: Uuid,
        #[arg(long)]
        expected: PathBuf,
        #[arg(long)]
        require_recoverable_idle: bool,
    },
    /// Explicitly reopen admission after a completed/abandoned backup, exact operation only.
    Resume {
        #[arg(long)]
        operation: Uuid,
    },
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if let Command::InspectKubernetesCustody { request_sha256 } = &args.command {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let result = cicd::execution::kubernetes::inspection::command(&bytes, request_sha256).await;
        println!("{result}");
        std::io::stdout().flush()?;
        if result["status"] != "VERIFIED" {
            std::process::exit(2);
        }
        return Ok(());
    }
    let gate = Gate::open(&args.directory)?;
    match args.command {
        Command::InspectKubernetesCustody { .. } => {
            unreachable!("read-only branch handled before Gate::open")
        }
        Command::AdoptionStatus => println!("{}", cicd::quiescence::adoption_status(&gate)?),
        Command::AdoptPrepare {
            operation,
            handoff,
            handoff_sha256,
        } => adopt(gate, operation, handoff, handoff_sha256, false).await?,
        Command::AdoptCommit {
            operation,
            handoff,
            handoff_sha256,
        } => adopt(gate, operation, handoff, handoff_sha256, true).await?,
        Command::Request { operation } => {
            let _owner = Coordinator::request(gate, operation)?;
            println!(
                "{}",
                serde_json::json!({"schema":"forge/quiescence-request/v1","operationId":operation,"state":"draining","backupAcknowledged":false})
            );
        }
        Command::Resume { operation } => Coordinator::request(gate, operation)?.resume()?,
        Command::Hold {
            operation,
            expected,
            require_recoverable_idle,
        } => {
            ensure!(
                std::fs::metadata(&expected)?.len() <= 32768,
                "expected participant manifest exceeds bound"
            );
            let expected: Vec<Identity> = serde_json::from_slice(&std::fs::read(expected)?)?;
            let owner = Coordinator::request(gate, operation)?;
            let config = cicd::config::RuntimeConfig::from_env()?;
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(3))
                .connect(&config.database.url)
                .await?;
            let fence = loop {
                owner.gate().verify_participants(&expected)?;
                if let Some(fence) = owner.try_fence()? {
                    owner.gate().verify_no_effect_records(&fence)?;
                    let check = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                        cicd::quiescence::verify_idle_at(
                            &fence,
                            &pool,
                            &expected,
                            owner.gate(),
                            require_recoverable_idle,
                        )
                        .await
                    })
                    .await;
                    if matches!(check, Ok(Ok(()))) {
                        owner.gate().verify_participants(&expected)?;
                        break fence;
                    }
                    // Release promptly: existing execution ACK/reconciliation/cleanup must continue.
                    drop(fence);
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            };
            pool.close().await;
            owner.verify_adoption_seals()?;
            owner.publish_held(&fence, &expected)?;
            println!(
                "{}",
                serde_json::json!({"schema":"forge/quiescence-held/v1","operationId":operation,"coordinatorPid":std::process::id(),"participants":expected,"backupAcknowledged":true,"recoverableIdleRequired":require_recoverable_idle,"validity":"only while this exact foreground coordinator retains its kernel fence"})
            );
            std::io::stdout().flush()?;
            let mut rx = read_command();
            let mut exiting = false;
            let mut exited = false;
            loop {
                tokio::select! {
                    line=&mut rx=>{
                        let line=line.context("control stdin worker unavailable")??;
                        if line.trim()==format!("exit {operation}") && !exiting {
                            owner.request_exit(&fence,&expected)?;exiting=true;rx=read_command();
                        } else {
                            ensure!(line.trim()==format!("resume {operation}") && (!exiting || exited),"fence ended without authorized completed resume; admission remains closed");
                            owner.verify_adoption_seals()?;owner.resume_enrolled(&fence,&expected)?;drop(fence);break;
                        }
                    }
                    _=tokio::time::sleep(std::time::Duration::from_millis(100))=>{
                        if exiting {
                            let now_exited=owner.gate().all_exited(&expected)?;
                            if now_exited && !exited {
                                exited=true;
                                println!("{}",serde_json::json!({"schema":"forge/quiescence-exited/v1","operationId":operation,"participants":expected,"allPinnedKernelOwnersExited":true,"exclusiveFenceStillHeld":true}));
                                std::io::stdout().flush()?;
                            }
                        } else {owner.gate().verify_participants(&expected)?;}
                    }
                }
            }
        }
    }
    Ok(())
}

fn read_command() -> tokio::sync::oneshot::Receiver<std::io::Result<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        let result = std::io::stdin().read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    rx
}

async fn adopt(
    gate: Gate,
    operation: Uuid,
    path: PathBuf,
    sha: String,
    commit: bool,
) -> anyhow::Result<()> {
    let (handoff, bytes, old) = cicd::quiescence::load_adoption_handoff(&path, &sha)?;
    ensure!(
        handoff.operation_id == operation,
        "handoff operation differs"
    );
    let owner = Coordinator::for_adoption(gate, operation)?;
    let fence = owner
        .try_fence()?
        .context("old effects still own the kernel fence; HOLD")?;
    ensure!(
        owner.gate().all_exited(&old)?,
        "old local kernel owners remain; HOLD"
    );
    owner.gate().verify_no_effect_records(&fence)?;
    let config = cicd::config::RuntimeConfig::from_env()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(&config.database.url)
        .await?;
    let seals =
        cicd::quiescence::adoption_preflight(&fence, &pool, &handoff, &old, owner.gate()).await?;
    pool.close().await;
    if commit {
        owner.verify_adoption_seals()?;
        owner.commit_adoption(&fence, &handoff, &bytes, &old)?;
    } else {
        owner.prepare_adoption(&fence, &handoff, &bytes, &old, seals)?;
    }
    println!(
        "{}",
        serde_json::json!({"schema":if commit{"forge/quiescence-adoption-committed/v1"}else{"forge/quiescence-adoption-prepared/v1"},
        "operationId":operation,"generationId":handoff.generation_id,"handoffSha256":sha,"admissionClosed":true,"nativeRetirementVerifiedBy":"external-owner"})
    );
    Ok(())
}
