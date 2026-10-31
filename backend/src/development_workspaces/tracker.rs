//! Manual sessions read task metadata through the existing registered namespace reader.
use crate::api::ApiError;
use pdlc_shared::resource_context::{NamespaceRef, TaskCatalogItem};
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

fn unavailable() -> ApiError {
    ApiError::service_unavailable("workspace_tracker_reader_unavailable")
}

pub(super) async fn read_task(
    namespace: &NamespaceRef,
    tracker_instance: Uuid,
    project: Uuid,
    identifier: &str,
) -> Result<(Uuid, String), ApiError> {
    let registered = |name| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<Uuid>().ok())
            .filter(|v| !v.is_nil())
    };
    if registered("CICD_NAMESPACE__TRACKER_INSTANCE_ID") != Some(tracker_instance)
        || registered("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
            != Some(namespace.registry_instance_id)
    {
        return Err(unavailable());
    }
    let raw = std::env::var("CICD_NAMESPACE__TRACKER_URL").map_err(|_| unavailable())?;
    let base = Url::parse(&raw).map_err(|_| unavailable())?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.path() != "/"
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(unavailable());
    }
    let path = std::env::var("CICD_NAMESPACE__TRACKER_TOKEN_FILE").map_err(|_| unavailable())?;
    let token = tokio::fs::read_to_string(path)
        .await
        .map_err(|_| unavailable())?;
    if token.trim().is_empty() || token.len() > 65536 {
        return Err(unavailable());
    }
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| unavailable())?;
    tokio::time::timeout(
        Duration::from_secs(3),
        resolve(
            &client,
            &base,
            token.trim(),
            namespace,
            tracker_instance,
            project,
            identifier,
        ),
    )
    .await
    .map_err(|_| unavailable())?
}

async fn get(client: &Client, url: Url, token: &str) -> Result<Value, ApiError> {
    let mut response = client
        .get(url)
        .bearer_auth(token)
        .header("accept-encoding", "identity")
        .header("cache-control", "no-store")
        .send()
        .await
        .map_err(|_| unavailable())?;
    if matches!(
        response.status(),
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ) {
        return Err(ApiError::bad_request("workspace_task_not_in_project"));
    }
    if response.status() != StatusCode::OK
        || response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v != "identity")
        || response.content_length().is_some_and(|v| v > 262144)
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len().saturating_add(chunk.len()) > 262144 {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| unavailable())
}

async fn resolve(
    client: &Client,
    base: &Url,
    token: &str,
    namespace: &NamespaceRef,
    tracker_instance: Uuid,
    project: Uuid,
    identifier: &str,
) -> Result<(Uuid, String), ApiError> {
    if identifier.is_empty()
        || identifier.len() > 128
        || !identifier
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"-_.:".contains(&v))
    {
        return Err(ApiError::bad_request("workspace_task_reference_invalid"));
    }
    let endpoint = |path: &str| -> Result<Url, ApiError> {
        let mut url = base.join(path).map_err(|_| unavailable())?;
        url.query_pairs_mut()
            .append_pair(
                "registry_instance_id",
                &namespace.registry_instance_id.to_string(),
            )
            .append_pair("namespace_id", &namespace.namespace_id.to_string());
        Ok(url)
    };
    let explicit = identifier.parse::<Uuid>().ok();
    let task = if let Some(id) = explicit {
        if id.is_nil() {
            return Err(ApiError::bad_request("workspace_task_reference_invalid"));
        }
        id
    } else {
        let mut found = None;
        let mut seen = std::collections::BTreeSet::new();
        for offset in (0..10000).step_by(50) {
            let mut url = endpoint("api/v1/namespace-tasks")?;
            url.query_pairs_mut()
                .append_pair("offset", &offset.to_string());
            let page: Vec<TaskCatalogItem> = serde_json::from_value(get(client, url, token).await?)
                .map_err(|_| unavailable())?;
            if page.len() > 50 {
                return Err(unavailable());
            }
            for item in &page {
                if item.namespace != *namespace
                    || item.task.tracker_instance_id != tracker_instance
                    || item.task.task_id.is_nil()
                    || !seen.insert(item.task.task_id)
                {
                    return Err(unavailable());
                }
                if item.task_key == identifier {
                    if found.replace(item.task.task_id).is_some() {
                        return Err(unavailable());
                    }
                }
            }
            if found.is_some() || page.len() < 50 {
                break;
            }
        }
        found.ok_or_else(|| ApiError::bad_request("workspace_task_not_in_project"))?
    };
    let value = get(
        client,
        endpoint(&format!("api/v1/namespace-tasks/{task}"))?,
        token,
    )
    .await?;
    if value["schema_version"] != 1
        || value["namespace"] != serde_json::to_value(namespace).map_err(|_| unavailable())?
        || value["tracker_instance_id"] != tracker_instance.to_string()
        || value["project_id"] != project.to_string()
        || value["task_id"] != task.to_string()
        || value["state"] != "active"
    {
        return Err(ApiError::conflict("workspace_task_context_changed"));
    }
    let key = value["task_key"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or_else(unavailable)?;
    if explicit.is_none() && key != identifier {
        return Err(ApiError::conflict("workspace_task_identity_changed"));
    }
    Ok((task, key.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::Request, routing::get};
    use serde_json::json;
    #[tokio::test]
    async fn namespace_reader_resolves_keys_and_uuids_and_refuses_foreign_or_inactive_context() {
        for (key_lookup, changed) in [
            (true, "none"),
            (false, "none"),
            (true, "paged"),
            (true, "project"),
            (false, "state"),
            (true, "namespace"),
        ] {
            let namespace = NamespaceRef {
                registry_instance_id: Uuid::new_v4(),
                namespace_id: Uuid::new_v4(),
            };
            let instance = Uuid::new_v4();
            let project = Uuid::new_v4();
            let task = Uuid::new_v4();
            let key = format!("PROJECT{}-17", &project.simple().to_string()[..6]);
            let item = json!({"namespace":namespace,"task":{"tracker_instance_id":instance,"task_id":task},"task_key":key,"label":"A manual development task"});
            let mut context = json!({"schema_version":1,"namespace":namespace,"tracker_instance_id":instance,
                "project_id":project,"task_id":task,"task_key":key,"state":"active"});
            match changed {
                "project" => context["project_id"] = json!(Uuid::new_v4()),
                "state" => context["state"] = json!("suspended"),
                "namespace" => context["namespace"]["namespace_id"] = json!(Uuid::new_v4()),
                _ => {}
            }
            let expected = namespace.clone();
            let app = Router::new().fallback(get(move |request: Request| {
                let item = item.clone();
                let context = context.clone();
                let expected = expected.clone();
                async move {
                    assert_eq!(
                        request.headers()["authorization"],
                        "Bearer fixture-namespace-reader"
                    );
                    let pairs: std::collections::BTreeMap<_, _> =
                        Url::parse(&format!("http://fixture{}", request.uri()))
                            .unwrap()
                            .query_pairs()
                            .into_owned()
                            .collect();
                    assert_eq!(pairs["namespace_id"], expected.namespace_id.to_string());
                    assert_eq!(
                        pairs["registry_instance_id"],
                        expected.registry_instance_id.to_string()
                    );
                    if request.uri().path() == "/api/v1/namespace-tasks" {
                        if changed == "paged" && pairs["offset"] == "0" {
                            Json(Value::Array((0..50).map(|index|json!({
                                "namespace":expected,"task":{"tracker_instance_id":instance,"task_id":Uuid::new_v4()},
                                "task_key":format!("ANOTHER-{index}"),"label":"Another task"
                            })).collect()))
                        } else {
                            Json(json!([item]))
                        }
                    } else {
                        assert_eq!(
                            request.uri().path(),
                            format!("/api/v1/namespace-tasks/{task}")
                        );
                        Json(context)
                    }
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let identifier = if key_lookup {
                key.clone()
            } else {
                task.to_string()
            };
            let result = resolve(
                &Client::new(),
                &base,
                "fixture-namespace-reader",
                &namespace,
                instance,
                project,
                &identifier,
            )
            .await;
            server.abort();
            let _ = server.await;
            if matches!(changed, "none" | "paged") {
                assert_eq!(result.unwrap(), (task, key));
            } else {
                let error = result.unwrap_err();
                assert!(!error.message.contains("fixture-namespace-reader"));
            }
        }
    }
}
