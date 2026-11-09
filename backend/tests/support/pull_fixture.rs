//! Synthetic task-bound PR rows for tests of unrelated reads/merge/identity fences.
use sqlx::PgPool;
use uuid::Uuid;

#[allow(dead_code)]
pub async fn insert(
    pool: &PgPool,
    storage: &str,
    number: i32,
    title: &str,
    branches: (&str, &str),
    status: &str,
    author: &str,
) -> Uuid {
    let (repository, registry, namespace) = scope(pool, storage).await;
    let actor = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role,central_sub) VALUES($1,$2,'developer',$3)")
        .bind(actor)
        .bind(format!("pr-fixture-{}", actor.simple()))
        .bind(Uuid::new_v4().to_string())
        .execute(pool)
        .await
        .unwrap();
    let pull = Uuid::now_v7();
    let tracker = Uuid::new_v4();
    let task = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,status,created_by,primary_tracker_instance_id,primary_task_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(pull).bind(storage).bind(number).bind(title).bind(branches.0).bind(branches.1).bind(status).bind(author).bind(tracker).bind(task).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO pull_task_links(pull_request_id,repository_id,tracker_instance_id,task_id,registry_instance_id,namespace_id,source_commit_sha,task_key_snapshot,created_by_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,'FIXTURE-1',$8)")
        .bind(pull).bind(repository).bind(tracker).bind(task).bind(registry).bind(namespace).bind("a".repeat(40)).bind(actor).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO repository_pr_counters(repository_id,high_water_mark) VALUES($1,$2) ON CONFLICT(repository_id) DO UPDATE SET high_water_mark=GREATEST(repository_pr_counters.high_water_mark,EXCLUDED.high_water_mark)")
        .bind(repository).bind(number).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    pull
}

async fn scope(pool: &PgPool, storage: &str) -> (Uuid, Uuid, Uuid) {
    let (repository, group): (Uuid, Option<Uuid>) =
        sqlx::query_as("SELECT id,group_id FROM repository_catalog WHERE storage_name=$1")
            .bind(storage)
            .fetch_one(pool)
            .await
            .unwrap();
    let group = match group {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'PR fixture')")
                .bind(id)
                .bind(format!("pr-{}", id.simple()))
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("UPDATE repository_catalog SET group_id=$2 WHERE id=$1")
                .bind(repository)
                .bind(id)
                .execute(pool)
                .await
                .unwrap();
            id
        }
    };
    let binding: Option<(Uuid,Uuid)> = sqlx::query_as("SELECT registry_instance_id,namespace_id FROM forge_namespace_bindings WHERE resource_id=$1")
        .bind(group).fetch_optional(pool).await.unwrap();
    let (registry, namespace) = if let Some(binding) = binding {
        binding
    } else {
        let registry = std::env::var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID")
            .ok()
            .map(|v| v.parse::<Uuid>().unwrap())
            .unwrap_or(Uuid::from_u128(0x10000000000040008000000000000001));
        let instance = std::env::var("CICD_NAMESPACE__INSTANCE_ID")
            .ok()
            .map(|v| v.parse::<Uuid>().unwrap())
            .unwrap_or(Uuid::from_u128(0x10000000000040008000000000000002));
        let namespace = Uuid::new_v4();
        let command = serde_json::json!({"schema_version":1,"namespace":{"registry_instance_id":registry,"namespace_id":namespace},
            "resource":{"instance_id":instance,"resource_id":group,"kind":"git_group"},
            "operation_id":Uuid::new_v4(),"generation":1,"state":"active"});
        sqlx::query("INSERT INTO forge_namespace_bindings(resource_id,registry_instance_id,namespace_id,generation,state,command) VALUES($1,$2,$3,1,'active',$4)")
            .bind(group).bind(registry).bind(namespace).bind(command).execute(pool).await.unwrap();
        (registry, namespace)
    };
    let instance: Uuid = sqlx::query_scalar("SELECT (command#>>'{resource,instance_id}')::uuid FROM forge_namespace_bindings WHERE resource_id=$1")
        .bind(group).fetch_one(pool).await.unwrap();
    // Namespace admission fixtures run serially/in isolated processes, as their
    // existing Central/namespace environment configuration already requires.
    for (name, value) in [
        ("CICD_NAMESPACE__INSTANCE_ID", instance),
        ("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry),
    ] {
        match std::env::var(name) {
            Ok(configured) => assert_eq!(
                configured,
                value.to_string(),
                "fixture installation must match its namespace binding"
            ),
            Err(_) => unsafe {
                std::env::set_var(name, value.to_string());
            },
        }
    }
    (repository, registry, namespace)
}

/// Isolated admission fixtures use a real HTTP reader with synthetic Task metadata.
#[allow(dead_code)]
pub async fn task_reader(
    pool: &PgPool,
    storage: &str,
) -> (
    serde_json::Value,
    tokio::task::JoinHandle<()>,
    tempfile::NamedTempFile,
) {
    use std::io::Write;
    let (repository, registry, namespace) = scope(pool, storage).await;
    let instance: Uuid = sqlx::query_scalar("SELECT (b.command#>>'{resource,instance_id}')::uuid FROM forge_namespace_bindings b JOIN repository_catalog r ON r.group_id=b.resource_id WHERE r.id=$1")
        .bind(repository).fetch_one(pool).await.unwrap();
    let tracker = Uuid::new_v4();
    let task = Uuid::new_v4();
    let reference = serde_json::json!({"tracker_instance_id":tracker,"task_id":task});
    let readback = serde_json::json!({"schema_version":1,"namespace":{"registry_instance_id":registry,"namespace_id":namespace},"tracker_instance_id":tracker,"task_id":task,"task_key":"FIXTURE-1","state":"active"});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        &format!("/api/v1/namespace-tasks/{task}"),
        axum::routing::get(move || {
            let readback = readback.clone();
            async move { axum::Json(readback) }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut token = tempfile::NamedTempFile::new().unwrap();
    token.write_all(b"synthetic-task-reader").unwrap();
    // Callers run in a dedicated process, like the existing Central/admission fixtures.
    unsafe {
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
        std::env::set_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string());
        std::env::set_var("CICD_NAMESPACE__TRACKER_INSTANCE_ID", tracker.to_string());
        std::env::set_var("CICD_NAMESPACE__TRACKER_URL", format!("http://{address}"));
        std::env::set_var("CICD_NAMESPACE__TRACKER_TOKEN_FILE", token.path());
    }
    (reference, server, token)
}
