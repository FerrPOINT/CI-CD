//! Read-only closure of legacy, never-prepared attempts. This never ACKs a job.
use super::Identity;
use anyhow::{Context, ensure};
use serde_json::Value;
use sqlx::PgPool;
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    path::Path,
};
use uuid::Uuid;

fn documents(
    root: &Path,
    lease: Uuid,
    attempt: Uuid,
    generation: i64,
    completed: chrono::DateTime<chrono::Utc>,
    seen: &mut BTreeSet<(u64, u64)>,
) -> anyhow::Result<Option<File>> {
    let directory = root
        .join(".kubernetes-control")
        .join(format!("forge-{}-g{generation}", attempt.simple()));
    if !directory.try_exists()? {
        return Ok(None);
    }
    super::plain(&directory, true)?;
    use std::os::unix::fs::MetadataExt;
    let metadata = directory.metadata()?;
    // A nested PVC mount can expose the very same custody through two valid roots.
    if !seen.insert((metadata.dev(), metadata.ino())) {
        return Ok(None);
    }
    let names: BTreeSet<_> = std::fs::read_dir(&directory)?
        .map(|v| v.map(|v| v.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    ensure!(
        names
            == BTreeSet::from([
                ".submission.lock".into(),
                "not-submitted.json".into(),
                "not-submitted-owner-readback.json".into()
            ]),
        "unprepared custody acquired another execution intent"
    );
    #[cfg(unix)]
    let lock = {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join(".submission.lock"))?;
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "original submission still has a native owner"
        );
        file
    };
    #[cfg(not(unix))]
    anyhow::bail!("native unsubmitted custody requires Unix");
    let pending = super::json(&directory.join("not-submitted.json"))?;
    let observed = super::json(&directory.join("not-submitted-owner-readback.json"))?;
    validate(&pending, &observed, lease, attempt, generation, completed)?;
    Ok(Some(lock))
}

fn validate(
    p: &Value,
    r: &Value,
    lease: Uuid,
    attempt: Uuid,
    generation: i64,
    completed: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<()> {
    let workspace = p["workspace"]
        .as_str()
        .context("unprepared workspace missing")?;
    let parts: Vec<_> = Path::new(workspace).components().collect();
    let expected_prefix = format!("attempt-{attempt}-{generation}-");
    let first = parts
        .first()
        .and_then(|v| v.as_os_str().to_str())
        .unwrap_or("");
    let suffix = first.strip_prefix(&expected_prefix).unwrap_or("");
    ensure!(
        p.as_object().is_some_and(|v| v.len() == 7)
            && p["schemaVersion"] == 1
            && p["kind"] == "forge/kubernetes-unprepared/v1"
            && p["attemptId"] == attempt.to_string()
            && p["leaseId"] == lease.to_string()
            && p["generation"] == generation
            && parts.len() == 2
            && suffix.len() == 32
            && suffix.bytes().all(|v| v.is_ascii_hexdigit())
            && parts[1].as_os_str() == "workspace"
            && p["configSha256"]
                .as_str()
                .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())),
        "unprepared source identity differs"
    );
    ensure!(
        r.as_object().is_some_and(|v| v.len() == 8)
            && r["protocolVersion"] == 1
            && r["leaseId"] == lease.to_string()
            && r["attemptId"] == attempt.to_string()
            && r["fencingToken"] == generation
            && r["leaseStatus"] == "expired"
            && r["terminalStatus"] == "failed"
            && r["terminalAcknowledged"] == false
            && r["completedAt"]
                .as_str()
                .and_then(|v| v.parse::<chrono::DateTime<chrono::Utc>>().ok())
                == Some(completed),
        "unprepared owner readback differs from the original failed lease"
    );
    Ok(())
}

/// The caller retains the exclusive product fence and all returned submission locks.
pub(super) async fn verify(pool: &PgPool, expected: &[Identity]) -> anyhow::Result<Vec<File>> {
    let rows: Vec<(Uuid,Uuid,i64,chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT l.id,l.attempt_id,l.generation,l.completed_at FROM job_leases l \
         JOIN jobs j ON j.id=l.job_id JOIN execution_attempts a ON a.id=l.attempt_id \
         WHERE l.runner_id IS NOT NULL AND l.completion_received_at IS NULL \
         AND l.lease_status='expired' AND l.terminal_status='failed' \
         AND j.status='failed' AND a.status='failed' AND a.finished_at IS NOT NULL \
         AND l.completed_at IS NOT NULL AND l.lease_expires_at<=now() \
         AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=l.job_id) \
         AND NOT EXISTS(SELECT 1 FROM job_leases newer WHERE newer.job_id=l.job_id AND newer.lease_status='active')")
        .fetch_all(pool).await?;
    ensure!(rows.len() <= 4096, "legacy unprepared census exceeds bound");
    let roots: BTreeSet<_> = expected
        .iter()
        .flat_map(|p| &p.custody_roots)
        .filter(|r| r.kind == "workspace")
        .map(|r| r.path.clone())
        .collect();
    let mut locks = Vec::new();
    for (lease, attempt, generation, completed) in rows {
        let mut found = None;
        let mut seen = BTreeSet::new();
        for root in &roots {
            if let Some(lock) = documents(root, lease, attempt, generation, completed, &mut seen)? {
                ensure!(found.is_none(), "duplicate unprepared source custody");
                found = Some(lock);
            }
        }
        locks.push(found.context("unacknowledged lease has no exact never-prepared custody")?);
    }
    Ok(locks)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value, Uuid, Uuid, chrono::DateTime<chrono::Utc>) {
        let lease = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let time = "2026-10-27T13:51:24.138127Z".parse().unwrap();
        (
            serde_json::json!({"schemaVersion":1,"kind":"forge/kubernetes-unprepared/v1",
            "leaseId":lease,"attemptId":attempt,"generation":1,"configSha256":"a".repeat(64),
            "workspace":format!("attempt-{attempt}-1-{}/workspace",Uuid::new_v4().simple())}),
            serde_json::json!({"protocolVersion":1,"leaseId":lease,"attemptId":attempt,
            "fencingToken":1,"leaseStatus":"expired","terminalStatus":"failed",
            "completedAt":time,"terminalAcknowledged":false}),
            lease,
            attempt,
            time,
        )
    }
    #[test]
    fn exact_failed_unprepared_is_preserved_without_ack() {
        let (p, r, l, a, t) = fixture();
        validate(&p, &r, l, a, 1, t).unwrap();
        assert_eq!(r["terminalAcknowledged"], false);
    }
    #[test]
    fn another_lease_or_generation_is_not_closed() {
        let (p, r, l, a, t) = fixture();
        assert!(validate(&p, &r, Uuid::new_v4(), a, 1, t).is_err());
        assert!(validate(&p, &r, l, a, 2, t).is_err());
    }
    #[test]
    fn success_or_another_completion_is_not_inferred() {
        let (p, mut r, l, a, t) = fixture();
        r["terminalStatus"] = "success".into();
        assert!(validate(&p, &r, l, a, 1, t).is_err());
        r["terminalStatus"] = "failed".into();
        assert!(validate(&p, &r, l, a, 1, t + chrono::Duration::seconds(1)).is_err());
    }
    #[test]
    fn traversal_and_unknown_fields_fail_closed() {
        let (mut p, r, l, a, t) = fixture();
        p["workspace"] = "../workspace".into();
        assert!(validate(&p, &r, l, a, 1, t).is_err());
        let (mut p, r, l, a, t) = fixture();
        p["submitted"] = true.into();
        assert!(validate(&p, &r, l, a, 1, t).is_err());
    }
    #[test]
    fn same_nested_custody_inode_is_not_locked_twice() {
        let (pending, observed, lease, attempt, completed) = fixture();
        let root = std::env::temp_dir().join(format!("forge-unprepared-alias-{}", Uuid::new_v4()));
        let directory = root
            .join(".kubernetes-control")
            .join(format!("forge-{}-g1", attempt.simple()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("not-submitted.json"),
            serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("not-submitted-owner-readback.json"),
            serde_json::to_vec(&observed).unwrap(),
        )
        .unwrap();
        File::create(directory.join(".submission.lock")).unwrap();
        let mut seen = BTreeSet::new();
        let held = documents(&root, lease, attempt, 1, completed, &mut seen)
            .unwrap()
            .unwrap();
        assert!(
            documents(&root, lease, attempt, 1, completed, &mut seen)
                .unwrap()
                .is_none()
        );
        // Another verifier must still see the real original lock.
        assert!(documents(&root, lease, attempt, 1, completed, &mut BTreeSet::new()).is_err());
        drop(held);
        std::fs::remove_dir_all(root).unwrap();
    }
}
