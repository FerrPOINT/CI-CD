//! Private operator control. The live foreground process owns the backup fence and release authority.
use anyhow::{Context, ensure};
use cicd::quiescence::{Coordinator, Gate, Identity};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, io::Write};
use uuid::Uuid;
#[derive(Parser)]
struct Args {
    #[arg(long,env="CICD_QUIESCENCE_DIR")] directory:PathBuf,
    #[command(subcommand)] command:Command,
}
#[derive(Subcommand)]
enum Command {
    /// Persistent request; no idle or backup acknowledgement.
    Request { #[arg(long)] operation:Uuid },
    /// Hold the actual fence. Requires the owner's complete, runtime-pinned identity manifest.
    Hold { #[arg(long)] operation:Uuid, #[arg(long)] expected:PathBuf },
    /// Explicitly reopen admission after a completed/abandoned backup, exact operation only.
    Resume { #[arg(long)] operation:Uuid },
}
#[tokio::main]
async fn main()->anyhow::Result<()> {
    let args=Args::parse();let gate=Gate::open(&args.directory)?;
    match args.command {
        Command::Request{operation}=>{let _owner=Coordinator::request(gate,operation)?;println!("{}",serde_json::json!({"schema":"forge/quiescence-request/v1","operationId":operation,"state":"draining","backupAcknowledged":false}));}
        Command::Resume{operation}=>Coordinator::request(gate,operation)?.resume()?,
        Command::Hold{operation,expected}=>{
            ensure!(std::fs::metadata(&expected)?.len()<=32768,"expected participant manifest exceeds bound");
            let expected:Vec<Identity>=serde_json::from_slice(&std::fs::read(expected)?)?;
            let owner=Coordinator::request(gate,operation)?;
            let config=cicd::config::RuntimeConfig::from_env()?;
            let pool=sqlx::postgres::PgPoolOptions::new().max_connections(1).acquire_timeout(std::time::Duration::from_secs(3)).connect(&config.database.url).await?;
            let fence=loop {
                owner.gate().verify_participants(&expected)?;
                if let Some(fence)=owner.try_fence()? {
                    owner.gate().verify_no_effect_records(&fence)?;
                    let check=tokio::time::timeout(std::time::Duration::from_secs(3),cicd::quiescence::verify_idle(&fence,&pool,&expected)).await;
                    if matches!(check,Ok(Ok(()))){owner.gate().verify_participants(&expected)?;break fence}
                    // Release promptly: existing execution ACK/reconciliation/cleanup must continue.
                    drop(fence);
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            };
            owner.publish_held(&fence,&expected)?;
            println!("{}",serde_json::json!({"schema":"forge/quiescence-held/v1","operationId":operation,"coordinatorPid":std::process::id(),"participants":expected,"backupAcknowledged":true,"validity":"only while this exact foreground coordinator retains its kernel fence"}));
            std::io::stdout().flush()?;
            let mut rx=read_command();let mut exiting=false;let mut exited=false;
            loop {
                tokio::select! {
                    line=&mut rx=>{
                        let line=line.context("control stdin worker unavailable")??;
                        if line.trim()==format!("exit {operation}") && !exiting {
                            owner.request_exit(&fence,&expected)?;exiting=true;rx=read_command();
                        } else {
                            ensure!(line.trim()==format!("resume {operation}") && (!exiting || exited),"fence ended without authorized completed resume; admission remains closed");
                            drop(fence);owner.resume()?;break;
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
    }Ok(())
}

fn read_command()->tokio::sync::oneshot::Receiver<std::io::Result<String>>{
    let(tx,rx)=tokio::sync::oneshot::channel();tokio::task::spawn_blocking(move||{let mut line=String::new();let result=std::io::stdin().read_line(&mut line).map(|_|line);let _=tx.send(result);});rx
}
