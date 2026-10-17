//! Read the existing Tracker lifecycle; never claim, renew, release or dispatch.
use super::{files::sha, probes::TaskExecutionPolicy};
use crate::{
    api::{ApiError, AppState},
    domain::{sdlc_workspace::WorkspaceOperationReceipt, task_delivery::TaskDeliveryFence},
};
use anyhow::ensure;
use chrono::DateTime;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) struct Permit {
    until: Instant,
}

impl Permit {
    pub(super) fn check(&self) -> anyhow::Result<()> {
        ensure!(
            Instant::now() < self.until,
            "Tracker execution lease expired before effect"
        );
        Ok(())
    }
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted: std::collections::BTreeMap<_, _> = object
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(array) => Value::Array(array.into_iter().map(canonical).collect()),
        value => value,
    }
}

fn validate(
    value: &Value,
    policy: &TaskExecutionPolicy,
    original: &WorkspaceOperationReceipt,
    fence: &TaskDeliveryFence,
    reconciliation: bool,
) -> anyhow::Result<Duration> {
    let b = &original.request.binding;
    let task = &value["receipt"]["task"];
    let assignment = &value["receipt"]["assignment"];
    let lease = &value["receipt"]["lease"];
    let routing = &task["routing"];
    let run = &value["receipt"]["binding"];
    ensure!(
        policy.environment == "nonproduction"
            && policy.repository_id == original.request.repository_id
            && policy.tracker_instance_id == b.tracker_instance_id
            && policy.tracker_project_id == b.tracker_project_id
            && b.task_id == b.root_task_id
            && task["task_id"] == json!(b.task_id)
            && task["stage"] == "Deployment"
            && task["status"] == "InProgress"
            && task["runtime"] == "running"
            && value["runtime_state"] == "running"
            && task["cycle"].as_u64().is_some_and(|cycle| cycle <= 3)
            && task["version"]
                .as_i64()
                .is_some_and(|version| (1..=9007199254740991).contains(&version))
            && task.get("waiting_reason") == Some(&Value::Null)
            && task["owner_subject"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            && task["owner_subject"] == assignment["owner_subject"],
        "current single-task DevOps owner required"
    );
    ensure!(
        routing["tracker_instance_id"] == policy.tracker_instance_id
            && routing["project_id"] == json!(policy.tracker_project_id)
            && routing["task_id"] == json!(b.task_id)
            && routing["root_task_id"] == json!(b.task_id)
            && routing["snapshot_id"] == json!(b.routing_snapshot_id)
            && routing["requirement_revision"] == b.requirement_revision
            && routing["policy"]["tracker_instance_id"] == policy.tracker_instance_id
            && routing["policy"]["project_id"] == json!(policy.tracker_project_id)
            && routing["policy"]["routes"]["devops"]["agent_id"] == json!(fence.agent_id)
            && assignment["routing"] == *routing,
        "confirmed task routing changed"
    );
    ensure!(
        assignment["role"] == "devops"
            && assignment["stage"] == "Deployment"
            && assignment["task_id"] == json!(b.task_id)
            && assignment["assignment_id"] == json!(fence.assignment_id)
            && assignment["execution_id"] == json!(fence.execution_id)
            && assignment["agent_id"] == json!(fence.agent_id)
            && assignment["lease_id"] == json!(fence.lease_id)
            && assignment["fencing_token"] == fence.fencing_token
            && assignment["attempt"]
                .as_u64()
                .is_some_and(|attempt| attempt > 0)
            && assignment["assignment_hash"] == fence.assignment_hash
            && assignment["cycle"] == task["cycle"],
        "current assignment differs from delivery fence"
    );
    let mut hashed = assignment.clone();
    hashed
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("assignment absent"))?
        .remove("assignment_hash");
    ensure!(
        sha(&serde_json::to_vec(&canonical(hashed))?) == fence.assignment_hash,
        "assignment hash differs from accepted input"
    );
    ensure!(
        lease["lease_id"] == json!(fence.lease_id)
            && lease["version"].as_i64().is_some_and(|version| {
                if !(1..=9007199254740991).contains(&version) {
                    false
                } else if reconciliation {
                    version >= fence.expected_lease_version
                } else {
                    version == fence.expected_lease_version
                }
            })
            && lease["holder_subject"] == policy.holder_subject
            && run["machine_subject"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
            && run["machine_subject"] != policy.holder_subject
            && run["machine_subject"] != task["owner_subject"]
            && ["run_id", "chat_id"].iter().all(|key| run[*key]
                .as_str()
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .is_some_and(|id| !id.is_nil())),
        "current holder, run or lease version differs"
    );
    let time = |v: &Value| -> anyhow::Result<_> {
        Ok(DateTime::parse_from_rfc3339(
            v.as_str()
                .ok_or_else(|| anyhow::anyhow!("timestamp absent"))?,
        )?)
    };
    let observed = time(&value["observed_at"])?;
    let expires = time(&lease["expires_at"])?;
    let heartbeat = time(&lease["heartbeat_at"])?;
    let claimed = time(&lease["claimed_at"])?;
    ensure!(
        claimed <= heartbeat
            && heartbeat <= observed
            && observed < expires
            && (expires - heartbeat).num_milliseconds() <= 30_000,
        "expired or inconsistent execution lease"
    );
    Ok((expires - observed).to_std()?)
}

fn validate_source(
    result: &Value,
    original: &WorkspaceOperationReceipt,
    fence: &TaskDeliveryFence,
    cycle: &Value,
) -> anyhow::Result<()> {
    let accepted = &result["receipt"];
    ensure!(
        result["source"] == "delivery"
            && accepted["result_id"] == json!(fence.development_result_id)
            && accepted["task_id"] == json!(original.request.binding.task_id)
            && accepted["cycle"] == *cycle
            && accepted["output"]["role"] == "developer"
            && accepted["output"]["commit_sha"] == original.request.source_commit
            && accepted["next_stage"] == "Review",
        "accepted Developer result does not bind this build"
    );
    match original.request.role {
        crate::domain::sdlc_workspace::WorkspaceRole::Developer => ensure!(
            accepted["assignment_id"] == json!(original.request.binding.assignment_id),
            "build assignment changed"
        ),
        crate::domain::sdlc_workspace::WorkspaceRole::Devops => ensure!(
            original.request.binding.assignment_id == fence.assignment_id
                && original.request.binding.execution_id == fence.execution_id
                && original.request.binding.assignment_hash == fence.assignment_hash
                && original.request.binding.fencing_token == fence.fencing_token,
            "DevOps source context changed"
        ),
        _ => anyhow::bail!("only Developer build or DevOps read-only source context allowed"),
    }
    Ok(())
}

async fn get(client: &reqwest::Client, url: reqwest::Url, token: &str) -> Result<Value, ApiError> {
    let unavailable =
        || ApiError::service_unavailable("Readback жизненного цикла Tracker недоступен");
    let mut response = client
        .get(url)
        .bearer_auth(token)
        .header("accept-encoding", "identity")
        .header("cache-control", "no-store")
        .send()
        .await
        .map_err(|_| unavailable())?;
    if response.status() != reqwest::StatusCode::OK
        || response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v != "identity")
        || response.content_length().is_some_and(|n| n > 262_144)
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len().saturating_add(chunk.len()) > 262_144 {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| unavailable())
}

/// Existing server-only reader. Returns the request start time, not a renewed receipt clock.
pub(crate) async fn read_lifecycle(
    config: &crate::config::SdlcWorkspaceConfig,
    task: uuid::Uuid,
) -> Result<(Instant, Value), ApiError> {
    read_task(config, task, "lifecycle").await
}

pub(crate) async fn read_source_binding(
    config: &crate::config::SdlcWorkspaceConfig,
    task: uuid::Uuid,
) -> Result<(Instant, Value), ApiError> {
    read_task(config, task, "source-binding").await
}

async fn read_task(
    config: &crate::config::SdlcWorkspaceConfig,
    task: uuid::Uuid,
    resource: &str,
) -> Result<(Instant, Value), ApiError> {
    let unavailable = || ApiError::service_unavailable("Tracker lifecycle недоступен");
    let mut url = reqwest::Url::parse(config.tracker_read_url.as_deref().ok_or_else(unavailable)?)
        .map_err(|_| unavailable())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(unavailable());
    }
    let token = config
        .tracker_read_token
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(unavailable)?;
    url.set_path(&format!("/api/v1/issues/{task}/sdlc/{resource}"));
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| unavailable())?;
    let start = Instant::now();
    Ok((start, get(&client, url, token).await?))
}

pub(super) async fn read(
    state: &AppState,
    policy: &TaskExecutionPolicy,
    original: &WorkspaceOperationReceipt,
    fence: &TaskDeliveryFence,
    reconciliation: bool,
) -> Result<Permit, ApiError> {
    let unavailable =
        || ApiError::service_unavailable("Readback жизненного цикла Tracker недоступен");
    let config = &state.config.sdlc_workspace;
    let raw = config.tracker_read_url.as_deref().ok_or_else(unavailable)?;
    let token = config
        .tracker_read_token
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(unavailable)?;
    let mut url = reqwest::Url::parse(raw).map_err(|_| unavailable())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(unavailable());
    }
    url.set_path(&format!(
        "/api/v1/issues/{}/sdlc/lifecycle",
        original.request.binding.task_id
    ));
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| unavailable())?;
    let mut result_url = url.clone();
    result_url.set_path(&format!(
        "{}/results/{}",
        url.path(),
        fence.development_result_id
    ));
    let accepted = get(&client, result_url, token).await?;
    let started = Instant::now();
    let value = get(&client, url, token).await?;
    let remaining = validate(&value, policy, original, fence, reconciliation).map_err(|_| {
        ApiError::conflict("Текущий DevOps assignment или lease Tracker не совпадает")
    })?;
    validate_source(
        &accepted,
        original,
        fence,
        &value["receipt"]["task"]["cycle"],
    )
    .map_err(|_| {
        ApiError::conflict("Принятый Developer commit не совпадает с исходниками pipeline")
    })?;
    // Include all transport/readback time; never renew an old receipt using completion time.
    let permit = Permit {
        until: started + remaining,
    };
    permit
        .check()
        .map_err(|_| ApiError::conflict("Lease Tracker истёк во время readback"))?;
    Ok(permit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::sdlc_workspace::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn fixture() -> (
        Value,
        TaskExecutionPolicy,
        WorkspaceOperationReceipt,
        TaskDeliveryFence,
    ) {
        let task = Uuid::new_v4();
        let agent = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let policy = TaskExecutionPolicy {
            environment: "nonproduction".into(),
            environment_id: Uuid::new_v4(),
            repository_id: Uuid::new_v4(),
            tracker_instance_id: "tracker-qa".into(),
            tracker_project_id: Uuid::new_v4(),
            holder_subject: "fleet-delivery".into(),
        };
        let mut fence = TaskDeliveryFence {
            development_result_id: Uuid::new_v4(),
            assignment_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            agent_id: agent,
            lease_id: Uuid::new_v4(),
            fencing_token: 2,
            expected_lease_version: 3,
            assignment_hash: String::new(),
        };
        let original = WorkspaceOperationReceipt {
            schema: "forge/workspace-operation-receipt/v1".into(),
            operation_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            request_hash: "a".repeat(64),
            recorded_at: Utc::now(),
            status: WorkspaceOperationStatus::Blocked,
            dispatch_allowed: false,
            physical_source_observed: true,
            blockers: vec![
                WorkspaceOperationBlocker::TrackerAdmissionUnavailable,
                WorkspaceOperationBlocker::TrackerWorkspaceBindingUnavailable,
            ],
            request: WorkspaceOperationRequest {
                contract_version: 1,
                operation_key: "prepare".into(),
                repository_id: policy.repository_id,
                source_commit: "b".repeat(40),
                lease_id: Uuid::new_v4(),
                attempt_id: attempt,
                workspace_generation: 1,
                workspace_id: format!("attempt-{attempt}-1-{}", Uuid::new_v4().simple()),
                role: WorkspaceRole::Developer,
                access: WorkspaceAccess::ReadWrite,
                binding: WorkspaceTaskBinding {
                    tracker_instance_id: policy.tracker_instance_id.clone(),
                    tracker_project_id: policy.tracker_project_id,
                    task_id: task,
                    root_task_id: task,
                    assignment_id: Uuid::new_v4(),
                    execution_id: Uuid::new_v4(),
                    routing_snapshot_id: Uuid::new_v4(),
                    requirement_revision: 1,
                    fencing_token: 1,
                    assignment_hash: "c".repeat(64),
                    workflow_task_ref: "SDLC-1".into(),
                },
            },
        };
        let now = Utc::now();
        let route = json!({"agent_id":agent,"fleet_config_revision":1,"package_commit":"a".repeat(40),
            "package_manifest_sha256":"b".repeat(64),"namespace_id":"1","namespace_name":"QA",
            "workflow_id":"1","workflow_key":"hermes-sdlc:devops","profile":"hermes-sdlc-operations",
            "workflow_catalog_version":4,"workflow_catalog_sha256":"d".repeat(64)});
        let routes: serde_json::Map<String, Value> = [
            "project_manager",
            "analyst",
            "architect",
            "developer",
            "reviewer",
            "tester",
            "devops",
        ]
        .into_iter()
        .map(|key| (key.into(), route.clone()))
        .collect();
        let routing = json!({"contract_version":1,"snapshot_id":original.request.binding.routing_snapshot_id,
            "tracker_instance_id":policy.tracker_instance_id,"project_id":policy.tracker_project_id,
            "task_id":task,"root_task_id":task,"confirmation_id":Uuid::new_v4(),"requirement_revision":1,
            "content_hash":"e".repeat(64),"created_at":now,"policy":{"contract_version":1,
                "tracker_instance_id":policy.tracker_instance_id,"project_id":policy.tracker_project_id,
                "version":1,"routing_hash":sha(&serde_json::to_vec(&canonical(json!(routes))).unwrap()),
                "routes":routes,"verification":"declared","native_ready":false,"dispatch_allowed":false,
                "author_subject":"owner","created_at":now}});
        let mut assignment = json!({"assignment_id":fence.assignment_id,"execution_id":fence.execution_id,
            "task_id":task,"owner_subject":"owner","role":"devops","stage":"Deployment","cycle":0,
            "attempt":1,"agent_id":agent,"routing":routing,"previous_result_id":Uuid::new_v4(),
            "fencing_token":fence.fencing_token,"lease_id":fence.lease_id});
        fence.assignment_hash = sha(&serde_json::to_vec(&canonical(assignment.clone())).unwrap());
        assignment["assignment_hash"] = json!(fence.assignment_hash);
        let value = json!({"observed_at":now,"runtime_state":"running","receipt":{"task":{"task_id":task,"owner_subject":"owner",
            "version":1,"stage":"Deployment","status":"InProgress","cycle":0,"transient_retries":0,
            "waiting_reason":null,"runtime":"running","routing":routing,"previous_result_id":assignment["previous_result_id"]},
            "assignment":assignment,"lease":{"lease_id":fence.lease_id,"version":3,"holder_subject":policy.holder_subject,
                "claimed_at":now,"heartbeat_at":now,"expires_at":now+chrono::Duration::seconds(30)},
            "binding":{"chat_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"machine_subject":"devops-agent",
                "dispatch_receipt":{"reference":"fleet:original","sha256":"f".repeat(64)}},"result":null}});
        (value, policy, original, fence)
    }

    #[test]
    fn current_devops_uses_tracker_lease_not_old_build_lease() {
        let (value, policy, original, fence) = fixture();
        assert_ne!(original.request.lease_id, fence.lease_id);
        assert_ne!(original.request.binding.assignment_id, fence.assignment_id);
        assert_eq!(
            validate(&value, &policy, &original, &fence, false).unwrap(),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn stale_foreign_unknown_and_human_hold_never_authorize_effect() {
        let (value, policy, original, fence) = fixture();
        for (pointer, replacement) in [
            ("/runtime_state", json!("unknown")),
            ("/receipt/task/stage", json!("Testing")),
            ("/receipt/task/runtime", json!("unknown")),
            ("/receipt/task/waiting_reason", json!("owner_stop")),
            ("/receipt/task/owner_subject", json!("foreign")),
            ("/receipt/task/cycle", json!(null)),
            ("/receipt/assignment/role", json!("developer")),
            ("/receipt/assignment/assignment_id", json!(Uuid::new_v4())),
            ("/receipt/lease/holder_subject", json!("foreign")),
            ("/receipt/lease/version", json!(2)),
            ("/receipt/lease/expires_at", value["observed_at"].clone()),
            ("/receipt/binding/run_id", json!(Uuid::nil())),
            ("/receipt/task/routing/requirement_revision", json!(2)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate(&changed, &policy, &original, &fence, false).is_err(),
                "{pointer}"
            );
        }
        let mut foreign = policy.clone();
        foreign.repository_id = Uuid::new_v4();
        assert!(validate(&value, &foreign, &original, &fence, false).is_err());
        let mut child = original.clone();
        child.request.binding.root_task_id = Uuid::new_v4();
        assert!(validate(&value, &policy, &child, &fence, false).is_err());
    }

    #[test]
    fn heartbeat_cas_advance_can_only_reconcile_original_effect() {
        let (mut value, policy, original, fence) = fixture();
        value["receipt"]["lease"]["version"] = json!(4);
        assert!(validate(&value, &policy, &original, &fence, false).is_err());
        assert!(validate(&value, &policy, &original, &fence, true).is_ok());
        value["receipt"]["lease"]["lease_id"] = json!(Uuid::new_v4());
        assert!(validate(&value, &policy, &original, &fence, true).is_err());
    }

    #[test]
    fn expired_permit_and_assignment_tampering_are_closed() {
        let (mut value, policy, original, fence) = fixture();
        value["receipt"]["assignment"]["attempt"] = json!(2);
        assert!(validate(&value, &policy, &original, &fence, false).is_err());
        assert!(
            Permit {
                until: Instant::now()
            }
            .check()
            .is_err()
        );
        assert!(
            Permit {
                until: Instant::now() - Duration::from_secs(1)
            }
            .check()
            .is_err()
        );
    }

    fn state(url: String) -> AppState {
        let mut config = crate::config::RuntimeConfig::test_default();
        config.sdlc_workspace.tracker_read_url = Some(url);
        config.sdlc_workspace.tracker_read_token =
            Some("fixture-reader-not-a-production-token".into());
        AppState {
            pool: None,
            namespace_admission_pool: None,
            auth_secret: None,
            git: config.git.to_git_config(),
            config,
            running_jobs: None,
            rate_limiter: std::sync::Arc::new(crate::rate_limit::RateLimiter::default()),
        }
    }

    fn accepted(original: &WorkspaceOperationReceipt, fence: &TaskDeliveryFence) -> Value {
        json!({"source":"delivery","receipt":{"result_id":fence.development_result_id,
            "assignment_id":original.request.binding.assignment_id,"task_id":original.request.binding.task_id,
            "cycle":0,"output":{"role":"developer","commit_sha":original.request.source_commit},"next_stage":"Review"}})
    }

    #[test]
    fn accepted_developer_result_binds_exact_task_cycle_assignment_and_commit() {
        let (_, _, original, fence) = fixture();
        let good = accepted(&original, &fence);
        validate_source(&good, &original, &fence, &json!(0)).unwrap();
        for (path, value) in [
            ("/receipt/task_id", json!(Uuid::new_v4())),
            ("/receipt/assignment_id", json!(Uuid::new_v4())),
            ("/receipt/cycle", json!(1)),
            ("/receipt/output/commit_sha", json!("c".repeat(40))),
            ("/receipt/output/role", json!("tester")),
        ] {
            let mut bad = good.clone();
            *bad.pointer_mut(path).unwrap() = value;
            assert!(
                validate_source(&bad, &original, &fence, &json!(0)).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn configured_reader_credential_is_not_formatted_in_runtime_debug() {
        assert!(
            !format!("{:?}", state("http://localhost:7711".into()).config)
                .contains("fixture-reader-not-a-production-token")
        );
    }

    #[tokio::test]
    async fn existing_lifecycle_transport_is_get_only_bounded_and_redacts_upstream_errors() {
        use axum::{Router, http::HeaderMap, response::IntoResponse, routing::get};
        for (status, body) in [
            (200, None),
            (403, Some(b"private-sql-token-path".to_vec())),
            (200, Some(vec![b'x'; 262_145])),
            (200, Some(b"{malformed private-token".to_vec())),
        ] {
            let (value, policy, original, fence) = fixture();
            let valid = status == 200 && body.is_none();
            let body = body.unwrap_or_else(|| serde_json::to_vec(&value).unwrap());
            let result_body = serde_json::to_vec(&accepted(&original, &fence)).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observed = hits.clone();
            let app =
                Router::new().fallback(get(move |headers: HeaderMap, uri: axum::http::Uri| {
                    let body = if valid && uri.path().contains("/results/") {
                        result_body.clone()
                    } else {
                        body.clone()
                    };
                    let hits = observed.clone();
                    async move {
                        assert_eq!(
                            headers["authorization"],
                            "Bearer fixture-reader-not-a-production-token"
                        );
                        assert_eq!(headers["accept-encoding"], "identity");
                        hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        axum::http::Response::builder()
                            .status(status)
                            .body(axum::body::Body::from(body))
                            .unwrap()
                    }
                }));
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let result = read(&state(url), &policy, &original, &fence, false).await;
            server.abort();
            assert_eq!(
                hits.load(std::sync::atomic::Ordering::SeqCst),
                if valid { 2 } else { 1 }
            );
            if valid {
                assert!(result.is_ok());
                continue;
            }
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("unsafe upstream response accepted"),
            };
            let bytes = axum::body::to_bytes(error.into_response().into_body(), 4096)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("private"));
            assert!(!String::from_utf8_lossy(&bytes).contains("fixture-reader"));
        }
    }

    #[tokio::test]
    async fn readback_transport_time_cannot_extend_a_short_execution_lease() {
        use axum::{Json, Router, routing::get};
        let (mut value, policy, original, fence) = fixture();
        let now = Utc::now();
        value["observed_at"] = json!(now);
        value["receipt"]["lease"]["expires_at"] = json!(now + chrono::Duration::milliseconds(20));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let accepted = accepted(&original, &fence);
        let app = Router::new().fallback(get(move |uri: axum::http::Uri| {
            let value = value.clone();
            let accepted = accepted.clone();
            async move {
                if uri.path().contains("/results/") {
                    return Json(accepted);
                }
                tokio::time::sleep(Duration::from_millis(60)).await;
                Json(value)
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(
            read(&state(url), &policy, &original, &fence, false)
                .await
                .is_err()
        );
        server.abort();
    }
}
