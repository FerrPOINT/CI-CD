//! Forge product-local admission and backup closure.
#[path="quiescence/kernel.rs"]
mod kernel;
pub use kernel::*;
use anyhow::{Context, ensure};
use std::{fs, path::{Path,PathBuf}};
use uuid::Uuid;
/// Admit every request before auth/cache DB effects; only exact existing-execution continuations survive drain.
pub async fn middleware(request:axum::extract::Request,next:axum::middleware::Next)->axum::response::Response{
    guarded_request(request,next,enter).await
}
async fn guarded_request(request:axum::extract::Request,next:axum::middleware::Next,acquire:impl FnOnce(bool)->anyhow::Result<Lease>)->axum::response::Response{
    use axum::response::IntoResponse;
    let path=request.uri().path();
    if matches!(path,"/health"|"/api/v1/health"|"/api/v1/readiness"|"/metrics"){return next.run(request).await}
    let continuation=continuation(path) || git_fetch_continuation(&request);
    let Ok(lease)=acquire(!continuation) else{return (axum::http::StatusCode::SERVICE_UNAVAILABLE,"Forge приостановлен для резервного копирования").into_response()};
    // Client cancellation drops only this waiter. The actual handler retains its effect lease.
    match tokio::spawn(async move {let _lease=lease;next.run(request).await}).await {Ok(response)=>response,Err(_)=>(axum::http::StatusCode::SERVICE_UNAVAILABLE,"Forge: результат операции не подтверждён").into_response()}
}

fn git_fetch_continuation(request:&axum::extract::Request)->bool{
    use axum::http::{Method,header};
    if request.headers().get(header::AUTHORIZATION).and_then(|v|v.to_str().ok()).is_none_or(|v|!v.starts_with("Bearer ") && !v.starts_with("Basic ")){return false}
    let parts:Vec<_>=request.uri().path().trim_start_matches('/').split('/').collect();
    let info=matches!(parts.as_slice(),["git",repo,"info","refs"] if !repo.is_empty()) || matches!(parts.as_slice(),["git",group,repo,"info","refs"] if !group.is_empty() && !repo.is_empty());
    if request.method()==Method::GET && info{return request.uri().query()==Some("service=git-upload-pack")}
    let rpc=matches!(parts.as_slice(),["git",repo,"git-upload-pack"] if !repo.is_empty()) || matches!(parts.as_slice(),["git",group,repo,"git-upload-pack"] if !group.is_empty() && !repo.is_empty());
    request.method()==Method::POST && rpc && request.uri().query().is_none() && request.headers().get(header::CONTENT_TYPE).and_then(|v|v.to_str().ok())==Some("application/x-git-upload-pack-request")
}
fn continuation(path:&str)->bool{
    if matches!(path,"/api/v1/runner/heartbeat"|"/api/v1/internal/git-push"){return true}
    let parts:Vec<_>=path.trim_start_matches('/').split('/').collect();
    if parts.len()>=6 && parts[..4]==["api","v1","runner","leases"] && Uuid::parse_str(parts[4]).is_ok(){
        return matches!(parts[5..], ["ack"]|["renew"]|["control"]|["receipt"]|["logs"]|["secrets:resolve"]|["complete"]|["artifact-sessions"]) || (parts.len()==8 && parts[5]=="artifact-sessions" && Uuid::parse_str(parts[6]).is_ok() && matches!(parts[7],"chunks"|"complete"|"abort"));
    }
    if matches!(parts.as_slice(),["api","v1","projects",project,"pipelines",pipeline,"credential-renewals"] if Uuid::parse_str(project).is_ok() && Uuid::parse_str(pipeline).is_ok()) {return true}
    if matches!(parts.as_slice(),["api","v1","projects",project,"pipelines",pipeline,"credential-renewals",key] if Uuid::parse_str(project).is_ok() && Uuid::parse_str(pipeline).is_ok() && !key.is_empty() && key.len()<=128) {return true}
    false
}

#[cfg(all(test,unix))]
mod tests{
    use super::*;
    fn fixture()->(PathBuf,Gate){let path=std::env::temp_dir().join(format!("forge-quiescence-{}",Uuid::new_v4()));let gate=Gate::open(&path).unwrap();(path,gate)}
    #[test]fn drain_closes_new_effects_but_continuations_and_cleanup_finish_before_fence(){let(path,gate)=fixture();let job=gate.enter(true).unwrap();let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();assert!(gate.enter(true).is_err());let cleanup=gate.enter(false).unwrap();assert!(coordinator.try_fence().unwrap().is_none());drop(job);assert!(coordinator.try_fence().unwrap().is_none());drop(cleanup);let fence=coordinator.try_fence().unwrap().unwrap();assert!(gate.enter(false).is_err());drop(fence);coordinator.resume().unwrap();assert!(gate.enter(true).is_ok());fs::remove_dir_all(path).unwrap();}
    #[test]fn missing_stale_restarted_and_unexpected_participants_hold(){let(path,gate)=fixture();let(participant,identity)=gate.register("server","server",vec![]).unwrap();assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_ok());let (other,_)=gate.register("other","runner",vec![]).unwrap();assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_err());drop(other);assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_ok());drop(participant);assert!(gate.verify_participants(std::slice::from_ref(&identity)).is_err());let(_replacement,_)=gate.register("server","server",vec![]).unwrap();assert!(gate.verify_participants(&[identity]).is_err());fs::remove_dir_all(path).unwrap();}
    #[test]fn operation_cannot_be_released_by_another_coordinator_and_restart_stays_closed(){let(path,gate)=fixture();let operation=Uuid::new_v4();let coordinator=Coordinator::request(gate.clone(),operation).unwrap();assert!(Coordinator::request(gate.clone(),operation).is_err());drop(coordinator);assert!(gate.enter(true).is_err());assert!(Coordinator::request(gate.clone(),Uuid::new_v4()).is_err());Coordinator::request(gate,operation).unwrap().resume().unwrap();fs::remove_dir_all(path).unwrap();}
    #[test]fn route_classification_keeps_only_existing_execution_continuations(){let id=Uuid::new_v4();for suffix in ["renew","ack","control","complete","logs","secrets:resolve","receipt"]{assert!(continuation(&format!("/api/v1/runner/leases/{id}/{suffix}")));}assert!(!continuation("/api/v1/runner/work:poll"));assert!(!continuation("/api/v1/pipelines"));assert!(!continuation("/api/v1/runner/leases/not-a-uuid/complete"));}

    #[tokio::test]async fn http_boundary_closes_submissions_and_poll_but_allows_lease_completion(){
        use axum::{Router,body::Body,http::{Request,StatusCode}};use tower::ServiceExt;
        let(path,gate)=fixture();let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();
        let app=Router::new().fallback(||async{StatusCode::NO_CONTENT}).layer(axum::middleware::from_fn(move|request,next|{let gate=gate.clone();async move{guarded_request(request,next,|new|gate.enter(new)).await}}));
        for path in ["/api/v1/pipelines","/api/v1/runner/work:poll","/api/v1/projects","/git/repo/git-receive-pack"]{
            let response=app.clone().oneshot(Request::builder().method("POST").uri(path).body(Body::empty()).unwrap()).await.unwrap();assert_eq!(response.status(),StatusCode::SERVICE_UNAVAILABLE);
        }
        for path in ["/health".to_owned(),format!("/api/v1/runner/leases/{}/complete",Uuid::new_v4())]{
            let response=app.clone().oneshot(Request::builder().method("POST").uri(path).body(Body::empty()).unwrap()).await.unwrap();assert_eq!(response.status(),StatusCode::NO_CONTENT);
        }
        coordinator.resume().unwrap();let response=app.oneshot(Request::builder().method("POST").uri("/api/v1/pipelines").body(Body::empty()).unwrap()).await.unwrap();assert_eq!(response.status(),StatusCode::NO_CONTENT);fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]async fn canceled_waiter_does_not_release_actual_blocking_writer(){
        let(path,gate)=fixture();let effect=gate.enter(true).unwrap();let(started,ready)=tokio::sync::oneshot::channel();let(release,held)=std::sync::mpsc::channel();let(done,finished)=tokio::sync::oneshot::channel();
        let task=tokio::spawn(async move{tokio::task::spawn_blocking(move||{let _effect=effect;let _=started.send(());held.recv().unwrap();let _=done.send(());}).await.unwrap()});
        ready.await.unwrap();task.abort();assert!(task.await.unwrap_err().is_cancelled());
        let coordinator=Coordinator::request(gate,Uuid::new_v4()).unwrap();assert!(coordinator.try_fence().unwrap().is_none());release.send(()).unwrap();finished.await.unwrap();
        let mut fence=None;for _ in 0..100{fence=coordinator.try_fence().unwrap();if fence.is_some(){break}tokio::task::yield_now().await;}assert!(fence.is_some());drop(fence);coordinator.resume().unwrap();fs::remove_dir_all(path).unwrap();
    }
    #[test]fn actual_participant_process_is_required_and_its_effect_blocks_the_fence(){
        use std::io::{BufRead,Write};use std::process::{Command,Stdio};
        let(path,gate)=fixture();let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","quiescence::tests::participant_process_fixture","--nocapture"]).env("FORGE_QUIESCENCE_TEST_ROOT",&path).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut output=std::io::BufReader::new(child.stdout.take().unwrap());let identity=loop{let mut line=String::new();assert!(output.read_line(&mut line).unwrap()>0);if let Some(bytes)=line.find("IDENTITY ").map(|offset|line[offset+9..].trim()){break serde_json::from_str::<Identity>(bytes).unwrap()}};
        gate.verify_participants(std::slice::from_ref(&identity)).unwrap();let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();assert!(coordinator.try_fence().unwrap().is_none());
        child.stdin.as_mut().unwrap().write_all(b"finish\n").unwrap();loop{let mut line=String::new();assert!(output.read_line(&mut line).unwrap()>0);if line.trim()=="FINISHED"{break}}
        let fence=coordinator.try_fence().unwrap().unwrap();gate.verify_participants(std::slice::from_ref(&identity)).unwrap();drop(fence);
        child.stdin.as_mut().unwrap().write_all(b"exit\n").unwrap();assert!(child.wait().unwrap().success());assert!(gate.verify_participants(&[identity]).is_err());coordinator.resume().unwrap();fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]async fn authenticated_fetch_continues_until_held_fence_without_receive_pack_bypass(){
        use axum::{Router,body::Body,http::{Request,StatusCode}};use tower::ServiceExt;use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        let(path,gate)=fixture();let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();let calls=Arc::new(AtomicUsize::new(0));let observed=calls.clone();let admission=gate.clone();
        let app=Router::new().fallback(move||{let observed=observed.clone();async move{
            // Exercise actual native IO only after the production admission boundary.
            assert!(tokio::process::Command::new("git").arg("--version").status().await.unwrap().success());observed.fetch_add(1,Ordering::SeqCst);StatusCode::NO_CONTENT
        }}).layer(axum::middleware::from_fn(move|request,next|{let gate=admission.clone();async move{guarded_request(request,next,|new|gate.enter(new)).await}}));
        let request=|method:&str,uri:&str|Request::builder().method(method).uri(uri).header("authorization","Bearer fixture-token").header("content-type","application/x-git-upload-pack-request").body(Body::empty()).unwrap();
        for uri in ["/git/repo/info/refs?service=git-upload-pack","/git/group/repo/info/refs?service=git-upload-pack"]{assert_eq!(app.clone().oneshot(request("GET",uri)).await.unwrap().status(),StatusCode::NO_CONTENT);}
        assert_eq!(app.clone().oneshot(request("POST","/git/group/repo/git-upload-pack")).await.unwrap().status(),StatusCode::NO_CONTENT);
        for(method,uri)in[("GET","/git/repo/info/refs?service=git-receive-pack"),("POST","/git/repo/git-receive-pack"),("POST","/git/repo/info/refs?service=git-upload-pack"),("GET","/git/repo/git-upload-pack"),("GET","/git/repo/info/refs?service=git-upload-pack&alias=yes"),("POST","/git/repo/git-upload-pack?alias=yes")]{
            assert_eq!(app.clone().oneshot(request(method,uri)).await.unwrap().status(),StatusCode::SERVICE_UNAVAILABLE);
        }
        let missing_auth=Request::builder().uri("/git/repo/info/refs?service=git-upload-pack").body(Body::empty()).unwrap();assert_eq!(app.clone().oneshot(missing_auth).await.unwrap().status(),StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(calls.load(Ordering::SeqCst),3);
        let fence=coordinator.try_fence().unwrap().unwrap();
        assert_eq!(app.clone().oneshot(request("GET","/git/repo/info/refs?service=git-upload-pack")).await.unwrap().status(),StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(app.oneshot(request("POST","/git/repo/git-upload-pack")).await.unwrap().status(),StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(calls.load(Ordering::SeqCst),3);drop(fence);coordinator.resume().unwrap();fs::remove_dir_all(path).unwrap();
    }
    #[test]fn private_exit_rejects_wrong_operation_stale_holder_and_replacement(){
        let(path,gate)=fixture();let(participant,identity)=gate.register("server","server",vec![]).unwrap();let expected=std::slice::from_ref(&identity);let operation=Uuid::new_v4();let owner=Coordinator::request(gate.clone(),operation).unwrap();let fence=owner.try_fence().unwrap().unwrap();
        assert!(owner.request_exit(&fence,expected).is_err());owner.publish_held(&fence,expected).unwrap();owner.request_exit(&fence,expected).unwrap();assert!(gate.exit_for(&identity).unwrap());
        let exit=path.join("exit.json");let good=fs::read(&exit).unwrap();let mut wrong:serde_json::Value=serde_json::from_slice(&good).unwrap();wrong["operationId"]=serde_json::json!(Uuid::new_v4());fs::write(&exit,serde_json::to_vec(&wrong).unwrap()).unwrap();assert!(gate.exit_for(&identity).is_err());fs::write(&exit,&good).unwrap();
        drop(fence);assert!(gate.exit_for(&identity).is_err());drop(owner);
        let owner=Coordinator::request(gate.clone(),operation).unwrap();let fence=owner.try_fence().unwrap().unwrap();assert!(gate.exit_for(&identity).is_err());assert!(owner.request_exit(&fence,expected).is_err());
        drop(participant);assert!(gate.all_exited(expected).unwrap());let(_replacement,_)=gate.register("server","server",vec![]).unwrap();assert!(gate.all_exited(expected).is_err());assert!(owner.request_exit(&fence,expected).is_err());drop(fence);drop(owner);fs::remove_dir_all(path).unwrap();
    }
    #[test]fn private_exit_finishes_actual_child_normally_and_retains_closed_fence(){
        use std::io::{BufRead,Write};use std::process::{Child,Command,Stdio};
        struct Fixture(Child);impl Drop for Fixture{fn drop(&mut self){if self.0.try_wait().ok().flatten().is_none(){let _=self.0.kill();let _=self.0.wait();}}}
        let(path,gate)=fixture();let mut child=Fixture(Command::new(std::env::current_exe().unwrap()).args(["--exact","quiescence::tests::controlled_exit_process_fixture","--nocapture"]).env("FORGE_QUIESCENCE_EXIT_FIXTURE","1").env("CICD_QUIESCENCE_DIR",&path).env("CICD_QUIESCENCE_PARTICIPANT","server").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap());
        let mut output=std::io::BufReader::new(child.0.stdout.take().unwrap());let identity=loop{let mut line=String::new();assert!(output.read_line(&mut line).unwrap()>0);if let Some(bytes)=line.find("IDENTITY ").map(|offset|line[offset+9..].trim()){break serde_json::from_str::<Identity>(bytes).unwrap()}};
        let operation=Uuid::new_v4();let owner=Coordinator::request(gate.clone(),operation).unwrap();let fence=owner.try_fence().unwrap().unwrap();let expected=std::slice::from_ref(&identity);owner.publish_held(&fence,expected).unwrap();owner.request_exit(&fence,expected).unwrap();
        // Ask the child to inspect the genuine kernel fence and exact private intent.
        child.0.stdin.as_mut().unwrap().write_all(b"inspect\n").unwrap();
        let mut stopped=false;for _ in 0..100{if let Some(status)=child.0.try_wait().unwrap(){assert!(status.success());stopped=true;break}std::thread::sleep(std::time::Duration::from_millis(20));}assert!(stopped,"controlled fixture did not return normally");
        assert!(gate.all_exited(expected).unwrap());assert!(gate.enter(true).is_err());assert!(gate.enter(false).is_err());drop(fence);owner.resume().unwrap();fs::remove_dir_all(path).unwrap();
    }
    #[test]fn controlled_exit_process_fixture(){
        use std::io::Write;
        if std::env::var_os("FORGE_QUIESCENCE_EXIT_FIXTURE").is_none(){return}
        initialize("server",vec![]).unwrap();let root=PathBuf::from(std::env::var_os("CICD_QUIESCENCE_DIR").unwrap());let identity:Identity=serde_json::from_slice(&fs::read(root.join("participants/server.json")).unwrap()).unwrap();
        println!("IDENTITY {}",serde_json::to_string(&identity).unwrap());std::io::stdout().flush().unwrap();let mut line=String::new();std::io::stdin().read_line(&mut line).unwrap();assert_eq!(line.trim(),"inspect");
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(wait_for_exit()).unwrap();assert!(enter(false).is_err());
    }
    #[test]fn admitted_git_callback_finishes_but_unrelated_hook_cannot_submit_during_drain(){
        let(path,gate)=fixture();let owner=gate.git_hook_owner("original-storage").unwrap();let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();assert!(gate.require_git_hook_owner("original-storage").is_ok());assert!(gate.require_git_hook_owner("other-storage").is_err());drop(owner);assert!(gate.require_git_hook_owner("original-storage").is_err());coordinator.resume().unwrap();fs::remove_dir_all(path).unwrap();
    }
    #[test]fn panic_retains_effect_record_and_prevents_false_restart_ack(){
        let(path,gate)=fixture();{let _completed=gate.enter(true).unwrap();}let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();let fence=coordinator.try_fence().unwrap().unwrap();assert!(gate.verify_no_effect_records(&fence).is_ok());drop(fence);coordinator.resume().unwrap();
        let child_gate=gate.clone();assert!(std::thread::spawn(move||{let _effect=child_gate.enter(true).unwrap();panic!("fixture owner panic");}).join().is_err());let coordinator=Coordinator::request(gate.clone(),Uuid::new_v4()).unwrap();let fence=coordinator.try_fence().unwrap().unwrap();assert!(gate.verify_no_effect_records(&fence).is_err());drop(fence);drop(coordinator);fs::remove_dir_all(path).unwrap();
    }
    #[test]fn participant_process_fixture(){
        use std::io::Write;
        let Some(root)=std::env::var_os("FORGE_QUIESCENCE_TEST_ROOT") else{return};let gate=Gate::open(Path::new(&root)).unwrap();let(_participant,identity)=gate.register("server","server",vec![]).unwrap();let effect=gate.enter(true).unwrap();println!("IDENTITY {}",serde_json::to_string(&identity).unwrap());std::io::stdout().flush().unwrap();
        let mut line=String::new();std::io::stdin().read_line(&mut line).unwrap();assert_eq!(line.trim(),"finish");drop(effect);println!("FINISHED");std::io::stdout().flush().unwrap();line.clear();std::io::stdin().read_line(&mut line).unwrap();assert_eq!(line.trim(),"exit");
    }
    #[test]fn unknown_workspace_and_compose_custody_hold_even_when_no_process_lease_exists(){
        let root=std::env::temp_dir().join(format!("forge-quiescence-custody-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let workspace=crate::runner_workspace::OwnedWorkspace::create(&root,Uuid::new_v4(),Uuid::new_v4(),1).unwrap();workspace.create_empty_checkout().unwrap();assert!(workspace_idle(&root).is_err());workspace.record_completion("failed").unwrap();workspace.acknowledge_completion("failed").unwrap();assert!(workspace_idle(&root).is_ok());
        let control=root.join(".compose-control").join("unknown-owned-effect");fs::create_dir_all(&control).unwrap();fs::write(control.join("journal.json"),br#"{"phase":"cleanup-required","stopped":false}"#).unwrap();assert!(workspace_idle(&root).is_err());fs::write(control.join("journal.json"),br#"{"phase":"cleaned","stopped":true}"#).unwrap();assert!(workspace_idle(&root).is_err());fs::remove_dir_all(control).unwrap();assert!(workspace_idle(&root).is_ok());fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature="integration")]
    #[tokio::test]async fn actual_postgres_running_work_and_unfinished_statement_prevent_backup_ack(){
        use sqlx::{Connection,postgres::{PgConnectOptions,PgPoolOptions}};use std::str::FromStr;
        let url=std::env::var("CICD_TEST_DATABASE_URL").expect("CICD_TEST_DATABASE_URL required");
        let admin=PgPoolOptions::new().max_connections(1).connect(&url).await.unwrap();let schema=format!("it_forge_quiescence_{}",Uuid::new_v4().simple());sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        let options=PgConnectOptions::from_str(&url).unwrap().options([("search_path",schema.as_str())]);let pool=PgPoolOptions::new().max_connections(1).connect_with(options).await.unwrap();crate::migrations().await.unwrap().run(&pool).await.unwrap();
        let(path,gate)=fixture();let(_participant,identity)=gate.register("server","server",vec![]).unwrap();let coordinator=Coordinator::request(gate,Uuid::new_v4()).unwrap();let fence=coordinator.try_fence().unwrap().unwrap();
        assert!(verify_idle(&fence,&pool,std::slice::from_ref(&identity)).await.is_ok());
        let project=Uuid::new_v4();let pipeline=Uuid::new_v4();let stage=Uuid::new_v4();let job=Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,'quiescence-fixture','https://example.invalid/source.git')").bind(project).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','running')").bind(pipeline).bind(project).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO stages(id,pipeline_id,name,position,status) VALUES($1,$2,'fixture',0,'running')").bind(stage).bind(pipeline).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,stage_id,name,image,command,position,status) VALUES($1,$2,'fixture','unused','true',0,'running')").bind(job).bind(stage).execute(&pool).await.unwrap();
        assert!(verify_idle(&fence,&pool,std::slice::from_ref(&identity)).await.is_err());
        sqlx::query("UPDATE jobs SET status='success',finished_at=now() WHERE id=$1").bind(job).execute(&pool).await.unwrap();
        assert!(verify_idle(&fence,&pool,std::slice::from_ref(&identity)).await.is_ok());
        let mut other=admin.acquire().await.unwrap();let tx=other.begin().await.unwrap();assert!(verify_idle(&fence,&pool,std::slice::from_ref(&identity)).await.is_err());tx.rollback().await.unwrap();drop(other);
        assert!(verify_idle(&fence,&pool,&[identity]).await.is_ok());drop(fence);coordinator.resume().unwrap();pool.close().await;sqlx::query(&format!("DROP SCHEMA {schema} CASCADE")).execute(&admin).await.unwrap();admin.close().await;fs::remove_dir_all(path).unwrap();
    }

}

/// Must run only while the coordinator holds the exclusive effect fence. Reads are not stop proofs.
pub async fn verify_idle(_fence:&Fence,pool:&sqlx::PgPool,expected:&[Identity])->anyhow::Result<()> {
    let (jobs,attempts,leases,leased_queue,unacked,sync_runs,unknown_refs,uploads):(i64,i64,i64,i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM jobs WHERE status='running'),(SELECT count(*) FROM execution_attempts WHERE status='running'),(SELECT count(*) FROM job_leases WHERE lease_status='active'),(SELECT count(*) FROM job_queue WHERE state='leased'),(SELECT count(*) FROM job_leases WHERE runner_id IS NOT NULL AND completion_received_at IS NULL),(SELECT count(*) FROM repository_sync_runs WHERE state IN ('running','stopping','reconciling')),(SELECT count(*) FROM repository_sync_refs WHERE state='unknown'),(SELECT count(*) FROM artifact_upload_sessions WHERE status='open')").fetch_one(pool).await?;
    ensure!([jobs,attempts,leases,leased_queue,unacked,sync_runs,unknown_refs,uploads].iter().all(|n|*n==0),"issued execution, uncertain completion, sync or artifact custody still present");
    // An aborted SQLx future can leave its already-sent statement running on PostgreSQL.
    // Do not acknowledge until those statements/transactions have actually finished.
    let sql_active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND backend_type='client backend' AND (state IS NULL OR state='active' OR xact_start IS NOT NULL))").fetch_one(pool).await?;
    ensure!(!sql_active,"database statements/transactions remain in flight");
    let mut roots=std::collections::BTreeSet::new();
    for identity in expected{for root in &identity.custody_roots{ensure!(root.path.is_absolute(),"relative custody root rejected");if roots.insert((root.kind.clone(),root.path.clone())){match root.kind.as_str(){"workspace"=>workspace_idle(&root.path)?,"local-delivery"=>delivery_idle(&root.path)?,"git"=>{plain(&root.path,true)?;let repositories:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM repository_catalog ORDER BY id LIMIT 4097").fetch_all(pool).await?;ensure!(repositories.len()<=4096,"Git custody inventory overflow");for repository in repositories{let guard=cicd_infra::sync_git::MutationGuard::try_acquire(&root.path,repository).map_err(|_|anyhow::anyhow!("Git mutation custody unavailable"))?;ensure!(guard.is_some(),"native Git process still retains its mutation guard");}},_=>anyhow::bail!("unknown writer custody kind")}}}}
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
    if control.try_exists()?{plain(&control,true)?;let mut count=0;for entry in fs::read_dir(control)?{count+=1;ensure!(count<=4096,"Compose custody inventory overflow");let path=entry?.path();plain(&path,true)?;if path.file_name().and_then(|p|p.to_str()).is_some_and(|p|p.starts_with("image-build-")){crate::runner_image_build::quiescence_idle(&path)?;}else{let journal=json(&path.join("journal.json"))?;let manifest_path=path.join("compose.json");let manifest=json(&manifest_path)?;let labels=&manifest["services"]["transfer"]["labels"];
    let attempt=labels["sdlc.attempt"].as_str().and_then(|v|Uuid::parse_str(v).ok()).context("Compose attempt identity missing")?;let job=labels["sdlc.job"].as_str().and_then(|v|Uuid::parse_str(v).ok()).context("Compose job identity missing")?;let project=format!("sdlc-build-job-{}",attempt.simple());
    let daemon=std::env::var("CICD_RUNNER_DOCKER_DAEMON_ID").context("Compose daemon identity unavailable")?;let volume=std::env::var("CICD_RUNNER_WORKSPACE_VOLUME").context("Compose workspace volume identity unavailable")?;
    ensure!(!daemon.is_empty() && !volume.is_empty() && journal["project"]==project && journal["daemon_id"]==daemon && journal["manifest"].as_str()==manifest_path.to_str() && manifest["name"]==project && path.file_name().and_then(|v|v.to_str())==Some(project.as_str()) && labels["sdlc.control-volume"]==volume && labels["sdlc.task"]==format!("job-{job}") && labels["sdlc.purpose"]=="cicd-job","Compose terminal custody scope mismatch");
    ensure!(journal["phase"]=="cleaned" && journal["stopped"]==true,"Compose cleanup/stop custody unresolved");}}}
    Ok(())
}
fn delivery_idle(root:&Path)->anyhow::Result<()> {
    // No initialized target is also no issued delivery. Its existing parent must be visible.
    if !root.try_exists()? {plain(root.parent().context("delivery parent missing")?,true)?;return Ok(())}
    plain(root,true)?;let operations=root.join("operations");if !operations.try_exists()?{ensure!(fs::read_dir(root)?.next().is_none(),"unknown delivery target layout");return Ok(())}plain(&operations,true)?;
    target_idle(&root.join(".lock"))?;
    let owner=json(&root.join("owner.json"))?;ensure!(matches!(owner["schema"].as_str(),Some("forge/local-static-target/v1"|"forge/local-oci-target/v1"|"forge/isolated-pg-owner/v1")),"unknown delivery owner schema");
    let mut count=0;for entry in fs::read_dir(operations)?{count+=1;ensure!(count<=1024,"delivery custody overflow");let path=entry?.path();plain(&path,true)?;let intent=json(&path.join("intent.json"))?;
        let reconciled=path.join("reconciled.json");let result=if reconciled.try_exists()?{json(&reconciled)?}else{json(&path.join("result.json"))?};
        ensure!(result["commandSha256"].as_str().is_some_and(|v|v.len()==64) && result["commandSha256"]==intent["receipt"]["commandSha256"] && result["schema"]==intent["receipt"]["schema"],"delivery terminal receipt identity differs from original intent");
        let status=result["status"].as_str().context("delivery terminal status missing")?;
        ensure!(matches!(status,"verified"|"unavailable"|"failed"),"unknown delivery operation outcome");
    }Ok(())
}
