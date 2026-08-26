#![allow(dead_code)]

use std::{collections::HashMap, process::Stdio, sync::Arc, time::Duration};

use sqlx::PgPool;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};
use uuid::Uuid;

use crate::api::ApiError;

/// Job processes currently executed by the embedded runner.
/// Maps job_id -> child process id so that cancel can kill it.
pub type RunningJobs = Arc<Mutex<HashMap<Uuid, u32>>>;

/// Executes a single job: marks running, streams stdout/stderr into job_logs,
/// sets success/failed from the exit code and refreshes stage/pipeline status.
pub(crate) async fn run_job(pool: PgPool, job_id: Uuid, running: RunningJobs) {
    if let Err(error) = run_job_inner(pool.clone(), job_id, running).await {
        tracing::error!(%job_id, error = ?error, "runner job failed");
        let _ = sqlx::query("UPDATE jobs SET status = 'failed', finished_at = now() WHERE id = $1 AND status NOT IN ('canceled')")
            .bind(job_id)
            .execute(&pool)
            .await;
    }
}

async fn run_job_inner(pool: PgPool, job_id: Uuid, running: RunningJobs) -> Result<(), ApiError> {
    let job = sqlx::query_as::<_, JobRow>(
        "SELECT j.id, j.stage_id, j.name, j.image, j.command, j.status, s.pipeline_id, p.project_id \
         FROM jobs j JOIN stages s ON s.id = j.stage_id JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.id = $1",
    )
    .bind(job_id)
    .fetch_optional(&pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;

    if job.status != "queued" && job.status != "running" {
        return Ok(()); // terminal already
    }
    if job.status == "queued" {
        // Manual path (no supervisor claim): flip to running ourselves.
        let _ = sqlx::query(
            "UPDATE jobs SET status = 'running', started_at = now() WHERE id = $1 AND status = 'queued'",
        )
        .bind(job_id)
        .execute(&pool)
        .await
        .map_err(ApiError::internal)?;
    }

    append_log(&pool, job_id, &format!("runner: starting job {}", job.name)).await?;
    refresh_stage(pool.clone(), job.id).await?;

    // Resolve workspace: clone project repository at pipeline git_ref into a temp dir.
    let workspace = prepare_workspace(&pool, &job).await?;
    let command_shell = format!("{} 2>&1", job.command);

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&command_shell)
        .current_dir(&workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;

    if let Some(pid) = child.id() {
        running.lock().await.insert(job_id, pid);
    }

    let stdout = child.stdout.take();
    let exit_status = child.wait().await;

    running.lock().await.remove(&job_id);

    // Stream lines into job_logs (bounded read afterwards if pipe closed).
    if let Some(stdout) = stdout {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            append_log(&pool, job_id, line.trim_end()).await?;
        }
    }

    // Cleanup workspace unless CICD_RUNNER_KEEP_WORKSPACE=1.
    if std::env::var("CICD_RUNNER_KEEP_WORKSPACE").ok().as_deref() != Some("1") {
        let _ = tokio::fs::remove_dir_all(&workspace).await;
    }

    let final_status = match exit_status {
        Ok(status) if status.success() => "success",
        Ok(_) => "failed",
        Err(_) => "failed",
    };
    let _ = sqlx::query(
        "UPDATE jobs SET status = $2, finished_at = now() WHERE id = $1 AND status NOT IN ('canceled')",
    )
    .bind(job_id)
    .bind(final_status)
    .execute(&pool)
    .await
    .map_err(ApiError::internal)?;
    refresh_stage(pool, job.id).await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct JobRow {
    id: Uuid,
    #[allow(dead_code)]
    stage_id: Uuid,
    name: String,
    #[allow(dead_code)]
    image: String,
    command: String,
    status: String,
    #[allow(dead_code)]
    pipeline_id: Uuid,
    #[allow(dead_code)]
    project_id: Uuid,
}

/// Clones the project repository (bare) into /tmp/forge-runner/<job> at git_ref.
async fn prepare_workspace(pool: &PgPool, job: &JobRow) -> Result<std::path::PathBuf, ApiError> {
    let repo_url: String = sqlx::query_scalar("SELECT repository_url FROM projects WHERE id = $1")
        .bind(job.project_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;

    let git_ref: String = sqlx::query_scalar("SELECT git_ref FROM pipelines WHERE id = $1")
        .bind(job.pipeline_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;

    let workspace = std::env::temp_dir().join(format!("forge-runner-{}", job.id));
    let _ = tokio::fs::remove_dir_all(&workspace).await;
    tokio::fs::create_dir_all(&workspace)
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;

    append_log(
        pool,
        job.id,
        &format!("runner: cloning {} at {}", repo_url, git_ref),
    )
    .await?;
    let clone = Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg("--depth")
        .arg("50")
        .arg("--branch")
        .arg(&git_ref)
        .arg(&repo_url)
        .arg("workspace")
        .current_dir(&workspace)
        .output()
        .await
        .map_err(|e| ApiError::internal(sqlx::Error::Io(e)))?;
    if !clone.status.success() {
        let stderr = String::from_utf8_lossy(&clone.stderr);
        append_log(
            pool,
            job.id,
            &format!("runner: clone failed: {}", stderr.trim()),
        )
        .await?;
        return Err(ApiError::bad_request(stderr.to_string()));
    }
    Ok(workspace.join("workspace"))
}

async fn refresh_stage(pool: PgPool, job_id: Uuid) -> Result<(), ApiError> {
    let stage_id: Option<Uuid> = sqlx::query_scalar("SELECT stage_id FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(&pool)
        .await
        .map_err(ApiError::internal)?;
    if let Some(stage_id) = stage_id {
        crate::api::refresh_statuses(&pool, stage_id).await?;
    }
    Ok(())
}

async fn append_log(pool: &PgPool, job_id: Uuid, message: &str) -> Result<(), ApiError> {
    let seq = crate::store::next_log_sequence(pool, job_id)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO job_logs (job_id, sequence, message) VALUES ($1, $2, $3)")
        .bind(job_id)
        .bind(seq)
        .bind(message)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(())
}

/// Wakes up on newly queued pipelines and executes stage-by-stage.
pub async fn supervisor_loop(pool: PgPool, running: RunningJobs) {
    loop {
        match poll_and_dispatch(&pool, running.clone()).await {
            Ok(()) => {}
            Err(error) => tracing::error!(%error, "runner poll failed"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Picks the first queued job of every non-terminal pipeline whose previous
/// stages all finished successfully, and spawns it.
async fn poll_and_dispatch(pool: &PgPool, running: RunningJobs) -> Result<(), sqlx::Error> {
    // Cancel jobs of canceled pipelines.
    sqlx::query(
        "UPDATE jobs SET status = 'canceled', finished_at = now() \
         WHERE status IN ('queued','running') AND stage_id IN \
         (SELECT id FROM stages WHERE pipeline_id IN \
          (SELECT id FROM pipelines WHERE status = 'canceled'))",
    )
    .execute(pool)
    .await?;

    let candidates = sqlx::query_as::<_, Candidate>(
        "SELECT j.id, j.stage_id \
         FROM jobs j \
         JOIN stages s ON s.id = j.stage_id \
         JOIN pipelines p ON p.id = s.pipeline_id \
         WHERE j.status = 'queued' \
           AND p.status IN ('queued','running') \
           AND NOT EXISTS ( \
             SELECT 1 FROM jobs x JOIN stages xs ON xs.id = x.stage_id \
             WHERE xs.pipeline_id = p.id AND xs.position < s.position \
               AND x.status NOT IN ('success','skipped') \
           ) \
           AND NOT EXISTS ( \
             SELECT 1 FROM jobs y JOIN stages ys ON ys.id = y.stage_id \
             WHERE ys.pipeline_id = p.id AND ys.position = s.position \
               AND y.status = 'failed' \
           ) \
         ORDER BY p.created_at, s.position, j.position \
         LIMIT 16",
    )
    .fetch_all(pool)
    .await?;

    for candidate in candidates {
        // Atomic claim: queued -> running prevents double dispatch (CHECK-safe statuses).
        let claimed = sqlx::query_scalar::<_, bool>(
            "UPDATE jobs SET status = 'running', started_at = now() WHERE id = $1 AND status = 'queued' RETURNING TRUE",
        )
        .bind(candidate.id)
        .fetch_optional(pool)
        .await?
        .unwrap_or(false);
        if !claimed {
            continue;
        }
        let pool2 = pool.clone();
        let running2 = running.clone();
        tokio::spawn(async move {
            run_job(pool2, candidate.id, running2).await;
        });
    }
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct Candidate {
    id: Uuid,
    #[allow(dead_code)]
    stage_id: Uuid,
}
