//! Disposable database fixture teardown is separate from product deletion policy.
pub async fn projects(pool: &sqlx::PgPool) {
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(pool)
        .await
        .expect("read fixture schema");
    assert!(
        schema.starts_with("it_"),
        "teardown requires an isolated integration schema"
    );
    sqlx::query("TRUNCATE projects CASCADE")
        .execute(pool)
        .await
        .expect("clear disposable fixture rows");
}
