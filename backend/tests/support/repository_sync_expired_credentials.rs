//! A real issuer-expired credential, previously proved valid by the coordinator.
use super::*;

#[tokio::test]
#[ignore = "Requires owned provider and an issuer-expired credential previously proved valid; never substitute an invalid or revoked token"]
async fn issuer_expired_credential_fails_safely_and_rotation_recovers() {
    let expired = std::env::var("CICD_SYNC_LIVE_EXPIRED_TOKEN")
        .expect("issuer-expired credential supplied privately");
    let issuer = std::env::var("CICD_SYNC_LIVE_EXPIRED_ISSUER")
        .expect("coordinator evidence must identify natural issuer expiration");
    assert!(matches!(
        issuer.as_str(),
        "github_job_natural_completion" | "gitlab_deploy_expiration"
    ));
    let mut f = Live::new().await;
    f.settings(Direction::Pull, false).await;
    let sha = f.commit(None, "QA real issuer-expired credential").await;
    let incoming = f.head("expired-credential");
    f.external(&incoming, None, &sha).await;
    let before = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    let (_, saved) = f.request("GET", "", None, None).await;
    let (status, _) = f
        .request(
            "PUT",
            "",
            Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings,"token":expired})),
            Some(Uuid::new_v4()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, check) = f
        .request(
            "POST",
            "/check",
            Some(serde_json::json!({"settings":f.settings})),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(check.to_string().contains("git_access_denied"));
    assert!(!check.to_string().contains(&expired));
    let failed = f.manual().await;
    assert_eq!(failed.state, "failed");
    assert!(
        failed
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("git_access_denied")
    );
    let pipelines: i64 = sqlx::query_scalar("SELECT count(*) FROM pipelines WHERE project_id=$1")
        .bind(f.project)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        pipelines, 0,
        "rejected credential never produces incoming CI"
    );
    let local_names = git(&f.local, &["for-each-ref", "--format=%(refname)"]).await;
    assert!(!local_names.lines().any(|name| name == incoming));
    assert_eq!(f.remote.as_ref().unwrap().refs(true).await.unwrap(), before);
    let (_, saved) = f.request("GET", "", None, None).await;
    let (status, _) = f
        .request(
            "PUT",
            "",
            Some(serde_json::json!({"expected_version":saved["version"],"settings":f.settings,"token":std::env::var("CICD_SYNC_LIVE_TOKEN").unwrap()})),
            Some(Uuid::new_v4()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.manual().await.state, "success");
    f.same(&incoming, &sha).await;
    assert_eq!(f.remote.as_ref().unwrap().refs(true).await.unwrap(), before);
    let (status, history) = f.request("GET", "/runs", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!history.to_string().contains(&expired));
    let retained = f.store.run(f.repository, failed.id).await.unwrap();
    assert_eq!(retained.state, "failed");
    f.stop_tasks().await;
    f.pool.close().await;
    println!(
        "LIVE_ISSUER_EXPIRED_CREDENTIAL_OK provider={:?} prefix={} issuer={issuer} failed_run={} check_diagnostic=git_access_denied failed_diagnostic=git_access_denied",
        f.settings.provider, f.prefix, failed.id
    );
}
