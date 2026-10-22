//! Thin embedded lifecycle integration. Execution/custody is shared with forge-runner.
use super::*;
use crate::execution::kubernetes::{Config,Execution,Scope};
use anyhow::{Context,ensure};
use serde_json::{Value,json};

pub(super) async fn run(pool:&PgPool,job:&JobRow,lease:&EmbeddedJobLease,running:&RunningJobs,config:&RuntimeRunnerConfig)->Result<ExecutionOutcome,ApiError>{
    if crate::execution::kubernetes::retry_unconfirmed(&workspace_root(config),lease.attempt_id).map_err(|_|ApiError::conflict("Kubernetes custody unavailable"))? {return Ok(ExecutionOutcome::Held)}
    let input=image_build_input(pool,job,config).await?;
    let workspace=prepare_workspace(pool,job,lease,config).await?;
    let kube=Config::from_env().map_err(|_|ApiError::conflict("Kubernetes operator config unavailable"))?;
    let secrets=crate::platform::project_secret_pairs_for_names_with_config(pool,job.project_id,&job.required_secrets,&config.secrets).await?;
    let masks:Vec<String>=secrets.iter().filter(|(_,v)|!v.is_empty()).map(|(_,v)|v.clone()).collect();
    let timeout=sqlx::query_scalar::<_,Option<i32>>("SELECT timeout_seconds FROM jobs WHERE id=$1").bind(job.id).fetch_one(pool).await.map_err(ApiError::internal)?.unwrap_or(3600).clamp(5,86400) as u64;
    let scope=Scope{job_id:job.id,attempt_id:lease.attempt_id,lease_id:lease.id,generation:lease.generation,source_commit:job.commit_sha.clone().ok_or_else(||ApiError::conflict("Kubernetes job requires source pin"))?};
    let mut env=vec![("CICD_PIPELINE_ID".into(),job.pipeline_id.to_string()),("CICD_JOB_ID".into(),job.id.to_string()),("CICD_JOB_NAME".into(),job.name.clone()),("CICD_STAGE_NAME".into(),job.stage_name.clone()),("CICD_PROJECT_ID".into(),job.project_id.to_string()),("CICD_PROJECT_NAME".into(),job.project_name.clone()),("CICD_COMMIT_REF".into(),job.git_ref.clone()),("CICD_COMMIT_SHA".into(),scope.source_commit.clone()),("CICD_ARTIFACTS_DIR".into(),"/workspace/artifacts".into()),("CICD_PIPELINE_ARTIFACTS_DIR".into(),"/workspace/pipeline-artifacts".into())];
    env.extend(secrets);
    let vars=sqlx::query_scalar::<_,Option<Value>>("SELECT variables FROM pipelines WHERE id=$1").bind(job.pipeline_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if let Some(Value::Object(vars))=vars {for (key,value) in vars {env.push((format!("CICD_VAR_{}",key.to_uppercase().replace('-',"_")),value.as_str().map(str::to_owned).unwrap_or(value.to_string())));}}
    tokio::fs::create_dir_all(workspace.checkout().join("artifacts")).await.map_err(|_|ApiError::conflict("Cannot create artifact directory"))?;
    tokio::fs::create_dir_all(workspace.checkout().join("pipeline-artifacts")).await.map_err(|_|ApiError::conflict("Cannot create pipeline artifacts directory"))?;
    let registry=format!("{}/attempt-{}:g{}",kube.registry_prefix,lease.attempt_id.simple(),lease.generation);
    let (image,command)=if let Some((spec,source))=&input {
        crate::runner_image_build::export_kubernetes_source(source,spec,workspace.checkout()).await.map_err(|_|ApiError::conflict("Exact build source export unavailable"))?;
        env=vec![("BUILDKITD_FLAGS".into(),"--oci-worker-snapshotter=native".into()),("XDG_RUNTIME_DIR".into(),"/tmp/buildkit-runtime".into())];
        if kube.registry_secret.is_some(){env.push(("DOCKER_CONFIG".into(),"/registry-auth".into()));}
        let context=if spec.context=="."{"/workspace/source".into()}else{format!("/workspace/source/{}",spec.context)};
        let mut args=vec!["buildctl-daemonless.sh".into(),"build".into(),"--frontend".into(),"dockerfile.v0".into(),"--local".into(),format!("context={context}"),"--local".into(),format!("dockerfile={context}"),"--opt".into(),format!("filename={}",spec.dockerfile),"--opt".into(),format!("label:org.opencontainers.image.revision={}",source.source_commit),"--opt".into(),format!("label:io.forge.attempt={}",lease.attempt_id),"--output".into(),format!("type=image,name={registry},push=true"),"--metadata-file".into(),"/workspace/build-output/metadata.json".into()];
        if let Some(target)=&spec.target{args.extend(["--opt".into(),format!("target={target}")]);}
        (kube.builder_image.clone(),args)
    }else{
        (crate::execution::kubernetes::resolve_image(&job.image).await.map_err(|_|ApiError::conflict("OCI image resolution unavailable"))?,vec!["sh".into(),"-lc".into(),shell_capture_command(&job.command)])
    };
    let mut execution=Execution::prepare(kube.clone(),&workspace_root(config),scope,workspace.checkout(),&image,&command,&env,timeout).map_err(|_|ApiError::conflict("Kubernetes effect preparation unavailable"))?;
    if input.is_some(){execution.build_template(&registry).map_err(|_|ApiError::conflict("BuildKit security template unavailable"))?;}
    let until=match crate::api::sdlc_workspace::task_pipeline::before_spawn(pool,job.pipeline_id,lease.attempt_id,&config.sdlc_workspace,&config.secrets).await?{
        crate::api::sdlc_workspace::task_pipeline::Spawn::Held=>return Ok(ExecutionOutcome::Held),
        crate::api::sdlc_workspace::task_pipeline::Spawn::Legacy=>None,
        crate::api::sdlc_workspace::task_pipeline::Spawn::Permit(until)=>Some(until),
    };
    let mut guard=running.lock().await;
    if !image_build_attempt_active(pool,lease).await? || until.is_some_and(|v|std::time::Instant::now()>=v){return Ok(ExecutionOutcome::Held)}
    // Reservation sentinel cannot be mistaken for a local PID by the cancellation route.
    guard.insert(job.id,u32::MAX);drop(guard);
    if execution.start().await.is_err(){return Ok(ExecutionOutcome::Held)}
    let deadline=std::time::Instant::now()+Duration::from_secs(timeout);
    let terminal=loop {
        match execution.observe().await {Ok(Some(value))=>break value,Ok(None)=>{},Err(_)=>return Ok(ExecutionOutcome::Held)}
        if !image_build_attempt_active(pool,lease).await? || std::time::Instant::now()>=deadline || until.is_some_and(|v|std::time::Instant::now()>=v){if execution.cancel().await.is_err(){return Ok(ExecutionOutcome::Held)}}
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    let logs=match execution.logs().await{Ok(logs)=>logs,Err(_)=>return Ok(ExecutionOutcome::Held)};
    for line in logs.lines(){append_attempt_log(pool,job.id,lease.attempt_id,&mask_secrets(line,&masks)).await?;}
    let mut status=if execution.cancelled(){"canceled"}else if terminal.exit_code==0{"success"}else{"failed"};
    let mut artifact_root=workspace.checkout().to_path_buf();
    if let Some((spec,_))=&input {
        if status!="success"{return Ok(ExecutionOutcome::Held)}
        if !image_build_attempt_active(pool,lease).await?{return Ok(ExecutionOutcome::Held)}
        artifact_root=match publish_build(&kube,job,lease,spec,workspace.checkout(),&registry).await{Ok(root)=>root,Err(_)=>return Ok(ExecutionOutcome::Held)};
    }
    crate::api::sdlc_workspace::task_pipeline::before_delegated_effect(pool,job.pipeline_id,lease.attempt_id,config).await?;
    if collect_declared_artifacts(pool,&config.artifacts,job.id,lease.attempt_id,&artifact_root,&job.artifact_paths).await.is_err(){if input.is_some(){return Ok(ExecutionOutcome::Held)}status="failed";}
    if input.is_some(){
        let until=crate::api::sdlc_workspace::task_pipeline::verify_submitted_attempt(pool,job.pipeline_id,lease.attempt_id,&config.sdlc_workspace,&config.secrets).await?;
        let _guard=running.lock().await;
        if !finish_image_build_success(pool,job.id,lease,Some(terminal.exit_code),until).await?{return Ok(ExecutionOutcome::Held)}
    }else{
        let updated=sqlx::query("UPDATE jobs SET status=$2,finished_at=now() WHERE id=$1 AND status='running' AND EXISTS(SELECT 1 FROM execution_attempts WHERE id=$3 AND status='running')").bind(job.id).bind(status).bind(lease.attempt_id).execute(pool).await.map_err(ApiError::internal)?;
        if updated.rows_affected()==0 {status="canceled";mark_attempt_canceled(pool,lease.attempt_id,"Kubernetes execution canceled").await?;}else{
            sqlx::query("UPDATE execution_attempts SET status=$2,finished_at=now(),exit_code=$3 WHERE id=$1 AND status='running'").bind(lease.attempt_id).bind(status).bind(terminal.exit_code).execute(pool).await.map_err(ApiError::internal)?;
        }
        complete_embedded_job_lease(pool,lease.id,status,None).await?;
    }
    // Fresh exact DB completion readback precedes releasing the finalizer and any filesystem cleanup.
    let ack:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id WHERE l.id=$1 AND l.attempt_id=$2 AND l.generation=$3 AND l.completed_at IS NOT NULL AND l.lease_status IN ('completed','canceled') AND a.status=l.terminal_status AND a.finished_at IS NOT NULL)").bind(lease.id).bind(lease.attempt_id).bind(lease.generation).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !ack || execution.acknowledge().await.is_err(){return Ok(ExecutionOutcome::Held)}
    refresh_stage(pool.clone(),job.id).await?;
    Ok(finish_workspace_retention(pool,lease,workspace,None,config.keep_workspace).await)
}

async fn crane_json(kind:&str,image:&str)->anyhow::Result<Value>{
    let output=tokio::time::timeout(Duration::from_secs(60),tokio::process::Command::new("crane").args([kind,"--",image]).stdin(Stdio::null()).kill_on_drop(true).output()).await??;
    ensure!(output.status.success() && output.stdout.len()<=4*1024*1024,"registry readback unavailable");Ok(serde_json::from_slice(&output.stdout)?)
}
async fn publish_build(kube:&Config,job:&JobRow,lease:&EmbeddedJobLease,spec:&crate::domain::image_build::ImageBuildSpec,workspace:&Path,registry:&str)->anyhow::Result<PathBuf>{
    let file=resolve_workspace_artifact(workspace,"build-output/metadata.json").await.map_err(|_|anyhow::anyhow!("metadata missing"))?;
    let metadata:Value=serde_json::from_slice(&tokio::fs::read(file).await?)?;
    let digest=metadata["containerimage.digest"].as_str().context("builder digest missing")?;
    ensure!(crate::domain::image_build::valid_image_id(digest),"invalid registry digest");
    let repository=registry.rsplit_once(':').context("registry tag missing")?.0;
    let image=format!("{repository}@{digest}");
    ensure!(crate::execution::kubernetes::resolve_image(&image).await?==image,"registry manifest changed");
    let manifest=crane_json("manifest",&image).await?;
    let image_id=manifest["config"]["digest"].as_str().context("config digest missing")?;
    ensure!(metadata["containerimage.config.digest"]==image_id,"builder config digest mismatch");
    let config=crane_json("config",&image).await?;
    ensure!(config["config"]["Labels"]["org.opencontainers.image.revision"]==job.commit_sha.as_deref().unwrap_or("") && config["config"]["Labels"]["io.forge.attempt"]==lease.attempt_id.to_string(),"registry provenance mismatch");
    let descriptor=crate::domain::image_build::OciCandidateDescriptor::new(image_id.into(),job.commit_sha.clone().context("pin missing")?,spec.readable_schema_versions.clone()).map_err(anyhow::Error::msg)?;
    let root=Path::new("/workspaces/.kubernetes-control").join(format!("forge-{}-g{}",lease.attempt_id.simple(),lease.generation));
    let output=root.join("output");tokio::fs::create_dir(&output).await?;
    let path=output.join(&spec.descriptor_artifact);if let Some(parent)=path.parent(){tokio::fs::create_dir_all(parent).await?;}
    tokio::fs::write(&path,serde_json::to_vec(&descriptor)?).await?;
    tokio::fs::write(root.join("registry-candidate.json"),serde_json::to_vec(&json!({"schema":"forge/kubernetes-registry-candidate/v1","namespace":kube.namespace,"namespaceUid":kube.namespace_uid,"workspaceClaimUid":kube.workspace_claim_uid,"attemptId":lease.attempt_id,"leaseId":lease.id,"generation":lease.generation,"sourceCommit":job.commit_sha,"image":image,"descriptor":descriptor}))?).await?;
    Ok(output)
}
