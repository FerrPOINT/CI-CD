//! Bounded QA observations only, never an execution or acceptance receipt.
use serde_json::{Value, json};
use std::{fs, io::Read, os::unix::fs::OpenOptionsExt, path::Path, time::UNIX_EPOCH};

fn canonical_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn source_binding<'a>(
    compiled: Option<&'a str>,
    expected: Option<&str>,
) -> Result<Option<&'a str>, &'static str> {
    match (compiled, expected) {
        // Ordinary developer runs remain unqualified, not falsely SHA-bound.
        (None, None) => Ok(None),
        (Some(source), Some(expected)) if canonical_sha(source) && source == expected => {
            Ok(Some(source))
        }
        _ => Err("Forge diagnostic compile/run source binding differs"),
    }
}

pub(super) fn assert_source_binding() {
    let expected = match std::env::var("FORGE_SOURCE_SHA") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!("FORGE_SOURCE_SHA must be canonical UTF-8")
        }
    };
    let source = source_binding(option_env!("FORGE_SOURCE_SHA"), expected.as_deref())
        .expect("Forge legacy QA requires the same source SHA during compilation and execution");
    eprintln!("DELIVERY_SOURCE_SHA={}", json!(source));
}

// File state: 0 missing, 1 regular, 2 invalid type/path, 3 unavailable, 4 oversized.
fn metadata(path: &Path) -> Value {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => json!({"state":1,"bytes":m.len(),"modifiedMs":m.modified().ok()
            .and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_millis() as u64)}),
        Ok(_) => json!({"state":2}),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({"state":0}),
        Err(e) => json!({"state":3,"errno":e.raw_os_error()}),
    }
}

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, Value> {
    let read = || -> std::io::Result<Vec<u8>> {
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let m = file.metadata()?;
        if !m.is_file() {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        if m.len() > limit {
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        Ok(bytes)
    };
    read().map_err(|e| {
        json!({"state":if e.kind()==std::io::ErrorKind::FileTooLarge {4} else {3},
        "errno":e.raw_os_error()})
    })
}

// Status: 0 absent, 1 unavailable, 2 failed, 3 unknown, 4 verified, 5 invalid.
fn status(value: &Value) -> u8 {
    match value.as_str() {
        Some("unavailable") => 1,
        Some("failed") => 2,
        Some("unknown") => 3,
        Some("verified") => 4,
        None if value.is_null() => 0,
        _ => 5,
    }
}

fn operation(root: &Path, digest: &str) -> Value {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return json!({"state":2});
    }
    let directory = root.join("operations").join(digest);
    for path in [
        root.to_path_buf(),
        root.join("operations"),
        directory.clone(),
    ] {
        match fs::symlink_metadata(path) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return json!({"state":2}),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return json!({"state":0}),
            Err(e) => return json!({"state":3,"errno":e.raw_os_error()}),
        }
    }
    let mut files = json!({});
    for name in [
        "intent.json",
        "active.json",
        "result.json",
        "reconciled.json",
        "verified-checks.json",
    ] {
        let path = directory.join(name);
        let mut observed = metadata(&path);
        if observed["state"] == 1 && name != "active.json" {
            match bounded_read(&path, 128 * 1024) {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(value) => {
                        let receipt = if name == "intent.json" {
                            &value["receipt"]
                        } else {
                            &value
                        };
                        observed["status"] = json!(status(&receipt["status"]));
                        for field in ["version", "servedArtifact", "health", "acceptance"] {
                            observed[field] = json!(status(&receipt[field]["status"]));
                        }
                    }
                    Err(_) => observed["jsonState"] = json!(5),
                },
                Err(error) => observed["read"] = error,
            }
        }
        files[name] = observed;
    }
    json!({"state":1,"operationHash":digest,"files":files})
}

fn kernel_symbol(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b)))
    .then_some(value)
}

fn kernel_frame(line: &str) -> Option<&str> {
    let mut parts = line.split_whitespace();
    let address = parts.next()?.strip_prefix("[<")?.strip_suffix(">]")?;
    if address.is_empty() || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    kernel_symbol(parts.next()?.split('+').next()?)
}

fn kernel_read(path: &Path, stack: bool) -> Value {
    match bounded_read(path, 4096) {
        Ok(bytes) => {
            let Ok(text) = std::str::from_utf8(&bytes) else {
                return json!({"state":2});
            };
            if stack {
                let frames: Vec<_> = text.lines().take(8).filter_map(kernel_frame).collect();
                json!({"state":1,"frames":frames,"truncated":text.lines().count()>8})
            } else {
                match kernel_symbol(text.trim()) {
                    Some(symbol) => json!({"state":1,"symbol":symbol}),
                    None => json!({"state":2}),
                }
            }
        }
        Err(error) => error,
    }
}

fn child(pid: Option<u32>) -> Value {
    let Some(pid) = pid else {
        return json!({"state":0});
    };
    let root = std::path::PathBuf::from(format!("/proc/{pid}"));
    let mut observed = json!({"pid":pid,"wchan":kernel_read(&root.join("wchan"),false),
        "stack":kernel_read(&root.join("stack"),true)});
    match fs::read_dir(root.join("task")) {
        Ok(entries) => {
            let entries: Vec<_> = entries.take(33).collect();
            observed["threadsTruncated"] = json!(entries.len() > 32);
            let mut ids: Vec<_> = entries
                .into_iter()
                .take(32)
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
                .collect();
            ids.sort_unstable();
            observed["threads"] = json!(
                ids.into_iter()
                    .map(|id| {
                        let path = root.join("task").join(id.to_string());
                        json!({"tid":id,"wchan":kernel_read(&path.join("wchan"),false),
                    "stack":kernel_read(&path.join("stack"),true)})
                    })
                    .collect::<Vec<_>>()
            );
        }
        Err(e) => observed["threads"] = json!({"state":3,"errno":e.raw_os_error()}),
    }
    observed
}

pub(super) fn emit(root: &Path, digest: &str, pid: Option<u32>) {
    // Sample the live child before cancellation; never read cmdline, environ or user memory.
    let process = child(pid);
    let mut markers = json!({});
    if let Some(controls) = root.parent() {
        for name in [
            "block-version",
            "entered-block-version",
            "block-health",
            "entered-block-health",
        ] {
            markers[name] = metadata(&controls.join(name));
        }
    }
    let source = option_env!("FORGE_SOURCE_SHA").filter(|s| canonical_sha(s));
    eprintln!(
        "DELIVERY_TIMEOUT_STATE={}",
        json!({"sourceSha":source,
        "operation":operation(root,digest),"markers":markers,"child":process})
    );
}

#[test]
fn delivery_timeout_source_binding_rejects_runtime_only_stale_and_malformed_sha() {
    let sha = "a".repeat(40);
    let other = "b".repeat(40);
    assert_eq!(
        source_binding(Some(&sha), Some(&sha)),
        Ok(Some(sha.as_str()))
    );
    assert_eq!(source_binding(None, None), Ok(None));
    for (compiled, expected) in [
        (None, Some(sha.as_str())),
        (Some(sha.as_str()), None),
        (Some(sha.as_str()), Some(other.as_str())),
        (Some("a7f9183"), Some("a7f9183")),
        (Some(""), Some("")),
    ] {
        assert!(source_binding(compiled, expected).is_err());
    }
    for invalid in ["A".repeat(40), "g".repeat(40), "a".repeat(41)] {
        assert!(source_binding(Some(&invalid), Some(&invalid)).is_err());
    }
}

#[test]
fn delivery_timeout_projection_is_bounded_numeric_and_secret_free() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "forge-delivery-diagnostic-{}",
        uuid::Uuid::new_v4()
    ));
    let _cleanup = Cleanup(root.clone());
    let digest = "a".repeat(64);
    let dir = root.join("operations").join(&digest);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("intent.json"), br#"{"receipt":{"status":"unknown","command":{"token":"private-journal-secret"},"reason":"private-reason"}}"#).unwrap();
    fs::write(dir.join("active.json"), b"private-active-secret").unwrap();
    fs::write(dir.join("result.json"), br#"{"status":"unknown","version":{"status":"unavailable"},"health":{"status":"verified"}}"#).unwrap();
    let observed = operation(&root, &digest);
    assert_eq!(observed["files"]["intent.json"]["status"], 3);
    assert_eq!(observed["files"]["result.json"]["version"], 1);
    assert_eq!(observed["files"]["result.json"]["health"], 4);
    assert_eq!(observed["files"]["reconciled.json"]["state"], 0);
    assert!(!observed.to_string().contains("private"));
    fs::write(dir.join("result.json"), vec![b'x'; 128 * 1024 + 1]).unwrap();
    assert_eq!(
        operation(&root, &digest)["files"]["result.json"]["read"]["state"],
        4
    );
    fs::remove_file(dir.join("result.json")).unwrap();
    std::os::unix::fs::symlink(dir.join("intent.json"), dir.join("result.json")).unwrap();
    assert_eq!(
        operation(&root, &digest)["files"]["result.json"]["state"],
        2
    );
    assert_eq!(operation(&root, "../foreign")["state"], 2);
    assert_eq!(operation(&root, &"b".repeat(64))["state"], 0);
}

#[test]
fn delivery_timeout_kernel_projection_discards_addresses_and_arbitrary_text() {
    assert_eq!(kernel_symbol("futex_wait_queue"), Some("futex_wait_queue"));
    assert!(kernel_symbol("private argument=/token").is_none());
    assert!(kernel_symbol(&"x".repeat(129)).is_none());
    assert_eq!(
        kernel_frame("[<00000000abcdef12>] futex_wait_queue+0x1/0x20"),
        Some("futex_wait_queue")
    );
    assert!(kernel_frame("private argument=/token").is_none());
    assert!(kernel_frame("[<not-an-address>] private").is_none());
    assert_eq!(status(&json!("private-status")), 5);
    assert_eq!(status(&Value::Null), 0);
}
