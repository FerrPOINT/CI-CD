use std::process::Command;

#[test]
fn cli_exposes_project_pipeline_and_job_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "failed to run cicd-cli --help: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("project"));
    assert!(stdout.contains("pipeline"));
    assert!(stdout.contains("job"));
}

#[test]
fn cli_exposes_job_attempt_history_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .args(["job", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "failed to run cicd-cli job --help: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("attempts"));
    assert!(stdout.contains("logs"));
}
