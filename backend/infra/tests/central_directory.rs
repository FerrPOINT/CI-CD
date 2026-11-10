//! Manual insert-only working-directory projection on the real PostgreSQL schema.
use cicd_infra::central_directory::project_directory_users;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::str::FromStr;
use uuid::Uuid;
#[path = "../../migration/catalog.rs"]
mod catalog;
#[tokio::test]
#[ignore = "manual: owned PostgreSQL"]
async fn directory_reads_preserve_existing_privileges_disable_state_and_jit_winners() {
    let url = std::env::var("FORGE_SHARED_SPACE_TEST_DATABASE_URL").unwrap();
    let root = PgPool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_database()")
            .fetch_one(&root)
            .await
            .unwrap(),
        "forge_space_test"
    );
    let schema = format!("directory_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&root)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(
            PgConnectOptions::from_str(&url)
                .unwrap()
                .options([("search_path", schema.as_str())]),
        )
        .await
        .unwrap();
    catalog::load(std::path::Path::new("../migrations"))
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let result=tokio::spawn({
        let pool=pool.clone();
        async move{
            let old=Uuid::new_v4();
            sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'retained-name','maintainer',false,'existing')")
                .bind(old).execute(&pool).await.unwrap();
            let subjects=vec!["existing".to_owned(),"new-a".to_owned(),"new-b".to_owned(),"new-a".to_owned()];
            let first=project_directory_users(&pool,&subjects).await.unwrap();
            assert_eq!(first.len(),3);
            let retained=first.iter().find(|u|u.central_sub=="existing").unwrap();
            assert_eq!(retained.id,old);assert_eq!(retained.role,"maintainer");assert!(!retained.enabled);assert_eq!(retained.username,"retained-name");
            let first:Vec<_>=first.into_iter().map(|u|(u.central_sub,u.id,u.role,u.enabled,u.created_at)).collect();
            let second=project_directory_users(&pool,&subjects).await.unwrap();
            for u in second {assert!(first.contains(&(u.central_sub,u.id,u.role,u.enabled,u.created_at)));}
            assert!(project_directory_users(&pool,&[]).await.unwrap().is_empty());
            let jit=Uuid::new_v4();let race_subjects=vec!["jit-race".to_owned()];
            let (directory,inserted)=tokio::join!(
                project_directory_users(&pool,&race_subjects),
                sqlx::query_scalar::<_,Uuid>("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,'jit-winner','developer',true,'jit-race') ON CONFLICT(central_sub) WHERE central_sub IS NOT NULL DO NOTHING RETURNING id").bind(jit).fetch_optional(&pool)
            );
            let projected=directory.unwrap().remove(0);
            if let Some(id)=inserted.unwrap(){assert_eq!(projected.id,id);}
            let actual:(Uuid,String,bool)=sqlx::query_as("SELECT id,role,enabled FROM users WHERE central_sub='jit-race'").fetch_one(&pool).await.unwrap();
            assert_eq!((projected.id,projected.role,projected.enabled),actual);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM users WHERE central_sub='jit-race'").fetch_one(&pool).await.unwrap(),1);
        }
    }).await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&root)
        .await
        .unwrap();
    root.close().await;
    result.unwrap();
}
