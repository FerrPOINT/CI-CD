//! Real stored credential lookup against the complete current migration chain.
use chrono::Utc;
use cicd_app::product_token::{ProductTokenError, lookup};
use pdlc_auth_core::application_access::{
    ApplicationAccessV1, HumanRole, VerifiedApplicationAccess,
};
use sqlx::postgres::PgPoolOptions;
use std::time::Instant;
use uuid::Uuid;
fn proof(subject: Uuid, apps: &[&str]) -> VerifiedApplicationAccess {
    let now = Utc::now();
    VerifiedApplicationAccess::confirm(
        ApplicationAccessV1 {
            schema_version: 1,
            issuer: "https://owner.test".into(),
            subject: subject.to_string(),
            role: HumanRole::User,
            applications: apps.iter().map(|key| key.to_string()).collect(),
            revision: 1,
            enforced: true,
            observed_at: now,
            expires_at: now + chrono::Duration::seconds(30),
        },
        "https://owner.test",
        &subject.to_string(),
        Instant::now(),
        now,
    )
    .unwrap()
}
#[tokio::test]
#[ignore = "requires disposable FORGE_PRODUCT_TOKEN_TEST_DATABASE_URL"]
async fn original_product_token_keeps_scope_project_and_current_owner_limits() {
    let url = std::env::var("FORGE_PRODUCT_TOKEN_TEST_DATABASE_URL").unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "forge_product_token_test", "refuse working database");
    let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
    sqlx::migrate::Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let user = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let token_id = Uuid::new_v4();
    let project = Uuid::new_v4();
    let token = "cicd_owned_disposable_fixture";
    sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'fixture','admin',true,$2)")
        .bind(user).bind(owner.to_string()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url) VALUES($1,'fixture','https://fixture.invalid/repo')")
        .bind(project).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,project_id,scopes,expires_at)
        VALUES($1,'fixture',$2,'fixture',$3,$4,ARRAY['git:read'],clock_timestamp()+interval '60 seconds')")
        .bind(token_id).bind(cicd_app::auth::hash_token(token)).bind(user).bind(project)
        .execute(&pool).await.unwrap();
    for old_role in ["admin", "maintainer", "developer", "viewer"] {
        sqlx::query("UPDATE users SET role=$1 WHERE id=$2")
            .bind(old_role)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let local = lookup(&pool, token).await.unwrap();
        assert_eq!(local.owner(), owner);
        let value = local.confirm(proof(owner, &["ci-cd"])).unwrap();
        assert_eq!(value.claims().role, "user");
        assert_eq!(value.claims().sub, user);
        assert_eq!(value.claims().token_id, Some(token_id));
        assert_eq!(value.claims().token_project_id, Some(project));
        assert_eq!(value.claims().token_scopes, vec!["git:read"]);
        let saved: String = sqlx::query_scalar("SELECT role FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(saved, old_role, "historical assignments remain data");
    }
    assert!(
        lookup(&pool, token)
            .await
            .unwrap()
            .confirm(proof(owner, &[]))
            .is_err()
    );
    assert!(
        lookup(&pool, token)
            .await
            .unwrap()
            .confirm(proof(Uuid::new_v4(), &["ci-cd"]))
            .is_err()
    );
    sqlx::query("UPDATE api_tokens SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        lookup(&pool, token).await,
        Err(ProductTokenError::Invalid)
    ));
    sqlx::query("UPDATE api_tokens SET revoked_at=NULL,expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(token_id).execute(&pool).await.unwrap();
    assert!(matches!(
        lookup(&pool, token).await,
        Err(ProductTokenError::Invalid)
    ));
    sqlx::query("UPDATE api_tokens SET expires_at=NULL WHERE id=$1")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    let current = lookup(&pool, token)
        .await
        .unwrap()
        .confirm(proof(owner, &["ci-cd"]))
        .unwrap();
    assert!(current.claims().exp <= (Utc::now() + chrono::Duration::seconds(30)).timestamp());
    sqlx::query("UPDATE users SET enabled=false WHERE id=$1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        lookup(&pool, token).await,
        Err(ProductTokenError::Invalid)
    ));
    for mapping in [
        None,
        Some("old-email@example.test"),
        Some("00000000-0000-0000-0000-000000000000"),
    ] {
        sqlx::query("UPDATE users SET enabled=true,central_sub=$1 WHERE id=$2")
            .bind(mapping)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            lookup(&pool, token).await,
            Err(ProductTokenError::Denied)
        ));
    }
    assert!(matches!(
        lookup(&pool, "forge_sat_other_fixture").await,
        Err(ProductTokenError::Invalid)
    ));
    assert!(matches!(
        lookup(&pool, "cicd_unknown_fixture").await,
        Err(ProductTokenError::Invalid)
    ));
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users),
        (SELECT count(*) FROM api_tokens)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1), "no replacement identities or credentials");
    pool.close().await;
}
