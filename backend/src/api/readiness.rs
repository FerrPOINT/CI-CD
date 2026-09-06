//! Health/readiness/migration checks (ADR-0012).

use super::{AppState, READINESS_TIMEOUT};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Serialize;
use sqlx::FromRow;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "health",
    responses((status = 200, description = "Liveness"))
)]
pub(crate) async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "cicd"}))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct Readiness {
    pub(crate) status: String,
    pub(crate) service: String,
    pub(crate) database: String,
    pub(crate) migrations: MigrationReadiness,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct MigrationReadiness {
    pub(crate) status: String,
    pub(crate) latest_applied_version: Option<i64>,
    pub(crate) latest_required_version: i64,
    pub(crate) pending_versions: Vec<i64>,
    pub(crate) checksum_mismatches: Vec<i64>,
    pub(crate) unknown_applied_versions: Vec<i64>,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
struct ExpectedMigration {
    version: i64,
    checksum: Vec<u8>,
}

#[utoipa::path(
    get,
    path = "/api/v1/readiness",
    tag = "health",
    responses(
        (status = 200, description = "Database-aware readiness", body = Readiness),
        (status = 503, description = "Database or migrations are not ready", body = Readiness)
    )
)]
pub(crate) async fn readiness(State(state): State<Arc<AppState>>) -> axum::response::Response {
    let expected = match expected_migrations().await {
        Ok(expected) => expected,
        Err(_) => {
            return readiness_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                "unknown",
                migration_readiness_error(&[], "unknown", "migration source failed"),
            );
        }
    };
    let Some(db) = state.pool.as_ref() else {
        return readiness_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_ready",
            "unavailable",
            migration_readiness_error(&expected, "unknown", "database pool is not configured"),
        );
    };

    let database_probe = tokio::time::timeout(
        READINESS_TIMEOUT,
        sqlx::query_scalar::<_, i64>("SELECT 1::BIGINT").fetch_one(db),
    )
    .await;
    match database_probe {
        Ok(Ok(_)) => {}
        Ok(Err(_)) => {
            return readiness_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                "unavailable",
                migration_readiness_error(&expected, "unknown", "database query failed"),
            );
        }
        Err(_) => {
            return readiness_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                "unavailable",
                migration_readiness_error(&expected, "unknown", "database query timed out"),
            );
        }
    }

    let migrations =
        match tokio::time::timeout(READINESS_TIMEOUT, migration_readiness(db, &expected)).await {
            Ok(readiness) => readiness,
            Err(_) => migration_readiness_error(&expected, "unknown", "migration check timed out"),
        };
    let ready = migrations.status == "ok";
    readiness_response(
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        if ready { "ready" } else { "not_ready" },
        "ok",
        migrations,
    )
}

fn readiness_response(
    status: StatusCode,
    readiness_status: &str,
    database: &str,
    migrations: MigrationReadiness,
) -> axum::response::Response {
    (
        status,
        Json(Readiness {
            status: readiness_status.to_string(),
            service: "cicd".to_string(),
            database: database.to_string(),
            migrations,
        }),
    )
        .into_response()
}

async fn expected_migrations() -> Result<Vec<ExpectedMigration>, sqlx::migrate::MigrateError> {
    Ok(crate::migrations()
        .await?
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| ExpectedMigration {
            version: migration.version,
            checksum: migration.checksum.to_vec(),
        })
        .collect())
}

async fn migration_readiness(db: &PgPool, expected: &[ExpectedMigration]) -> MigrationReadiness {
    let applied: Vec<(i64, Vec<u8>)> = match sqlx::query_as(
        "SELECT version, checksum FROM _sqlx_migrations WHERE success ORDER BY version",
    )
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(_) => {
            return migration_readiness_error(
                expected,
                "unknown",
                "migration history query failed",
            );
        }
    };

    let latest_applied_version = applied.iter().map(|(version, _)| *version).max();
    let applied_by_version: HashMap<i64, Vec<u8>> = applied.into_iter().collect();
    let expected_versions: HashSet<i64> =
        expected.iter().map(|migration| migration.version).collect();
    let pending_versions = expected
        .iter()
        .filter(|migration| !applied_by_version.contains_key(&migration.version))
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let checksum_mismatches = expected
        .iter()
        .filter_map(|migration| {
            applied_by_version
                .get(&migration.version)
                .filter(|checksum| checksum.as_slice() != migration.checksum.as_slice())
                .map(|_| migration.version)
        })
        .collect::<Vec<_>>();
    let mut unknown_applied_versions = applied_by_version
        .keys()
        .copied()
        .filter(|version| !expected_versions.contains(version))
        .collect::<Vec<_>>();
    unknown_applied_versions.sort_unstable();

    let status = if checksum_mismatches.is_empty()
        && pending_versions.is_empty()
        && unknown_applied_versions.is_empty()
    {
        "ok"
    } else if !checksum_mismatches.is_empty() || !unknown_applied_versions.is_empty() {
        "mismatch"
    } else {
        "pending"
    };

    MigrationReadiness {
        status: status.to_string(),
        latest_applied_version,
        latest_required_version: latest_required_version(expected),
        pending_versions,
        checksum_mismatches,
        unknown_applied_versions,
        error: None,
    }
}

fn migration_readiness_error(
    expected: &[ExpectedMigration],
    status: &str,
    error: &str,
) -> MigrationReadiness {
    MigrationReadiness {
        status: status.to_string(),
        latest_applied_version: None,
        latest_required_version: latest_required_version(expected),
        pending_versions: Vec::new(),
        checksum_mismatches: Vec::new(),
        unknown_applied_versions: Vec::new(),
        error: Some(error.to_string()),
    }
}

fn latest_required_version(expected: &[ExpectedMigration]) -> i64 {
    expected
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or_default()
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema, serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct PageParams {
    /// Max items (1..=200, default 50).
    #[serde(default)]
    pub(crate) limit: Option<i64>,
    /// Offset (default 0).
    #[serde(default)]
    pub(crate) offset: Option<i64>,
}

impl PageParams {
    pub(crate) fn bounded(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(50).clamp(1, 200);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}
