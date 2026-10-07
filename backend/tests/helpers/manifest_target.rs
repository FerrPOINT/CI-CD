//! Standalone QA application, built with pinned rustc. Never installed in product runtime.
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    time::{Duration, Instant},
};

fn string_field<'a>(text: &'a str, field: &str) -> Option<&'a str> {
    text.split_once(&format!("\"{field}\":\""))?
        .1
        .split_once('"')
        .map(|(v, _)| v)
}

fn response(mut stream: TcpStream, root: PathBuf) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = [0u8; 4096];
    let Ok(n) = stream.read(&mut request) else {
        return;
    };
    let request = String::from_utf8_lossy(&request[..n]);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let controls = root.parent().unwrap();
    let blocked = if path == "/health" {
        "block-health"
    } else if path == "/.forge/version" {
        "block-version"
    } else {
        "none"
    };
    if controls.join(blocked).exists() {
        let _ = fs::write(controls.join(format!("entered-{blocked}")), b"entered");
        let deadline = Instant::now() + Duration::from_secs(20);
        while controls.join(blocked).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let result = (|| {
        let current = fs::read_to_string(root.join("current.json")).ok()?;
        let hash = string_field(&current, "manifestSha256")?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let manifest = fs::read(root.join("manifests").join(format!("{hash}.json"))).ok()?;
        let text = std::str::from_utf8(&manifest).ok()?;
        let artifact = string_field(text, "artifactSha256")?;
        if artifact.len() != 64 || !artifact.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let body = fs::read(root.join("objects").join(artifact)).ok()?;
        let (status, bytes) = match path {
            "/.forge/version" => (200, manifest),
            "/" => (200, body),
            "/health"
                if body
                    .windows(b"health=failed".len())
                    .any(|w| w == b"health=failed") =>
            {
                (503, b"unhealthy\n".to_vec())
            }
            "/health" => (200, b"ok\n".to_vec()),
            "/acceptance"
                if body.is_empty()
                    || body
                        .windows(b"acceptance=failed".len())
                        .any(|w| w == b"acceptance=failed") =>
            {
                (422, b"rejected\n".to_vec())
            }
            "/acceptance" => (200, b"accepted\n".to_vec()),
            _ => (404, b"missing\n".to_vec()),
        };
        Some((status, bytes))
    })()
    .unwrap_or((503, b"unavailable\n".to_vec()));
    let (status, bytes) = result;
    let head = format!(
        "HTTP/1.1 {status} QA\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&bytes);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: manifest-target ROOT LISTEN");
    let root = PathBuf::from(&args[1]);
    assert!(root.is_absolute());
    let listener = TcpListener::bind(&args[2]).unwrap();
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || response(stream, root));
    }
}
