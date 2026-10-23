//! Real private CLI boundary; no DB/API/native container calls are needed.
use std::{io::Write, process::{Command, Stdio}};
#[test]
fn challenged_hold_does_not_open_gate_or_emit_input_secret() {
    let root=std::env::temp_dir().join(format!("forge-inspection-cli-{}",uuid::Uuid::new_v4()));
    assert!(!root.exists());
    let mut child=Command::new(env!("CARGO_BIN_EXE_forge-quiesce"))
        .args(["--directory",root.to_str().unwrap(),"inspect-kubernetes-custody","--request-sha256",&"0".repeat(64)])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(br#"{"testSecret":"must-not-appear"}"#).unwrap();
    let output=child.wait_with_output().unwrap();
    assert_eq!(output.status.code(),Some(2));
    let value:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"],"HOLD");
    assert_eq!(value["lifecycleEffectsPerformed"],false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("must-not-appear"));
    assert!(!root.exists(),"readonly private command must not initialize Gate");
}
