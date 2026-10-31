//! Opt-in TLS qualification through the production HTTP API and a separate worker.
//! The HTTPS endpoint is an owned Git smart-HTTP fault fixture, not a live provider.
use super::*;
use std::os::unix::process::CommandExt;
use std::{collections::BTreeSet, process::Stdio, time::Duration};
use tokio::process::{Child, Command};

const PROVIDER_TOKEN: &str = "fictional-tls-chain-provider-token";

const HTTPS_SERVER: &str = r#"
import base64, http.server, json, os, pathlib, ssl, subprocess, sys, urllib.parse
root, certificate, port = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
expected = 'Basic ' + base64.b64encode(b'oauth2:fictional-tls-chain-provider-token').decode()
class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *args): pass
    def do_GET(self): self.exchange()
    def do_POST(self): self.exchange()
    def exchange(self):
        url = urllib.parse.urlsplit(self.path)
        if not url.path.startswith(('/fixture/repository.git/', '/fixture/other.git/')):
            self.send_error(404); return
        required = ('Basic ' + base64.b64encode(b'oauth2:fictional-other-connection-token').decode()) if url.path.startswith('/fixture/other.git/') else expected
        authenticated = self.headers.get('Authorization') == required
        service = urllib.parse.parse_qs(url.query).get('service', [url.path.rsplit('/', 1)[-1]])[0]
        with (root / 'http-events.jsonl').open('a', encoding='utf-8') as log:
            log.write(json.dumps({'method': self.command, 'service': service, 'authenticated': authenticated}) + '\n')
        if not authenticated:
            self.send_response(401)
            self.send_header('WWW-Authenticate', 'Basic realm="Forge disposable TLS QA"')
            self.send_header('Content-Length', '0')
            self.send_header('Connection', 'close')
            self.end_headers(); self.close_connection = True; return
        length = int(self.headers.get('Content-Length', '0'))
        request = self.rfile.read(length)
        env = {'PATH': '/usr/bin:/bin', 'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null',
               'GIT_PROJECT_ROOT': str(root / 'http-root'), 'GIT_HTTP_EXPORT_ALL': '1',
               'REQUEST_METHOD': self.command, 'PATH_INFO': url.path, 'QUERY_STRING': url.query,
               'CONTENT_TYPE': self.headers.get('Content-Type', ''), 'CONTENT_LENGTH': str(length),
               'REMOTE_USER': 'fixture', 'REMOTE_ADDR': '127.0.0.1'}
        if self.headers.get('Git-Protocol'): env['HTTP_GIT_PROTOCOL'] = self.headers['Git-Protocol']
        if self.headers.get('Content-Encoding'): env['HTTP_CONTENT_ENCODING'] = self.headers['Content-Encoding']
        result = subprocess.run(['/usr/bin/git', 'http-backend'], input=request, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, env=env, timeout=30)
        headers, separator, body = result.stdout.partition(b'\r\n\r\n')
        if not separator: headers, separator, body = result.stdout.partition(b'\n\n')
        if result.returncode != 0 or not separator:
            self.send_error(502); return
        fields = [line.split(':', 1) for line in headers.decode().splitlines() if ':' in line]
        status = next((int(value.strip().split()[0]) for key, value in fields if key.lower() == 'status'), 200)
        self.send_response(status)
        for key, value in fields:
            if key.lower() not in ('status', 'content-length', 'connection'):
                self.send_header(key, value.strip())
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(body); self.close_connection = True
class Server(http.server.ThreadingHTTPServer):
    allow_reuse_address = True
    daemon_threads = True
server = Server(('127.0.0.1', port), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(str(root / certificate), str(root / 'leaf.key'))
server.socket = context.wrap_socket(server.socket, server_side=True)
(root / 'https-ready.tmp').write_text(str(server.server_address[1]), encoding='ascii')
os.replace(root / 'https-ready.tmp', root / 'https-ready')
server.serve_forever()
"#;

struct TlsSandbox {
    root: PathBuf,
    trust: PathBuf,
    owns_trust: bool,
    server: Option<Child>,
    port: u16,
}

fn kill_group(child: &mut Child) {
    if let Some(pid) = child.id() {
        // The Child is still owned and unreaped; this server was its group leader.
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }
    let _ = child.start_kill();
}

impl Drop for TlsSandbox {
    fn drop(&mut self) {
        if let Some(server) = self.server.as_mut() {
            kill_group(server);
        }
        if self.owns_trust {
            let _ = std::fs::remove_file(&self.trust);
            let _ = std::process::Command::new("update-ca-certificates")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn openssl(root: &Path, args: &[&str]) {
    assert!(
        Command::new("openssl")
            .current_dir(root)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap()
            .success(),
        "owned TLS fixture certificate preparation succeeds"
    );
}

impl TlsSandbox {
    async fn new() -> Self {
        assert_eq!(
            std::env::var("FORGE_SYNC_PRIVATE_TLS_QA").as_deref(),
            Ok("1")
        );
        // This opt-in must only be selected in the coordinator's disposable container.
        assert_eq!(unsafe { libc::geteuid() }, 0);
        let id = Uuid::new_v4();
        let value = Self {
            root: std::env::temp_dir().join(format!("forge-tls-chain-{id}")),
            trust: PathBuf::from(format!(
                "/usr/local/share/ca-certificates/forge-tls-chain-{id}.crt"
            )),
            owns_trust: false,
            server: None,
            port: 0,
        };
        std::fs::create_dir(&value.root).unwrap();
        openssl(
            &value.root,
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
                &format!("/CN=Forge-disposable-TLS-chain-{id}"),
                "-days",
                "1",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
            ],
        )
        .await;
        openssl(
            &value.root,
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
        for (name, hostname, days) in [
            ("valid", "localhost", "1"),
            ("wrong", "wrong.example.invalid", "1"),
            ("expired", "localhost", "-1"),
        ] {
            std::fs::write(value.root.join("extensions"), format!(
                "subjectAltName=DNS:{hostname}\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n"
            )).unwrap();
            openssl(
                &value.root,
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
                    &format!("{name}.crt"),
                    "-days",
                    days,
                    "-extfile",
                    "extensions",
                ],
            )
            .await;
        }
        std::fs::write(value.root.join("https-server.py"), HTTPS_SERVER).unwrap();
        let remote = value.remote();
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "--initial-branch=main"]).await;
        git(&remote, &["config", "http.receivepack", "true"]).await;
        value
    }

    fn remote(&self) -> PathBuf {
        self.root.join("http-root/fixture/repository.git")
    }

    fn origin(&self) -> String {
        format!("https://localhost:{}", self.port)
    }

    async fn start(&mut self, certificate: &str) {
        if let Some(mut old) = self.server.take() {
            kill_group(&mut old);
            let _ = old.wait().await;
        }
        let ready = self.root.join("https-ready");
        let _ = std::fs::remove_file(&ready);
        let mut command = Command::new("python3");
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .arg(self.root.join("https-server.py"))
            .arg(&self.root)
            .arg(certificate)
            .arg(self.port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        self.server = Some(command.spawn().unwrap());
        self.port = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(port) = std::fs::read_to_string(&ready) {
                    break port.parse::<u16>().unwrap();
                }
                assert!(
                    self.server.as_mut().unwrap().try_wait().unwrap().is_none(),
                    "owned HTTPS server starts"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("owned HTTPS listener becomes ready");
    }

    async fn trust(&mut self) {
        assert!(
            !self.trust.exists(),
            "never replace an existing trust-store file"
        );
        self.owns_trust = true;
        std::fs::copy(self.root.join("ca.crt"), &self.trust).unwrap();
        assert!(
            Command::new("update-ca-certificates")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .unwrap()
                .success()
        );
    }

    fn events(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.join("http-events.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

// Only groups carrying this unique fixture's repository lock can be signaled.
fn stop_owned_git_groups(root: &Path) {
    let mut groups = BTreeSet::new();
    if let Ok(processes) = std::fs::read_dir("/proc") {
        for process in processes.flatten() {
            let path = process.path();
            if !std::fs::read_link(path.join("fd/8"))
                .is_ok_and(|lock| lock.starts_with(root.join(".forge-mutation-locks")))
            {
                continue;
            }
            if let Ok(stat) = std::fs::read_to_string(path.join("stat"))
                && let Some(fields) = stat.rsplit_once(')').map(|(_, fields)| fields)
                && let Some(group) = fields
                    .split_whitespace()
                    .nth(2)
                    .and_then(|value| value.parse::<i32>().ok())
                && group > 1
                && group != unsafe { libc::getpgrp() }
            {
                groups.insert(group);
            }
        }
    }
    for group in groups {
        // Custody FD8 above proves this process group belongs to the owned fixture.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
}

struct RuntimeCleanup {
    root: PathBuf,
    api: tokio::task::JoinHandle<()>,
    worker: Option<Child>,
}
impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        self.api.abort();
        if let Some(worker) = self.worker.as_mut() {
            let _ = worker.start_kill();
        }
        stop_owned_git_groups(&self.root);
    }
}

async fn tls_live(origin: &str) -> Live {
    let pool = test_pool().await;
    let repository = Uuid::new_v4();
    let project = Uuid::new_v4();
    let account = Uuid::new_v4();
    let root = std::env::temp_dir().join(format!("forge-capabilities-{repository}"));
    std::fs::create_dir(&root).unwrap();
    let name = format!("tls-sync-{}", repository.simple());
    let local = root.join(format!("{name}.git"));
    let actor = root.join("actor.git");
    for path in [&local, &actor] {
        std::fs::create_dir(path).unwrap();
        git(path, &["init", "--bare", "--initial-branch=main"]).await;
    }
    sqlx::query("INSERT INTO repositories(id,name) VALUES($1,$2)")
        .bind(repository)
        .bind(&name)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name,repository_url,repository_id,default_branch) VALUES($1,$2,'https://forge.invalid/tls-fixture',$3,'qa-protected-default')")
        .bind(project).bind(format!("tls-{project}")).bind(repository).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO service_accounts(id,name) VALUES($1,$2)")
        .bind(account)
        .bind(format!("tls-{account}"))
        .execute(&pool)
        .await
        .unwrap();
    let authorization = format!("forge_sat_fixture_{}", Uuid::new_v4().simple());
    sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,principal_type,service_account_id,project_id,scopes) VALUES($1,'TLS chain fixture',$2,'fictional','service_account',$3,$4,ARRAY['api:read','api:write','git:read','git:write'])")
        .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(&authorization)).bind(account).bind(project).execute(&pool).await.unwrap();
    let settings = Settings {
        provider: Provider::Gitlab,
        instance_url: origin.to_owned(),
        repository_path: "fixture/repository".into(),
        direction: Direction::Both,
        automatic: false,
        protected_branch_updates: cicd_domain::repository_sync::ProtectedBranchUpdates::Blocked,
    };
    let mut config = cicd::config::RuntimeConfig::from_env_source(|_| None, false)
        .unwrap()
        .with_auth_secret(Some("fixture-live-api-secret".into()));
    config.git.root = root.clone();
    config.secrets.key = Some([19; 32]);
    config.git_sync.enabled = true;
    config.git_sync.allowed_gitlab_origins = vec![origin.to_owned()];
    let api = cicd::api::app_with_git_and_config(
        Some(pool.clone()),
        config.git.to_git_config(),
        None,
        config,
    )
    .unwrap();
    Live {
        store: PgSyncStore {
            pool: pool.clone(),
            worker_enabled: true,
            interval_seconds: 300,
        },
        pool,
        repository,
        project,
        root,
        local,
        actor,
        remote: None,
        initial: BTreeMap::new(),
        prefix: format!("qa/{}", Uuid::new_v4().simple()),
        settings,
        api,
        authorization,
        workers: vec![],
    }
}

async fn http(
    f: &Live,
    base: &str,
    method: &str,
    suffix: &str,
    body: Option<serde_json::Value>,
    operation: Option<Uuid>,
) -> (StatusCode, serde_json::Value) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let mut request = client
        .request(
            method.parse().unwrap(),
            format!(
                "{base}/api/v1/catalog/repositories/{}/sync{suffix}",
                f.repository
            ),
        )
        .bearer_auth(&f.authorization);
    if let Some(body) = body {
        request = request.json(&body);
    }
    if let Some(operation) = operation {
        request = request.header("Idempotency-Key", operation.to_string());
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(
        !body.to_string().contains(PROVIDER_TOKEN),
        "provider credential is absent from HTTP readback"
    );
    (status, body)
}

async fn run(f: &Live, base: &str) -> SyncRun {
    let (status, body) = http(f, base, "POST", "/runs", None, Some(Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let queued: SyncRun = serde_json::from_value(body).unwrap();
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let (status, body) =
                http(f, base, "GET", &format!("/runs/{}", queued.id), None, None).await;
            assert_eq!(status, StatusCode::OK);
            let current: SyncRun = serde_json::from_value(body).unwrap();
            if current.finished_at.is_some() {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("owned TLS worker completes a localhost exchange")
}

#[tokio::test]
#[ignore = "Requires root in an owned disposable Linux QA container with Python3, OpenSSL, Git and CA tools"]
async fn api_and_os_worker_enforce_system_ca_and_hostname_for_real_git_exchange() {
    let mut tls = TlsSandbox::new().await;
    tls.start("valid.crt").await;
    let f = tls_live(&tls.origin()).await;
    let incoming = f.head("tls-incoming");
    let outgoing = f.head("tls-outgoing");
    let a = f.commit(None, "TLS chain incoming").await;
    let b = f.commit(Some(&a), "TLS chain outgoing").await;
    git(
        &f.actor,
        &[
            "push",
            tls.remote().to_str().unwrap(),
            &format!("{a}:{incoming}"),
        ],
    )
    .await;
    f.local(&outgoing, &b).await;
    let local_before = git(&f.local, &["for-each-ref"]).await;
    let remote_before = git(&tls.remote(), &["for-each-ref"]).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_base = format!("http://{}", listener.local_addr().unwrap());
    let api = f.api.clone();
    let mut cleanup = RuntimeCleanup {
        root: f.root.clone(),
        worker: None,
        api: tokio::spawn(async move {
            axum::serve(listener, api).await.unwrap();
        }),
    };
    cleanup.worker = Some(f.process(Path::new("/usr/bin")).await);
    let check = serde_json::json!({"settings":f.settings,"token":PROVIDER_TOKEN});
    let (status, body) = http(&f, &api_base, "POST", "/check", Some(check), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.to_string().contains("git_tls_failed"));
    let (status, initial) = http(&f, &api_base, "GET", "", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        initial["version"], 0,
        "failed unsaved check never persists a connection"
    );
    let (status, saved) = http(
        &f,
        &api_base,
        "PUT",
        "",
        Some(serde_json::json!({
            "expected_version":0,"settings":f.settings,"token":PROVIDER_TOKEN,
        })),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["has_credential"], true);
    let untrusted = run(&f, &api_base).await;
    assert_eq!(untrusted.state, "failed");
    assert!(
        untrusted
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("git_tls_failed")
    );
    assert_eq!(git(&f.local, &["for-each-ref"]).await, local_before);
    assert_eq!(git(&tls.remote(), &["for-each-ref"]).await, remote_before);
    assert!(
        tls.events().is_empty(),
        "untrusted TLS never reaches HTTP/auth or Git backend"
    );

    tls.trust().await;
    let (status, checked) = http(
        &f,
        &api_base,
        "POST",
        "/check",
        Some(serde_json::json!({"settings":f.settings})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(checked["read"], true);
    assert_eq!(checked["write"], "preliminary");
    assert_eq!(git(&f.local, &["for-each-ref"]).await, local_before);
    assert_eq!(git(&tls.remote(), &["for-each-ref"]).await, remote_before);
    let trusted = run(&f, &api_base).await;
    assert_eq!(trusted.state, "success");
    for (reference, wanted) in [(&incoming, &a), (&outgoing, &b)] {
        assert_eq!(git(&f.local, &["rev-parse", reference]).await, *wanted);
        assert_eq!(git(&tls.remote(), &["rev-parse", reference]).await, *wanted);
        assert!(!git(&f.local, &["cat-file", "-p", wanted]).await.is_empty());
    }
    let events = tls.events();
    for service in ["git-upload-pack", "git-receive-pack"] {
        assert!(
            events.iter().any(|event| event["authenticated"] == true
                && event["method"] == "POST"
                && event["service"] == service),
            "trusted CA permits actual authenticated Git object exchange"
        );
    }
    let mut rejected_runs = Vec::new();
    for (case, certificate) in [("wrong-hostname", "wrong.crt"), ("expired", "expired.crt")] {
        let pending_incoming = f.head(&format!("tls-{case}-incoming"));
        let pending_outgoing = f.head(&format!("tls-{case}-outgoing"));
        git(
            &f.actor,
            &[
                "push",
                tls.remote().to_str().unwrap(),
                &format!("{a}:{pending_incoming}"),
            ],
        )
        .await;
        f.local(&pending_outgoing, &b).await;
        let local_before = git(&f.local, &["for-each-ref"]).await;
        let remote_before = git(&tls.remote(), &["for-each-ref"]).await;
        let events_before = tls.events().len();
        tls.start(certificate).await;
        let (status, body) = http(
            &f,
            &api_base,
            "POST",
            "/check",
            Some(serde_json::json!({"settings":f.settings})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{case}");
        assert!(body.to_string().contains("git_tls_failed"), "{case}");
        let rejected = run(&f, &api_base).await;
        assert_eq!(rejected.state, "failed", "{case}");
        assert!(
            rejected
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("git_tls_failed"),
            "{case}"
        );
        assert_eq!(
            git(&f.local, &["for-each-ref"]).await,
            local_before,
            "{case}"
        );
        assert_eq!(
            git(&tls.remote(), &["for-each-ref"]).await,
            remote_before,
            "{case}"
        );
        assert_eq!(
            tls.events().len(),
            events_before,
            "{case} rejection precedes HTTP/auth"
        );
        rejected_runs.push(rejected.id);
    }
    let (_, after) = http(&f, &api_base, "GET", "", None, None).await;
    assert_eq!(after["version"], saved["version"]);
    assert_eq!(after["settings"], saved["settings"]);
    let (status, history) = http(&f, &api_base, "GET", "/runs", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["total"], 4);
    let mut worker = cleanup.worker.take().unwrap();
    worker.kill().await.unwrap();
    worker.wait().await.unwrap();
    cleanup.api.abort();
    if let Some(mut server) = tls.server.take() {
        kill_group(&mut server);
        server.wait().await.unwrap();
    }
    f.pool.close().await;
    println!(
        "TLS_CHAIN_API_WORKER_OK repository={} untrusted={} trusted={} wrong_hostname={} expired={} incoming_sha={} outgoing_sha={} authenticated_git_http=true system_trust=true",
        f.repository, untrusted.id, trusted.id, rejected_runs[0], rejected_runs[1], a, b
    );
}

#[tokio::test]
#[ignore = "Requires an owned disposable Linux QA container with root, real Git HTTPS and CA tools; no external provider"]
async fn multiple_sync_real_https_workers_relay_between_independent_connections() {
    let mut tls = TlsSandbox::new().await;
    let other = tls.root.join("http-root/fixture/other.git");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "--bare", "--initial-branch=main"]).await;
    git(&other, &["config", "http.receivepack", "true"]).await;
    tls.start("valid.crt").await;
    tls.trust().await;
    let f = tls_live(&tls.origin()).await;
    let second = Uuid::new_v4();
    let reference = f.head("relay");
    let a = f.commit(None, "Multiple HTTPS connections root").await;
    let b = f
        .commit(Some(&a), "External fast-forward relayed through Forge")
        .await;
    f.local(&reference, &a).await;
    for (id, path, token) in [
        (f.repository, "fixture/repository", PROVIDER_TOKEN),
        (second, "fixture/other", "fictional-other-connection-token"),
    ] {
        let mut settings = f.settings.clone();
        settings.repository_path = path.into();
        settings.automatic = true;
        let (status, body) = f
            .request(
                "PUT",
                &format!("?connection_id={id}"),
                Some(serde_json::json!({"expected_version":0,"settings":settings,"token":token})),
                Some(Uuid::new_v4()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], id.to_string());
    }
    let mut cleanup = RuntimeCleanup {
        root: f.root.clone(),
        api: tokio::spawn(async {}),
        worker: Some(f.process(Path::new("/usr/bin")).await),
    };
    let mut second_worker = f.process(Path::new("/usr/bin")).await;
    let first_run = f
        .store
        .enqueue_at(f.repository, f.repository, "manual", None)
        .await
        .unwrap();
    let second_run = f
        .store
        .enqueue_at(f.repository, second, "manual", None)
        .await
        .unwrap();
    assert_eq!(f.completed(first_run.id).await.state, "success");
    assert_eq!(f.completed(second_run.id).await.state, "success");
    assert_eq!(git(&tls.remote(), &["rev-parse", &reference]).await, a);
    assert_eq!(git(&other, &["rev-parse", &reference]).await, a);
    // An actual external advance is pulled by the scheduled connection and must
    // wake the other automatic connection even when CI auto-trigger is disabled.
    git(
        &f.actor,
        &[
            "push",
            tls.remote().to_str().unwrap(),
            &format!("{b}:{reference}"),
        ],
    )
    .await;
    sqlx::query("UPDATE repository_sync_connections SET next_check_at=now() WHERE id=$1")
        .bind(f.repository)
        .execute(&f.pool)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if git(&other, &["rev-parse", &reference]).await == b {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("incoming HTTPS update must fan out through Forge to the second HTTPS connection");
    assert_eq!(git(&f.local, &["rev-parse", &reference]).await, b);
    let (history, total) = f.store.runs_at(f.repository, second, 20, 0).await.unwrap();
    assert_eq!(total, 2);
    assert_eq!(history[0].source, "git_change");
    assert_eq!(history[0].connection_id, second);
    assert_eq!(f.completed(history[0].id).await.state, "success");
    // Wait for the coalesced no-op on the inbound connection as well. Observing
    // a remote SHA alone does not prove that its execution journal is terminal.
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let idle: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM repository_sync_runs WHERE repository_id=$1 AND finished_at IS NULL) AND NOT EXISTS(SELECT 1 FROM repository_sync_connections WHERE repository_id=$1 AND dirty)")
                .bind(f.repository).fetch_one(&f.pool).await.unwrap();
            if idle { break; }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }).await.expect("automatic fan-out must settle instead of looping indefinitely");
    for (id, path) in [
        (f.repository, "fixture/repository"),
        (second, "fixture/other"),
    ] {
        let mut settings = f.settings.clone();
        settings.repository_path = path.into();
        settings.automatic = false;
        let (status, _) = f
            .request(
                "PUT",
                &format!("?connection_id={id}"),
                Some(serde_json::json!({"expected_version":1,"settings":settings})),
                Some(Uuid::new_v4()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    let c = f
        .commit(Some(&b), "First external repository advances")
        .await;
    let d = f
        .commit(Some(&b), "Second external repository diverges")
        .await;
    for (remote, sha) in [(tls.remote(), &c), (other.clone(), &d)] {
        git(
            &f.actor,
            &[
                "push",
                remote.to_str().unwrap(),
                &format!("{sha}:{reference}"),
            ],
        )
        .await;
    }
    let inbound = f.store.enqueue(f.repository, "manual", None).await.unwrap();
    assert_eq!(f.completed(inbound.id).await.state, "success");
    let divergent = f
        .store
        .enqueue_at(f.repository, second, "manual", None)
        .await
        .unwrap();
    assert_eq!(f.completed(divergent.id).await.state, "conflict");
    assert_eq!(git(&f.local, &["rev-parse", &reference]).await, c);
    assert_eq!(git(&tls.remote(), &["rev-parse", &reference]).await, c);
    assert_eq!(git(&other, &["rev-parse", &reference]).await, d);
    let refs = f.store.refs(divergent.id, 20, 0).await.unwrap().0;
    assert!(
        refs.iter()
            .any(|result| result.ref_name == reference && result.state == "conflict")
    );
    assert!(
        tls.events()
            .iter()
            .any(|event| event["authenticated"] == true)
    );
    second_worker.kill().await.unwrap();
    second_worker.wait().await.unwrap();
    if let Some(mut worker) = cleanup.worker.take() {
        worker.kill().await.unwrap();
        worker.wait().await.unwrap();
    }
    f.pool.close().await;
}
