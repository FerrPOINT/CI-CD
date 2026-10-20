//! Real Forge-worker workflow transport on an existing nonempty GitHub repository.
use super::*;

const WORKFLOW_PATH: &str = ".github/workflows/forge-sync-qa.yml";

async fn dispatch_commit(f: &Live, parent: Option<&str>, content: &str) -> (String, String) {
    let file = f.root.join("dispatch-only-workflow.yml");
    std::fs::write(&file, content.as_bytes()).unwrap();
    let blob = git(&f.actor, &["hash-object", "-w", file.to_str().unwrap()]).await;
    let workflows = tree_with_entry(
        &f.actor,
        &format!("100644 blob {blob}\tforge-sync-qa.yml\n"),
    )
    .await;
    let github = tree_with_entry(&f.actor, &format!("040000 tree {workflows}\tworkflows\n")).await;
    let tree = tree_with_entry(&f.actor, &format!("040000 tree {github}\t.github\n")).await;
    let mut args = vec![
        "-c",
        "user.name=Forge QA",
        "-c",
        "user.email=qa@example.invalid",
        "commit-tree",
        &tree,
        "-m",
        "QA dispatch-only workflow transport",
    ];
    if let Some(parent) = parent {
        args.extend(["-p", parent]);
    }
    (git(&f.actor, &args).await, blob)
}

async fn exact_workflow(path: &Path, commit: &str, expected_blob: &str, content: &str) {
    let blob = git(path, &["rev-parse", &format!("{commit}:{WORKFLOW_PATH}")]).await;
    assert_eq!(blob, expected_blob, "the exact workflow blob is preserved");
    // Read raw stdout so the final newline and every workflow byte are checked.
    let output = tokio::process::Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(path)
        .args(["cat-file", "blob", &blob])
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "owned workflow blob is readable");
    assert_eq!(output.stdout, content.as_bytes());
}

async fn no_actions_for_branch(client: &reqwest::Client, f: &Live, branch: &str, phase: &str) {
    assert!(branch.starts_with(&format!("{}/", f.prefix)));
    let response = client
        .get(format!(
            "https://api.github.com/repos/{}/actions/runs",
            f.settings.repository_path
        ))
        .query(&[("branch", branch), ("per_page", "100")])
        .bearer_auth(std::env::var("CICD_SYNC_LIVE_TOKEN").unwrap())
        .header("User-Agent", "Forge-owned-QA")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "Actions listing must actually succeed; authorization failure is not zero runs"
    );
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["total_count"], 0, "no Actions run in phase {phase}");
    assert!(body["workflow_runs"].as_array().unwrap().is_empty());
    println!(
        "LIVE_GITHUB_WORKFLOW_ACTIONS_ABSENT prefix={} phase={phase} branch={branch} total_count=0",
        f.prefix
    );
}

#[tokio::test]
#[ignore = "Requires owned nonempty GitHub repository; transports dispatch-only workflow through Forge worker, never dispatches Actions"]
async fn dispatch_only_workflow_transfers_both_directions_without_actions_runs() {
    let mut f = Live::new().await;
    assert_eq!(f.settings.provider, Provider::Github);
    assert!(
        !f.initial.is_empty(),
        "use the preserved original QA repository"
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    let reference = f.head("dispatch-only-workflow");
    let branch = reference.strip_prefix("refs/heads/").unwrap();
    no_actions_for_branch(&client, &f, branch, "before").await;
    let first = "name: Forge sync QA outgoing\non: workflow_dispatch\njobs:\n  noop:\n    runs-on: ubuntu-latest\n    steps:\n      - run: ':'\n";
    let (outgoing_sha, outgoing_blob) = dispatch_commit(&f, None, first).await;
    f.local(&reference, &outgoing_sha).await;
    f.settings(Direction::Push, false).await;
    let pushed = f.manual().await;
    assert_eq!(pushed.state, "success");
    f.ref_result(pushed.id, &reference, "applied").await;
    f.same(&reference, &outgoing_sha).await;
    let remote = f.remote.as_ref().unwrap();
    remote.fetch().await.unwrap();
    exact_workflow(&remote.scratch, &outgoing_sha, &outgoing_blob, first).await;
    no_actions_for_branch(&client, &f, branch, "after_forge_push").await;

    let second = "name: Forge sync QA incoming\non: workflow_dispatch\njobs:\n  noop:\n    runs-on: ubuntu-latest\n    steps:\n      - run: 'printf incoming'\n";
    let (incoming_sha, incoming_blob) = dispatch_commit(&f, Some(&outgoing_sha), second).await;
    f.external(&reference, Some(&outgoing_sha), &incoming_sha)
        .await;
    assert_eq!(
        git(&f.local, &["rev-parse", &reference]).await,
        outgoing_sha
    );
    no_actions_for_branch(&client, &f, branch, "after_external_fast_forward").await;
    f.settings(Direction::Pull, false).await;
    let pulled = f.manual().await;
    assert_eq!(pulled.state, "success");
    f.ref_result(pulled.id, &reference, "applied").await;
    f.same(&reference, &incoming_sha).await;
    exact_workflow(&f.local, &incoming_sha, &incoming_blob, second).await;
    exact_workflow(
        &f.remote.as_ref().unwrap().scratch,
        &incoming_sha,
        &incoming_blob,
        second,
    )
    .await;
    let mut after = f.remote.as_ref().unwrap().refs(true).await.unwrap();
    assert_eq!(
        after.remove(&reference).as_deref(),
        Some(incoming_sha.as_str())
    );
    assert_eq!(after, f.initial, "all original provider refs remain exact");
    for sample in 0..3 {
        if sample > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        no_actions_for_branch(&client, &f, branch, "after_forge_pull").await;
    }
    f.settings(Direction::Disabled, false).await;
    f.stop_tasks().await;
    f.pool.close().await;
    println!(
        "LIVE_GITHUB_DISPATCH_ONLY_WORKFLOW_TRANSPORT_OK prefix={} pushed_run={} pulled_run={} outgoing_sha={outgoing_sha} incoming_sha={incoming_sha} outgoing_blob={outgoing_blob} incoming_blob={incoming_blob} exact_bytes=true actions_runs=0",
        f.prefix, pushed.id, pulled.id
    );
}
