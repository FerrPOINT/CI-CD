use std::process::Command;

#[test]
fn cli_exposes_project_pipeline_and_job_groups() {
    let binary = env!("CARGO_BIN_EXE_cicd-cli");
    let output = Command::new(binary).arg("--help").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success());
    assert!(stdout.contains("project"));
    assert!(stdout.contains("pipeline"));
    assert!(stdout.contains("job"));
}
