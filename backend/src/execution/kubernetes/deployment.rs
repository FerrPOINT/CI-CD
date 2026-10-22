//! Closed operator mapping for an exact Git deployment recipe. No arbitrary manifest apply.
use super::*;
use std::collections::{BTreeMap,BTreeSet};

#[derive(Clone,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Recipe {pub compose_sha256:String,pub services:Vec<Service>}
#[derive(Clone,Debug,Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Service {
    pub name:String,
    #[serde(default)]pub image:Option<String>,
    #[serde(default)]pub image_build_job:Option<String>,
    pub port:u16,
    #[serde(default)]pub health_path:Option<String>,
    #[serde(default)]pub environment:BTreeMap<String,String>,
    #[serde(default)]pub mounts:BTreeMap<String,String>,
    pub cpu_millis:u32,
    pub memory_bytes:u64,
}
impl Recipe {
    pub fn validate(&self,config:&Config)->anyhow::Result<()> {
        ensure!(self.compose_sha256.len()==64 && self.compose_sha256.bytes().all(|b|b.is_ascii_hexdigit()) && !self.services.is_empty() && self.services.len()<=8,"invalid configured deployment recipe");
        let mut names=BTreeSet::new();let mut ports=BTreeSet::new();let mut cpu=0u64;let mut memory=0u64;
        for s in &self.services {
            ensure!(dns(&s.name) && s.name.len()<=32 && names.insert(&s.name) && s.port>0 && ports.insert(s.port),"invalid/duplicate service identity or port");
            ensure!(matches!((&s.image,&s.image_build_job),(Some(image),None) if digest_image(image)) || matches!((&s.image,&s.image_build_job),(None,Some(key)) if !key.is_empty() && key.len()<=128),"exactly one service image selector required");
            ensure!(s.health_path.as_ref().is_none_or(|p|p.starts_with('/') && p.len()<=1024 && !p.chars().any(char::is_control)),"invalid health path");
            ensure!(s.cpu_millis>0 && s.memory_bytes>=16*1024*1024,"invalid service resources");cpu+=s.cpu_millis as u64;memory+=s.memory_bytes;
            ensure!(s.environment.len()<=64 && s.environment.keys().all(|k|!k.is_empty() && k.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')),"invalid environment mapping");
            ensure!(s.mounts.len()<=8 && s.mounts.iter().all(|(name,path)|dns(name) && path.starts_with('/') && !path.contains("..") && !path.starts_with("/workspace") && !path.starts_with("/var/run/secrets") && !matches!(path.as_str(),"/"|"/tmp"|"/run")),"only bounded disposable application mounts allowed");
        }
        ensure!(cpu+100<=config.resources.cpu_millis as u64 && memory+64*1024*1024<=config.resources.memory_bytes,"dependencies leave no command resource budget");Ok(())
    }
}
impl Execution {
    /// `images` has already passed the backend's exact upstream artifact/custody checks.
    pub fn deployment_template(&mut self,recipe:&Recipe,images:&BTreeMap<String,String>,secrets:&[(String,String)])->anyhow::Result<()> {
        recipe.validate(&self.config)?;
        ensure!(images.len()==recipe.services.len(),"incomplete owner image inventory");
        let mut cpu=self.config.resources.cpu_millis;let mut memory=self.config.resources.memory_bytes;
        let mut containers=Vec::new();let mut volumes=Vec::new();let mut aliases=Vec::new();
        let ephemeral=self.config.resources.ephemeral_storage_bytes/(recipe.services.len() as u64+1);
        let mut data_volumes:BTreeMap<String,u64>=BTreeMap::new();
        for service in &recipe.services {
            let image=images.get(&service.name).context("service image missing")?;ensure!(digest_image(image),"immutable dependency image required");
            cpu-=service.cpu_millis;memory-=service.memory_bytes;
            let mut env=Vec::new();
            for (key,value) in &service.environment {
                let value=if value.starts_with("${") && value.ends_with('}') {let name=&value[2..value.len()-1];secrets.iter().find(|(key,_)|key==name).map(|(_,value)|value.clone()).context("undeclared deployment secret")?}else{ensure!(!value.contains("${"),"mixed/default secret expansion denied");value.clone()};
                env.push(json!({"name":key,"value":value}));
            }
            let mut mounts=vec![json!({"name":format!("{}-tmp",service.name),"mountPath":"/tmp"}),json!({"name":format!("{}-run",service.name),"mountPath":"/run"})];
            volumes.extend([json!({"name":format!("{}-tmp",service.name),"emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}}),json!({"name":format!("{}-run",service.name),"emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}})]);
            for (name,path) in &service.mounts {let volume=format!("deployment-{name}");mounts.push(json!({"name":volume,"mountPath":path}));let bound=ephemeral/service.mounts.len().max(1) as u64;data_volumes.entry(volume).and_modify(|v|*v=(*v).min(bound)).or_insert(bound);}
            let readiness=service.health_path.as_ref().map(|path|json!({"httpGet":{"port":service.port,"path":path},"periodSeconds":1,"failureThreshold":3})).unwrap_or_else(||json!({"tcpSocket":{"port":service.port},"periodSeconds":1,"failureThreshold":3}));
            let r=json!({"cpu":format!("{}m",service.cpu_millis),"memory":service.memory_bytes.to_string(),"ephemeral-storage":ephemeral.to_string()});
            containers.push(json!({"name":service.name,"image":image,"imagePullPolicy":"IfNotPresent","env":env,"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"resources":{"requests":r,"limits":r},"readinessProbe":readiness,"volumeMounts":mounts}));
            aliases.push(service.name.clone());
        }
        for (name,bound) in data_volumes{volumes.push(json!({"name":name,"emptyDir":{"sizeLimit":bound.to_string()}}));}
        self.manifest["metadata"]["labels"]["pdlc.forge-executor"]=json!("workspace-deploy");
        self.manifest["metadata"]["annotations"]["pdlc.deployment-recipe-sha256"]=json!(recipe.compose_sha256);
        let spec=&mut self.manifest["spec"];
        let r=json!({"cpu":format!("{cpu}m"),"memory":memory.to_string(),"ephemeral-storage":(self.config.resources.ephemeral_storage_bytes-ephemeral*recipe.services.len() as u64).to_string()});spec["containers"][0]["resources"]=json!({"requests":r,"limits":r});
        let original=spec["containers"][0]["command"].as_array().context("command missing")?.last().and_then(Value::as_str).context("shell command missing")?.to_owned();
        spec["containers"][0]["command"]=json!(["sh","-c","while [ ! -f /workspace/.forge-deployment-ready ]; do sleep 1; done; exec sh -lc \"$1\"","forge-validation",original]);
        spec["containers"].as_array_mut().context("containers missing")?.extend(containers);
        spec["volumes"].as_array_mut().context("volumes missing")?.extend(volumes);
        spec["hostAliases"]=json!([{"ip":"127.0.0.1","hostnames":aliases}]);
        self.seal_effect()?;Ok(())
    }
    pub async fn dependencies_ready(&self)->anyhow::Result<bool>{
        let value=self.get(&self.path()).await?.ok_or(Held)?;self.identity(&value)?;
        let Some(statuses)=value["status"]["containerStatuses"].as_array() else{return Ok(false)};
        let expected=value["spec"]["containers"].as_array().context("container list missing")?.len();
        Ok(statuses.len()==expected && statuses.iter().filter(|s|s["name"]!="execution").all(|s|s["ready"]==true && s["restartCount"]==0 && !s["state"]["running"].is_null()))
    }
    pub async fn command_stopped(&self)->anyhow::Result<bool>{
        let value=self.get(&self.path()).await?.ok_or(Held)?;self.identity(&value)?;
        Ok(value["status"]["containerStatuses"].as_array().is_some_and(|v|v.iter().any(|s|s["name"]=="execution" && !s["state"]["terminated"].is_null())))
    }
    pub async fn finish_services(&self)->anyhow::Result<()> {
        let value=self.get(&self.path()).await?.ok_or(Held)?;let uid=self.identity(&value)?;
        write_once(&self.directory.join("validation-stop.json"),b"true")?;
        let (status,_)=self.request(Method::DELETE,&self.path(),Some(&json!({"apiVersion":"v1","kind":"DeleteOptions","gracePeriodSeconds":20,"preconditions":{"uid":uid}}))).await?;
        ensure!(status.is_success(),"validation services stop unconfirmed");Ok(())
    }
    pub fn deployment_receipt(&self,compose_sha256:&str,terminal:&Terminal)->anyhow::Result<Value>{
        ensure!(read::<Terminal>(&self.directory.join("terminal.json"))?==*terminal,"stopped observation missing");
        Ok(json!({"schema":"forge/workspace-deployment/v1","adapter":"kubernetes","intent":{"scope":self.intent.scope,"source_commit":self.intent.scope.source_commit,"manifest_sha256":self.intent.action_sha256,"compose_sha256":compose_sha256},"validation":if terminal.exit_code==0{"verified"}else{"failed"},"runtime_state":"stopped-after-validation","exit_code":terminal.exit_code,"observed":{"cluster_ca_sha256":self.config.cluster_ca_sha256,"namespace":self.config.namespace,"namespace_uid":self.config.namespace_uid,"workspace_claim_uid":self.config.workspace_claim_uid,"pod_uid":terminal.pod_uid}}))
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn deployment_mapping_cannot_add_mutable_images_or_conflicting_ports(){let config=super::super::tests::config();let mut recipe=Recipe{compose_sha256:"a".repeat(64),services:vec![Service{name:"app".into(),image:Some(config.allowed_images[0].clone()),image_build_job:None,port:8000,health_path:Some("/health".into()),environment:BTreeMap::new(),mounts:BTreeMap::new(),cpu_millis:100,memory_bytes:64*1024*1024}]};assert!(recipe.validate(&config).is_ok());recipe.services.push(recipe.services[0].clone());assert!(recipe.validate(&config).is_err());recipe.services.pop();recipe.services[0].image=Some("app:latest".into());assert!(recipe.validate(&config).is_err());}
}
