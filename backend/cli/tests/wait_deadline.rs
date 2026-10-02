mod http_faults;
use http_faults::FaultServer;
use serde_json::Value;
use std::time::{Duration, Instant};

#[tokio::test]
async fn wait_respects_http_and_overall_deadlines_for_headers_and_body() {
    for body_stall in [false, true] {
        for (http, wait, code) in [
            ("1", "5", "TRANSPORT_ERROR"),
            ("5", "1", "WAIT_TIMEOUT"),
            ("1", "1", "WAIT_TIMEOUT"),
        ] {
            for from_env in [false, true] {
                let server = FaultServer::start(
                    if body_stall {
                        Duration::ZERO
                    } else {
                        Duration::from_secs(4)
                    },
                    if body_stall {
                        Duration::from_secs(4)
                    } else {
                        Duration::ZERO
                    },
                    if body_stall {
                        b"{"
                    } else {
                        br#"{"pipeline":{"status":"success"}}"#
                    },
                    33,
                )
                .await;
                let mut args = vec!["--error-format", "json"];
                if !from_env {
                    args.extend(["--timeout-seconds", http]);
                }
                args.extend([
                    "pipeline",
                    "wait",
                    "--id",
                    "pipeline",
                    "--wait-timeout-seconds",
                    wait,
                ]);
                let start = Instant::now();
                let output = server
                    .run(&args, if from_env { Some(http) } else { Some("8") })
                    .await;
                assert_eq!(
                    output.status.code(),
                    Some(3),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "wait ignored the shorter timeout"
                );
                assert!(output.stdout.is_empty());
                let error: Value = serde_json::from_slice(&output.stderr).unwrap();
                assert_eq!(error["error"]["code"], code);
                assert!(error["error"]["status"].is_null());
                assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-token"));
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 1);
                assert!(requests[0].starts_with("GET /api/v1/pipelines/pipeline "));
            }
        }
    }
}
