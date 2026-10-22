//! One execution/custody adapter for embedded and external Forge runners.
//! A missing Pod or a failed API request never proves that execution stopped.
use anyhow::{Context, ensure};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs::{self, OpenOptions}, io::Write, path::{Component, Path, PathBuf}, time::Duration};
use uuid::Uuid;

pub mod deployment;

pub const FINALIZER: &str = "forge.pdlc.acm-ai.ru/execution-custody";
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Resources { pub cpu_millis: u32, pub memory_bytes: u64, pub ephemeral_storage_bytes: u64 }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Security { pub run_as_user: u32, pub run_as_group: u32, pub fs_group: u32 }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub installation_sha256: String,
    pub installation: String,
    pub builder_service_account: String,
    pub cluster_ca_sha256: String,
    pub namespace: String,
    pub namespace_uid: String,
    pub workspace_claim: String,
    pub workspace_claim_uid: String,
    pub job_service_account: String,
    #[serde(default)] pub allowed_images: Vec<String>,
    pub builder_image: String,
    pub registry_prefix: String,
    #[serde(default)] pub registry_secret: Option<String>,
    #[serde(default)] pub deployment_recipes: Vec<deployment::Recipe>,
    pub resources: Resources,
    pub security: Security,
}
impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        ensure!(path.is_absolute(), "Kubernetes runner config must be absolute");
        // ConfigMaps use projected symlinks; contents are operator-owned and strictly validated.
        let bytes=fs::read(path)?;
        ensure!(bytes.len()<=64*1024, "runner config exceeds bound");
        let config: Self=serde_json::from_slice(&bytes)?;
        config.validate()?;
        Ok(config)
    }
    pub fn from_env() -> anyhow::Result<Self> {
        Self::load(Path::new(&std::env::var("CICD_KUBERNETES_RUNNER_CONFIG").context("Kubernetes runner config missing")?))
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.installation_sha256.len()==64 && self.installation_sha256.bytes().all(|b|b.is_ascii_hexdigit()),"installation SHA256 required");
        ensure!(self.schema_version==1 && dns(&self.namespace) && dns(&self.workspace_claim) && dns(&self.job_service_account) && dns(&self.builder_service_account) && self.builder_service_account!=self.job_service_account && dns(&self.installation), "invalid runner config identity");
        ensure!(self.cluster_ca_sha256.len()==64 && self.cluster_ca_sha256.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "pinned cluster CA SHA256 required");
        for uid in [&self.namespace_uid,&self.workspace_claim_uid] { ensure!(Uuid::parse_str(uid).is_ok(), "explicit Kubernetes UID required"); }
        ensure!(self.allowed_images.len()<=128 && self.allowed_images.iter().all(|v| digest_image(v)) && digest_image(&self.builder_image), "pinned image digest allowlist required");
        ensure!(!self.registry_prefix.is_empty() && !self.registry_prefix.contains(['@',' ', '\\']) && reqwest::Url::parse(&format!("https://{}",self.registry_prefix)).is_ok_and(|u|u.host_str().is_some() && u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none()), "invalid approved registry prefix");
        ensure!(self.registry_secret.as_ref().is_none_or(|s|dns(s)), "invalid registry secret name");
        ensure!(self.resources.cpu_millis>0 && self.resources.cpu_millis<=4000 && self.resources.memory_bytes>=16*1024*1024 && self.resources.memory_bytes<=4*1024*1024*1024 && self.resources.ephemeral_storage_bytes>=16*1024*1024 && self.resources.ephemeral_storage_bytes<=16*1024*1024*1024, "invalid workload resource budget");
        ensure!(self.security.run_as_user>0 && self.security.run_as_group>0 && self.security.fs_group>0, "numeric nonroot workload identity required");
        ensure!(self.deployment_recipes.len()<=32,"deployment recipe inventory exceeds bound");
        for recipe in &self.deployment_recipes {recipe.validate(self)?;}
        Ok(())
    }
}
pub fn digest_image(image:&str)->bool {
    image.rsplit_once("@sha256:").is_some_and(|(name,d)|!name.is_empty() && !name.contains([' ', '\\', '\n']) && d.len()==64 && d.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn dns(v:&str)->bool { !v.is_empty() && v.len()<=253 && v.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'||b==b'.') && v.as_bytes()[0].is_ascii_alphanumeric() && v.as_bytes()[v.len()-1].is_ascii_alphanumeric() }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Scope { pub job_id: Uuid, pub attempt_id: Uuid, pub lease_id: Uuid, pub generation: i64, pub source_commit: String }
impl Scope {
    pub fn validate(&self)->anyhow::Result<()> {
        ensure!(!self.attempt_id.is_nil() && !self.lease_id.is_nil() && self.generation>0 && crate::domain::image_build::valid_commit(&self.source_commit), "pinned attempt/lease/generation required"); Ok(())
    }
    fn name(&self)->String { format!("forge-{}-g{}",self.attempt_id.simple(),self.generation) }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Intent { config: Config, scope: Scope, pod_name: String, workspace: String, image: String, action_sha256: String, environment_sha256: String }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Terminal { pub pod_uid: String, pub exit_code: i32, pub reason: String }
/// Held is deliberately different from an ordinary failed job.
#[derive(Debug, thiserror::Error)]
#[error("Kubernetes execution has unknown effects; retain original custody and reconcile")]
pub struct Held;

pub struct Execution {
    config: Config, client: Client, server: String, token_path: Option<PathBuf>,
    directory: PathBuf, intent: Intent, manifest: Value,
}
impl Execution {
    /// Workspace is already owner-prepared and pinned. This function neither clones nor grants admission.
    pub fn prepare(config: Config, root: &Path, scope: Scope, workspace: &Path, image: &str, command: &[String], env: &[(String,String)], timeout: u64) -> anyhow::Result<Self> {
        config.validate()?; scope.validate()?;
        ensure!(digest_image(image) && (config.allowed_images.is_empty() || config.allowed_images.iter().any(|v|v==image) || image==config.builder_image), "image outside operator allowlist");
        ensure!((5..=86400).contains(&timeout) && !command.is_empty(), "invalid execution bounds");
        guarded(root)?; guarded(workspace)?;
        #[cfg(unix)]{use std::os::unix::fs::MetadataExt;let meta=fs::metadata(root)?;ensure!(meta.gid()==config.security.fs_group && meta.mode()&0o2070==0o2070,"workspace PVC root must already have configured fsGroup, group rwx and setgid; recursive fsGroup mutation is forbidden");}
        ensure!(root.is_absolute() && root.canonicalize()?==root && workspace.canonicalize()?==workspace, "canonical workspace mount required");
        let sub=workspace.strip_prefix(root)?;
        ensure!(sub.components().all(|c|matches!(c,Component::Normal(_))) && sub.components().count()>=2, "only attempt workspace may be mounted");
        let parent=workspace.parent().context("attempt wrapper missing")?;
        let marker:Value=serde_json::from_slice(&fs::read(parent.join(".forge-attempt.json"))?)?;
        ensure!(marker["attempt_id"]==json!(scope.attempt_id) && marker["lease_id"]==json!(scope.lease_id) && marker["generation"]==scope.generation, "workspace owner mismatch");
        let pod_name=scope.name();
        let manifest=pod(&config,&scope,&pod_name,&sub.to_string_lossy(),image,command,env,timeout);
        let intent=Intent { config:config.clone(),scope,pod_name,workspace:sub.to_string_lossy().into(),image:image.into(),action_sha256:hash(&serde_json::to_vec(&manifest)?),environment_sha256:hash(&serde_json::to_vec(env)?) };
        let control=root.join(".kubernetes-control"); private_dir(&control)?;
        let directory=control.join(&intent.pod_name); private_dir(&directory)?;
        write_once(&directory.join("intent.json"),&serde_json::to_vec(&intent)?)?;
        let (client,server,token_path)=transport(&config)?;
        Ok(Self{config,client,server,token_path,directory,intent,manifest})
    }
    pub fn reopen(root:&Path,attempt:Uuid,generation:i64)->anyhow::Result<Self>{
        let name=format!("forge-{}-g{}",attempt.simple(),generation);
        let directory=root.join(".kubernetes-control").join(name);
        let mut intent:Intent=read(&directory.join("intent.json"))?;
        if directory.join("effect-identity.json").exists(){let effect:Value=read(&directory.join("effect-identity.json"))?;intent.action_sha256=effect["actionSha256"].as_str().context("effective digest missing")?.into();}
        ensure!(intent.scope.attempt_id==attempt && intent.scope.generation==generation,"custody scope mismatch");
        let current=Config::from_env()?;ensure!(current.cluster_ca_sha256==intent.config.cluster_ca_sha256 && current.namespace==intent.config.namespace && current.namespace_uid==intent.config.namespace_uid && current.workspace_claim==intent.config.workspace_claim && current.workspace_claim_uid==intent.config.workspace_claim_uid && current.installation==intent.config.installation,"installation physical identity changed");
        let config=intent.config.clone();
        let (client,server,token_path)=transport(&config)?;
        Ok(Self{config,client,server,token_path,directory,intent,manifest:Value::Null})
    }
    async fn request(&self, method:Method, path:&str, body:Option<&Value>)->anyhow::Result<(StatusCode,Vec<u8>)> {
        let mut request=self.client.request(method,format!("{}{path}",self.server));
        if let Some(path)=&self.token_path { let token=fs::read_to_string(path)?; request=request.bearer_auth(token.trim()); }
        if let Some(body)=body { request=request.json(body); }
        let mut response=request.send().await?;
        let status=response.status(); let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await? { ensure!(bytes.len()+chunk.len()<=MAX_RESPONSE,"API response exceeds bound");bytes.extend_from_slice(&chunk); }
        Ok((status,bytes))
    }
    async fn get(&self,path:&str)->anyhow::Result<Option<Value>> {
        let (status,bytes)=self.request(Method::GET,path,None).await?;
        if status==StatusCode::NOT_FOUND {return Ok(None)}
        ensure!(status.is_success(),"Kubernetes read failed ({status})"); Ok(Some(serde_json::from_slice(&bytes)?))
    }
    async fn identities(&self)->anyhow::Result<()> {
        for (path,uid) in [(format!("/api/v1/namespaces/{}",self.config.namespace),&self.config.namespace_uid),(format!("/api/v1/namespaces/{}/persistentvolumeclaims/{}",self.config.namespace,self.config.workspace_claim),&self.config.workspace_claim_uid)] {
            let value=self.get(&path).await?.context("configured identity missing")?;
            ensure!(value["metadata"]["uid"].as_str()==Some(uid.as_str()),"Kubernetes identity changed");
            if path.contains("persistentvolumeclaims") {ensure!(value["status"]["phase"]=="Bound","workspace claim is not Bound");}
        } Ok(())
    }
    fn path(&self)->String {format!("/api/v1/namespaces/{}/pods/{}",self.config.namespace,self.intent.pod_name)}
    fn identity(&self,value:&Value)->anyhow::Result<String> {
        ensure!(value["spec"]["restartPolicy"]=="Never" && value["spec"]["automountServiceAccountToken"]==false && value["spec"]["serviceAccountName"]==self.expected_service_account()? && value["spec"]["containers"][0]["image"]==self.intent.image && value["spec"]["volumes"][0]["persistentVolumeClaim"]["claimName"]==self.config.workspace_claim && value["spec"]["containers"][0]["volumeMounts"][0]["subPath"]==self.intent.workspace,"Pod execution/mount policy changed");
        ensure!(value["metadata"]["name"]==self.intent.pod_name && value["metadata"]["namespace"]==self.config.namespace && value["metadata"]["annotations"]["forge.pdlc.acm-ai.ru/intent"]==self.intent.action_sha256,"foreign Pod identity");
        let uid=value["metadata"]["uid"].as_str().context("Pod UID missing")?;
        Uuid::parse_str(uid)?;
        if self.directory.join("effect-identity.json").exists(){let effect:Value=read(&self.directory.join("effect-identity.json"))?;let expected=effect["containers"].as_array().context("container identity missing")?;let actual=value["spec"]["containers"].as_array().context("container inventory missing")?;ensure!(expected.len()==actual.len() && expected.iter().zip(actual).all(|(e,a)|e["name"]==a["name"] && e["image"]==a["image"]),"execution service images changed");}
        ensure!(value["spec"]["initContainers"].as_array().is_none_or(|v|v.is_empty()) && value["spec"]["ephemeralContainers"].as_array().is_none_or(|v|v.is_empty()),"foreign execution containers; reconciliation held");
        if !self.directory.join("effect-identity.json").exists(){ensure!(value["spec"]["containers"].as_array().is_some_and(|v|v.len()==1),"ordinary Pod container inventory changed");}
        let created=self.directory.join("created.json");
        if created.exists() {ensure!(read::<String>(&created)?==uid,"Pod UID changed");}
        Ok(uid.into())
    }
    /// Persist issued before POST. After crash/transport loss only GET of the same name is legal.
    pub async fn start(&self)->anyhow::Result<()> {self.start_with_admission(||admission(&self.config)).await}
    async fn start_with_admission(&self,authorize:impl FnOnce()->anyhow::Result<Admission>)->anyhow::Result<()> {
        self.identities().await?;
        let issued=self.directory.join("issued.json");
        if !issued.exists() {
            ensure!(self.manifest.is_object(),"reopened custody cannot create a new effect");
            let admission=authorize()?;
            write_once(&issued,b"true")?;
            let mut manifest=self.manifest.clone();
            manifest["metadata"]["annotations"]["pdlc.admission-epoch"]=json!(admission.epoch);
            manifest["metadata"]["annotations"]["pdlc.operation-id"]=json!(admission.operation_id);
            manifest["metadata"]["annotations"]["forge.pdlc.acm-ai.ru/intent"]=json!(self.intent.action_sha256);
            let _=self.request(Method::POST,&format!("/api/v1/namespaces/{}/pods",self.config.namespace),Some(&manifest)).await;
        }
        let value=self.get(&self.path()).await.map_err(|_|Held)?.ok_or(Held)?;
        let uid=self.identity(&value).map_err(|_|Held)?;
        write_once(&self.directory.join("created.json"),&serde_json::to_vec(&uid)?)?;
        Ok(())
    }
    pub async fn observe(&self)->anyhow::Result<Option<Terminal>> {
        self.identities().await.map_err(|_|Held)?;
        let value=self.get(&self.path()).await.map_err(|_|Held)?.ok_or(Held)?;
        let uid=self.identity(&value).map_err(|_|Held)?;
        let terminal=terminal(&value,&uid)?;
        if let Some(proof)=&terminal {write_once(&self.directory.join("terminal.json"),&serde_json::to_vec(proof)?)?;}
        Ok(terminal)
    }
    /// Finalizer keeps the API object until exact terminated-container readback is recorded.
    pub async fn cancel(&self)->anyhow::Result<()> {
        let value=self.get(&self.path()).await.map_err(|_|Held)?.ok_or(Held)?;
        let uid=self.identity(&value).map_err(|_|Held)?;
        write_once(&self.directory.join("cancel.json"),b"true")?;
        if value["metadata"]["deletionTimestamp"].is_null() {
            let (status,_)=self.request(Method::DELETE,&self.path(),Some(&json!({"apiVersion":"v1","kind":"DeleteOptions","gracePeriodSeconds":20,"preconditions":{"uid":uid}}))).await.map_err(|_|Held)?;
            ensure!(status.is_success(),"Pod cancellation unconfirmed");
        } Ok(())
    }
    pub async fn stream_logs<F,Fut>(&self,mut emit:F)->anyhow::Result<()> where F:FnMut(String)->Fut,Fut:std::future::Future<Output=anyhow::Result<()>> {
        let value=self.get(&self.path()).await?.ok_or(Held)?;self.identity(&value)?;
        let mut request=self.client.get(format!("{}{}/log?container=execution",self.server,self.path())).timeout(Duration::from_secs(300));
        if let Some(path)=&self.token_path{request=request.bearer_auth(fs::read_to_string(path)?.trim());}
        let mut response=request.send().await?;ensure!(response.status().is_success(),"Pod logs unavailable");
        let mut line=Vec::new();
        while let Some(chunk)=response.chunk().await? {
            for byte in chunk {if byte==b'\n'{emit(String::from_utf8_lossy(&line).into_owned()).await?;line.clear();}else{ensure!(line.len()<1024*1024,"single log line exceeds bound");line.push(byte);}}
        }
        if !line.is_empty(){emit(String::from_utf8_lossy(&line).into_owned()).await?;}Ok(())
    }
    /// Called only after exact durable owner terminal ACK. Never deletes workspace files.
    pub async fn acknowledge(&self)->anyhow::Result<()> {
        if self.directory.join("cleaned.json").exists(){ensure!(read::<bool>(&self.directory.join("cleaned.json"))?,"invalid cleanup receipt");return self.complete_cleanup()}
        let previous:Terminal=read(&self.directory.join("terminal.json"))?;
        self.identities().await?;
        let cleanup_started=self.directory.join("cleanup-intent.json").exists() && read::<Terminal>(&self.directory.join("cleanup-intent.json"))?==previous && read::<Terminal>(&self.directory.join("ack.json"))?==previous;
        let Some(value)=self.get(&self.path()).await? else {
            ensure!(cleanup_started,"Pod absence is not stop proof");
            return self.complete_cleanup();
        };
        let uid=self.identity(&value)?;
        ensure!(terminal(&value,&uid)?==Some(previous.clone()),"terminal proof changed");
        write_once(&self.directory.join("ack.json"),&serde_json::to_vec(&previous)?)?;
        write_once(&self.directory.join("cleanup-intent.json"),&serde_json::to_vec(&previous)?)?;
        let finalizers=match &value["metadata"]["finalizers"]{Value::Null=>Vec::new(),Value::Array(values)=>values.clone(),_=>return Err(Held.into())};
        ensure!(finalizers.iter().all(|v|v==FINALIZER),"foreign finalizer; cleanup held");
        if !finalizers.is_empty(){
            let patch=json!({"metadata":{"uid":uid,"resourceVersion":value["metadata"]["resourceVersion"],"finalizers":[]}});
            let mut request=self.client.patch(format!("{}{}",self.server,self.path())).header("Content-Type","application/merge-patch+json").json(&patch);
            if let Some(path)=&self.token_path {request=request.bearer_auth(fs::read_to_string(path)?.trim());}
            ensure!(request.send().await?.status().is_success(),"finalizer release unconfirmed");
        }
        let (status,_)=self.request(Method::DELETE,&self.path(),Some(&json!({"apiVersion":"v1","kind":"DeleteOptions","preconditions":{"uid":uid}}))).await?;
        ensure!(status.is_success()||status==StatusCode::NOT_FOUND,"Pod cleanup unconfirmed");self.complete_cleanup()
    }
    fn complete_cleanup(&self)->anyhow::Result<()> {
        write_once(&self.directory.join("cleaned.json"),b"true")?;
        let credential=self.directory.join("lease-token");if credential.exists(){guarded(&credential)?;fs::remove_file(credential)?;}Ok(())
    }
    pub fn build_template(&mut self, registry:&str)->anyhow::Result<()> {
        ensure!(self.intent.image==self.config.builder_image && registry.starts_with(&format!("{}/",self.config.registry_prefix)),"builder template identity mismatch");
        self.manifest["spec"]["serviceAccountName"]=json!(self.config.builder_service_account);
        self.manifest["metadata"]["labels"]["pdlc.forge-executor"]=json!("builder");
        let spec=&mut self.manifest["spec"];
        spec["securityContext"]["runAsUser"]=json!(1000);
        spec["securityContext"]["runAsGroup"]=json!(1000);
        // Native rootless BuildKit only; no unconfined/privileged/no-process-sandbox fallback.
        spec["volumes"].as_array_mut().context("volumes missing")?.push(json!({"name":"buildkit-state","emptyDir":{"sizeLimit":self.config.resources.ephemeral_storage_bytes.to_string()}}));
        spec["containers"][0]["volumeMounts"].as_array_mut().context("mounts missing")?.push(json!({"name":"buildkit-state","mountPath":"/home/user/.local/share/buildkit"}));
        if let Some(secret)=&self.config.registry_secret {
            spec["volumes"].as_array_mut().context("volumes missing")?.push(json!({"name":"registry","secret":{"secretName":secret,"items":[{"key":".dockerconfigjson","path":"config.json"}]}}));
            spec["containers"][0]["volumeMounts"].as_array_mut().context("mounts missing")?.push(json!({"name":"registry","mountPath":"/registry-auth","readOnly":true}));
        }
        self.intent.action_sha256=hash(&serde_json::to_vec(&self.manifest)?);
        // prepare wrote the original pure template; changing the effect is create-new evidence.
        self.seal_effect()?;
        Ok(())
    }
    fn seal_effect(&mut self)->anyhow::Result<()> {
        self.intent.action_sha256=hash(&serde_json::to_vec(&self.manifest)?);
        let containers:Vec<Value>=self.manifest["spec"]["containers"].as_array().context("containers missing")?.iter().map(|c|json!({"name":c["name"],"image":c["image"]})).collect();
        write_once(&self.directory.join("effect-identity.json"),&serde_json::to_vec(&json!({"actionSha256":self.intent.action_sha256,"containers":containers,"serviceAccount":self.manifest["spec"]["serviceAccountName"]}))?)?;Ok(())
    }
    fn expected_service_account(&self)->anyhow::Result<String>{
        if self.directory.join("effect-identity.json").exists(){let effect:Value=read(&self.directory.join("effect-identity.json"))?;return Ok(effect["serviceAccount"].as_str().context("effective service account missing")?.into())}Ok(self.config.job_service_account.clone())
    }
    pub fn termination_proof(&self)->anyhow::Result<Value>{let terminal=self.terminal_record()?;Ok(json!({"clusterCaSha256":self.config.cluster_ca_sha256,"namespaceUid":self.config.namespace_uid,"workspaceClaimUid":self.config.workspace_claim_uid,"podUid":terminal.pod_uid}))}
    pub async fn recovery_masks(&self,names:&[String])->anyhow::Result<Vec<String>>{let value=self.get(&self.path()).await?.ok_or(Held)?;self.identity(&value)?;let env=value["spec"]["containers"][0]["env"].as_array().context("original env missing")?;let mut masks=Vec::new();for name in names {let value=env.iter().find(|v|v["name"]==*name).and_then(|v|v["value"].as_str()).context("original declared secret missing")?;if !value.is_empty(){masks.push(value.into());}}Ok(masks)}
    pub fn scope(&self)->&Scope{&self.intent.scope}
    pub fn image(&self)->&str{&self.intent.image}
    pub fn config(&self)->&Config{&self.config}
    pub fn finish_workspace_cleanup(&self)->anyhow::Result<()>{ensure!(self.directory.join("cleaned.json").exists(),"Pod cleanup not acknowledged");write_once(&self.directory.join("workspace-closed.json"),b"true")}
    pub fn terminal_record(&self)->anyhow::Result<Terminal>{read(&self.directory.join("terminal.json"))}
    pub async fn reconcile(&self)->anyhow::Result<Option<Terminal>>{
        ensure!(self.directory.join("issued.json").exists(),"effect never issued; no automatic create");
        if self.directory.join("cleanup-intent.json").exists(){let terminal=self.terminal_record()?;ensure!(read::<Terminal>(&self.directory.join("ack.json"))?==terminal && read::<Terminal>(&self.directory.join("cleanup-intent.json"))?==terminal,"cleanup intent mismatch");self.identities().await?;if self.get(&self.path()).await?.is_none(){return Ok(Some(terminal))}}
        self.start().await?;self.observe().await
    }
    pub fn save_external_recovery(&self,token:&str,artifacts:&[String],secrets:&[String])->anyhow::Result<()> {
        ensure!(!token.trim().is_empty() && token.len()<=4096,"invalid lease credential");
        write_once(&self.directory.join("lease-token"),token.as_bytes())?;
        write_once(&self.directory.join("external-recovery.json"),&serde_json::to_vec(&json!({"artifacts":artifacts,"secrets":secrets}))?)?;Ok(())
    }
    pub fn external_recovery(&self)->anyhow::Result<(String,Vec<String>,Vec<String>)>{
        let path=self.directory.join("lease-token");guarded(&path)?;
        #[cfg(unix)]{use std::os::unix::fs::MetadataExt;ensure!(fs::metadata(&path)?.mode()&0o077==0 && fs::metadata(&self.directory)?.mode()&0o077==0,"private credential permissions required");}
        let mut options=OpenOptions::new();options.read(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.custom_flags(libc::O_NOFOLLOW);}
        let mut token=String::new();use std::io::Read;options.open(path)?.take(4097).read_to_string(&mut token)?;ensure!(token.len()<=4096,"lease credential exceeds bound");let spec:Value=read(&self.directory.join("external-recovery.json"))?;
        Ok((token,serde_json::from_value(spec["artifacts"].clone())?,serde_json::from_value(spec["secrets"].clone())?))
    }
    pub fn cancelled(&self)->bool {self.directory.join("cancel.json").exists()}
}
fn transport(config:&Config)->anyhow::Result<(Client,String,Option<PathBuf>)>{
        let host=std::env::var("KUBERNETES_SERVICE_HOST").context("in-cluster API host missing")?;
        let port=std::env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_|"443".into());
        let server=format!("https://{host}:{port}");
        let url=reqwest::Url::parse(&server)?;
        ensure!(url.host_str().is_some() && url.path()=="/" && url.username().is_empty() && url.password().is_none() && port.parse::<u16>().is_ok(), "invalid in-cluster API address");
        let service=Path::new("/var/run/secrets/kubernetes.io/serviceaccount");
        let ca_bytes=fs::read(service.join("ca.crt"))?;
        ensure!(hash(&ca_bytes)==config.cluster_ca_sha256,"in-cluster CA changed");
        let ca=reqwest::Certificate::from_pem(&ca_bytes)?;
        let client=Client::builder().tls_built_in_root_certs(false).add_root_certificate(ca).https_only(true).redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never()).timeout(Duration::from_secs(30)).build()?;
        Ok((client,server,Some(service.join("token"))))
}
fn pod(config:&Config, scope:&Scope, name:&str, workspace:&str, image:&str,command:&[String],env:&[(String,String)],timeout:u64)->Value {
    let resources=json!({"cpu":format!("{}m",config.resources.cpu_millis),"memory":config.resources.memory_bytes.to_string(),"ephemeral-storage":config.resources.ephemeral_storage_bytes.to_string()});
    json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":name,"namespace":config.namespace,"finalizers":[FINALIZER],"labels":{"app.kubernetes.io/managed-by":"forge-runner","pdlc.installation":config.installation,"pdlc.forge-executor":"job","forge.pdlc.acm-ai.ru/attempt":scope.attempt_id.to_string(),"pdlc.forge-job":scope.job_id.to_string(),"pdlc.forge-attempt":scope.attempt_id.to_string()},"annotations":{"forge.pdlc.acm-ai.ru/intent":"pending","pdlc.workspace-subpath":workspace,"pdlc.workspace-claim-uid":config.workspace_claim_uid}},"spec":{"restartPolicy":"Never","automountServiceAccountToken":false,"serviceAccountName":config.job_service_account,"enableServiceLinks":false,"activeDeadlineSeconds":timeout,"terminationGracePeriodSeconds":20,"securityContext":{"runAsNonRoot":true,"runAsUser":config.security.run_as_user,"runAsGroup":config.security.run_as_group,"fsGroup":config.security.fs_group,"fsGroupChangePolicy":"OnRootMismatch","seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"execution","image":image,"imagePullPolicy":"IfNotPresent","command":command,"workingDir":"/workspace","env":env.iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>(),"resources":{"requests":resources,"limits":resources},"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"workspace","mountPath":"/workspace","subPath":workspace},{"name":"tmp","mountPath":"/tmp"}]}],"volumes":[{"name":"workspace","persistentVolumeClaim":{"claimName":config.workspace_claim}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}}]}})
}
fn terminal(pod:&Value,uid:&str)->anyhow::Result<Option<Terminal>> {
    if !matches!(pod["status"]["phase"].as_str(),Some("Succeeded"|"Failed")){return Ok(None)}
    let statuses=pod["status"]["containerStatuses"].as_array().ok_or(Held)?;
    ensure!(!statuses.is_empty() && statuses.len()<=9 && statuses.iter().all(|s|s["restartCount"]==0 && s["state"]["terminated"]["finishedAt"].as_str().is_some_and(|v|!v.is_empty())),"container termination incomplete or restarted");
    let expected=pod["spec"]["containers"].as_array().ok_or(Held)?;
    let actual_names:std::collections::BTreeSet<&str>=statuses.iter().filter_map(|s|s["name"].as_str()).collect();
    let expected_names:std::collections::BTreeSet<&str>=expected.iter().filter_map(|s|s["name"].as_str()).collect();
    ensure!(actual_names.len()==statuses.len() && expected_names.len()==expected.len() && statuses.len()==expected.len() && actual_names==expected_names,"full container stop inventory mismatch");
    let main=statuses.iter().find(|s|s["name"]=="execution").ok_or(Held)?;
    let state=&main["state"]["terminated"];
    let code=state["exitCode"].as_i64().ok_or(Held)?;
    ensure!(state["finishedAt"].as_str().is_some_and(|s|!s.is_empty()),"termination timestamp missing");
    Ok(Some(Terminal{pod_uid:uid.into(),exit_code:i32::try_from(code)?,reason:state["reason"].as_str().unwrap_or("Terminated").into()}))
}
fn guarded(path:&Path)->anyhow::Result<()> {
    let mut current=PathBuf::new();
    for part in path.components(){ensure!(!matches!(part,Component::ParentDir),"parent path denied");current.push(part);ensure!(!fs::symlink_metadata(&current)?.file_type().is_symlink(),"linked custody path denied");} Ok(())
}
fn private_dir(path:&Path)->anyhow::Result<()> {
    if !path.exists(){let mut builder=fs::DirBuilder::new();#[cfg(unix)]{use std::os::unix::fs::DirBuilderExt;builder.mode(0o700);}builder.create(path)?;}
    guarded(path)?;ensure!(path.is_dir(),"custody directory required");#[cfg(unix)]{use std::os::unix::fs::MetadataExt;ensure!(fs::metadata(path)?.mode()&0o077==0,"private custody parent required");}Ok(())
}
pub(crate) fn read<T:serde::de::DeserializeOwned>(path:&Path)->anyhow::Result<T>{guarded(path)?;let bytes=fs::read(path)?;ensure!(bytes.len()<=64*1024,"custody exceeds bound");Ok(serde_json::from_slice(&bytes)?)}
pub(crate) fn write_once(path:&Path,bytes:&[u8])->anyhow::Result<()> {
    if path.exists(){guarded(path)?;ensure!(fs::read(path)?==bytes,"retained custody differs");return Ok(())}
    let mut opts=OpenOptions::new();opts.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);}
    let mut file=match opts.open(path){Ok(file)=>file,Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{guarded(path)?;ensure!(fs::read(path)?==bytes,"concurrent custody differs");return Ok(())},Err(e)=>return Err(e.into())};file.write_all(bytes)?;file.sync_all()?;fs::File::open(path.parent().context("parent missing")?)?.sync_all()?;Ok(())
}
fn hash(bytes:&[u8])->String{use sha2::{Digest,Sha256};format!("{:x}",Sha256::digest(bytes))}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn config()->Config{Config{schema_version:1,installation_sha256:"d".repeat(64),installation:"fixture".into(),builder_service_account:"forge-builder-job".into(),cluster_ca_sha256:"c".repeat(64),namespace:"app-a".into(),namespace_uid:Uuid::new_v4().to_string(),workspace_claim:"workspaces".into(),workspace_claim_uid:Uuid::new_v4().to_string(),job_service_account:"forge-job".into(),allowed_images:vec![format!("registry.test/job@sha256:{}","a".repeat(64))],builder_image:format!("registry.test/buildkit@sha256:{}","b".repeat(64)),registry_prefix:"registry.test/forge".into(),registry_secret:None,deployment_recipes:Vec::new(),resources:Resources{cpu_millis:500,memory_bytes:256*1024*1024,ephemeral_storage_bytes:1024*1024*1024},security:Security{run_as_user:1000,run_as_group:1000,fs_group:1000}}}
    #[test]fn pod_is_single_attempt_without_controller_or_host_authority(){let c=config();c.validate().unwrap();let s=Scope{job_id:Uuid::new_v4(),attempt_id:Uuid::new_v4(),lease_id:Uuid::new_v4(),generation:2,source_commit:"a".repeat(40)};let p=pod(&c,&s,&s.name(),"attempt-x/workspace",&c.allowed_images[0],&["sh".into(),"-c".into(),"echo done".into()],&[("DECLARED".into(),"value".into())],60);assert_eq!(p["spec"]["restartPolicy"],"Never");assert_eq!(p["spec"]["automountServiceAccountToken"],false);assert_eq!(p["spec"]["containers"][0]["image"],c.allowed_images[0]);assert_eq!(p["spec"]["containers"][0]["volumeMounts"][0]["subPath"],"attempt-x/workspace");assert!(p.to_string().find("hostPath").is_none());assert_eq!(p["metadata"]["finalizers"][0],FINALIZER);}
    #[test]fn terminal_requires_exact_stopped_container_not_phase_or_absence(){let mut p=json!({"status":{"phase":"Succeeded"}});assert!(terminal(&p,"uid").is_err());p["spec"]=json!({"containers":[{"name":"execution"}]});p["status"]["containerStatuses"]=json!([{"name":"execution","restartCount":0,"state":{"terminated":{"exitCode":0,"finishedAt":"2026-10-22T00:00:00Z","reason":"Completed"}}}]);assert_eq!(terminal(&p,"uid").unwrap().unwrap().exit_code,0);p["status"]["containerStatuses"][0]["restartCount"]=json!(1);assert!(terminal(&p,"uid").is_err());}
    #[test]fn custody_inventory_overflow_fails_closed(){let root=std::env::temp_dir().join(format!("forge-inventory-bound-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let control=root.join(".kubernetes-control");fs::create_dir(&control).unwrap();for index in 0..4097{fs::write(control.join(format!("retained-{index}")),b"{}").unwrap();}assert!(retry_unconfirmed(&root,Uuid::new_v4()).is_err());fs::remove_dir_all(root).unwrap();}
    #[test]fn config_rejects_mutable_images_root_and_cross_namespace_aliases(){let mut c=config();c.allowed_images[0]="registry.test/job:latest".into();assert!(c.validate().is_err());c=config();c.security.run_as_user=0;assert!(c.validate().is_err());c=config();c.namespace="other/../app-a".into();assert!(c.validate().is_err());}
}

/// Controller-only OCI resolver. No shell, job environment or credential arguments.
/// Packaging pins crane; existing operator registry credentials remain controller-local.
pub async fn resolve_image(image:&str)->anyhow::Result<String> {
    ensure!(!image.is_empty() && !image.starts_with('-') && image.len()<=2048 && !image.contains([' ', '\\','\n','\r','\0']),"invalid OCI image reference");
    if digest_image(image){return Ok(image.into())}
    let output=tokio::time::timeout(Duration::from_secs(60),tokio::process::Command::new("crane").args(["digest","--",image]).stdin(std::process::Stdio::null()).kill_on_drop(true).output()).await??;
    ensure!(output.status.success() && output.stdout.len()<=256,"OCI tag resolution failed");
    let digest=String::from_utf8(output.stdout)?;let digest=digest.trim();
    let pinned=format!("{}@{}",image.split('@').next().context("image missing")?,digest);
    ensure!(digest_image(&pinned),"resolver returned invalid digest");Ok(pinned)
}
/// A retained issued effect blocks API retry until exact stopped/ACK/cleanup evidence exists.
pub fn retry_unconfirmed(root:&Path,attempt:Uuid)->anyhow::Result<bool>{
    let control=root.join(".kubernetes-control");if !control.exists(){return Ok(false)}guarded(&control)?;
    let prefix=format!("forge-{}-g",attempt.simple());
    let entries=fs::read_dir(control)?.collect::<Result<Vec<_>,_>>()?;ensure!(entries.len()<=4096,"custody inventory exceeds bound; retry held");
    for entry in entries{let name=entry.file_name();if name.to_string_lossy().starts_with(&prefix){guarded(&entry.path())?;if !entry.path().join("cleaned.json").exists(){return Ok(true)}}}
    Ok(false)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {schema_version:u32,installation_sha256:String,namespace_uid:String,namespace:String,allow_new_operations:bool,observed_epoch:u64,expires_epoch:u64,operation_id:String,epoch:String}
fn admission(config:&Config)->anyhow::Result<Admission>{
    let path=std::env::var("CICD_KUBERNETES_ADMISSION_FILE").unwrap_or_else(|_|"/run/pdlc-admission/admission.json".into());
    let bytes=fs::read(path)?;ensure!(bytes.len()<=8192,"admission exceeds bound");
    let admission:Admission=serde_json::from_slice(&bytes)?;
    validate_admission(config,&admission,std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs())?;Ok(admission)
}
fn validate_admission(config:&Config,a:&Admission,now:u64)->anyhow::Result<()>{
    ensure!(a.schema_version==1 && a.installation_sha256==config.installation_sha256 && a.namespace_uid==config.namespace_uid && a.namespace==config.namespace && a.allow_new_operations && a.observed_epoch<=now && a.expires_epoch>now && a.expires_epoch<=a.observed_epoch.saturating_add(45) && Uuid::parse_str(&a.epoch).is_ok() && !a.operation_id.is_empty() && a.operation_id.len()<=128 && !a.operation_id.chars().any(char::is_control),"new execution admission unavailable/expired/mismatched");Ok(())
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::{Arc,Mutex};
    use axum::{Router,body::{Body,to_bytes},extract::Request,response::Response};
    #[derive(Default)]struct Api {pod:Option<Value>,creates:usize,deletes:usize}
    async fn fixture()->(Execution,Arc<Mutex<Api>>,tokio::task::JoinHandle<()>) {
        let config=super::tests::config();let state=Arc::new(Mutex::new(Api::default()));let api=state.clone();let c=config.clone();
        let app=Router::new().fallback(move |request:Request|{let api=api.clone();let c=c.clone();async move {
            let path=request.uri().path().to_owned();let method=request.method().clone();let bytes=to_bytes(request.into_body(),MAX_RESPONSE).await.unwrap();let mut api=api.lock().unwrap();
            let (status,value)=if path.ends_with("/persistentvolumeclaims/workspaces"){(200,json!({"metadata":{"uid":c.workspace_claim_uid},"status":{"phase":"Bound"}}))}
            else if path==format!("/api/v1/namespaces/{}",c.namespace){(200,json!({"metadata":{"uid":c.namespace_uid}}))}
            else if method==Method::POST && path.ends_with("/pods") {api.creates+=1;let mut pod:Value=serde_json::from_slice(&bytes).unwrap();pod["metadata"]["uid"]=json!(Uuid::new_v4().to_string());pod["metadata"]["resourceVersion"]=json!("1");pod["status"]=json!({"phase":"Running","containerStatuses":[{"name":"execution","restartCount":0,"state":{"running":{"startedAt":"2026-10-22T00:00:00Z"}}}]});api.pod=Some(pod.clone());(201,pod)}
            else if method==Method::DELETE {api.deletes+=1;if let Some(pod)=&mut api.pod{pod["metadata"]["deletionTimestamp"]=json!("2026-10-22T00:01:00Z");pod["status"]=json!({"phase":"Failed","containerStatuses":[{"name":"execution","restartCount":0,"state":{"terminated":{"exitCode":143,"finishedAt":"2026-10-22T00:01:00Z","reason":"Terminated"}}}]});(200,pod.clone())}else{(404,json!({}))}}
            else if method==Method::PATCH {if let Some(pod)=&mut api.pod{pod["metadata"]["finalizers"]=json!([]);(200,pod.clone())}else{(404,json!({}))}}
            else {api.pod.clone().map(|p|(200,p)).unwrap_or((404,json!({})))};
            Response::builder().status(status).body(Body::from(serde_json::to_vec(&value).unwrap())).unwrap()
        }});
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let server=format!("http://{}",listener.local_addr().unwrap());let task=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
        let directory=std::env::temp_dir().join(format!("forge-kubernetes-test-{}",Uuid::new_v4()));fs::create_dir(&directory).unwrap();
        let scope=Scope{job_id:Uuid::new_v4(),attempt_id:Uuid::new_v4(),lease_id:Uuid::new_v4(),generation:3,source_commit:"a".repeat(40)};
        let manifest=pod(&config,&scope,&scope.name(),"attempt-fixture/workspace",&config.allowed_images[0],&["sh".into(),"-lc".into(),"echo secret".into()],&[("TOKEN".into(),"private-value".into())],60);
        let intent=Intent{config:config.clone(),scope:scope.clone(),pod_name:scope.name(),workspace:"attempt-fixture/workspace".into(),image:config.allowed_images[0].clone(),action_sha256:hash(&serde_json::to_vec(&manifest).unwrap()),environment_sha256:"e".repeat(64)};
        let client=Client::builder().retry(reqwest::retry::never()).build().unwrap();
        (Execution{config,client,server,token_path:None,directory,intent,manifest},state,task)
    }
    fn grant(config:&Config)->Admission {Admission{schema_version:1,installation_sha256:config.installation_sha256.clone(),namespace_uid:config.namespace_uid.clone(),namespace:config.namespace.clone(),allow_new_operations:true,observed_epoch:100,expires_epoch:145,operation_id:"candidate".into(),epoch:Uuid::new_v4().to_string()}}
    #[tokio::test]async fn cancel_requires_stopped_uid_before_ack_and_never_recreates(){
        let (execution,state,task)=fixture().await;execution.start_with_admission(||Ok(grant(&execution.config))).await.unwrap();assert!(execution.observe().await.unwrap().is_none());assert!(execution.acknowledge().await.is_err());
        execution.start_with_admission(||panic!("existing issued effect must not seek new admission")).await.unwrap();assert_eq!(state.lock().unwrap().creates,1);
        execution.cancel().await.unwrap();assert_eq!(execution.observe().await.unwrap().unwrap().exit_code,143);execution.acknowledge().await.unwrap();assert!(execution.directory.join("cleaned.json").exists());assert_eq!(state.lock().unwrap().creates,1);
        let files=fs::read_dir(&execution.directory).unwrap().map(|e|fs::read_to_string(e.unwrap().path()).unwrap()).collect::<String>();assert!(!files.contains("private-value"));fs::remove_dir_all(&execution.directory).unwrap();task.abort();
    }
    #[tokio::test]async fn absent_or_replaced_pod_holds_instead_of_duplicate_effect(){
        let (execution,state,task)=fixture().await;execution.start_with_admission(||Ok(grant(&execution.config))).await.unwrap();let original=state.lock().unwrap().pod.take();assert!(execution.observe().await.unwrap_err().is::<Held>());assert!(execution.start_with_admission(||panic!("no new admission")).await.is_err());assert_eq!(state.lock().unwrap().creates,1);
        state.lock().unwrap().pod=original;state.lock().unwrap().pod.as_mut().unwrap()["metadata"]["uid"]=json!(Uuid::new_v4().to_string());assert!(execution.observe().await.is_err());fs::remove_dir_all(&execution.directory).unwrap();task.abort();
    }
    #[tokio::test]async fn cleanup_after_ack_recovers_missing_pod_without_reinterpreting_absence_as_stop(){
        let (execution,state,task)=fixture().await;execution.start_with_admission(||Ok(grant(&execution.config))).await.unwrap();
        assert_eq!(execution.recovery_masks(&["TOKEN".into()]).await.unwrap(),vec!["private-value"]);
        execution.cancel().await.unwrap();let terminal=execution.observe().await.unwrap().unwrap();
        write_once(&execution.directory.join("ack.json"),&serde_json::to_vec(&terminal).unwrap()).unwrap();write_once(&execution.directory.join("cleanup-intent.json"),&serde_json::to_vec(&terminal).unwrap()).unwrap();
        state.lock().unwrap().pod=None;assert_eq!(execution.reconcile().await.unwrap(),Some(terminal));execution.acknowledge().await.unwrap();assert!(execution.directory.join("cleaned.json").exists());assert_eq!(state.lock().unwrap().creates,1);fs::remove_dir_all(&execution.directory).unwrap();task.abort();
    }
    #[test]fn stopped_proof_requires_every_original_companion_not_only_execution(){
        let terminated=|name|json!({"name":name,"restartCount":0,"state":{"terminated":{"exitCode":0,"finishedAt":"2026-10-22T00:00:00Z"}}});
        let mut value=json!({"spec":{"containers":[{"name":"execution"},{"name":"database"}]},"status":{"phase":"Failed","containerStatuses":[terminated("execution")]}});
        assert!(terminal(&value,"uid").is_err());value["status"]["containerStatuses"].as_array_mut().unwrap().push(terminated("database"));assert!(terminal(&value,"uid").is_ok());value["status"]["containerStatuses"][1]["name"]=json!("foreign");assert!(terminal(&value,"uid").is_err());
    }
    #[test]fn admission_rejects_stale_future_wrong_namespace_and_disabled(){let c=super::tests::config();let mut a=grant(&c);assert!(validate_admission(&c,&a,110).is_ok());assert!(validate_admission(&c,&a,145).is_err());assert!(validate_admission(&c,&a,99).is_err());a.namespace="app-b".into();assert!(validate_admission(&c,&a,110).is_err());a=grant(&c);a.allow_new_operations=false;assert!(validate_admission(&c,&a,110).is_err());a=grant(&c);a.expires_epoch=146;assert!(validate_admission(&c,&a,110).is_err());}
}

pub fn inventory(root:&Path)->anyhow::Result<Vec<Scope>> {
    let path=root.join(".kubernetes-control");if !path.exists(){return Ok(Vec::new())}guarded(&path)?;
    let entries=fs::read_dir(path)?.collect::<Result<Vec<_>,_>>()?;ensure!(entries.len()<=4096,"custody inventory exceeds bound");
    let mut scopes=Vec::new();for entry in entries{if !entry.path().join("intent.json").exists(){continue}let intent:Intent=read(&entry.path().join("intent.json"))?;if !entry.path().join("workspace-closed.json").exists(){scopes.push(intent.scope)}}Ok(scopes)
}

pub fn external_marker(root:&Path,attempt:Uuid,generation:i64)->anyhow::Result<bool>{let directory=root.join(".kubernetes-control").join(format!("forge-{}-g{}",attempt.simple(),generation));if !directory.exists(){return Ok(false)}guarded(&directory)?;Ok(directory.join("external-recovery.json").exists())}
