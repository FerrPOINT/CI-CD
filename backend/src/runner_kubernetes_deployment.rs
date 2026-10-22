//! Configured target mapping retains the existing task/source/upstream artifact gates.
use super::*;
use crate::{execution::kubernetes::{deployment::Recipe,read},runner_image_build::ImageBuildScope};
use std::collections::BTreeMap;
use sqlx::Row;

pub(super) struct Prepared {pub recipe:Recipe,pub images:BTreeMap<String,String>,pub artifact:String,pub producers:Value}
pub(super) async fn prepare(pool:&PgPool,job:&JobRow,lease:&EmbeddedJobLease,config:&RuntimeRunnerConfig,kube:&Config)->anyhow::Result<Option<Prepared>>{
    let scope=ImageBuildScope{project_id:job.project_id,pipeline_id:job.pipeline_id,job_id:job.id,attempt_id:lease.attempt_id,lease_id:lease.id,generation:lease.generation};
    let Some((plan,artifact))=crate::runner_image_build::workspace_deploy::sealed_requested(pool,&scope).await.map_err(|_|anyhow::anyhow!("immutable deployment plan unavailable"))? else{return Ok(None)};
    let source=task_job_source(pool,job,config).await.map_err(|_|anyhow::anyhow!("task source unavailable"))?;
    let observed=crate::runner_image_build::workspace_deploy::read_owner_compose_source(pool,&source,&scope,std::time::Instant::now()+Duration::from_secs(30)).await?;
    let (_,bytes)=observed.owner_input(&scope,&source)?;
    use sha2::{Digest,Sha256};let compose_hash=format!("{:x}",Sha256::digest(bytes));
    let recipes:Vec<_>=kube.deployment_recipes.iter().filter(|r|r.compose_sha256==compose_hash).collect();ensure!(recipes.len()==1,"unique operator mapping for exact Compose required");let recipe=recipes[0].clone();recipe.validate(kube)?;
    let source_recipe:Value=serde_json::from_slice(bytes)?;let services=source_recipe["services"].as_object().context("source services missing")?;
    ensure!(services.len()==recipe.services.len() && recipe.services.iter().all(|s|services.contains_key(&s.name)),"configured service inventory differs from Git");
    let upstream=crate::runner_image_build::workspace_deploy::upstream_jobs(&plan,&job.name)?;
    let mut images=BTreeMap::new();let mut producers=serde_json::Map::new();
    for service in &recipe.services {
        let selected=&services[&service.name];
        if let Some(key)=&service.image_build_job {
            ensure!(selected["x-forge-image-build"]==*key && selected.get("image").is_none() && upstream.contains(key),"image build selector not declared upstream");
            let planned=plan["jobs"].as_array().context("plan jobs missing")?.iter().find(|v|v["key"]==*key).context("build job missing")?;
            let spec:crate::domain::image_build::ImageBuildSpec=serde_json::from_value(planned["image_build"].clone())?;spec.validate().map_err(anyhow::Error::msg)?;
            let names=crate::domain::artifact_names::declared_artifact_names(std::slice::from_ref(&spec.descriptor_artifact)).map_err(anyhow::Error::msg)?;
            let mut conn=pool.acquire().await?;let jobs=cicd_infra::sdlc_workspace::candidate_jobs(&mut conn,job.pipeline_id).await?;let artifacts=cicd_infra::sdlc_workspace::candidate_artifacts(&mut conn,job.pipeline_id).await?;ensure!(jobs.len()<=1000 && artifacts.len()<=32,"owner inventory exceeds bound");
            let rows=sqlx::query("SELECT l.id,l.attempt_id,l.generation,j.id AS job_id,ar.id AS artifact_id FROM job_leases l JOIN jobs j ON j.id=l.job_id JOIN stages s ON s.id=j.stage_id JOIN artifacts ar ON ar.job_id=j.id AND ar.attempt_id=l.attempt_id WHERE s.pipeline_id=$1 AND j.name=$2 AND ar.name=$3 AND l.lease_status='completed' AND l.terminal_status='success' AND l.completed_at IS NOT NULL AND l.completion_received_at IS NOT NULL AND l.acknowledged_at IS NOT NULL AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id) LIMIT 2").bind(job.pipeline_id).bind(key).bind(&names[0]).fetch_all(&mut *conn).await?;drop(conn);ensure!(rows.len()==1,"unique acknowledged producer unavailable");let row=&rows[0];let attempt:Uuid=row.try_get("attempt_id")?;let producer:Uuid=row.try_get("job_id")?;let generation:i64=row.try_get("generation")?;let producer_lease:Uuid=row.try_get("id")?;let artifact_id:Uuid=row.try_get("artifact_id")?;
            let artifact=artifacts.iter().find(|a|a.artifact_id==artifact_id && a.attempt_id==Some(attempt) && a.retained).context("retained producer artifact unavailable")?;
            ensure!(jobs.iter().any(|j|j.job_id==producer && j.attempt_id==Some(attempt) && j.terminal_acknowledged && j.artifacts_complete),"producer terminal receipt unavailable");
            let candidate:Value=read(&Path::new("/workspaces/.kubernetes-control").join(format!("forge-{}-g{}",attempt.simple(),generation)).join("registry-candidate.json"))?;
            ensure!(candidate["schema"]=="forge/kubernetes-registry-candidate/v1" && candidate["attemptId"]==json!(attempt) && candidate["leaseId"]==json!(producer_lease) && candidate["generation"]==generation && candidate["sourceCommit"]==source.source_commit && candidate["clusterCaSha256"]==kube.cluster_ca_sha256 && candidate["namespaceUid"]==kube.namespace_uid && candidate["workspaceClaimUid"]==kube.workspace_claim_uid,"producer Kubernetes custody mismatch");
            let descriptor:crate::domain::image_build::OciCandidateDescriptor=serde_json::from_value(candidate["descriptor"].clone())?;descriptor.validate().map_err(anyhow::Error::msg)?;let descriptor_bytes=serde_json::to_vec(&descriptor)?;let hash=format!("{:x}",Sha256::digest(&descriptor_bytes));ensure!(artifact.sha256.as_deref()==Some(&hash) && artifact.size_bytes==descriptor_bytes.len() as i64 && descriptor.source_commit==source.source_commit,"producer descriptor artifact differs");
            crate::candidate_evidence::verify_artifact(&config.artifacts.root,Path::new(&artifact.storage_path),&hash,artifact.size_bytes).await?;
            let image=candidate["image"].as_str().context("registry image missing")?;ensure!(crate::execution::kubernetes::digest_image(image) && image.starts_with(&format!("{}/",kube.registry_prefix)),"producer registry mismatch");let manifest=crane_json("manifest",image).await?;ensure!(manifest["config"]["digest"]==descriptor.image_id,"registry config differs from descriptor");
            images.insert(service.name.clone(),image.to_owned());producers.insert(service.name.clone(),json!({"artifactId":artifact_id,"artifactSha256":hash,"producerAttemptId":attempt,"image":image}));
        }else{
            let image=service.image.as_ref().context("dependency image missing")?;let manifest=crane_json("manifest",image).await?;ensure!(selected["image"]==manifest["config"]["digest"] && selected.get("x-forge-image-build").is_none(),"dependency immutable image differs from Git recipe");images.insert(service.name.clone(),image.clone());
        }
        ensure!((selected["environment"].is_null() && service.environment.is_empty()) || selected["environment"]==serde_json::to_value(&service.environment)?,"operator environment mapping differs from Git recipe");
        ensure!(selected["mem_limit"].as_u64()==Some(service.memory_bytes),"operator memory differs from Git recipe");
        let cpus=selected["cpus"].as_f64().or_else(||selected["cpus"].as_str().and_then(|v|v.parse::<f64>().ok())).context("source CPU missing")?;
        ensure!((cpus*1000.0-service.cpu_millis as f64).abs()<0.001,"operator CPU differs from Git recipe");
        let source_mounts:Vec<String>=selected["volumes"].as_array().map(|v|v.iter().map(|m|m.as_str().map(str::to_owned).context("source mount string required")).collect::<anyhow::Result<Vec<_>>>()).transpose()?.unwrap_or_default();
        let configured:std::collections::BTreeSet<String>=service.mounts.iter().map(|(name,path)|format!("{name}:{path}")).collect();
        ensure!(source_mounts.len()==configured.len() && source_mounts.iter().all(|v|configured.contains(v)),"operator disposable mount mapping differs from Git recipe");
        let runtime=crane_json("config",&images[&service.name]).await?;
        let numeric_user=selected["user"].as_str().or_else(||runtime["config"]["User"].as_str()).context("nonroot source user missing")?;
        ensure!(numeric_user==format!("{}:{}",kube.security.run_as_user,kube.security.run_as_group),"configured Pod user differs from source image/recipe");
    }
    ensure!(image_build_attempt_active(pool,lease).await.map_err(|_|anyhow::anyhow!("lease readback failed"))?,"deployment lease expired during inventory");
    Ok(Some(Prepared{recipe,images,artifact,producers:Value::Object(producers)}))
}
