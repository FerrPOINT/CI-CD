//! Forge-local effect admission for a coherent backup. Kernel leases, never expiry, prove quiescence.
//! The installation owner must pin the complete participant set and its runtime identities.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::Write, path::{Path, PathBuf}, sync::OnceLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct Gate { root: PathBuf }
pub struct Lease { _file: Option<File>, record:Option<PathBuf> }
impl Drop for Lease { fn drop(&mut self){if !std::thread::panicking(){if let Some(path)=&self.record{let _=fs::remove_file(path);}}} }
pub struct Participant { _file: File }
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct CustodyRoot { pub kind:String, pub path:PathBuf }
impl CustodyRoot { pub fn workspace(path:PathBuf)->Self{Self{kind:"workspace".into(),path}} pub fn delivery(path:PathBuf)->Self{Self{kind:"local-delivery".into(),path}} pub fn git(path:PathBuf)->Self{Self{kind:"git".into(),path}} }
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Identity { pub schema_version:u32, pub participant:String, pub boot_id:Uuid, pub kind:String, pub process_id:u32, pub process_start:String, pub custody_roots:Vec<CustodyRoot> }
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Request { pub schema_version:u32, pub operation_id:Uuid }
pub struct Coordinator { gate:Gate, _owner:File, operation:Uuid }
pub struct Fence { _lease:File }
pub fn target_idle(path:&Path)->anyhow::Result<()> {
    let meta=fs::symlink_metadata(path)?;ensure!(meta.is_file() && !meta.file_type().is_symlink(),"target lock identity unavailable");
    let mut options=OpenOptions::new();options.read(true).write(true);
    #[cfg(unix)] {use std::os::unix::fs::{OpenOptionsExt,MetadataExt};ensure!(meta.uid()==unsafe{libc::geteuid()},"target lock owner differs");options.custom_flags(libc::O_NOFOLLOW|libc::O_CLOEXEC);}
    let f=options.open(path)?;ensure!(lock(&f,true)?,"native target owner still running");Ok(())
}
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
    pub fn git_hook_owner(&self,storage:&str)->anyhow::Result<Lease>{let f=file(&hook_path(self,storage)?)?;ensure!(lock(&f,false)?,"Git callback custody unavailable");Ok(Lease{_file:Some(f),record:None})}
    pub fn require_git_hook_owner(&self,storage:&str)->anyhow::Result<()>{if self.requested()?{let path=hook_path(self,storage)?;ensure!(path.try_exists()?,"no admitted Git owner");let f=file(&path)?;ensure!(!lock(&f,true)?,"no live Git owner");}Ok(())}
    pub fn requested(&self)->anyhow::Result<bool>{match fs::symlink_metadata(self.root.join("request.json")){Ok(_)=>{let r:Request=read(&self.root.join("request.json"))?;ensure!(r.schema_version==1 && !r.operation_id.is_nil(),"invalid drain request");Ok(true)},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(false),Err(e)=>Err(e.into())}}
    pub fn enter(&self,new_work:bool)->anyhow::Result<Lease>{
        if new_work{ensure!(!self.requested()?,"Forge draining: new work closed")}
        let f=file(&self.root.join("effects.lock"))?;ensure!(lock(&f,false)?,"Forge backup fence held");
        // The request can win between the first read and the kernel lease.
        if new_work{ensure!(!self.requested()?,"Forge draining: new work closed")}
        let directory=self.root.join("effects");if !directory.exists(){fs::create_dir(&directory)?;#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&directory,fs::Permissions::from_mode(0o700))?;}}private(&directory,true)?;
        let id=Uuid::new_v4();let record=directory.join(format!("{id}.json"));write_new(&record,&serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"effectId":id,"processId":std::process::id()}))?)?;
        Ok(Lease{_file:Some(f),record:Some(record)})
    }
    pub fn verify_no_effect_records(&self,_fence:&Fence)->anyhow::Result<()>{let directory=self.root.join("effects");if directory.try_exists()?{private(&directory,true)?;ensure!(fs::read_dir(directory)?.next().is_none(),"orphaned/panicked effect custody remains; original-owner reconciliation required");}Ok(())}
    pub fn register(&self,id:&str,kind:&str,custody_roots:Vec<CustodyRoot>)->anyhow::Result<(Participant,Identity)>{
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
pub fn initialize(kind:&str,custody_roots:Vec<CustodyRoot>)->anyhow::Result<()> {
    if let Some(gate)=Gate::from_env()?{let id=std::env::var("CICD_QUIESCENCE_PARTICIPANT").context("configured quiescence requires pinned participant name")?;let (participant,_)=gate.register(&id,kind,custody_roots)?;ensure!(PARTICIPANT.set(participant).is_ok(),"Forge participant already initialized");}Ok(())
}
pub fn enter(new_work:bool)->anyhow::Result<Lease>{match Gate::from_env()?{Some(gate)=>{ensure!(PARTICIPANT.get().is_some(),"Forge writer has no live participant registration");gate.enter(new_work)},None=>Ok(Lease{_file:None,record:None})}}
pub fn accepting()->bool{Gate::from_env().is_ok_and(|gate|gate.is_none_or(|gate|gate.requested().is_ok_and(|r|!r)))}
impl Coordinator {
    pub fn request(gate:Gate,operation:Uuid)->anyhow::Result<Self>{ensure!(!operation.is_nil(),"operation identity required");let owner=file(&gate.root.join("coordinator.lock"))?;ensure!(lock(&owner,true)?,"another coordinator owns release authority");let path=gate.root.join("request.json");if path.exists(){let r:Request=read(&path)?;ensure!(r.schema_version==1 && r.operation_id==operation,"drain operation differs; refusing takeover/release");}else{write_new(&path,&serde_json::to_vec(&Request{schema_version:1,operation_id:operation})?)?;}Ok(Self{gate,_owner:owner,operation})}
    pub fn try_fence(&self)->anyhow::Result<Option<Fence>>{let r:Request=read(&self.gate.root.join("request.json"))?;ensure!(r.operation_id==self.operation,"drain operation changed");let f=file(&self.gate.root.join("effects.lock"))?;Ok(lock(&f,true)?.then_some(Fence{_lease:f}))}
    pub fn gate(&self)->&Gate{&self.gate}
    /// Only the live coordinator, after dropping its fence, can explicitly resume its own operation.
    pub fn resume(self)->anyhow::Result<()> {let r:Request=read(&self.gate.root.join("request.json"))?;ensure!(r.operation_id==self.operation,"foreign drain release rejected");fs::remove_file(self.gate.root.join("request.json"))?;File::open(&self.gate.root)?.sync_all()?;Ok(())}
}


pub fn git_hook_owner(storage:&str)->anyhow::Result<Lease>{
    let Some(gate)=Gate::from_env()? else{return Ok(Lease{_file:None,record:None})};
    gate.git_hook_owner(storage)
}
fn hook_path(gate:&Gate,storage:&str)->anyhow::Result<PathBuf>{
    use sha2::{Digest,Sha256};
    ensure!(!storage.is_empty() && storage.len()<=256,"invalid Git storage identity");
    let dir=gate.root.join("git-hooks");if !dir.exists(){fs::create_dir(&dir)?;#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&dir,fs::Permissions::from_mode(0o700))?;}}private(&dir,true)?;
    Ok(dir.join(format!("{:x}.lock",Sha256::digest(storage.as_bytes()))))
}
pub fn require_git_hook_owner(storage:&str)->anyhow::Result<()> {
    if let Some(gate)=Gate::from_env()?{gate.require_git_hook_owner(storage)?;}Ok(())
}
