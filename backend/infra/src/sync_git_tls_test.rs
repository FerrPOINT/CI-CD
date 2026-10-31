//! Explicit opt-in: changes trust only inside the disposable QA container.
use super::*;
struct Fixture {
    root: PathBuf,
    trust: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.trust);
        let _ = std::process::Command::new("update-ca-certificates")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
async fn tool(root: &Path, args: &[&str]) {
    assert!(
        Command::new("openssl")
            .current_dir(root)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .unwrap()
            .success(),
        "OpenSSL QA fixture preparation failed"
    );
}
async fn observe(
    root: &Path,
    certificate: &str,
    raw_http: bool,
) -> Result<BTreeMap<String, String>, Error> {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let mut server = Command::new("openssl")
        .current_dir(root.join("web"))
        .args([
            "s_server",
            "-quiet",
            if raw_http { "-HTTP" } else { "-WWW" },
            "-accept",
            &format!("127.0.0.1:{port}"),
            "-cert",
            root.join(certificate).to_str().unwrap(),
            "-key",
            root.join("leaf.key").to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let settings = Settings {
        provider: Provider::Gitlab,
        instance_url: format!("https://localhost:{port}"),
        repository_path: "fixture/repository".into(),
        direction: cicd_domain::repository_sync::Direction::Pull,
        automatic: false,
        protected_branch_updates: Default::default(),
    };
    let session = GitSession::new(
        root,
        root.join("local.git"),
        settings,
        "fictional-tls-canary".into(),
        Uuid::new_v4(),
        1,
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    let result = session.refs(true).await;
    let _ = server.kill().await;
    let _ = server.wait().await;
    result
}

async fn same_origin_redirect_is_not_contacted(root: &Path) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    // Without -WWW/-HTTP, s_server exposes decrypted application data through
    // its pipes. Count actual paths, never retain or print request headers.
    let mut server = Command::new("openssl")
        .args([
            "s_server",
            "-quiet",
            "-accept",
            &format!("127.0.0.1:{port}"),
            "-cert",
            root.join("valid.crt").to_str().unwrap(),
            "-key",
            root.join("leaf.key").to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = server.stdin.take().unwrap();
    let output = server.stdout.take().unwrap();
    let initial_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let target_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let written_responses = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_initial = initial_requests.clone();
    let observed_target = target_requests.clone();
    let observed_written = written_responses.clone();
    let target = format!(
        "https://localhost:{port}/redirect-target/fixture/repository.git/info/refs?service=git-upload-pack"
    );
    let handler = tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(output).lines();
        while let Some(request) = lines.next_line().await.unwrap() {
            let Some(path) = request
                .strip_prefix("GET ")
                .and_then(|s| s.split(' ').next())
            else {
                continue;
            };
            let redirected = path.starts_with("/redirect-target/");
            // Consume headers solely in memory. The fixture token must not
            // become a request-log or an assertion diagnostic.
            while let Some(header) = lines.next_line().await.unwrap() {
                if header.is_empty() {
                    break;
                }
            }
            let response = if redirected {
                observed_target.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let body = format!("{}\trefs/heads/main\n", "a".repeat(40));
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else {
                observed_initial.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
            };
            input.write_all(response.as_bytes()).await.unwrap();
            input.flush().await.unwrap();
            observed_written.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    struct Handler(tokio::task::JoinHandle<()>);
    impl Drop for Handler {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let mut handler = Handler(handler);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let session = GitSession::new(
        root,
        root.join("local.git"),
        Settings {
            provider: Provider::Gitlab,
            instance_url: format!("https://localhost:{port}"),
            repository_path: "fixture/repository".into(),
            direction: cicd_domain::repository_sync::Direction::Pull,
            automatic: false,
        },
        "fictional-tls-canary".into(),
        Uuid::new_v4(),
        1,
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    let result = session.refs(true).await;
    let _ = server.kill().await;
    let _ = server.wait().await;
    tokio::time::timeout(Duration::from_secs(5), &mut handler.0)
        .await
        .expect("owned TLS request handler terminates after its server exits")
        .expect("owned TLS request handler must not hide a failed response write");
    assert_eq!(
        initial_requests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the trusted TLS origin actually served the redirect response"
    );
    assert_eq!(
        written_responses.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the complete HTTP 302 response was written and flushed"
    );
    assert_eq!(
        target_requests.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "Git must not request even the same-origin redirect target"
    );
    // Git versions either mention a redirect or report HTTP 302 as a generic
    // operation failure. TLS/network/timeout failures cannot satisfy this oracle.
    assert!(
        matches!(
            result,
            Err(Error::Unavailable(
                "git_redirect_rejected" | "git_operation_failed"
            ))
        ),
        "the observed HTTP 302 must cause a redirect/operation rejection: {result:?}"
    );
    println!(
        "TLS_SAME_ORIGIN_REDIRECT_REJECTED initial_requests=1 responses_flushed=1 target_requests=0"
    );
}
#[tokio::test]
#[ignore = "Requires root in an owned disposable QA container with OpenSSL and CA update tools"]
async fn git_session_rejects_untrusted_and_wrong_hostname_but_accepts_system_ca() {
    assert_eq!(
        std::env::var("FORGE_SYNC_PRIVATE_TLS_QA").as_deref(),
        Ok("1")
    );
    let id = Uuid::new_v4();
    let fixture = Fixture {
        root: std::env::temp_dir().join(format!("forge-tls-{id}")),
        trust: PathBuf::from(format!(
            "/usr/local/share/ca-certificates/forge-qa-{id}.crt"
        )),
    };
    std::fs::create_dir(&fixture.root).unwrap();
    tool(
        &fixture.root,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "ca.key",
            "-out",
            "ca.crt",
            "-subj",
            "/CN=Forge-disposable-QA-CA",
            "-days",
            "1",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    )
    .await;
    tool(
        &fixture.root,
        &[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "leaf.key",
            "-out",
            "leaf.csr",
            "-subj",
            "/CN=localhost",
        ],
    )
    .await;
    for (file, host, days) in [
        ("valid", "localhost", "1"),
        ("wrong", "wrong.example.invalid", "1"),
        ("expired", "localhost", "-1"),
    ] {
        std::fs::write(fixture.root.join("extensions"),format!("subjectAltName=DNS:{host}\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n")).unwrap();
        tool(
            &fixture.root,
            &[
                "x509",
                "-req",
                "-in",
                "leaf.csr",
                "-CA",
                "ca.crt",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-out",
                &format!("{file}.crt"),
                "-days",
                days,
                "-extfile",
                "extensions",
            ],
        )
        .await;
    }
    let web = fixture.root.join("web/fixture/repository.git");
    std::fs::create_dir_all(web.join("info")).unwrap();
    let refs = format!("{}\trefs/heads/main\n", "a".repeat(40));
    std::fs::write(web.join("info/refs"), &refs).unwrap();
    std::fs::write(web.join("info/refs?service=git-upload-pack"), &refs).unwrap();
    std::fs::write(web.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    assert!(matches!(
        observe(&fixture.root, "valid.crt", false).await,
        Err(Error::Unavailable("git_tls_failed"))
    ));
    std::fs::copy(fixture.root.join("ca.crt"), &fixture.trust).unwrap();
    assert!(
        Command::new("update-ca-certificates")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .unwrap()
            .success()
    );
    assert_eq!(
        observe(&fixture.root, "valid.crt", false).await.unwrap()["refs/heads/main"],
        "a".repeat(40)
    );
    assert!(matches!(
        observe(&fixture.root, "wrong.crt", false).await,
        Err(Error::Unavailable("git_tls_failed"))
    ));
    assert!(matches!(
        observe(&fixture.root, "expired.crt", false).await,
        Err(Error::Unavailable("git_tls_failed"))
    ));
    same_origin_redirect_is_not_contacted(&fixture.root).await;
    let forbidden = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1:{}/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        forbidden.local_addr().unwrap().port()
    );
    for file in ["info/refs", "info/refs?service=git-upload-pack"] {
        std::fs::write(web.join(file), &redirect).unwrap();
    }
    assert!(
        observe(&fixture.root, "valid.crt", true).await.is_err(),
        "redirect cannot be accepted as a successful ref observation"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), forbidden.accept())
            .await
            .is_err(),
        "Git never contacts the redirect destination"
    );
}
