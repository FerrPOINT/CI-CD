//! Isolated opt-in policy on actual Git routes, with the real timed SDK client.
use super::Profile;
use axum::{Json, Router, routing::post};
use pdlc_shared::{resource_context::*, space_access::*, space_lifecycle::*};
use sqlx::PgPool;
use std::{
    path::Path,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

struct OwnerFixture<'a> {
    profile: &'a Arc<RwLock<Profile>>,
    origin: &'a str,
    instance: Uuid,
    subject: Uuid,
}
async fn attach(
    pool: &PgPool,
    repository: Uuid,
    space: &SpaceRef,
    owner: &OwnerFixture<'_>,
) -> String {
    let instance = owner.instance;
    let saved = owner.profile.read().unwrap().clone();
    *owner.profile.write().unwrap() = Profile {
        subject: owner.subject,
        role: "service_account".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:write"],
    };
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let group = Uuid::new_v4();
    let slug = format!("access-{}", group.simple());
    sqlx::query("INSERT INTO git_groups(id,slug,name) VALUES($1,$2,'Space fixture')")
        .bind(group)
        .bind(&slug)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE repository_catalog SET group_id=$1,slug='repository' WHERE id=$2")
        .bind(group)
        .bind(repository)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repository_aliases(alias,repository_id) VALUES($1,$2)")
        .bind(format!("{slug}/repository"))
        .bind(repository)
        .execute(pool)
        .await
        .unwrap();
    let old = OwnerCommand {
        schema_version: 1,
        namespace: NamespaceRef {
            registry_instance_id: space.registry_instance_id,
            namespace_id: space.space_id,
        },
        resource: ResourceRef {
            kind: ResourceKind::GitGroup,
            instance_id: instance,
            resource_id: group,
        },
        operation_id: Uuid::new_v4(),
        generation: 1,
        state: "active".into(),
        create_spec: None,
    };
    let response = http
        .put(format!(
            "{}/api/v1/namespace-resources/git_group/{group}",
            owner.origin
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .json(&old)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 200, "fixture real legacy owner: {text}");
    let command = SpaceOwnerCommandV2 {
        schema_version: 2,
        space: space.clone(),
        participant_instance_id: instance,
        operation_id: Uuid::new_v4(),
        revision: 1,
        desired_state: DesiredSpaceState::Active,
        metadata: SpaceMetadata {
            slug: slug.clone(),
            name: "Space fixture".into(),
            description: "".into(),
            responsible_subject: "fixture-subject".into(),
        },
    };
    let response = http
        .put(format!(
            "{}/api/v2/spaces/{}/{}/owner-state",
            owner.origin, space.registry_instance_id, space.space_id
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .json(&command)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 200, "fixture real v2 owner: {text}");
    let receipt: SpaceOwnerReadbackV2 = serde_json::from_str(&text).unwrap();
    assert!(receipt.matches(&command) && receipt.drained);
    *owner.profile.write().unwrap() = saved;
    slug
}

async fn catalog(
    http: &reqwest::Client,
    origin: &str,
    query: &str,
) -> cicd::repository_catalog::CatalogPage {
    let response = http
        .get(format!("{origin}/api/v1/catalog/repositories{query}"))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .send()
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await.unwrap();
    assert_eq!(status, 200, "common catalog {query}: {text}");
    assert_eq!(headers["cache-control"], "private, no-store, no-transform");
    serde_json::from_str(&text).unwrap()
}

async fn detail(
    http: &reqwest::Client,
    origin: &str,
    id: &str,
    expected: u16,
) -> Option<cicd::repository_catalog::CatalogRepository> {
    let request = http
        .get(format!("{origin}/api/v1/catalog/repositories/{id}"))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .build()
        .unwrap();
    if id.contains('%') {
        assert!(
            request.url().path().contains('%'),
            "wire path preserves encoding"
        );
    }
    let response = http.execute(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await.unwrap();
    assert_eq!(status, expected, "catalog detail {id}: {text}");
    assert_eq!(headers["cache-control"], "private, no-store, no-transform");
    if expected == 200 {
        Some(serde_json::from_str(&text).unwrap())
    } else {
        assert!(!text.contains("clone_url"));
        None
    }
}
async fn group_page(
    http: &reqwest::Client,
    origin: &str,
    group: Uuid,
    query: &str,
) -> cicd::repository_catalog::CatalogPage {
    let response = http
        .get(format!(
            "{origin}/api/v1/git-groups/{group}/repositories{query}"
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .send()
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await.unwrap();
    assert_eq!(status, 200, "group catalog: {text}");
    assert_eq!(headers["cache-control"], "private, no-store, no-transform");
    serde_json::from_str(&text).unwrap()
}

/// Actual HTTP handlers with real Git objects/SQL; only Admin admission is a fixture.
async fn repository_read(
    http: &reqwest::Client,
    origin: &str,
    path: &str,
    expected: u16,
) -> serde_json::Value {
    let response = http
        .get(format!("{origin}{path}"))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .send()
        .await
        .unwrap();
    let status = response.status();
    if expected == 200 {
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "private, no-store, no-transform"
        );
    }
    let text = response.text().await.unwrap();
    assert_eq!(status.as_u16(), expected, "repository read {path}: {text}");
    serde_json::from_str(&text).unwrap()
}

async fn verify_repository_reads(
    pool: &PgPool,
    http: &reqwest::Client,
    origin: &str,
    visible: (&str, &str, Uuid),
    hidden: (&str, Uuid),
    profile: &Arc<RwLock<Profile>>,
) {
    let (name, canonical, repository) = visible;
    let (hidden_name, hidden_id) = hidden;
    for (storage, id, title) in [
        (name, repository, "Allowed review"),
        (hidden_name, hidden_id, "Hidden review"),
    ] {
        let number: i32 = sqlx::query_scalar("SELECT allocate_repository_pr_number($1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(
            number, 1,
            "fixture allocates through the real repository counter"
        );
        sqlx::query("INSERT INTO pull_requests(id,repository_name,number,title,source_branch,target_branch,status,created_by) VALUES($1,$2,1,$3,'task/current','main','open','fixture')")
            .bind(Uuid::new_v4()).bind(storage).bind(title).execute(pool).await.unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM pull_requests WHERE repository_id=$1")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(
            count, 1,
            "existing repository identity trigger is preserved"
        );
    }
    let encoded = name.replace('-', "%2D");
    let public_alias = format!("{canonical}%2Frepository");
    for alias in [name, &encoded, &public_alias] {
        let prefix = format!("/api/v1/repos/{alias}");
        let refs = repository_read(http, origin, &format!("{prefix}/refs"), 200).await;
        assert!(
            refs.as_array()
                .unwrap()
                .iter()
                .any(|r| r["name"] == "task/current")
        );
        let tree = repository_read(
            http,
            origin,
            &format!("{prefix}/tree?ref=task/current"),
            200,
        )
        .await;
        assert!(tree.as_array().unwrap().iter().any(|r| r["name"] == "data"));
        let blob = repository_read(
            http,
            origin,
            &format!("{prefix}/blob?ref=task/current&path=data"),
            200,
        )
        .await;
        assert_eq!(blob["content"], "original proof\n");
        repository_read(http, origin, &format!("{prefix}/tags"), 200).await;
        let commits = repository_read(
            http,
            origin,
            &format!("{prefix}/commits?branch=task/current"),
            200,
        )
        .await;
        assert!(!commits.as_array().unwrap().is_empty());
        repository_read(
            http,
            origin,
            &format!("{prefix}/compare?from=task/current&to=task/current"),
            200,
        )
        .await;
        let list = repository_read(http, origin, &format!("{prefix}/pulls"), 200).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["title"], "Allowed review");
        let page = repository_read(
            http,
            origin,
            &format!("{prefix}/pulls/page?limit=1&offset=1"),
            200,
        )
        .await;
        assert_eq!(page["total"], 1);
        assert!(page["items"].as_array().unwrap().is_empty());
        let detail = repository_read(http, origin, &format!("{prefix}/pulls/1"), 200).await;
        assert_eq!(detail["title"], "Allowed review");
    }
    for id in [
        repository.to_string(),
        repository.to_string().replace('-', "%2D"),
    ] {
        let list = repository_read(
            http,
            origin,
            &format!("/api/v1/catalog/repositories/{id}/pulls"),
            200,
        )
        .await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        let detail = repository_read(
            http,
            origin,
            &format!("/api/v1/catalog/repositories/{id}/pulls/1"),
            200,
        )
        .await;
        assert_eq!(detail["repository_id"], repository.to_string());
    }
    for alias in [hidden_name.to_owned(), hidden_name.replace('-', "%2D")] {
        for tail in [
            "refs",
            "tree?ref=task/current",
            "blob?ref=task/current&path=data",
            "tags",
            "commits",
            "compare?from=task/current&to=task/current",
            "pulls",
            "pulls/page?limit=1",
            "pulls/1",
        ] {
            let denied =
                repository_read(http, origin, &format!("/api/v1/repos/{alias}/{tail}"), 403).await;
            assert!(!denied.to_string().contains("Hidden review"));
        }
    }
    for id in [
        hidden_id.to_string(),
        hidden_id.to_string().replace('-', "%2D"),
    ] {
        for tail in ["pulls", "pulls/1"] {
            repository_read(
                http,
                origin,
                &format!("/api/v1/catalog/repositories/{id}/{tail}"),
                403,
            )
            .await;
        }
    }
    sqlx::query("UPDATE repository_catalog SET ready=false WHERE id=$1")
        .bind(hidden_id)
        .execute(pool)
        .await
        .unwrap();
    repository_read(
        http,
        origin,
        &format!("/api/v1/catalog/repositories/{hidden_id}/pulls"),
        403,
    )
    .await;
    repository_read(
        http,
        origin,
        &format!("/api/v1/repos/{hidden_name}/tree"),
        403,
    )
    .await;
    sqlx::query("UPDATE repository_catalog SET ready=true WHERE id=$1")
        .bind(hidden_id)
        .execute(pool)
        .await
        .unwrap();
    let saved = profile.read().unwrap().clone();
    profile.write().unwrap().scopes = vec!["git:read"];
    repository_read(
        http,
        origin,
        &format!("/api/v1/repos/{name}/blob?ref=task/current&path=data"),
        403,
    )
    .await;
    *profile.write().unwrap() = saved.clone();
    profile.write().unwrap().role = "service_account".into();
    repository_read(http, origin, &format!("/api/v1/repos/{name}/pulls"), 403).await;
    *profile.write().unwrap() = saved;
    println!(
        "REST repository refs/tree/blob/tags/commits/compare/PR, aliases and encoded owner admission: PASS"
    );
}

pub(super) async fn verify(
    pool: &PgPool,
    profile: Arc<RwLock<Profile>>,
    root: &Path,
    name: &str,
    repository: Uuid,
    origin: &str,
) {
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT active FROM forge_space_admission_state")
            .fetch_one(pool)
            .await
            .unwrap(),
        "source migration defaults OFF"
    );
    let mut human = profile.read().unwrap().clone();
    human.scopes = vec!["git:read", "git:write", "ci-cd:read"];
    let space = SpaceRef {
        registry_instance_id: Uuid::new_v4(),
        space_id: Uuid::new_v4(),
    };
    let previous_origin = std::env::var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN").ok();
    unsafe {
        std::env::set_var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN", origin);
    }
    let instance = Uuid::new_v4();
    unsafe {
        std::env::set_var(
            "CICD_NAMESPACE__REGISTRY_INSTANCE_ID",
            space.registry_instance_id.to_string(),
        );
        std::env::set_var("CICD_NAMESPACE__INSTANCE_ID", instance.to_string());
    }
    let owner_fixture = OwnerFixture {
        profile: &profile,
        origin,
        instance,
        subject: Uuid::new_v4(),
    };
    unsafe {
        std::env::set_var(
            "CICD_NAMESPACE__OWNER_SUBJECTS",
            owner_fixture.subject.to_string(),
        );
    }
    let canonical = attach(pool, repository, &space, &owner_fixture).await;
    let other = SpaceRef {
        registry_instance_id: space.registry_instance_id,
        space_id: Uuid::new_v4(),
    };
    let config = cicd::git_host::GitConfig {
        root: root.to_owned(),
        token: None,
        internal_token: None,
    };
    let other_name = format!("public-{}", Uuid::new_v4().simple());
    let other_repo =
        cicd::git_host::create_repository_core(pool, &config, &other_name, Some("public"))
            .await
            .unwrap();
    let other_canonical = attach(pool, other_repo.id, &other, &owner_fixture).await;
    let unbound_name = format!("unbound-{}", Uuid::new_v4().simple());
    let unbound_repo =
        cicd::git_host::create_repository_core(pool, &config, &unbound_name, Some("public"))
            .await
            .unwrap();
    let owner_space = space.clone();
    let owner_subject = human.subject;
    let all_subject = Uuid::new_v4();
    let owner = Router::new().route(
        "/effective",
        post(
            move |headers: axum::http::HeaderMap, Json(request): Json<SpaceAdmissionRequest>| {
                let space = owner_space.clone();
                async move {
                    assert_eq!(
                        headers["authorization"],
                        "Bearer fixture-narrow-admin-reader"
                    );
                    let now = chrono::Utc::now().timestamp();
                    Json(SpaceAdmission {
                        inheritance: None,
                        schema_version: 1,
                        identity: request.identity.clone(),
                        identity_active: true,
                        policy_revision: 1,
                        policy: if request.identity.subject == all_subject.to_string() {
                            SpaceAccessPolicy::All {}
                        } else {
                            SpaceAccessPolicy::Selected {
                                spaces: if request.identity.subject == owner_subject.to_string() {
                                    vec![space]
                                } else {
                                    vec![]
                                },
                            }
                        },
                        validity: AdmissionValidity::confirmed(now, now + 60, now + 60).unwrap(),
                        spaces: request
                            .spaces
                            .into_iter()
                            .map(|space| ConfirmedSpace {
                                space,
                                revision: 1,
                                state: SpaceState::Active,
                            })
                            .collect(),
                    })
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner_origin = format!("http://{}", listener.local_addr().unwrap());
    let owner_server = tokio::spawn(async move { axum::serve(listener, owner).await.unwrap() });
    unsafe {
        std::env::set_var(
            "CICD_SPACES__ADMISSION_URL",
            format!("{owner_origin}/effective"),
        );
        std::env::set_var(
            "CICD_SPACES__ADMISSION_TOKEN",
            "fixture-narrow-admin-reader",
        );
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let read = |repository: String| {
        http.get(format!(
            "{origin}/git/{repository}/info/refs?service=git-upload-pack"
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .send()
    };
    // The old public behavior is distinct and remains available only while OFF.
    assert_eq!(
        http.get(format!(
            "{origin}/git/{other_name}/info/refs?service=git-upload-pack"
        ))
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    sqlx::query("UPDATE forge_space_admission_state SET active=true")
        .execute(pool)
        .await
        .unwrap();
    profile.write().unwrap().scopes = vec!["git:read", "git:write", "ci-cd:read"];
    sqlx::query("DELETE FROM project_memberships WHERE user_id IN (SELECT id FROM users WHERE central_sub=$1)")
        .bind(human.subject.to_string()).execute(pool).await.unwrap();
    sqlx::query("UPDATE users SET role='viewer' WHERE central_sub=$1")
        .bind(human.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    for alias in [name.to_owned(), format!("{canonical}/repository")] {
        let response = read(alias).await.unwrap();
        assert_eq!(
            response.status(),
            200,
            "common admission grants reading without an old membership"
        );
        assert_eq!(response.headers()["cache-control"], "private, no-store");
    }
    assert_eq!(
        http.get(format!(
            "{origin}/git/{name}/info/refs?service=git-receive-pack"
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .send()
        .await
        .unwrap()
        .status(),
        403,
        "common read does not upgrade a viewer"
    );
    for alias in [
        other_name.clone(),
        format!("{other_canonical}/repository"),
        unbound_name.clone(),
    ] {
        assert_eq!(
            read(alias).await.unwrap().status(),
            403,
            "public/alias/unbound cannot bypass Space policy"
        );
    }
    assert_eq!(
        http.get(format!(
            "{origin}/git/{other_name}/info/refs?service=git-upload-pack"
        ))
        .send()
        .await
        .unwrap()
        .status(),
        401,
        "public requires central identity after activation"
    );
    let legacy = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        cicd::git_host::GitConfig {
            root: root.to_owned(),
            token: Some("fixture-legacy".into()),
            internal_token: None,
        },
        Some("fixture-auth-secret-not-a-live-secret".into()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let legacy_origin = format!("http://{}", listener.local_addr().unwrap());
    let legacy_server = tokio::spawn(async move { axum::serve(listener, legacy).await.unwrap() });
    assert_eq!(
        http.get(format!(
            "{legacy_origin}/git/{name}/info/refs?service=git-upload-pack"
        ))
        .header("x-git-token", "fixture-legacy")
        .send()
        .await
        .unwrap()
        .status(),
        401,
        "configured legacy operator token is not common admission"
    );
    assert_eq!(
        http.post(format!("{origin}/git/{other_name}/git-upload-pack"))
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .header("content-type", "application/x-git-upload-pack-request")
            .body("0000")
            .send()
            .await
            .unwrap()
            .status(),
        403,
        "RPC shares the policy"
    );
    assert_eq!(
        http.get(format!(
            "{origin}/git/{name}/info/refs?service=git-upload-pack"
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .header("x-forge-task-git-context", "{}")
        .send()
        .await
        .unwrap()
        .status(),
        403,
        "V1 task context cannot borrow human admission"
    );
    let page = catalog(&http, origin, "?limit=1").await;
    assert_eq!(
        page.total, 1,
        "count is filtered, not only the returned page"
    );
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, repository);
    let later = catalog(&http, origin, "?limit=1&offset=1").await;
    assert_eq!(later.total, 1);
    assert!(later.items.is_empty());
    let hidden_group =
        sqlx::query_scalar::<_, Uuid>("SELECT group_id FROM repository_catalog WHERE id=$1")
            .bind(other_repo.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let hidden = catalog(&http, origin, &format!("?group_id={hidden_group}&limit=1")).await;
    assert_eq!(hidden.total, 0);
    assert!(hidden.items.is_empty());
    assert_eq!(
        catalog(
            &http,
            origin,
            &format!("?search={}", other_canonical.to_uppercase())
        )
        .await
        .total,
        0
    );
    assert_eq!(
        catalog(
            &http,
            origin,
            &format!("?search={}", canonical.to_uppercase())
        )
        .await
        .total,
        1
    );
    assert_eq!(
        catalog(&http, origin, "?search=%25_").await.total,
        0,
        "search remains literal"
    );
    let visible = detail(&http, origin, &repository.to_string(), 200)
        .await
        .unwrap();
    assert_eq!(visible.id, repository);
    assert_eq!(
        visible.namespace.as_ref().unwrap().namespace_id,
        space.space_id
    );
    let visible_group = visible.group_id.unwrap();
    let owner_page = group_page(&http, origin, visible_group, "?limit=1").await;
    assert_eq!(owner_page.total, 1);
    assert_eq!(owner_page.items[0].id, repository);
    let next = group_page(&http, origin, visible_group, "?limit=1&offset=1").await;
    assert_eq!(next.total, 1);
    assert!(next.items.is_empty());
    let hidden = group_page(&http, origin, hidden_group, "?limit=1").await;
    assert_eq!(hidden.total, 0);
    assert!(hidden.items.is_empty());
    assert_eq!(
        group_page(&http, origin, visible_group, "?search=%25_")
            .await
            .total,
        0
    );
    detail(&http, origin, &other_repo.id.to_string(), 404).await;
    detail(&http, origin, &unbound_repo.id.to_string(), 404).await;
    detail(
        &http,
        origin,
        &repository.to_string().replace('-', "%2D"),
        200,
    )
    .await;
    detail(
        &http,
        origin,
        &other_repo.id.to_string().replace('-', "%2D"),
        404,
    )
    .await;
    verify_repository_reads(
        pool,
        &http,
        origin,
        (name, &canonical, repository),
        (&other_name, other_repo.id),
        &profile,
    )
    .await;
    repository_read(
        &http,
        origin,
        &format!("/api/v1/repos/{unbound_name}/tree"),
        403,
    )
    .await;
    let clone_url = visible.clone_url.unwrap();
    assert_eq!(
        clone_url,
        format!("{origin}/git/{canonical}/repository.git")
    );
    // The allowed side of RPC is exercised by a real clone while the local role is viewer.
    let cloned = tokio::process::Command::new("git")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("-c")
        .arg("http.extraHeader=Authorization: Bearer sdlc_pat_fixture_product_roles")
        .args(["clone", &clone_url])
        .arg(root.join("common-reader"))
        .output()
        .await
        .unwrap();
    assert!(
        cloned.status.success(),
        "common viewer clone: {}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    // all is evaluated for a Space created after the first admitted request.
    profile.write().unwrap().subject = all_subject;
    assert_eq!(read(name.to_owned()).await.unwrap().status(), 200);
    assert_eq!(
        catalog(&http, origin, "?limit=1").await.total,
        2,
        "all includes bound groups, not unbound data"
    );
    assert_eq!(
        detail(&http, origin, &other_repo.id.to_string(), 200)
            .await
            .unwrap()
            .id,
        other_repo.id
    );
    assert_eq!(
        group_page(&http, origin, hidden_group, "?limit=1")
            .await
            .total,
        1
    );
    let reviews = repository_read(
        &http,
        origin,
        &format!("/api/v1/repos/{other_name}/pulls"),
        200,
    )
    .await;
    assert_eq!(
        reviews[0]["title"], "Hidden review",
        "all admits the legitimate neighboring Space"
    );
    let future = SpaceRef {
        registry_instance_id: space.registry_instance_id,
        space_id: Uuid::new_v4(),
    };
    let future_name = format!("future-{}", Uuid::new_v4().simple());
    let future_repo =
        cicd::git_host::create_repository_core(pool, &config, &future_name, Some("private"))
            .await
            .unwrap();
    attach(pool, future_repo.id, &future, &owner_fixture).await;
    assert_eq!(
        read(future_name.clone()).await.unwrap().status(),
        200,
        "all includes a future Space"
    );
    assert_eq!(
        catalog(&http, origin, "?limit=1").await.total,
        3,
        "all includes a later group before pagination"
    );
    *profile.write().unwrap() = human.clone();
    assert_eq!(
        catalog(&http, origin, "?limit=1").await.total,
        1,
        "selected filters the future catalog"
    );
    assert_eq!(
        read(future_name).await.unwrap().status(),
        403,
        "selected does not include the future Space"
    );
    // Different immutable identity has no policy; cached selected access is not transferable.
    profile.write().unwrap().subject = Uuid::new_v4();
    let empty = catalog(&http, origin, "?limit=1").await;
    assert_eq!(empty.total, 0);
    assert!(
        empty.items.is_empty(),
        "selected empty cannot become unrestricted SQL NULL"
    );
    let empty_subject = profile.read().unwrap().subject.to_string();
    sqlx::query("UPDATE users SET role='admin' WHERE central_sub=$1")
        .bind(empty_subject)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        catalog(&http, origin, "?limit=1").await.total,
        0,
        "a local admin role cannot bypass selected empty visibility"
    );

    assert_eq!(
        read(name.to_owned()).await.unwrap().status(),
        403,
        "selected empty denies"
    );
    detail(&http, origin, &repository.to_string(), 404).await;
    repository_read(&http, origin, &format!("/api/v1/repos/{name}/pulls"), 403).await;

    assert_eq!(
        group_page(&http, origin, visible_group, "?limit=1")
            .await
            .total,
        0
    );
    *profile.write().unwrap() = human.clone();
    profile.write().unwrap().role = "service_account".into();
    assert_eq!(
        read(name.to_owned()).await.unwrap().status(),
        403,
        "human path does not accept machine identity"
    );
    *profile.write().unwrap() = human.clone();
    sqlx::query("UPDATE users SET role='developer' WHERE central_sub=$1")
        .bind(human.subject.to_string())
        .execute(pool)
        .await
        .unwrap();
    super::git_pr_admission::verify(
        pool,
        &profile,
        root,
        origin,
        (name, repository, &canonical),
        (&other_name, other_repo.id),
    )
    .await;
    let push = || async {
        tokio::process::Command::new("git")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("-C")
            .arg(root.join("client"))
            .arg("-c")
            .arg("http.extraHeader=Authorization: Bearer sdlc_pat_fixture_product_roles")
            .args(["push", "origin", "HEAD:refs/heads/task/common-space"])
            .output()
            .await
            .unwrap()
    };
    let missing_application_scope = push().await;
    assert!(!missing_application_scope.status.success());
    assert!(
        String::from_utf8_lossy(&missing_application_scope.stderr).contains("403"),
        "legacy git:write cannot replace central ci-cd:write"
    );
    profile.write().unwrap().scopes = vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"];
    let pushed = tokio::process::Command::new("git")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("-C")
        .arg(root.join("client"))
        .arg("-c")
        .arg("http.extraHeader=Authorization: Bearer sdlc_pat_fixture_product_roles")
        .args(["push", "origin", "HEAD:refs/heads/task/common-space"])
        .output()
        .await
        .unwrap();
    assert!(
        pushed.status.success(),
        "common admitted write: {}",
        String::from_utf8_lossy(&pushed.stderr)
    );
    // Real owner archive advances local revision while the original read proof
    // is still valid. This does not revoke visibility of the same immutable Space.
    let mut command: SpaceOwnerCommandV2 = serde_json::from_value(
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT command FROM forge_space_catalog WHERE registry_instance_id=$1 AND space_id=$2",
        )
        .bind(space.registry_instance_id)
        .bind(space.space_id)
        .fetch_one(pool)
        .await
        .unwrap(),
    )
    .unwrap();
    command.revision += 1;
    command.operation_id = Uuid::new_v4();
    command.desired_state = DesiredSpaceState::Archived;
    let owner = Uuid::new_v4();
    unsafe {
        std::env::set_var("CICD_NAMESPACE__OWNER_SUBJECTS", owner.to_string());
    }
    *profile.write().unwrap() = Profile {
        subject: owner,
        role: "service_account".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:write"],
    };
    let archived = http
        .put(format!(
            "{origin}/api/v2/spaces/{}/{}/owner-state",
            space.registry_instance_id, space.space_id
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .json(&command)
        .send()
        .await
        .unwrap();
    let status = archived.status();
    let text = archived.text().await.unwrap();
    assert_eq!(status, 200, "actual owner archive: {text}");
    let receipt: SpaceOwnerReadbackV2 = serde_json::from_str(&text).unwrap();
    assert!(
        receipt.drained && receipt.matches(&command),
        "archive requires real owner/drain confirmation"
    );
    *profile.write().unwrap() = human;
    assert_eq!(
        read(name.to_owned()).await.unwrap().status(),
        200,
        "archive changes write lifecycle, not the original confirmed read visibility"
    );
    assert_eq!(
        http.get(format!(
            "{origin}/git/{name}/info/refs?service=git-receive-pack"
        ))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .send()
        .await
        .unwrap()
        .status(),
        409,
        "the same admitted identity cannot write after confirmed archive"
    );
    let saved = profile.read().unwrap().clone();
    profile.write().unwrap().scopes = vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"];
    let archived_write=http.post(format!("{origin}/api/v1/catalog/repositories/{repository}/pulls"))
        .bearer_auth("sdlc_pat_fixture_product_roles")
        .json(&serde_json::json!({"title":"archived denied","source_branch":"task/current","target_branch":"main"}))
        .send().await.unwrap();
    assert!(
        matches!(archived_write.status().as_u16(), 409 | 503),
        "archived or stale owner cannot authorize PR creation"
    );
    *profile.write().unwrap() = saved;
    let archived_page = catalog(&http, origin, "?limit=1").await;
    assert_eq!(archived_page.total, 1);
    assert_eq!(archived_page.items[0].state.as_deref(), Some("archived"));
    assert_eq!(
        catalog(&http, origin, "?sync_available=true&limit=1")
            .await
            .total,
        0
    );
    repository_read(
        &http,
        origin,
        &format!("/api/v1/repos/{name}/blob?ref=task/current&path=data"),
        200,
    )
    .await;
    repository_read(
        &http,
        origin,
        &format!("/api/v1/catalog/repositories/{repository}/pulls/1"),
        200,
    )
    .await;
    profile.write().unwrap().expires_in = 2;
    let mut wait = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE pull_requests IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *wait)
        .await
        .unwrap();
    let client = http.clone();
    let url = format!("{origin}/api/v1/catalog/repositories/{repository}/pulls/1");
    let delayed = tokio::spawn(async move {
        client
            .get(url)
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT id, repository_id, repository_name, number, title%')")
                .fetch_one(pool).await.unwrap();
            if waiting {break}
            assert!(!delayed.is_finished(),"protected PR query must reach SQL wait");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("actual protected PR query waits");
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    wait.rollback().await.unwrap();
    let expired = delayed.await.unwrap().unwrap();
    let status = expired.status();
    let body = expired.text().await.unwrap();
    assert_eq!(status, 403, "original deadline after PR SQL wait: {body}");
    assert!(
        !body.contains("Allowed review"),
        "expired read returns no PR metadata"
    );
    profile.write().unwrap().expires_in = 300;
    repository_read(
        &http,
        origin,
        &format!("/api/v1/catalog/repositories/{repository}/pulls/1"),
        200,
    )
    .await;
    println!(
        "REST repository archived read and original proof expiry during actual PR SQL wait: PASS"
    );
    // Expire the original actor proof while the actual count query waits.
    profile.write().unwrap().expires_in = 2;
    let mut blocked = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE repository_catalog IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocked)
        .await
        .unwrap();
    let delayed_http = http.clone();
    let delayed_url = format!("{origin}/api/v1/catalog/repositories?limit=1");
    let delayed = tokio::spawn(async move {
        delayed_http
            .get(delayed_url)
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT count(*) FROM repository_catalog r%')")
                .fetch_one(pool).await.unwrap();
            if waiting {break;}
            assert!(!delayed.is_finished(),"request must reach the protected count query");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("catalog reached the actual SQL lock");
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    blocked.rollback().await.unwrap();
    let expired = delayed.await.unwrap().unwrap();
    let status = expired.status();
    let denied = expired.text().await.unwrap();
    assert_eq!(
        status, 403,
        "original proof expired after count IO: {denied}"
    );
    assert!(
        !denied.contains("storage_name"),
        "no protected catalog body on expiry"
    );
    profile.write().unwrap().expires_in = 300;
    assert_eq!(
        catalog(&http, origin, "?limit=1").await.total,
        1,
        "expired caller cannot poison a new proof or delete legitimate rows"
    );
    assert_eq!(
        detail(&http, origin, &repository.to_string(), 200)
            .await
            .unwrap()
            .state
            .as_deref(),
        Some("archived")
    );
    assert_eq!(
        group_page(&http, origin, visible_group, "?limit=1")
            .await
            .items[0]
            .state
            .as_deref(),
        Some("archived")
    );
    println!(
        "REST catalog detail/group counts, encoded IDs, API clone URL and archived read: PASS"
    );
    println!(
        "REST catalog original proof expires during real SQL wait without returning metadata: PASS"
    );
    println!(
        "REST catalog count/page, selected/all/future/empty, literal search and archived read: PASS"
    );
    println!("Git common admitted push and actual owner archive preserve read-only access: PASS");
    sqlx::query("UPDATE forge_space_admission_state SET active=false")
        .execute(pool)
        .await
        .unwrap();
    unsafe {
        match previous_origin {
            Some(value) => std::env::set_var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN", value),
            None => std::env::remove_var("CICD_NAMESPACE__PUBLIC_GIT_ORIGIN"),
        }
    }
    owner_server.abort();
    legacy_server.abort();
    let _ = owner_server.await;
    let _ = legacy_server.await;
    println!(
        "Git common Space admission: selected/all/future, public, aliases, real clone/RPC, legacy, unbound, roles and machine fences: PASS"
    );
}
