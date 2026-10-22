//! Forge-local effect admission for a coherent backup. Kernel leases, never expiry, prove quiescence.
//! The installation owner must pin the complete participant set and its runtime identities.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::Write, path::{Path, PathBuf}, sync::OnceLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct Gate { root: PathBuf }
pub struct Lease { _file: Option<File> }
pub struct Participant { _file: File }
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Identity { pub schema_version:u32, pub participant:String, pub boot_id:Uuid, pub kind:String, pub process_id:u32, pub process_start:String, pub custody_roots:Vec<PathBuf> }
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Request { pub schema_version:u32, pub operation_id:Uuid }
pub struct Coordinator { gate:Gate, _owner:File, operation:Uuid }
pub struct Fence { _lease:File }
static PARTICIPANT:OnceLock<Participant>=OnceLock::new();

fn safe_name(s:&str)->bool { !s.is_empty() && s.len()<=80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) }
#[cfg(unix)]
fn private(path:&Path, directory:bool)->anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m=fs::symlink_metadata(path)?;
    ensure!(!m.file_type().is_symlink() && if directory{m.is_dir()}else{m.is_file()},"quiescence path type/link rejected");
    ensure!(m.uid()==unsafe{libc::geteuid()} && m.mode()&0o077==0,"quiescence path must be owner-only");Ok(())
}
#[cfg(not(unix))]
fn private(_: &Path,_:bool)->anyhow::Result<()> { anyhow::bail!("quiescence requires Unix kernel custody") }
fn file(path:&Path)->anyhow::Result<File>{
    let mut options=OpenOptions::new();options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt;options.mode(0o600).custom_flags(libc::O_NOFOLLOW|libc::O_CLOEXEC); }
    let f=options.open(path)?;private(path,false)?;Ok(f)
}
fn lock(file:&File,exclusive:bool)->anyhow::Result<bool>{
    #[cfg(unix)] { use std::os::fd::AsRawFd;let mode=if exclusive{libc::LOCK_EX}else{libc::LOCK_SH};
        if unsafe{libc::flock(file.as_raw_fd(),mode|libc::LOCK_NB)}==0{return Ok(true)}
        let error=std::io::Error::last_os_error();if error.kind()==std::io::ErrorKind::WouldBlock{return Ok(false)}return Err(error.into()); }
    #[cfg(not(unix))] { let _=(file,exclusive);anyhow::bail!("quiescence requires flock") }
}
fn write_new(path:&Path,bytes:&[u8])->anyhow::Result<()> {
    let mut options=OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600).custom_flags(libc::O_NOFOLLOW);}
    let mut f=options.open(path)?;f.write_all(bytes)?;f.sync_all()?;File::open(path.parent().context("parent missing")?)?.sync_all()?;Ok(())
}
fn read<T:serde::de::DeserializeOwned>(path:&Path)->anyhow::Result<T>{private(path,false)?;ensure!(fs::metadata(path)?.len()<=32768,"quiescence record oversized");Ok(serde_json::from_slice(&fs::read(path)?)?)}
impl Gate {
    pub fn open(root:&Path)->anyhow::Result<Self>{
        ensure!(root.is_absolute(),"absolute quiescence directory required");
        for parent in root.ancestors(){if parent.exists(){ensure!(!fs::symlink_metadata(parent)?.file_type().is_symlink(),"linked quiescence ancestor");}}
        fs::create_dir_all(root)?;
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;if fs::read_dir(root)?.next().is_none(){fs::set_permissions(root,fs::Permissions::from_mode(0o700))?;}}
        private(root,true)?;let peers=root.join("participants");if !peers.exists(){fs::create_dir(&peers)?;#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&peers,fs::Permissions::from_mode(0o700))?;}}private(&peers,true)?;
        Ok(Self{root:root.canonicalize()?})
    }
    pub fn from_env()->anyhow::Result<Option<Self>>{std::env::var_os("CICD_QUIESCENCE_DIR").map(|p|Self::open(Path::new(&p))).transpose()}
    pub fn requested(&self)->anyhow::Result<bool>{match fs::symlink_metadata(self.root.join("request.json")){Ok(_)=>{let r:Request=read(&self.root.join("request.json"))?;ensure!(r.schema_version==1 && !r.operation_id.is_nil(),"invalid drain request");Ok(true)},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(false),Err(e)=>Err(e.into())}}
    pub fn enter(&self,new_work:bool)->anyhow::Result<Lease>{
        if new_work{ensure!(!self.requested()?,"Forge draining: new work closed")}
        let f=file(&self.root.join("effects.lock"))?;ensure!(lock(&f,false)?,"Forge backup fence held");
        // The request can win between the first read and the kernel lease.
        if new_work{ensure!(!self.requested()?,"Forge draining: new work closed")}
        Ok(Lease{_file:Some(f)})
    }
    pub fn register(&self,id:&str,kind:&str,custody_roots:Vec<PathBuf>)->anyhow::Result<(Participant,Identity)>{
        ensure!(safe_name(id) && safe_name(kind),"invalid participant identity");
        let live=file(&self.root.join("participants").join(format!("{id}.lock")))?;ensure!(lock(&live,true)?,"duplicate live Forge participant");
        let stat=fs::read_to_string("/proc/self/stat")?;let start=stat.rsplit_once(") ").context("process identity unavailable")?.1.split_whitespace().nth(19).context("process start unavailable")?.to_owned();
        let identity=Identity{schema_version:1,participant:id.into(),boot_id:Uuid::new_v4(),kind:kind.into(),process_id:std::process::id(),process_start:start,custody_roots};
        let path=self.root.join("participants").join(format!("{id}.json"));if path.exists(){private(&path,false)?;fs::remove_file(&path)?;}write_new(&path,&serde_json::to_vec(&identity)?)?;
        Ok((Participant{_file:live},identity))
    }
    pub fn verify_participants(&self,expected:&[Identity])->anyhow::Result<()> {
        ensure!(!expected.is_empty() && expected.len()<=64,"complete bounded participant manifest required");
        let mut names=std::collections::BTreeSet::new();
        for identity in expected{ensure!(identity.schema_version==1 && safe_name(&identity.participant) && names.insert(identity.participant.clone()),"invalid/duplicate expected participant");let stem=self.root.join("participants").join(&identity.participant);let actual:Identity=read(&stem.with_extension("json"))?;ensure!(&actual==identity,"participant boot identity changed");let live=file(&stem.with_extension("lock"))?;ensure!(!lock(&live,true)?,"participant registration is stale; no live kernel owner");}
        let mut count=0;for entry in fs::read_dir(self.root.join("participants"))?{let entry=entry?;count+=1;ensure!(count<=128,"participant inventory overflow");let p=entry.path();if p.extension().is_some_and(|v|v=="lock"){let id=p.file_stem().and_then(|v|v.to_str()).context("invalid participant file")?;let live=file(&p)?;if !lock(&live,true)?{ensure!(names.contains(id),"unexpected live Forge writer participant");}}}Ok(())
    }
}
pub fn initialize(kind:&str,custody_roots:Vec<PathBuf>)->anyhow::Result<()> {
    if let Some(gate)=Gate::from_env()?{let id=std::env::var("CICD_QUIESCENCE_PARTICIPANT").context("configured quiescence requires pinned participant name")?;let (participant,_)=gate.register(&id,kind,custody_roots)?;ensure!(PARTICIPANT.set(participant).is_ok(),"Forge participant already initialized");}Ok(())
}
pub fn enter(new_work:bool)->anyhow::Result<Lease>{match Gate::from_env()?{Some(gate)=>{ensure!(PARTICIPANT.get().is_some(),"Forge writer has no live participant registration");gate.enter(new_work)},None=>Ok(Lease{_file:None})}}
pub fn accepting()->bool{Gate::from_env().is_ok_and(|gate|gate.is_none_or(|gate|gate.requested().is_ok_and(|r|!r)))}
impl Coordinator {
    pub fn request(gate:Gate,operation:Uuid)->anyhow::Result<Self>{ensure!(!operation.is_nil(),"operation identity required");let owner=file(&gate.root.join("coordinator.lock"))?;ensure!(lock(&owner,true)?,"another coordinator owns release authority");let path=gate.root.join("request.json");if path.exists(){let r:Request=read(&path)?;ensure!(r.schema_version==1 && r.operation_id==operation,"drain operation differs; refusing takeover/release");}else{write_new(&path,&serde_json::to_vec(&Request{schema_version:1,operation_id:operation})?)?;}Ok(Self{gate,_owner:owner,operation})}
    pub fn try_fence(&self)->anyhow::Result<Option<Fence>>{let r:Request=read(&self.gate.root.join("request.json"))?;ensure!(r.operation_id==self.operation,"drain operation changed");let f=file(&self.gate.root.join("effects.lock"))?;Ok(lock(&f,true)?.then_some(Fence{_lease:f}))}
    pub fn gate(&self)->&Gate{&self.gate}
    /// Only the live coordinator, after dropping its fence, can explicitly resume its own operation.
    pub fn resume(self)->anyhow::Result<()> {let r:Request=read(&self.gate.root.join("request.json"))?;ensure!(r.operation_id==self.operation,"foreign drain release rejected");fs::remove_file(self.gate.root.join("request.json"))?;File::open(&self.gate.root)?.sync_all()?;Ok(())}
}

/// Admit every request before auth/cache DB effects; only exact existing-execution continuations survive drain.
pub async fn middleware(request:axum::extract::Request,next:axum::middleware::Next)->axum::response::Response{
    use axum::response::IntoResponse;
    let path=request.uri().path();
    if matches!(path,"/health"|"/api/v1/health"|"/api/v1/readiness"|"/metrics"){return next.run(request).await}
    let continuation=continuation(path);
    let Ok(_lease)=enter(!continuation) else{return (axum::http::StatusCode::SERVICE_UNAVAILABLE,"Forge приостановлен для резервного копирования").into_response()};
    next.run(request).await
}
fn continuation(path:&str)->bool{
    if path=="/api/v1/runner/heartbeat"{return true}
    let parts:Vec<_>=path.trim_start_matches('/').split('/').collect();
    if parts.len()>=6 && parts[..4]==["api","v1","runner","leases"] && Uuid::parse_str(parts[4]).is_ok(){
        return matches!(parts[5..], ["ack"]|["renew"]|["control"]|["receipt"]|["logs"]|["secrets:resolve"]|["complete"]|["artifact-sessions"]) || (parts.len()==8 && parts[5]=="artifact-sessions" && Uuid::parse_str(parts[6]).is_ok() && matches!(parts[7],"chunks"|"complete"|"abort"));
    }false
}

#[cfg(all(test,unix))]
mod tests{
    use super::*;
    fn fixture()->(PathBuf,Gate){let path=std::env::temp_dir().join(format!("forge-quiescence-{}",Uuid::new_v4()));let gate=Gate::open(&path).unwrap();(path,gate)}
    #[test]fn drain_closes_new_effects_but_continuations_and_cleanup_finish_before_fence(){let(path,gate)=fixture();let job=gate.enter(true).unwrap();let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();assert!(gate.enter(true).is_err());let cleanup=gate.enter(false).unwrap();assert!(coordinator.try_fence().unwrap().is_none());drop(job);assert!(coordinator.try_fence().unwrap().is_none());drop(cleanup);let fence=coordinator.try_fence().unwrap().unwrap();assert!(gate.enter(false).is_err());drop(fence);coordinator.resume().unwrap();assert!(gate.enter(true).is_ok());fs::remove_dir_all(path).unwrap();}
    #[test]fn missing_stale_restarted_and_unexpected_participants_hold(){let(path,gate)=fixture();let(participant,identity)=gate.register("server","server",vec![]).unwrap();assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_ok());let (other,_)=gate.register("other","runner",vec![]).unwrap();assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_err());drop(other);assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_ok());drop(participant);assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_err());let(_replacement,_)=gate.register("server","server",vec![]).unwrap();assert!(gate.verify_participants(&[identity]).is_err());fs::remove_dir_all(path).unwrap();}
    #[test]fn operation_cannot_be_released_by_another_coordinator_and_restart_stays_closed(){let(path,gate)=fixture();let operation=Uuid::new_v4();let coordinator=Coordinator::request(gate.clone(),operation).unwrap();assert!(Coordinator::request(gate.clone(),operation).is_err());drop(coordinator);assert!(gate.enter(true).is_err());assert!(Coordinator::request(gate.clone(),Uuid::new_v4()).is_err());Coordinator::request(gate,operation).unwrap().resume().unwrap();fs::remove_dir_all(path).unwrap();}
    #[test]fn route_classification_keeps_only_existing_execution_continuations(){let id=Uuid::new_v4();for suffix in ["renew","ack","control","complete","logs","secrets:resolve","receipt"]{assert!(continuation(&format!("/api/v1/runner/leases/{id}/{suffix}")));}assert!(!continuation("/api/v1/runner/work:poll"));assert!(!continuation("/api/v1/pipelines"));assert!(!continuation("/api/v1/runner/leases/not-a-uuid/complete"));}
}

/// Must run only while the coordinator holds the exclusive effect fence. Reads are not stop proofs.
pub async fn verify_idle(_fence:&Fence,pool:&sqlx::PgPool,expected:&[Identity])->anyhow::Result<()> {
    ensure!(false, "source quiescence entrypoint coverage is not yet qualified; HOLD");
    let (jobs,attempts,leases,leased_queue,unacked,sync_runs,unknown_refs,uploads):(i64,i64,i64,i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM jobs WHERE status='running'),(SELECT count(*) FROM execution_attempts WHERE status='running'),(SELECT count(*) FROM job_leases WHERE lease_status='active'),(SELECT count(*) FROM job_queue WHERE state='leased'),(SELECT count(*) FROM job_leases WHERE runner_id IS NOT NULL AND completion_received_at IS NULL),(SELECT count(*) FROM repository_sync_runs WHERE state IN ('running','stopping','reconciling')),(SELECT count(*) FROM repository_sync_refs WHERE state='unknown'),(SELECT count(*) FROM artifact_upload_sessions WHERE status='open')").fetch_one(pool).await?;
    ensure!([jobs,attempts,leases,leased_queue,unacked,sync_runs,unknown_refs,uploads].iter().all(|n|*n==0),"issued execution, uncertain completion, sync or artifact custody still present");
    // An aborted SQLx future can leave its already-sent statement running on PostgreSQL.
    // Do not acknowledge until those statements/transactions have actually finished.
    let sql_active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND backend_type='client backend' AND (state IS NULL OR state='active' OR xact_start IS NOT NULL))").fetch_one(pool).await?;
    ensure!(!sql_active,"database statements/transactions remain in flight");
    let mut roots=std::collections::BTreeSet::new();
    for identity in expected{for root in &identity.custody_roots{ensure!(root.path.is_absolute(),"relative custody root rejected");if roots.insert((root.kind.clone(),root.path.clone())){match root.kind.as_str(){"workspace"=>workspace_idle(&root.path)?,"local-delivery"=>delivery_idle(&root.path)?,_=>anyhow::bail!("unknown writer custody kind")}}}}
    Ok(())
}
fn plain(path:&Path,directory:bool)->anyhow::Result<()> {for ancestor in path.ancestors(){ensure!(!fs::symlink_metadata(ancestor)?.file_type().is_symlink(),"linked custody path rejected");}let m=fs::symlink_metadata(path)?;ensure!(if directory{m.is_dir()}else{m.is_file()},"custody type rejected");Ok(())}
fn json(path:&Path)->anyhow::Result<serde_json::Value>{plain(path,false)?;ensure!(fs::metadata(path)?.len()<=1024*1024,"oversized custody record");Ok(serde_json::from_slice(&fs::read(path)?)?)}
fn workspace_idle(root:&Path)->anyhow::Result<()> {
    plain(root,true)?;
    let workspaces=crate::runner_workspace::OwnedWorkspace::inventory(root)?;
    ensure!(workspaces.iter().all(|w|w.acknowledged),"unacknowledged physical attempt workspace");
    ensure!(crate::execution::kubernetes::inventory(root)?.is_empty(),"Kubernetes effect/workspace cleanup custody remains");
    let control=root.join(".compose-control");
    if control.try_exists()?{plain(&control,true)?;let mut count=0;for entry in fs::read_dir(control)?{count+=1;ensure!(count<=4096,"Compose custody inventory overflow");let path=entry?.path();plain(&path,true)?;if path.file_name().and_then(|p|p.to_str()).is_some_and(|p|p.starts_with("image-build-")){crate::runner_image_build::quiescence_idle(&path)?;}else{let journal=json(&path.join("journal.json"))?;ensure!(journal["phase"]=="cleaned" && journal["stopped"]==true,"Compose cleanup/stop custody unresolved");}}}
    Ok(())
}
fn delivery_idle(root:&Path)->anyhow::Result<()> {
    // No initialized target is also no issued delivery. Its existing parent must be visible.
    if !root.try_exists()? {plain(root.parent().context("delivery parent missing")?,true)?;return Ok(())}
    plain(root,true)?;let operations=root.join("operations");if !operations.try_exists()?{ensure!(fs::read_dir(root)?.next().is_none(),"unknown delivery target layout");return Ok(())}plain(&operations,true)?;
    let mut count=0;for entry in fs::read_dir(operations)?{count+=1;ensure!(count<=1024,"delivery custody overflow");let path=entry?.path();plain(&path,true)?;let _intent=json(&path.join("intent.json"))?;
        let reconciled=path.join("reconciled.json");let result=if reconciled.try_exists()?{json(&reconciled)?}else{json(&path.join("result.json"))?};
        let status=result["status"].as_str().context("delivery terminal status missing")?;
        ensure!(matches!(status,"verified"|"unavailable"|"rejected"|"failed"|"rolled_back"),"unknown delivery operation outcome");
    }Ok(())
}
