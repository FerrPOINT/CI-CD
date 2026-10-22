//! Real owner routes and full schema: no implicit group, durable replay and confirmed drain.
use super::{Profile, group_drain};
use pdlc_shared::{resource_context::*, space_access::SpaceRef, space_lifecycle::*};
use sqlx::PgPool;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "sdlc_pat_fixture_product_roles";

// Reuse an existing human profile; owner calls must never JIT a new human.
async fn catalog_projection(
    http: &reqwest::Client,
    origin: &str,
    profile: &Arc<RwLock<Profile>>,
    human: &Profile,
    expected: (Uuid, Uuid, SpaceRef, &str),
) {
    let (group, repository, space, state) = expected;
    let owner = profile.read().unwrap().clone();
    let mut reader = human.clone();
    reader.expires_in = 300;
    reader.scopes = vec!["ci-cd:read"];
    *profile.write().unwrap() = reader;
    let previous_origin = std::env::var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN").ok();
    unsafe {
        std::env::set_var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN", origin);
    }
    for path in [
        format!("/api/v1/catalog/repositories?group_id={group}&managed=true&limit=1"),
        format!("/api/v1/git-groups/{group}/repositories?limit=1"),
    ] {
        let response = http
            .get(format!("{origin}{path}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert_eq!(status, reqwest::StatusCode::OK, "{path}: {text}");
        let page: cicd::repository_catalog::CatalogPage = serde_json::from_str(&text).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, repository);
        assert_eq!(page.items[0].state.as_deref(), Some(state));
        let namespace = page.items[0].namespace.as_ref().unwrap();
        assert_eq!(namespace.registry_instance_id, space.registry_instance_id);
        assert_eq!(namespace.namespace_id, space.space_id);
    }
    let response = http
        .get(format!("{origin}/api/v1/catalog/repositories/{repository}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let item: cicd::repository_catalog::CatalogRepository = response.json().await.unwrap();
    assert_eq!(item.id, repository);
    assert_eq!(item.group_id, Some(group));
    assert_eq!(item.state.as_deref(), Some(state));
    let response = http
        .get(format!(
            "{origin}/api/v1/catalog/repositories?group_id={group}&sync_available=true&limit=1"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let page: cicd::repository_catalog::CatalogPage = response.json().await.unwrap();
    assert_eq!(page.total, i64::from(state == "active"));
    assert_eq!(page.items.len(), usize::from(state == "active"));
    let response = http
        .get(format!(
            "{origin}/api/v1/catalog/repositories?group_id={group}&limit=1&offset=1"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let page: cicd::repository_catalog::CatalogPage = response.json().await.unwrap();
    assert_eq!(page.total, 1);
    assert!(
        page.items.is_empty(),
        "lifecycle projection must not duplicate pagination rows"
    );
    *profile.write().unwrap() = owner;
    unsafe {
        if let Some(previous) = previous_origin {
            std::env::set_var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN", previous);
        } else {
            std::env::remove_var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN");
        }
    }
}

fn command(registry: Uuid, instance: Uuid, space: Uuid) -> SpaceOwnerCommandV2 {
    SpaceOwnerCommandV2 {
        schema_version: 2,
        space: SpaceRef {
            registry_instance_id: registry,
            space_id: space,
        },
        participant_instance_id: instance,
        operation_id: Uuid::new_v4(),
        revision: 1,
        desired_state: DesiredSpaceState::Active,
        metadata: SpaceMetadata {
            slug: format!("space-{}", space.simple()),
            name: "Общее пространство".into(),
            description: "Без обязательного проекта или группы".into(),
            responsible_subject: "confirmed-responsible".into(),
        },
    }
}
fn next(c: &SpaceOwnerCommandV2, state: DesiredSpaceState) -> SpaceOwnerCommandV2 {
    let mut c = c.clone();
    c.revision += 1;
    c.operation_id = Uuid::new_v4();
    c.desired_state = state;
    c
}
async fn send(http: &reqwest::Client, origin: &str, c: &SpaceOwnerCommandV2) -> reqwest::Response {
    http.put(format!(
        "{origin}/api/v2/spaces/{}/{}/owner-state",
        c.space.registry_instance_id, c.space.space_id
    ))
    .bearer_auth(TOKEN)
    .json(c)
    .send()
    .await
    .unwrap()
}
async fn ack(response: reqwest::Response, c: &SpaceOwnerCommandV2) -> SpaceOwnerReadbackV2 {
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::OK, "owner response: {body}");
    let receipt: SpaceOwnerReadbackV2 = serde_json::from_str(&body).unwrap();
    let mut checked = receipt.clone();
    checked.drained = true;
    assert!(checked.matches(c));
    receipt
}
async fn bind_legacy(
    http: &reqwest::Client,
    origin: &str,
    c: &SpaceOwnerCommandV2,
    group: Uuid,
    create: bool,
) -> OwnerCommand {
    let old = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: c.space.registry_instance_id,
            namespace_id: c.space.space_id,
        },
        resource: ResourceRef {
            kind: ResourceKind::GitGroup,
            instance_id: c.participant_instance_id,
            resource_id: group,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: if create {
            Some(
                serde_json::json!({"slug":format!("legacy-{}",group.simple()),"name":"Legacy Git name"}),
            )
        } else {
            None
        },
    };
    let response = http
        .put(format!(
            "{origin}/api/v1/namespace-resources/git_group/{group}"
        ))
        .bearer_auth(TOKEN)
        .json(&old)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::OK, "legacy attach: {body}");
    old
}
pub(super) async fn verify(pool: &PgPool, profile: Arc<RwLock<Profile>>) {
    let original = profile.read().unwrap().clone();
    let owner = Uuid::new_v4();
    let registry = Uuid::new_v4();
    let instance = Uuid::new_v4();
    unsafe {
        std::env::set_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID", registry.to_string());
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
        std::env::set_var("CICD_NAMESPACE__OWNER_SUBJECTS", owner.to_string());
    }
    *profile.write().unwrap() = Profile {
        subject: owner,
        role: "member".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:write"],
    };
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap();
    let app =
        cicd::api::app_with_auth_secret(Some(pool.clone()), Some("fixture-owner-secret".into()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap();
    let empty = command(registry, instance, Uuid::new_v4());
    assert_eq!(
        send(&http, &origin, &empty).await.status(),
        reqwest::StatusCode::FORBIDDEN,
        "allowlisted human is not a machine owner"
    );
    profile.write().unwrap().role = "service_account".into();
    profile.write().unwrap().scopes = vec!["ci-cd:read"];
    assert_eq!(
        send(&http, &origin, &empty).await.status(),
        reqwest::StatusCode::FORBIDDEN
    );
    profile.write().unwrap().scopes = vec!["ci-cd:write"];
    profile.write().unwrap().subject = Uuid::new_v4();
    assert_eq!(
        send(&http, &origin, &empty).await.status(),
        reqwest::StatusCode::FORBIDDEN,
        "unregistered machine has no owner capability"
    );
    profile.write().unwrap().subject = owner;
    let groups: i64 = sqlx::query_scalar("SELECT count(*) FROM git_groups")
        .fetch_one(pool)
        .await
        .unwrap();
    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
        .fetch_one(pool)
        .await
        .unwrap();
    let receipt = ack(send(&http, &origin, &empty).await, &empty).await;
    assert!(receipt.local_resource.is_none() && receipt.drained);
    assert_eq!(
        ack(send(&http, &origin, &empty).await, &empty).await,
        receipt
    );
    assert_eq!(
        groups,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM git_groups")
            .fetch_one(pool)
            .await
            .unwrap()
    );
    assert_eq!(
        projects,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM projects")
            .fetch_one(pool)
            .await
            .unwrap()
    );
    let mut conflict = empty.clone();
    conflict.metadata.name = "Different payload".into();
    assert_eq!(
        send(&http, &origin, &conflict).await.status(),
        reqwest::StatusCode::CONFLICT
    );
    let mut foreign = command(registry, instance, Uuid::new_v4());
    foreign.operation_id = empty.operation_id;
    assert_eq!(
        send(&http, &origin, &foreign).await.status(),
        reqwest::StatusCode::CONFLICT
    );
    let archived_empty = next(&empty, DesiredSpaceState::Archived);
    assert!(
        ack(send(&http, &origin, &archived_empty).await, &archived_empty)
            .await
            .drained
    );
    let restored_empty = next(&archived_empty, DesiredSpaceState::Active);
    assert!(
        ack(send(&http, &origin, &restored_empty).await, &restored_empty)
            .await
            .drained
    );
    let collision = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: registry,
            namespace_id: empty.space.space_id,
        },
        resource: ResourceRef {
            kind: ResourceKind::GitGroup,
            instance_id: instance,
            resource_id: Uuid::new_v4(),
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: Some(
            serde_json::json!({"slug":"must-not-create","name":"No automatic group"}),
        ),
    };
    assert_eq!(
        http.put(format!(
            "{origin}/api/v1/namespace-resources/git_group/{}",
            collision.resource.resource_id
        ))
        .bearer_auth(TOKEN)
        .json(&collision)
        .send()
        .await
        .unwrap()
        .status(),
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM git_groups WHERE id=$1")
            .bind(collision.resource.resource_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );

    // Either legacy provisioning wins and v2 retains that exact group, or
    // barrier-only v2 wins and the old operation fails before creating a group.
    let racing = command(registry, instance, Uuid::new_v4());
    let racing_group = Uuid::new_v4();
    let mut legacy_race = collision.clone();
    legacy_race.namespace.namespace_id = racing.space.space_id;
    legacy_race.resource.resource_id = racing_group;
    legacy_race.operation_id = Uuid::new_v4();
    legacy_race.create_spec = Some(
        serde_json::json!({"slug":format!("race-{}",racing_group.simple()),"name":"Racing legacy group"}),
    );
    let old_send = http
        .put(format!(
            "{origin}/api/v1/namespace-resources/git_group/{racing_group}"
        ))
        .bearer_auth(TOKEN)
        .json(&legacy_race)
        .send();
    let (new_response, old_response) = tokio::join!(send(&http, &origin, &racing), old_send);
    let racing_ack = ack(new_response, &racing).await;
    let old_status = old_response.unwrap().status();
    assert!(matches!(
        old_status,
        reqwest::StatusCode::OK | reqwest::StatusCode::CONFLICT
    ));
    let expected = if old_status == reqwest::StatusCode::OK {
        Some(racing_group)
    } else {
        None
    };
    assert_eq!(
        racing_ack.local_resource.as_ref().map(|r| r.resource_id),
        expected
    );
    assert_eq!(
        sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT group_id FROM forge_space_catalog WHERE registry_instance_id=$1 AND space_id=$2"
        )
        .bind(registry)
        .bind(racing.space.space_id)
        .fetch_one(pool)
        .await
        .unwrap(),
        expected
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM git_groups WHERE id=$1")
            .bind(racing_group)
            .fetch_one(pool)
            .await
            .unwrap(),
        i64::from(expected.is_some())
    );

    let runner = Uuid::new_v4();
    let credential = "space-owner-runner-fixture";
    sqlx::query("INSERT INTO runners(id,name,credential_hash,credential_expires_at) VALUES($1,$2,$3,now()+interval '1 hour')").bind(runner).bind(format!("space-owner-{runner}")).bind(cicd::auth::hash_token(credential)).execute(pool).await.unwrap();
    // A confirmed legacy archive can be explicitly restored by v2 without
    // rewriting the strict-v1 command; the effective lifecycle then comes from v2.
    let legacy_empty = command(registry, instance, Uuid::new_v4());
    let empty_group = Uuid::new_v4();
    let mut old_archive = bind_legacy(&http, &origin, &legacy_empty, empty_group, true).await;
    old_archive.generation += 1;
    old_archive.operation_id = Uuid::new_v4();
    old_archive.state = "archived".into();
    old_archive.create_spec = None;
    let archived = http
        .put(format!(
            "{origin}/api/v1/namespace-resources/git_group/{empty_group}"
        ))
        .bearer_auth(TOKEN)
        .json(&old_archive)
        .send()
        .await
        .unwrap();
    assert_eq!(archived.status(), reqwest::StatusCode::OK);
    let old_receipt: OwnerReadback = archived.json().await.unwrap();
    assert!(old_receipt.drained);
    assert_eq!(
        ack(send(&http, &origin, &legacy_empty).await, &legacy_empty)
            .await
            .local_resource
            .unwrap()
            .resource_id,
        empty_group
    );
    let pair:(String,Option<String>)=sqlx::query_as("SELECT b.state,forge_effective_group_state(b.resource_id) FROM forge_namespace_bindings b WHERE resource_id=$1").bind(empty_group).fetch_one(pool).await.unwrap();
    assert_eq!(pair, ("archived".into(), Some("active".into())));
    let extra_repo = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories(id,name,visibility) VALUES($1,$2,'private')")
        .bind(extra_repo)
        .bind(format!("restored-{}", extra_repo.simple()))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$1 WHERE id=$2")
        .bind(empty_group)
        .bind(extra_repo)
        .execute(pool)
        .await
        .unwrap();
    catalog_projection(
        &http,
        &origin,
        &profile,
        &original,
        (
            empty_group,
            extra_repo,
            legacy_empty.space.clone(),
            "active",
        ),
    )
    .await;
    let mut rename_archived = next(&legacy_empty, DesiredSpaceState::Archived);
    rename_archived.metadata.name = "Архив знаний".into();
    assert!(
        ack(
            send(&http, &origin, &rename_archived).await,
            &rename_archived
        )
        .await
        .drained
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM git_groups WHERE id=$1")
            .bind(empty_group)
            .fetch_one(pool)
            .await
            .unwrap(),
        "Архив знаний"
    );

    catalog_projection(
        &http,
        &origin,
        &profile,
        &original,
        (
            empty_group,
            extra_repo,
            legacy_empty.space.clone(),
            "archived",
        ),
    )
    .await;

    let unknown = group_drain::work(pool, Some(runner)).await;
    let unknown_space = command(registry, instance, Uuid::new_v4());
    let mut old_unknown = bind_legacy(&http, &origin, &unknown_space, unknown.group, false).await;
    old_unknown.generation += 1;
    old_unknown.operation_id = Uuid::new_v4();
    old_unknown.state = "archived".into();
    let old_response = http
        .put(format!(
            "{origin}/api/v1/namespace-resources/git_group/{}",
            unknown.group
        ))
        .bearer_auth(TOKEN)
        .json(&old_unknown)
        .send()
        .await
        .unwrap();
    assert_eq!(old_response.status(), reqwest::StatusCode::OK);
    assert!(!old_response.json::<OwnerReadback>().await.unwrap().drained);
    assert_eq!(
        send(&http, &origin, &unknown_space).await.status(),
        reqwest::StatusCode::CONFLICT,
        "v2 adoption must not restore an unconfirmed legacy archive"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM forge_space_catalog WHERE group_id=$1")
            .bind(unknown.group)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );

    let work = group_drain::work(pool, Some(runner)).await;
    let active = command(registry, instance, Uuid::new_v4());
    let legacy = bind_legacy(&http, &origin, &active, work.group, false).await;
    let work_repository: Uuid =
        sqlx::query_scalar("SELECT repository_id FROM projects WHERE id=$1")
            .bind(work.project)
            .fetch_one(pool)
            .await
            .unwrap();
    catalog_projection(
        &http,
        &origin,
        &profile,
        &original,
        (work.group, work_repository, active.space.clone(), "active"),
    )
    .await;
    let old_command: serde_json::Value =
        sqlx::query_scalar("SELECT command FROM forge_namespace_bindings WHERE resource_id=$1")
            .bind(work.group)
            .fetch_one(pool)
            .await
            .unwrap();
    let adopted = ack(send(&http, &origin, &active).await, &active).await;
    assert_eq!(
        adopted.local_resource.as_ref().unwrap().resource_id,
        work.group
    );
    assert_eq!(
        old_command,
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT command FROM forge_namespace_bindings WHERE resource_id=$1"
        )
        .bind(work.group)
        .fetch_one(pool)
        .await
        .unwrap(),
        "v2 does not rewrite strict v1 command history"
    );
    let old_retry = http
        .put(format!(
            "{origin}/api/v1/namespace-resources/git_group/{}",
            work.group
        ))
        .bearer_auth(TOKEN)
        .json(&legacy)
        .send()
        .await
        .unwrap();
    assert_eq!(old_retry.status(), reqwest::StatusCode::CONFLICT);
    let role_write = sqlx::query("UPDATE git_groups SET name='Unauthorized rename' WHERE id=$1")
        .bind(work.group)
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(
        role_write.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );

    // Hold the same lease used by native Git until the archive waiter is visible.
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))")
        .bind(work.group.to_string())
        .execute(&mut *writer)
        .await
        .unwrap();
    let archive = next(&active, DesiredSpaceState::Archived);
    let started = archive.clone();
    let delayed_http = http.clone();
    let delayed_origin = origin.clone();
    let delayed = tokio::spawn(async move { send(&delayed_http, &delayed_origin, &started).await });
    tokio::time::timeout(std::time::Duration::from_secs(2),async{loop{
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT pg_advisory_xact_lock(hashtextextended%')").fetch_one(pool).await.unwrap();
        if waiting{break;}assert!(!delayed.is_finished());tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }}).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM forge_space_catalog WHERE group_id=$1")
            .bind(work.group)
            .fetch_one(pool)
            .await
            .unwrap(),
        "active"
    );
    writer.rollback().await.unwrap();
    let pending = ack(delayed.await.unwrap(), &archive).await;
    assert!(
        !pending.drained,
        "running attempt requires its accepted receipt"
    );
    assert!(
        !ack(send(&http, &origin, &archive).await, &archive)
            .await
            .drained
    );
    catalog_projection(
        &http,
        &origin,
        &profile,
        &original,
        (
            work.group,
            work_repository,
            active.space.clone(),
            "archiving",
        ),
    )
    .await;
    let restore = next(&archive, DesiredSpaceState::Active);
    assert_eq!(
        send(&http, &origin, &restore).await.status(),
        reqwest::StatusCode::CONFLICT,
        "cannot skip an unconfirmed archive"
    );
    let denied = sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','queued')",
    )
    .bind(Uuid::new_v4())
    .bind(work.project)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let complete = app
        .clone()
        .oneshot(group_drain::completion(&work, credential))
        .await
        .unwrap();
    assert_eq!(
        complete.status(),
        axum::http::StatusCode::OK,
        "terminal/control receipt remains reachable while archiving"
    );
    let finished = ack(send(&http, &origin, &archive).await, &archive).await;
    assert!(finished.drained);
    let status: String =
        sqlx::query_scalar("SELECT state FROM forge_space_catalog WHERE group_id=$1")
            .bind(work.group)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "archived");
    assert_eq!(
        ack(send(&http, &origin, &archive).await, &archive).await,
        finished
    );
    let restored = ack(send(&http, &origin, &restore).await, &restore).await;
    assert!(restored.drained);
    sqlx::query(
        "INSERT INTO pipelines(id,project_id,git_ref,status) VALUES($1,$2,'main','queued')",
    )
    .bind(Uuid::new_v4())
    .bind(work.project)
    .execute(pool)
    .await
    .unwrap();
    let mut rename = next(&restore, DesiredSpaceState::Active);
    rename.metadata.name = "Имя общего пространства".into();
    ack(send(&http, &origin, &rename).await, &rename).await;
    let name: String = sqlx::query_scalar("SELECT name FROM git_groups WHERE id=$1")
        .bind(work.group)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(name, rename.metadata.name);
    assert_eq!(
        users,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(pool)
            .await
            .unwrap(),
        "owner service account never JITs a human profile"
    );
    // Readback is the current persisted receipt. GET never guesses drain from a timer.
    let get = http
        .get(format!(
            "{origin}/api/v2/spaces/{registry}/{}/owner-state",
            active.space.space_id
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(ack(get, &rename).await.operation_id, rename.operation_id);
    // Expiry while waiting for the native-Git barrier rolls back every metadata effect.
    let before_name = rename.metadata.name.clone();
    let mut expired_rename = next(&rename, DesiredSpaceState::Active);
    expired_rename.metadata.name = "Must roll back".into();
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))")
        .bind(work.group.to_string())
        .execute(&mut *writer)
        .await
        .unwrap();
    profile.write().unwrap().expires_in = 1;
    let delayed_http = http.clone();
    let delayed_origin = origin.clone();
    let delayed =
        tokio::spawn(async move { send(&delayed_http, &delayed_origin, &expired_rename).await });
    tokio::time::timeout(std::time::Duration::from_secs(2),async{loop{
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT pg_advisory_xact_lock(hashtextextended%')").fetch_one(pool).await.unwrap();
        if waiting{break;}assert!(!delayed.is_finished());tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }}).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    writer.rollback().await.unwrap();
    assert_eq!(
        delayed.await.unwrap().status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM git_groups WHERE id=$1")
            .bind(work.group)
            .fetch_one(pool)
            .await
            .unwrap(),
        before_name
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT revision FROM forge_space_catalog WHERE group_id=$1")
            .bind(work.group)
            .fetch_one(pool)
            .await
            .unwrap(),
        rename.revision
    );
    profile.write().unwrap().expires_in = 300;
    let doc: serde_json::Value = http
        .get(format!("{origin}/api/v1/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let owner_path = &doc["paths"]["/api/v2/spaces/{registry}/{space}/owner-state"];
    assert_eq!(
        owner_path["put"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SpaceOwnerCommandV2"
    );
    for schema in [
        "SpaceOwnerCommandV2",
        "SpaceOwnerReadbackV2",
        "SpaceRef",
        "SpaceMetadata",
    ] {
        assert!(
            doc["components"]["schemas"][schema].is_object(),
            "missing schema {schema}"
        );
    }
    let mut operations = std::collections::HashSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for method in ["get", "post", "put", "patch", "delete", "head", "options"] {
            if let Some(op) = item.get(method) {
                assert!(
                    cicd::authz::route_access(&method.to_ascii_uppercase(), path).is_some(),
                    "missing route policy {method} {path}"
                );
                assert!(
                    operations.insert(op["operationId"].as_str().unwrap().to_owned()),
                    "duplicate operation ID"
                );
            }
        }
    }
    for method in ["GET", "PUT"] {
        let path = format!(
            "/api/v2/spaces/{registry}/{}/owner-state",
            active.space.space_id
        );
        assert_eq!(
            cicd::authz::route_access(method, &path),
            Some(cicd::authz::RouteAccess::NamespaceOwner)
        );
        assert!(!cicd::authz::allows(
            cicd::authz::Role::Admin,
            method,
            &path
        ));
    }
    unsafe {
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", Uuid::new_v4().to_string());
    }
    let wrong_instance = http
        .get(format!(
            "{origin}/api/v2/spaces/{registry}/{}/owner-state",
            active.space.space_id
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_instance.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "a different configured participant must not acknowledge old receipts"
    );
    unsafe {
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
    }
    cicd::migrations().await.unwrap().run(pool).await.unwrap();
    server.abort();
    let _ = server.await;
    *profile.write().unwrap() = original;
    unsafe {
        std::env::remove_var("CICD_NAMESPACE__REGISTRY_INSTANCE_ID");
        std::env::remove_var("CICD_NAMESPACE__INSTANCE_ID");
        std::env::remove_var("CICD_NAMESPACE__OWNER_SUBJECTS");
    }
}
