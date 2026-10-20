//! Native profile persistence plus the real fresh SDK HTTP verification path.
#[path = "support/git_scope.rs"]
mod git_scope;
#[path = "support/group_drain.rs"]
mod group_drain;
use axum::{Json, Router, routing::get};
use cicd::central_auth::{link_central_user, try_verified_central};
use sqlx::PgPool;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
#[derive(Clone)]
struct Profile {
    subject: Uuid,
    role: String,
    expires_in: i64,
    scopes: Vec<&'static str>,
}
#[tokio::test]
#[ignore = "requires owned forge_space_test PostgreSQL and isolated test process"]
async fn unified_central_profile_keeps_local_roles_and_rejects_machine_or_expired_identity() {
    let url = std::env::var("FORGE_SHARED_SPACE_TEST_DATABASE_URL").expect("owned test database");
    let root = PgPool::connect(&url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&root)
        .await
        .unwrap();
    assert_eq!(database, "forge_space_test");
    let schema = format!("forge_role_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&root)
        .await
        .unwrap();
    let own = schema.clone();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .after_connect(move |connection, _| {
            let schema = own.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {schema}"))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    cicd::migrations().await.unwrap().run(&pool).await.unwrap();
    let profile = Arc::new(RwLock::new(Profile {
        subject: Uuid::new_v4(),
        role: "member".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    }));
    let expected = profile.clone();
    let router=Router::new().route("/oidc/jwks",get(||async{Json(serde_json::json!({"keys":[]}))}))
 .route("/auth/tokens/introspect",get(move|uri:axum::http::Uri,headers:axum::http::HeaderMap|{
  let profile=expected.read().unwrap().clone();async move{
   assert_eq!(uri.query(),Some("include_lifetime=true"));
   assert_eq!(headers.get("authorization").unwrap(),"Bearer sdlc_pat_fixture_product_roles");
   Json(serde_json::json!({"sub":profile.subject,"email":"fixture@example.test","subject_role":profile.role,"scopes":profile.scopes,"observed_at":chrono::Utc::now(),"expires_at":chrono::Utc::now()+chrono::Duration::seconds(profile.expires_in)}))
  }
 }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    unsafe {
        std::env::set_var("CICD_AUTH__CENTRAL_JWKS_URI", format!("{origin}/oidc/jwks"));
        std::env::set_var("CICD_AUTH__CENTRAL_ISSUER", &origin);
    }
    let actor = try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    let central = profile.read().unwrap().subject;
    let local = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'Preserved user','viewer',true,$2)").bind(local).bind(central.to_string()).execute(&pool).await.unwrap();
    for role in ["viewer", "developer", "maintainer", "admin"] {
        sqlx::query("UPDATE users SET role=$1 WHERE id=$2")
            .bind(role)
            .bind(local)
            .execute(&pool)
            .await
            .unwrap();
        let claims = link_central_user(&pool, &actor).await.unwrap();
        assert_eq!(
            claims.sub, local,
            "immutable subject retains the existing random local UUID"
        );
        assert_eq!(
            claims.role, role,
            "SSO must not synthesize admin or overwrite a confirmed product role"
        );
        assert_eq!(
            claims.exp,
            actor.lifetime.local_valid_until(),
            "local claims cannot outlive the original token"
        );
        let stored: (String, String) =
            sqlx::query_as("SELECT username,role FROM users WHERE id=$1")
                .bind(local)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, ("Preserved user".into(), role.into()));
    }
    let row_lock = pool.begin().await.unwrap();
    let mut row_lock = row_lock;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(local)
        .execute(&mut *row_lock)
        .await
        .unwrap();
    let unchanged = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        link_central_user(&pool, &actor),
    )
    .await
    .expect("existing profile mapping must remain a read")
    .unwrap();
    assert_eq!(unchanged.sub, local);
    row_lock.rollback().await.unwrap();
    // Even central platform-admin metadata does not replace the local product role.
    profile.write().unwrap().role = "admin".into();
    sqlx::query("UPDATE users SET role='viewer' WHERE id=$1")
        .bind(local)
        .execute(&pool)
        .await
        .unwrap();
    let platform_admin = try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        link_central_user(&pool, &platform_admin)
            .await
            .unwrap()
            .role,
        "viewer"
    );
    sqlx::query("UPDATE users SET enabled=false WHERE id=$1")
        .bind(local)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        link_central_user(&pool, &platform_admin).await.is_err(),
        "SSO cannot re-enable the local profile"
    );
    let machine = Uuid::new_v4();
    *profile.write().unwrap() = Profile {
        subject: machine,
        role: "service_account".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    };
    let machine = try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    assert!(link_central_user(&pool, &machine).await.is_err());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE central_sub=$1")
        .bind(&machine.context.user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "machine authentication cannot JIT a human profile");
    let fresh = Uuid::new_v4();
    *profile.write().unwrap() = Profile {
        subject: fresh,
        role: "member".into(),
        expires_in: 300,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    };
    let fresh = try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        link_central_user(&pool, &fresh).await.unwrap().role,
        "developer",
        "preserve the existing new-profile default without admin escalation"
    );
    let expired = Uuid::new_v4();
    *profile.write().unwrap() = Profile {
        subject: expired,
        role: "member".into(),
        expires_in: 2,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    };
    let expired = try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    assert!(link_central_user(&pool, &expired).await.is_err());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE central_sub=$1")
        .bind(&expired.context.user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "expired authority cannot create a profile");
    let waiting_subject = Uuid::new_v4();
    *profile.write().unwrap() = Profile {
        subject: waiting_subject,
        role: "member".into(),
        expires_in: 2,
        scopes: vec!["ci-cd:read", "ci-cd:write", "git:read", "git:write"],
    };
    let waiting_proof = try_verified_central("sdlc_pat_fixture_product_roles")
        .await
        .unwrap()
        .unwrap();
    let mut table_lock = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE users IN SHARE MODE")
        .execute(&mut *table_lock)
        .await
        .unwrap();
    let writer_pool = pool.clone();
    let waiting =
        tokio::spawn(async move { link_central_user(&writer_pool, &waiting_proof).await });
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    table_lock.commit().await.unwrap();
    assert!(
        waiting.await.unwrap().is_err(),
        "expired JIT cannot succeed after its INSERT lock wait"
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE central_sub=$1")
        .bind(waiting_subject.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        n, 0,
        "late JIT INSERT is rolled back, not just hidden by an error response"
    );
    git_scope::verify(&pool, profile.clone()).await;
    group_drain::verify(&pool).await;
    server.abort();
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&root)
        .await
        .unwrap();
    root.close().await;
}
