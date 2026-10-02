mod http_faults;
use http_faults::FaultServer;
use serde_json::Value;
use std::time::Duration;

#[tokio::test]
async fn interrupted_download_is_unavailable_and_preserves_destination() {
    for stalled in [false, true] {
        for format in ["json", "text"] {
            for overwrite in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("artifact.bin");
                if overwrite {
                    std::fs::write(&path, b"original").unwrap();
                }
                let server = FaultServer::start(
                    Duration::ZERO,
                    if stalled {
                        Duration::from_secs(4)
                    } else {
                        Duration::ZERO
                    },
                    b"partial",
                    100,
                )
                .await;
                let mut args = vec![
                    "--timeout-seconds",
                    "1",
                    "--error-format",
                    format,
                    "artifact",
                    "download",
                    "--id",
                    "artifact",
                    "--output",
                    path.to_str().unwrap(),
                ];
                if overwrite {
                    args.push("--overwrite");
                }
                let start = std::time::Instant::now();
                let output = server.run(&args, None).await;
                assert_eq!(
                    output.status.code(),
                    Some(3),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(start.elapsed() < Duration::from_secs(3));
                assert!(output.stdout.is_empty());
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(!stderr.contains("fixture-token"));
                assert!(!stderr.contains(&server.url));
                if format == "json" {
                    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
                    assert_eq!(error["error"]["code"], "TRANSPORT_ERROR");
                    assert!(error["error"]["status"].is_null());
                    assert_eq!(error["error"]["request_id"], "request-[REDACTED]");
                } else {
                    assert!(stderr.contains("TRANSPORT_ERROR"));
                }
                if overwrite {
                    assert_eq!(std::fs::read(&path).unwrap(), b"original");
                } else {
                    assert!(!path.exists());
                }
                assert_eq!(
                    std::fs::read_dir(dir.path()).unwrap().count(),
                    usize::from(overwrite)
                );
                assert_eq!(server.requests.lock().unwrap().len(), 1);
            }
        }
    }
}

#[tokio::test]
async fn complete_download_and_no_clobber_still_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("artifact.bin");
    let server = FaultServer::start(Duration::ZERO, Duration::ZERO, b"complete", 8).await;
    let args = [
        "artifact",
        "download",
        "--id",
        "artifact",
        "--output",
        path.to_str().unwrap(),
    ];
    let output = server.run(&args, None).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), b"complete");
    let output = server.run(&args, None).await;
    assert!(!output.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), b"complete");
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    let server = FaultServer::start(Duration::ZERO, Duration::ZERO, b"replaced", 8).await;
    let mut overwrite = args.to_vec();
    overwrite.push("--overwrite");
    assert!(server.run(&overwrite, None).await.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), b"replaced");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
