//! Disposable actual PostgreSQL application used only for protocol QA.
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{Connection, Row};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct App {
    reader: String,
    writer: String,
    mode: String,
    delayed: AtomicBool,
}
async fn connection(url: &str) -> Result<sqlx::PgConnection, (StatusCode, &'static str)> {
    sqlx::PgConnection::connect(url)
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "database unavailable"))
}
async fn version() -> impl IntoResponse {
    match tokio::fs::read("/forge/manifest.json").await {
        Ok(bytes) => (StatusCode::OK, bytes),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Vec::new()),
    }
}
async fn identity(
    State(app): State<Arc<App>>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let mut conn = connection(&app.reader).await?;
    let row=sqlx::query("SELECT current_database() AS database,(SELECT max(version) FROM public._sqlx_migrations WHERE success) AS version")
        .fetch_one(&mut conn).await.map_err(|_|(StatusCode::SERVICE_UNAVAILABLE,"identity unavailable"))?;
    Ok(Json(
        json!({"database":row.get::<String,_>("database"),"schemaVersion":row.get::<i64,_>("version")}),
    ))
}
async fn health(State(app): State<Arc<App>>) -> Result<&'static str, (StatusCode, &'static str)> {
    if app.mode == "D" && !app.delayed.swap(true, Ordering::SeqCst) {
        println!("PG_HEALTH_DELAY_ENTERED");
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
    let mut conn = connection(&app.reader).await?;
    sqlx::query("SELECT 1")
        .execute(&mut conn)
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "database unavailable"))?;
    if app.mode == "H" {
        Err((StatusCode::SERVICE_UNAVAILABLE, "failed\n"))
    } else {
        Ok("ok\n")
    }
}
async fn acceptance(
    State(app): State<Arc<App>>,
) -> Result<&'static str, (StatusCode, &'static str)> {
    let mut conn = connection(&app.reader).await?;
    let rows = sqlx::query("SELECT id,payload FROM public.records WHERE id IN(1,2) ORDER BY id")
        .fetch_all(&mut conn)
        .await
        .map_err(|_| (StatusCode::UNPROCESSABLE_ENTITY, "rejected\n"))?;
    let valid = rows.len() == 2
        && rows[0].get::<i64, _>("id") == 1
        && rows[0].get::<String, _>("payload") == "alpha"
        && rows[1].get::<i64, _>("id") == 2
        && rows[1].get::<String, _>("payload") == "beta";
    if app.mode != "A" {
        sqlx::query("SELECT note FROM public.records LIMIT 1")
            .execute(&mut conn)
            .await
            .map_err(|_| (StatusCode::UNPROCESSABLE_ENTITY, "rejected\n"))?;
    }
    if valid && app.mode != "C" {
        Ok("accepted:records\n")
    } else {
        Err((StatusCode::UNPROCESSABLE_ENTITY, "rejected\n"))
    }
}
#[derive(Deserialize)]
struct NewRow {
    payload: String,
}
async fn create(
    State(app): State<Arc<App>>,
    Json(input): Json<NewRow>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    if input.payload.len() > 128 {
        return Err((StatusCode::BAD_REQUEST, "payload exceeds bound"));
    }
    let mut conn = connection(&app.writer).await?;
    let id =
        sqlx::query_scalar::<_, i64>("INSERT INTO public.records(payload) VALUES($1) RETURNING id")
            .bind(input.payload)
            .fetch_one(&mut conn)
            .await
            .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "writes closed"))?;
    Ok(Json(json!({"id":id})))
}
#[tokio::main]
async fn main() {
    let file = PathBuf::from(std::env::var("CICD_PG_CONNECTION_FILE").unwrap());
    let values: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let state = Arc::new(App {
        reader: values["readerUrl"].as_str().unwrap().to_owned(),
        writer: values["writerUrl"].as_str().unwrap().to_owned(),
        mode: std::fs::read_to_string("/app/mode")
            .unwrap()
            .trim()
            .to_owned(),
        delayed: AtomicBool::new(false),
    });
    let app = Router::new()
        .route("/.forge/version", get(version))
        .route("/identity", get(identity))
        .route("/health", get(health))
        .route("/acceptance", get(acceptance))
        .route("/rows", post(create))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
