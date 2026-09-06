use std::process::Command;

fn cli_help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to run cicd-cli {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn assert_contains(stdout: &str, needle: &str) {
    assert!(
        stdout.contains(needle),
        "missing {needle:?} in help output:\n{stdout}"
    );
}

#[test]
fn cli_exposes_control_plane_groups_and_global_options() {
    let stdout = cli_help(&["--help"]);
    for needle in [
        "project",
        "pipeline",
        "job",
        "runner",
        "secret",
        "artifact",
        "environment",
        "deployment",
        "schedule",
        "webhook",
        "notification",
        "outbox",
        "report",
        "audit",
        "user",
        "member",
        "token",
        "--token",
        "--timeout-seconds",
        "--output",
    ] {
        assert_contains(&stdout, needle);
    }
}

#[test]
fn cli_exposes_job_attempt_history_commands() {
    let stdout = cli_help(&["job", "--help"]);
    assert_contains(&stdout, "attempts");
    assert_contains(&stdout, "logs");
}

#[test]
fn cli_exposes_pipeline_run_idempotency_key() {
    let stdout = cli_help(&["pipeline", "run", "--help"]);
    assert_contains(&stdout, "--idempotency-key");
}

#[test]
fn cli_exposes_pagination_on_list_commands() {
    let project = cli_help(&["project", "list", "--help"]);
    assert_contains(&project, "--limit");
    assert_contains(&project, "--offset");

    let pipeline = cli_help(&["pipeline", "list", "--help"]);
    assert_contains(&pipeline, "--limit");
    assert_contains(&pipeline, "--offset");
}

#[test]
fn cli_exposes_platform_resource_mutations() {
    let runner = cli_help(&["runner", "register", "--help"]);
    assert_contains(&runner, "--tag");

    let artifact = cli_help(&["artifact", "upload", "--help"]);
    assert_contains(&artifact, "--file");
    assert_contains(&artifact, "--content-type");

    let member = cli_help(&["member", "upsert", "--help"]);
    assert_contains(&member, "--role");

    let notification = cli_help(&["notification", "replace", "--help"]);
    assert_contains(&notification, "CHANNEL=TARGET");

    let token = cli_help(&["token", "create", "--help"]);
    assert_contains(&token, "--scope");
    assert_contains(&token, "--expires-in-days");

    let environment = cli_help(&["environment", "create", "--help"]);
    assert_contains(&environment, "--protected");
    assert_contains(&environment, "--required-approvals");

    let deployment = cli_help(&["deployment", "--help"]);
    assert_contains(&deployment, "approvals");
    assert_contains(&deployment, "approve");
    assert_contains(&deployment, "reject");
    assert_contains(&deployment, "rollback");
}

#[test]
fn cli_completions_emit_shell_scripts() {
    for shell in ["bash", "zsh", "fish"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(output.status.success(), "completions {shell} failed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.len() > 200,
            "completions {shell} look empty ({}/{} bytes)",
            stdout.len(),
            stdout.len()
        );
    }
}

#[test]
fn cli_exit_codes_are_stable() {
    // Connection refused -> network/server code 3.
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", "http://127.0.0.1:1")
        .args(["project", "list"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "network failure must exit 3");
}

#[test]
fn cli_ndjson_flag_parses() {
    let output = Command::new(env!("CARGO_BIN_EXE_cicd-cli"))
        .env("CICD_API_URL", "http://127.0.0.1:1")
        .args(["--output", "ndjson", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ndjson"),
        "ndjson must be documented in --help"
    );
}

#[test]
fn cli_profile_flag_documented() {
    let stdout = cli_help(&["--help"]);
    assert_contains(&stdout, "--profile");
}
