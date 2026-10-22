//! Explicit owner-authorized adoption. Native CID/namespace retirement is verified by Base, not JSON.
use super::*;
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct EvidenceFile {pub path:PathBuf,pub sha256:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RetiredProcess {pub participant:String,pub boot_id:Uuid,pub container_id:String,pub image_config_digest:String,pub process_id:u32,pub process_start:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct DestinationIdentity {pub installation_sha256:String,pub namespace:String,pub namespace_uid:Uuid,pub workspace_claim_uid:Uuid}
#[derive(Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RootMapping {pub source:CustodyRoot,pub destination:CustodyRoot}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Enrollment {pub source_participants:Vec<String>,pub participant:String,pub kind:String,pub roots:Vec<RootMapping>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct AdoptionHandoff {
    pub schema_version:u32,pub schema:String,pub operation_id:Uuid,pub generation_id:Uuid,
    pub source_exited_receipt:EvidenceFile,pub retired_processes:Vec<RetiredProcess>,
    pub destination:DestinationIdentity,pub participants:Vec<Enrollment>,
    pub source_docker_daemon_id:Option<String>,pub source_workspace_volume:Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ExitedReceipt {schema:String,operation_id:Uuid,participants:Vec<Identity>,all_pinned_kernel_owners_exited:bool,exclusive_fence_still_held:bool}
#[derive(Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CustodyFile {pub path:PathBuf,pub sha256:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RetiredCompose {
    pub root:PathBuf,pub source_root:PathBuf,pub name:String,pub files:Vec<CustodyFile>,
    pub daemon_id:String,pub workspace_volume:String,
}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct SnapshotFile {path:PathBuf,sha256:String}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Snapshot {files:Vec<SnapshotFile>,directories:Vec<PathBuf>}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Adoption {
    schema_version:u32,operation_id:Uuid,generation_id:Uuid,handoff_sha256:String,
    phase:String,archive_sha256:String,control_sha256:String,root_inode:u64,coordinator_inode:u64,effects_inode:u64,
    retired_compose:Vec<RetiredCompose>,enrolled:Vec<Identity>,
}
fn sha(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn hex(s:&str)->bool{s.len()==64 && s.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))}
fn supported(s:&str)->bool{matches!(s,"server"|"external-runner"|"local-delivery"|"migration")}
fn generation_root(gate:&Gate,generation:Uuid)->PathBuf{gate.root.join("generations").join(generation.to_string())}
fn inode(path:&Path)->anyhow::Result<u64>{#[cfg(unix)]{use std::os::unix::fs::MetadataExt;Ok(fs::symlink_metadata(path)?.ino())}#[cfg(not(unix))]{let _=path;anyhow::bail!("Unix inode identity required")}}
fn directory(path:&Path)->anyhow::Result<()> {
    if !path.try_exists()?{fs::create_dir(path)?;#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(path,fs::Permissions::from_mode(0o700))?;}File::open(path.parent().context("archive parent missing")?)?.sync_all()?;}
    private(path,true)
}
fn relative(path:&Path)->anyhow::Result<()> {
    use std::path::Component;
    ensure!(!path.as_os_str().is_empty() && path.components().all(|c|matches!(c,Component::Normal(_))),"noncanonical archived path");Ok(())
}
fn bounded_bytes(path:&Path)->anyhow::Result<Vec<u8>> {
    private(path,false)?;let before=fs::symlink_metadata(path)?;
    #[cfg(unix)]{use std::os::unix::fs::MetadataExt;ensure!(before.nlink()==1,"linked adoption evidence");}
    ensure!(before.len()<=1024*1024,"adoption evidence exceeds bound");
    let bytes=fs::read(path)?;let after=fs::symlink_metadata(path)?;
    ensure!(inode(path)?=={#[cfg(unix)]{use std::os::unix::fs::MetadataExt;before.ino()}#[cfg(not(unix))]{0}} && before.len()==after.len() && before.modified()?==after.modified()?,"adoption evidence changed while reading");Ok(bytes)
}
fn collect(root:&Path,at:&Path,snapshot:&mut Snapshot,control:bool)->anyhow::Result<()> {
    private(at,true)?;
    for entry in fs::read_dir(at)?{
        ensure!(snapshot.files.len()+snapshot.directories.len()<8192,"adoption inventory overflow");
        let path=entry?.path();let rel=path.strip_prefix(root)?.to_path_buf();relative(&rel)?;
        if control && at==root {
            if matches!(rel.to_str(),Some("adoption.json"|"adoption.next"|"generations")){continue}
            ensure!(matches!(rel.to_str(),Some("participants"|"effects"|"git-hooks"|"request.json"|"coordinator-current.json"|"held.json"|"exit.json"|"coordinator.lock"|"effects.lock")),"unknown control entry; HOLD");
        }
        let meta=fs::symlink_metadata(&path)?;
        ensure!(!meta.file_type().is_symlink(),"linked archived control");
        if meta.is_dir(){snapshot.directories.push(rel);collect(root,&path,snapshot,false)?;}
        else{snapshot.files.push(SnapshotFile{path:rel,sha256:sha(&bounded_bytes(&path)?)});}
    }Ok(())
}
fn snapshot(root:&Path,control:bool)->anyhow::Result<Snapshot>{
    let mut value=Snapshot{files:vec![],directories:vec![]};collect(root,root,&mut value,control)?;
    value.files.sort_by(|a,b|a.path.cmp(&b.path));value.directories.sort();Ok(value)
}
fn snapshot_sha(value:&Snapshot)->anyhow::Result<String>{Ok(sha(&serde_json::to_vec(value)?))}
fn save_exact(path:&Path,bytes:&[u8])->anyhow::Result<()> {
    if path.try_exists()?{ensure!(bounded_bytes(path)?==bytes,"archive/CAS bytes changed; HOLD");}
    else{write_new(path,bytes)?;}Ok(())
}
fn store(gate:&Gate,state:&Adoption)->anyhow::Result<()>{
    let next=gate.root.join("adoption.next");let bytes=serde_json::to_vec(state)?;
    save_exact(&next,&bytes)?;fs::rename(&next,gate.root.join("adoption.json"))?;File::open(&gate.root)?.sync_all()?;Ok(())
}
fn ensure_no_generation_bypass(gate:&Gate)->anyhow::Result<()>{
    if !gate.root.join("adoption.json").try_exists()? && gate.root.join("generations").try_exists()?{
        ensure!(fs::read_dir(gate.root.join("generations"))?.next().is_none(),"archived adoption lacks its journal; HOLD");
    }Ok(())
}
fn load(gate:&Gate)->anyhow::Result<Option<(Adoption,AdoptionHandoff)>>{
    ensure_no_generation_bypass(gate)?;
    let path=gate.root.join("adoption.json");if !path.try_exists()?{ensure!(!gate.root.join("adoption.next").try_exists()?,"unfinished initial adoption CAS; HOLD");return Ok(None)}
    let state:Adoption=read(&path)?;ensure!(state.schema_version==1 && !state.generation_id.is_nil() && !state.operation_id.is_nil() && hex(&state.handoff_sha256)
        && matches!(state.phase.as_str(),"preparing"|"prepared"|"committed"|"active"),"unknown adoption journal state");
    ensure!(inode(&gate.root)?==state.root_inode && inode(&gate.root.join("coordinator.lock"))?==state.coordinator_inode
        && inode(&gate.root.join("effects.lock"))?==state.effects_inode,"adoption root/lock inode changed");
    let archive=generation_root(gate,state.generation_id);private(&archive,true)?;
    for entry in fs::read_dir(&archive)?{ensure!(matches!(entry?.file_name().to_str(),Some("handoff.json"|"original-control"|"source-exited.json"|"inventory.json")),"unknown generation archive state");}
    let bytes=bounded_bytes(&archive.join("handoff.json"))?;ensure!(sha(&bytes)==state.handoff_sha256,"archived handoff changed");
    let handoff:AdoptionHandoff=serde_json::from_slice(&bytes)?;
    ensure!(handoff.operation_id==state.operation_id && handoff.generation_id==state.generation_id,"adoption handoff identity changed");
    if state.phase!="preparing"{
        let control=archive.join("original-control");
        ensure!(snapshot_sha(&snapshot(&control,false)?)?==state.control_sha256,"archived old control changed");
        let archive_bytes=bounded_bytes(&archive.join("inventory.json"))?;
        ensure!(sha(&archive_bytes)==state.archive_sha256,"adoption archive inventory changed");
        let inventory:Snapshot=serde_json::from_slice(&archive_bytes)?;
        ensure!(snapshot_sha(&snapshot(&control,false)?)?==snapshot_sha(&inventory)?,"archive entries differ from sealed inventory");
        ensure!(sha(&bounded_bytes(&archive.join("source-exited.json"))?)==handoff.source_exited_receipt.sha256,"archived source-exited evidence changed");
    }
    Ok(Some((state,handoff)))
}
pub fn load_adoption_handoff(path:&Path,expected_sha:&str)->anyhow::Result<(AdoptionHandoff,Vec<u8>,Vec<Identity>)>{
    ensure!(hex(expected_sha),"handoff requires a lowercase SHA256");
    let bytes=bounded_bytes(path)?;ensure!(sha(&bytes)==expected_sha,"owner handoff hash changed");
    let handoff:AdoptionHandoff=serde_json::from_slice(&bytes)?;
    ensure!(handoff.schema_version==1 && handoff.schema=="forge/quiescence-adoption/v1" && !handoff.operation_id.is_nil() && !handoff.generation_id.is_nil(),"unsupported adoption handoff");
    ensure!(hex(&handoff.destination.installation_sha256) && !handoff.destination.namespace_uid.is_nil() && !handoff.destination.workspace_claim_uid.is_nil()
        && !handoff.destination.namespace.is_empty() && handoff.destination.namespace.len()<=63 && handoff.destination.namespace.bytes().all(|b|b.is_ascii_lowercase() || b.is_ascii_digit() || b==b'-'),"destination binding incomplete");
    ensure!(hex(&handoff.source_exited_receipt.sha256) && handoff.source_exited_receipt.path.is_absolute(),"source-exited receipt reference incomplete");
    let exited_bytes=bounded_bytes(&handoff.source_exited_receipt.path)?;
    ensure!(sha(&exited_bytes)==handoff.source_exited_receipt.sha256,"source-exited receipt hash changed");
    let exited:ExitedReceipt=serde_json::from_slice(&exited_bytes)?;
    ensure!(exited.schema=="forge/quiescence-exited/v1" && exited.operation_id==handoff.operation_id && exited.all_pinned_kernel_owners_exited && exited.exclusive_fence_still_held,"source-exited receipt is not exact original held operation");
    ensure!(!exited.participants.is_empty() && exited.participants.len()<=64 && handoff.retired_processes.len()==exited.participants.len(),"complete retired process catalog required");
    let mut old=BTreeMap::new();
    for identity in &exited.participants{
        ensure!(identity.schema_version==1 && safe_name(&identity.participant) && !identity.boot_id.is_nil() && old.insert(identity.participant.clone(),identity).is_none(),"invalid old participant");
    }
    let mut retired=BTreeSet::new();
    for process in &handoff.retired_processes{
        let identity=old.get(&process.participant).context("foreign retired process")?;
        ensure!(retired.insert(process.participant.clone()) && process.boot_id==identity.boot_id && process.process_id==identity.process_id && process.process_start==identity.process_start
            && hex(&process.container_id) && process.image_config_digest.strip_prefix("sha256:").is_some_and(hex),"retired process does not bind original boot/image/CID");
    }
    ensure!(!handoff.participants.is_empty() && handoff.participants.len()<=64,"bounded actual destination catalog required");
    let mut names=BTreeSet::new();let mut mapped=BTreeSet::new();let mut mapped_roots=BTreeSet::new();let mut destination_roots=BTreeMap::new();
    for target in &handoff.participants{
        ensure!(safe_name(&target.participant) && supported(&target.kind) && names.insert(target.participant.clone()) && !target.source_participants.is_empty(),"invalid destination participant");
        let mut sources=BTreeSet::new();
        for source in &target.source_participants{ensure!(old.contains_key(source) && sources.insert(source),"unknown/duplicate source participant mapping");mapped.insert(source.clone());}
        let mut roots=BTreeSet::new();
        for binding in &target.roots{
            ensure!(binding.source.kind==binding.destination.kind && matches!(binding.source.kind.as_str(),"workspace"|"git"|"local-delivery") && binding.source.path.is_absolute() && binding.destination.path.is_absolute()
                && roots.insert((binding.destination.kind.clone(),binding.destination.path.clone())),"invalid destination root mapping");
            ensure!(sources.iter().any(|id|old[*id].custody_roots.contains(&binding.source)),"root mapping lacks its source owner");
            let key=(binding.destination.kind.clone(),binding.destination.path.clone());
            if let Some(previous)=destination_roots.insert(key,binding.source.clone()){ensure!(previous==binding.source,"destination root has ambiguous source ownership");}
            mapped_roots.insert((binding.source.kind.clone(),binding.source.path.clone()));
        }
    }
    ensure!(mapped==old.keys().cloned().collect(),"old participant omitted from generation mapping");
    ensure!(old.values().flat_map(|i|i.custody_roots.iter()).all(|r|mapped_roots.contains(&(r.kind.clone(),r.path.clone()))),"old custody root omitted");
    Ok((handoff,bytes,exited.participants))
}
pub fn adoption_root_mappings(handoff:&AdoptionHandoff)->Vec<RootMapping>{
    let mut result=vec![];for participant in &handoff.participants{for binding in &participant.roots{if !result.contains(binding){result.push(binding.clone());}}}result
}
impl Coordinator{
    /// Does not replace OLD current-owner/held bytes before the original control archive is durable.
    pub fn for_adoption(gate:Gate,operation:Uuid)->anyhow::Result<Self>{
        let owner=file(&gate.root.join("coordinator.lock"))?;ensure!(lock(&owner,true)?,"another coordinator owns control");
        let request:Request=read(&gate.root.join("request.json"))?;ensure!(request.schema_version==1 && request.operation_id==operation && !operation.is_nil(),"adoption requires original closed operation");
        Ok(Self{gate,_owner:owner,operation,nonce:Uuid::new_v4()})
    }
    pub fn prepare_adoption(&self,fence:&Fence,handoff:&AdoptionHandoff,bytes:&[u8],old:&[Identity],retired_compose:Vec<RetiredCompose>)->anyhow::Result<()>{
        ensure!(handoff.operation_id==self.operation,"foreign adoption operation");
        self.gate.verify_no_effect_records(fence)?;ensure!(self.gate.all_exited(old)?,"old local kernel owners remain");
        let old_snapshot=snapshot(&self.gate.root,true)?;let old_sha=snapshot_sha(&old_snapshot)?;
        let state_path=self.gate.root.join("adoption.json");
        let existing=if state_path.try_exists()?{Some(read::<Adoption>(&state_path)?)}else if self.gate.root.join("adoption.next").try_exists()?{
            let pending:Adoption=read(&self.gate.root.join("adoption.next"))?;
            ensure!(pending.phase=="preparing" && pending.operation_id==self.operation && pending.generation_id==handoff.generation_id && pending.handoff_sha256==sha(bytes) && pending.control_sha256==old_sha,"unknown initial adoption CAS");
            store(&self.gate,&pending)?;Some(pending)
        }else{None};
        let archive=generation_root(&self.gate,handoff.generation_id);
        if let Some(state)=&existing{
            ensure!(state.schema_version==1 && state.operation_id==self.operation && state.generation_id==handoff.generation_id && state.handoff_sha256==sha(bytes) && state.control_sha256==old_sha,"adoption preparation CAS changed");
            ensure!(inode(&self.gate.root)?==state.root_inode && inode(&self.gate.root.join("coordinator.lock"))?==state.coordinator_inode && inode(&self.gate.root.join("effects.lock"))?==state.effects_inode,"adoption lock/root inode changed");
            if state.phase!="preparing"{let _=load(&self.gate)?.context("adoption archive missing")?;return Ok(())}
        }else{
            directory(&self.gate.root.join("generations"))?;ensure!(fs::read_dir(self.gate.root.join("generations"))?.next().is_none(),"another generation archive exists");
            let state=Adoption{schema_version:1,operation_id:self.operation,generation_id:handoff.generation_id,handoff_sha256:sha(bytes),phase:"preparing".into(),archive_sha256:String::new(),control_sha256:old_sha,
                root_inode:inode(&self.gate.root)?,coordinator_inode:inode(&self.gate.root.join("coordinator.lock"))?,effects_inode:inode(&self.gate.root.join("effects.lock"))?,retired_compose,enrolled:vec![]};
            store(&self.gate,&state)?;
        }
        let mut state:Adoption=read(&state_path)?;
        directory(&archive)?;save_exact(&archive.join("handoff.json"),bytes)?;
        for entry in fs::read_dir(&archive)?{let name=entry?.file_name();ensure!(matches!(name.to_str(),Some("handoff.json"|"original-control"|"source-exited.json"|"inventory.json")),"unknown partial archive state; HOLD");}
        let control=archive.join("original-control");directory(&control)?;
        for entry in &old_snapshot.directories{relative(entry)?;}
        for relative_path in &old_snapshot.directories{directory(&control.join(relative_path))?;}
        for entry in &old_snapshot.files{let bytes=bounded_bytes(&self.gate.root.join(&entry.path))?;ensure!(sha(&bytes)==entry.sha256,"old control changed during archive");save_exact(&control.join(&entry.path),&bytes)?;}
        let exited=bounded_bytes(&handoff.source_exited_receipt.path)?;ensure!(sha(&exited)==handoff.source_exited_receipt.sha256,"source-exited evidence changed");save_exact(&archive.join("source-exited.json"),&exited)?;
        ensure!(snapshot_sha(&snapshot(&control,false)?)?==state.control_sha256 && snapshot_sha(&snapshot(&self.gate.root,true)?)?==state.control_sha256,"old control/archive CAS changed");
        let inventory=serde_json::to_vec(&old_snapshot)?;save_exact(&archive.join("inventory.json"),&inventory)?;
        state.archive_sha256=sha(&inventory);state.phase="prepared".into();store(&self.gate,&state)?;Ok(())
    }
    pub fn commit_adoption(&self,fence:&Fence,handoff:&AdoptionHandoff,bytes:&[u8],old:&[Identity])->anyhow::Result<()>{
        self.gate.verify_no_effect_records(fence)?;ensure!(self.gate.all_exited(old)?,"old kernel owner appeared");
        let (mut state,_)=load(&self.gate)?.context("adoption was not prepared")?;
        ensure!(state.operation_id==self.operation && handoff.operation_id==self.operation && state.handoff_sha256==sha(bytes) && state.generation_id==handoff.generation_id && matches!(state.phase.as_str(),"prepared"|"committed"),"adoption commit CAS changed");
        ensure!(snapshot_sha(&snapshot(&self.gate.root,true)?)?==state.control_sha256,"old control changed after prepare");
        state.phase="committed".into();store(&self.gate,&state)?;Ok(())
    }
    pub fn resume_enrolled(self,fence:&Fence,expected:&[Identity])->anyhow::Result<()>{
        if let Some((mut state,handoff))=load(&self.gate)?{
            ensure!(state.operation_id==self.operation && matches!(state.phase.as_str(),"committed"|"active"),"generation is not committed for release");
            self.gate.verify_participants(expected)?;self.gate.verify_no_effect_records(fence)?;
            ensure!(expected.len()==handoff.participants.len(),"destination participant catalog is incomplete");
            for identity in expected{validate_registration(&self.gate,&identity.participant,&identity.kind,&identity.custody_roots)?;}
            state.enrolled=expected.to_vec();state.phase="active".into();
            // Authorization is durable before the last admission marker is removed.
            store(&self.gate,&state)?;
        }
        self.remove_request()
    }
    pub fn verify_adoption_seals(&self)->anyhow::Result<()>{
        if let Some((state,_))=load(&self.gate)?{
            for entry in &state.retired_compose{
                ensure!(custody_files(&entry.root.join(".compose-control").join(&entry.name))?==entry.files,"retired custody seal changed");
            }
        }Ok(())
    }
}
pub(super) fn standalone_resume(gate:&Gate)->anyhow::Result<()>{
    ensure!(load(gate)?.is_none(),"adopted generation release requires a live held enrollment; HOLD");Ok(())
}
pub(super) fn validate_registration(gate:&Gate,id:&str,kind:&str,roots:&[CustodyRoot])->anyhow::Result<()>{
    if let Some((state,handoff))=load(gate)?{
        ensure!(!gate.root.join("adoption.next").try_exists()?,"adoption CAS pending; HOLD");
        ensure!(matches!(state.phase.as_str(),"committed"|"active"),"generation preparation not committed");
        ensure!(std::env::var("CICD_QUIESCENCE_GENERATION").ok().and_then(|v|Uuid::parse_str(&v).ok())==Some(state.generation_id),"destination generation env differs");
        let target=handoff.participants.iter().find(|p|p.participant==id).context("unapproved destination participant")?;
        let wanted:Vec<_>=target.roots.iter().map(|r|r.destination.clone()).collect();
        ensure!(target.kind==kind && wanted==roots,"destination participant roots/kind changed");
    }else{ensure!(std::env::var_os("CICD_QUIESCENCE_GENERATION").is_none(),"generation enrollment lacks its preserved adoption state");}Ok(())
}
pub(super) fn admission(gate:&Gate,identity:Option<&Identity>)->anyhow::Result<()>{
    if let Some((state,_))=load(gate)?{
        let identity=identity.context("adopted writer lacks exact enrolled identity")?;
        ensure!(state.phase=="active" && state.enrolled.contains(identity),"destination boot not released by owner");
        validate_registration(gate,&identity.participant,&identity.kind,&identity.custody_roots)?;
    }Ok(())
}
pub async fn wait_for_enrollment()->anyhow::Result<()>{
    loop{
        let Some(gate)=Gate::from_env()? else{return Ok(())};
        if load(&gate)?.is_none(){ensure!(std::env::var_os("CICD_QUIESCENCE_GENERATION").is_none(),"generation enrollment state missing");return Ok(())}
        if admission(&gate,IDENTITY.get()).is_ok() && !gate.requested()?{return Ok(())}
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
pub fn retired_compose_binding(root:&Path,name:&str)->anyhow::Result<Option<RetiredCompose>>{
    let Some(gate)=Gate::from_env()? else{return Ok(None)};
    let Some((state,_))=load(&gate)? else{return Ok(None)};
    ensure!(matches!(state.phase.as_str(),"committed"|"active"),"adoption not committed");
    let selected=state.retired_compose.into_iter().find(|entry|entry.root==root && entry.name==name);
    if let Some(entry)=&selected{
        let actual=custody_files(&root.join(".compose-control").join(name))?;
        ensure!(actual==entry.files,"retired Compose custody changed");
    }Ok(selected)
}
pub fn custody_files(root:&Path)->anyhow::Result<Vec<CustodyFile>>{
    Ok(snapshot(root,false)?.files.into_iter().map(|f|CustodyFile{path:f.path,sha256:f.sha256}).collect())
}
