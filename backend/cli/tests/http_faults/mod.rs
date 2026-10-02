use std::{
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct FaultServer {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for FaultServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl FaultServer {
    pub async fn start(
        header_delay: Duration,
        body_delay: Duration,
        body: &[u8],
        length: usize,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let recorded = requests.clone();
        let body = body.to_vec();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![];
            loop {
                let mut buffer = [0; 1024];
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    return;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|v| v == b"\r\n\r\n") {
                    break;
                }
            }
            recorded
                .lock()
                .unwrap()
                .push(String::from_utf8(request).unwrap());
            tokio::time::sleep(header_delay).await;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nX-Request-ID: request-fixture-token\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(headers.as_bytes()).await.is_err() {
                return;
            }
            if stream.write_all(&body).await.is_err() {
                return;
            }
            tokio::time::sleep(body_delay).await;
        });
        Self {
            url,
            requests,
            task,
        }
    }
    pub async fn run(&self, args: &[&str], timeout_env: Option<&str>) -> std::process::Output {
        let url = self.url.clone();
        let args: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        let timeout_env = timeout_env.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("empty.toml");
            std::fs::write(&config, "").unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_cicd-cli"));
            command
                .args(["--api-url", &url, "--token", "fixture-token"])
                .args(args)
                .env("CICD_CONFIG", config)
                .env_remove("CICD_PROFILE")
                .env_remove("CICD_OUTPUT")
                .env_remove("CICD_TIMEOUT_SECONDS")
                .env_remove("SDLC_API_TOKEN");
            if let Some(value) = timeout_env {
                command.env("CICD_TIMEOUT_SECONDS", value);
            }
            command.output().unwrap()
        })
        .await
        .unwrap()
    }
}
