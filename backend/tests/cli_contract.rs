use std::process::Command;

#[test]
fn cli_exposes_project_pipeline_and_job_groups() {
    // The CLI lives in its own workspace package and is built as cicd-cli.
    // Locate it relative to the current integration test executable.
    let mut binary = std::env::current_exe().unwrap();
    binary.pop(); // deps/
    binary.pop(); // debug/
    binary.push(format!("cicd-cli{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new(&binary).arg("--help").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "failed to run {}",
        binary.display()
    );
    assert!(stdout.contains("project"));
    assert!(stdout.contains("pipeline"));
    assert!(stdout.contains("job"));
}
