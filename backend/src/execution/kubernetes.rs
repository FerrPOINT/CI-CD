//! One execution/custody adapter for embedded and external Forge runners.
//! A missing Pod or a failed API request never proves that execution stopped.
use anyhow::{Context, ensure};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs::{self, OpenOptions}, io::Write, path::{Component, Path, PathBuf}, time::Duration};
use uuid::Uuid;

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
        ensure!(self.schema_version==1 && dns(&self.namespace) && dns(&self.workspace_claim) && dns(&self.job_service_account), "invalid runner config identity");
        ensure!(self.cluster_ca_sha256.len()==64 && self.cluster_ca_sha256.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "pinned cluster CA SHA256 required");
        for uid in [&self.namespace_uid,&self.workspace_claim_uid] { ensure!(Uuid::parse_str(uid).is_ok(), "explicit Kubernetes UID required"); }
        ensure!(self.allowed_images.len()<=128 && self.allowed_images.iter().all(|v| digest_image(v)) && digest_image(&self.builder_image), "pinned image digest allowlist required");
        ensure!(!self.registry_prefix.is_empty() && !self.registry_prefix.contains(['@',' ', '\\']) && reqwest::Url::parse(&format!("https://{}",self.registry_prefix)).is_ok_and(|u|u.host_str().is_some() && u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none()), "invalid approved registry prefix");
        ensure!(self.registry_secret.as_ref().is_none_or(|s|dns(s)), "invalid registry secret name");
        ensure!(self.resources.cpu_millis>0 && self.resources.cpu_millis<=4000 && self.resources.memory_bytes>=16*1024*1024 && self.resources.memory_bytes<=4*1024*1024*1024 && self.resources.ephemeral_storage_bytes>=16*1024*1024 && self.resources.ephemeral_storage_bytes<=16*1024*1024*1024, "invalid workload resource budget");
        ensure!(self.security.run_as_user>0 && self.security.run_as_group>0 && self.security.fs_group>0, "numeric nonroot workload identity required");
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
        if directory.join("build-template.json").exists(){let template:Value=read(&directory.join("build-template.json"))?;intent.action_sha256=hash(&serde_json::to_vec(&template)?);}
        ensure!(intent.scope.attempt_id==attempt && intent.scope.generation==generation,"custody scope mismatch");
        let config=Config::from_env()?;ensure!(config==intent.config,"installation identity/policy changed");
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
        ensure!(value["metadata"]["name"]==self.intent.pod_name && value["metadata"]["namespace"]==self.config.namespace && value["metadata"]["annotations"]["forge.pdlc.acm-ai.ru/intent"]==self.intent.action_sha256,"foreign Pod identity");
        let uid=value["metadata"]["uid"].as_str().context("Pod UID missing")?;
        Uuid::parse_str(uid)?;
        let created=self.directory.join("created.json");
        if created.exists() {ensure!(read::<String>(&created)?==uid,"Pod UID changed");}
        Ok(uid.into())
    }
    /// Persist issued before POST. After crash/transport loss only GET of the same name is legal.
    pub async fn start(&self)->anyhow::Result<()> {
        self.identities().await?;
        let issued=self.directory.join("issued.json");
        if !issued.exists() {
            let admission=admission(&self.config)?;
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
    pub async fn logs(&self)->anyhow::Result<String> {
        let value=self.get(&self.path()).await?.ok_or(Held)?;self.identity(&value)?;
        let (status,bytes)=self.request(Method::GET,&format!("{}/log?container=execution&limitBytes={MAX_RESPONSE}",self.path()),None).await?;
        ensure!(status.is_success(),"Pod logs unavailable");Ok(String::from_utf8(bytes)?)
    }
    /// Called only after exact durable owner terminal ACK. Never deletes workspace files.
    pub async fn acknowledge(&self)->anyhow::Result<()> {
        if self.directory.join("cleaned.json").exists(){return Ok(())}
        let previous:Terminal=read(&self.directory.join("terminal.json"))?;
        ensure!(self.observe().await?==Some(previous.clone()),"terminal proof changed");
        write_once(&self.directory.join("ack.json"),&serde_json::to_vec(&previous)?)?;
        let value=self.get(&self.path()).await?.ok_or(Held)?;
        let uid=self.identity(&value)?;
        let finalizers=value["metadata"]["finalizers"].as_array().context("finalizers missing")?;
        ensure!(finalizers.iter().all(|v|v==FINALIZER),"foreign finalizer; cleanup held");
        let patch=json!({"metadata":{"uid":uid,"resourceVersion":value["metadata"]["resourceVersion"],"finalizers":[]}});
        let mut request=self.client.patch(format!("{}{}",self.server,self.path())).header("Content-Type","application/merge-patch+json").json(&patch);
        if let Some(path)=&self.token_path {request=request.bearer_auth(fs::read_to_string(path)?.trim());}
        ensure!(request.send().await?.status().is_success(),"finalizer release unconfirmed");
        let (status,_)=self.request(Method::DELETE,&self.path(),Some(&json!({"apiVersion":"v1","kind":"DeleteOptions","preconditions":{"uid":uid}}))).await?;
        ensure!(status.is_success()||status==StatusCode::NOT_FOUND,"Pod cleanup unconfirmed");
        write_once(&self.directory.join("cleaned.json"),b"true")?;Ok(())
    }
    pub fn build_template(&mut self, registry:&str)->anyhow::Result<()> {
        ensure!(self.intent.image==self.config.builder_image && registry.starts_with(&format!("{}/",self.config.registry_prefix)),"builder template identity mismatch");
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
        write_once(&self.directory.join("build-template.json"),&serde_json::to_vec(&self.manifest)?)?;
        Ok(())
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
        let client=Client::builder().add_root_certificate(ca).https_only(true).redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never()).timeout(Duration::from_secs(30)).build()?;
        Ok((client,server,Some(service.join("token"))))
}
fn pod(config:&Config, scope:&Scope, name:&str, workspace:&str, image:&str,command:&[String],env:&[(String,String)],timeout:u64)->Value {
    let resources=json!({"cpu":format!("{}m",config.resources.cpu_millis),"memory":config.resources.memory_bytes.to_string(),"ephemeral-storage":config.resources.ephemeral_storage_bytes.to_string()});
    json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":name,"namespace":config.namespace,"finalizers":[FINALIZER],"labels":{"app.kubernetes.io/managed-by":"forge-runner","forge.pdlc.acm-ai.ru/attempt":scope.attempt_id.to_string(),"pdlc.forge-job":scope.job_id.to_string(),"pdlc.forge-attempt":scope.attempt_id.to_string()},"annotations":{"forge.pdlc.acm-ai.ru/intent":"pending","pdlc.workspace-subpath":workspace,"pdlc.workspace-claim-uid":config.workspace_claim_uid}},"spec":{"restartPolicy":"Never","automountServiceAccountToken":false,"serviceAccountName":config.job_service_account,"enableServiceLinks":false,"activeDeadlineSeconds":timeout,"terminationGracePeriodSeconds":20,"securityContext":{"runAsNonRoot":true,"runAsUser":config.security.run_as_user,"runAsGroup":config.security.run_as_group,"fsGroup":config.security.fs_group,"fsGroupChangePolicy":"OnRootMismatch","seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"execution","image":image,"imagePullPolicy":"IfNotPresent","command":command,"workingDir":"/workspace","env":env.iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>(),"resources":{"requests":resources,"limits":resources},"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"workspace","mountPath":"/workspace","subPath":workspace},{"name":"tmp","mountPath":"/tmp"}]}],"volumes":[{"name":"workspace","persistentVolumeClaim":{"claimName":config.workspace_claim}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}}]}})
}
fn terminal(pod:&Value,uid:&str)->anyhow::Result<Option<Terminal>> {
    if !matches!(pod["status"]["phase"].as_str(),Some("Succeeded"|"Failed")){return Ok(None)}
    let statuses=pod["status"]["containerStatuses"].as_array().ok_or(Held)?;
    ensure!(statuses.len()==1 && statuses[0]["name"]=="execution" && statuses[0]["restartCount"]==0,"container identity changed");
    let state=&statuses[0]["state"]["terminated"];
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
    guarded(path)?;ensure!(path.is_dir(),"custody directory required");Ok(())
}
fn read<T:serde::de::DeserializeOwned>(path:&Path)->anyhow::Result<T>{guarded(path)?;let bytes=fs::read(path)?;ensure!(bytes.len()<=64*1024,"custody exceeds bound");Ok(serde_json::from_slice(&bytes)?)}
fn write_once(path:&Path,bytes:&[u8])->anyhow::Result<()> {
    if path.exists(){guarded(path)?;ensure!(fs::read(path)?==bytes,"retained custody differs");return Ok(())}
    let mut opts=OpenOptions::new();opts.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
    let mut file=opts.open(path)?;file.write_all(bytes)?;file.sync_all()?;fs::File::open(path.parent().context("parent missing")?)?.sync_all()?;Ok(())
}
fn hash(bytes:&[u8])->String{use sha2::{Digest,Sha256};format!("{:x}",Sha256::digest(bytes))}

#[cfg(test)]
mod tests {
    use super::*;
    fn config()->Config{Config{schema_version:1,installation_sha256:"d".repeat(64),cluster_ca_sha256:"c".repeat(64),namespace:"app-a".into(),namespace_uid:Uuid::new_v4().to_string(),workspace_claim:"workspaces".into(),workspace_claim_uid:Uuid::new_v4().to_string(),job_service_account:"forge-job".into(),allowed_images:vec![format!("registry.test/job@sha256:{}","a".repeat(64))],builder_image:format!("registry.test/buildkit@sha256:{}","b".repeat(64)),registry_prefix:"registry.test/forge".into(),registry_secret:None,resources:Resources{cpu_millis:500,memory_bytes:256*1024*1024,ephemeral_storage_bytes:1024*1024*1024},security:Security{run_as_user:1000,run_as_group:1000,fs_group:1000}}}
    #[test]fn pod_is_single_attempt_without_controller_or_host_authority(){let c=config();c.validate().unwrap();let s=Scope{job_id:Uuid::new_v4(),attempt_id:Uuid::new_v4(),lease_id:Uuid::new_v4(),generation:2,source_commit:"a".repeat(40)};let p=pod(&c,&s,&s.name(),"attempt-x/workspace",&c.allowed_images[0],&["sh".into(),"-c".into(),"echo done".into()],&[("DECLARED".into(),"value".into())],60);assert_eq!(p["spec"]["restartPolicy"],"Never");assert_eq!(p["spec"]["automountServiceAccountToken"],false);assert_eq!(p["spec"]["containers"][0]["image"],c.allowed_images[0]);assert_eq!(p["spec"]["containers"][0]["volumeMounts"][0]["subPath"],"attempt-x/workspace");assert!(p.to_string().find("hostPath").is_none());assert_eq!(p["metadata"]["finalizers"][0],FINALIZER);}
    #[test]fn terminal_requires_exact_stopped_container_not_phase_or_absence(){let mut p=json!({"status":{"phase":"Succeeded"}});assert!(terminal(&p,"uid").is_err());p["status"]["containerStatuses"]=json!([{"name":"execution","restartCount":0,"state":{"terminated":{"exitCode":0,"finishedAt":"2026-10-22T00:00:00Z","reason":"Completed"}}}]);assert_eq!(terminal(&p,"uid").unwrap().unwrap().exit_code,0);p["status"]["containerStatuses"][0]["restartCount"]=json!(1);assert!(terminal(&p,"uid").is_err());}
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
    for entry in fs::read_dir(control)?.take(4097){let entry=entry?;let name=entry.file_name();if name.to_string_lossy().starts_with(&prefix){guarded(&entry.path())?;if !entry.path().join("cleaned.json").exists(){return Ok(true)}}}
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
