use std::process::Command;

#[test]
fn forge_runner_exposes_protocol_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_forge-runner"))
        .arg("--help")
        .output()
        .expect("run forge-runner --help");

    assert!(
        output.status.success(),
        "forge-runner --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("help output is utf-8");
    for flag in [
        "--api-url",
        "--credential",
        "--registration-token",
        "--tags",
        "--total-slots",
        "--poll-interval-seconds",
        "--work-dir",
        "--once",
        "--no-checkout",
        "--keep-workspace",
    ] {
        assert!(
            stdout.contains(flag),
            "forge-runner help should document {flag}"
        );
    }
}
