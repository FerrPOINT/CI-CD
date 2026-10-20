//! Real private Smart HTTP discovery: central PAT scopes stay independent of local token IDs.
use super::Profile;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub(super) async fn verify(pool: &sqlx::PgPool, profile: Arc<RwLock<Profile>>) {
    *profile.write().unwrap() = Profile {
        subject: Uuid::new_v4(),
        role: "member".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    };
    let verified = cicd::central_auth::try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    let actor = cicd::central_auth::link_central_user(pool, &verified)
        .await
        .unwrap();
    assert_eq!(actor.role, "developer");
    assert!(
        actor.token_id.is_none(),
        "central PAT must not invent a local API-token row"
    );
    let root = std::env::temp_dir().join(format!("forge-git-scope-{}", Uuid::new_v4()));
    let config = cicd::git_host::GitConfig {
        root: root.clone(),
        token: None,
        internal_token: None,
    };
    let name = format!("scoped-{}", Uuid::new_v4().simple());
    let repo = cicd::git_host::create_repository_core(pool, &config, &name, Some("private"))
        .await
        .unwrap();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id) VALUES($1,$2,'https://example.invalid/source.git',$3)")
        .bind(project).bind(format!("scope-{project}")).bind(repo.id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO project_memberships(project_id,user_id,role) VALUES($1,$2,'viewer')")
        .bind(project)
        .bind(actor.sub)
        .execute(pool)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = cicd::api::app_with_git_and_auth_secret(
        Some(pool.clone()),
        config,
        Some("fixture-auth-secret-not-a-live-secret".into()),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let request = |service: &str| {
        http.get(format!("{origin}/git/{name}/info/refs?service={service}"))
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
    };
    profile.write().unwrap().scopes = vec!["ci-cd:read"];
    let denied = request("git-upload-pack").await.unwrap();
    assert_eq!(
        denied.status(),
        reqwest::StatusCode::FORBIDDEN,
        "a central PAT without git:read must not become an unrestricted browser session"
    );
    profile.write().unwrap().scopes = vec!["git:read"];
    let read = request("git-upload-pack").await.unwrap();
    assert_eq!(read.status(), reqwest::StatusCode::OK);
    assert_eq!(
        read.headers()["content-type"],
        "application/x-git-upload-pack-advertisement"
    );
    let read_only_write = request("git-receive-pack").await.unwrap();
    assert_eq!(read_only_write.status(), reqwest::StatusCode::FORBIDDEN);
    profile.write().unwrap().scopes = vec!["git:write"];
    assert_eq!(
        request("git-upload-pack").await.unwrap().status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(
        request("git-receive-pack").await.unwrap().status(),
        reqwest::StatusCode::FORBIDDEN,
        "write scope does not replace the confirmed project role"
    );
    sqlx::query(
        "UPDATE project_memberships SET role='developer' WHERE project_id=$1 AND user_id=$2",
    )
    .bind(project)
    .bind(actor.sub)
    .execute(pool)
    .await
    .unwrap();
    let write = request("git-receive-pack").await.unwrap();
    assert_eq!(write.status(), reqwest::StatusCode::OK);
    assert_eq!(
        write.headers()["content-type"],
        "application/x-git-receive-pack-advertisement"
    );
    // Hold the real ACL query until the original introspection window expires.
    // The successful membership lookup must not mint a new token lifetime.
    profile.write().unwrap().scopes = vec!["git:read"];
    profile.write().unwrap().expires_in = 1;
    let mut blocked = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE project_memberships IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocked)
        .await
        .unwrap();
    let delayed_http = http.clone();
    let delayed_url = format!("{origin}/git/{name}/info/refs?service=git-upload-pack");
    let delayed = tokio::spawn(async move {
        delayed_http
            .get(delayed_url)
            .bearer_auth("sdlc_pat_fixture_product_roles")
            .send()
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT m.role FROM projects p JOIN project_memberships%')")
                .fetch_one(pool).await.unwrap();
            if waiting { break; }
            assert!(!delayed.is_finished(), "request must reach the real ACL lock");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("Git request reached blocked membership query");
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    blocked.rollback().await.unwrap();
    let expired = delayed.await.unwrap().unwrap();
    assert_eq!(
        expired.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "a token that expired during ACL IO cannot authorize Git discovery"
    );
    profile.write().unwrap().expires_in = 300;
    server.abort();
    let _ = server.await;
    assert!(
        root.starts_with(std::env::temp_dir())
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("forge-git-scope-")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
