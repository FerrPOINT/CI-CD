use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use reqwest::{RequestBuilder, Response};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ErrorFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug)]
pub struct ApiFailure {
    pub status: Option<u16>,
    pub code: Option<String>,
    pub message: String,
    pub request_id: Option<String>,
}
impl std::fmt::Display for ApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (message, details) = self
            .message
            .split_once("; details=")
            .map(|(m, d)| (m, Some(d)))
            .unwrap_or((&self.message, None));
        match self.status {
            Some(status) => write!(
                f,
                "API returned {}: ",
                reqwest::StatusCode::from_u16(status)
                    .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR)
            )?,
            None => write!(f, "Request failed: ")?,
        }
        if let Some(code) = &self.code {
            write!(f, "{code}: ")?;
        }
        write!(f, "{message}")?;
        if let Some(id) = &self.request_id {
            write!(f, "; requestId={id}")?;
        }
        if let Some(details) = details {
            write!(f, "; details={details}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ApiFailure {}

fn redact(text: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .flat_map(|value| [value.as_str(), value.trim()])
        .filter(|value| !value.is_empty())
        .fold(text.to_owned(), |text, value| {
            text.replace(value, "[REDACTED]")
        })
}
pub fn report(error: &anyhow::Error, format: ErrorFormat, secrets: &[String]) {
    let failure = error.downcast_ref::<ApiFailure>();
    if matches!(format, ErrorFormat::Json) {
        eprintln!(
            "{}",
            json!({"error": {
                "status": failure.and_then(|e| e.status),
                "code": failure.and_then(|e| e.code.as_deref()),
                "message": redact(failure.map(|e|e.message.as_str()).unwrap_or(&error.to_string()), secrets),
                "request_id": failure.and_then(|e| e.request_id.as_deref()).map(|s| redact(s, secrets))
            }})
        );
    } else {
        eprintln!("error: {}", redact(&error.to_string(), secrets));
    }
}

pub async fn checked_response(request: RequestBuilder, secrets: &[String]) -> Result<Response> {
    let response = request.send().await.map_err(|_| ApiFailure {
        status: None,
        code: Some("TRANSPORT_ERROR".into()),
        message: "Сервис недоступен или превышено время ожидания".into(),
        request_id: None,
    })?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| redact(s, secrets));
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    let envelope = body.get("error").filter(|e| e.is_object()).unwrap_or(&body);
    let field = |key: &str| {
        envelope
            .get(key)
            .and_then(Value::as_str)
            .map(|s| redact(s, secrets))
    };
    Err(ApiFailure {
        status: Some(status.as_u16()),
        code: field("code"),
        message: {
            let mut message = field("message")
                .or_else(|| {
                    body.get("error")
                        .and_then(Value::as_str)
                        .map(|s| redact(s, secrets))
                })
                .unwrap_or_else(|| "Запрос отклонён сервисом".into());
            if let Some(details) = envelope.get("details") {
                message.push_str("; details=");
                let rendered = details
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|d| {
                                d.get("message").and_then(Value::as_str).map(|m| {
                                    match d.get("field").and_then(Value::as_str) {
                                        Some(f) => format!("{f}: {m}"),
                                        None => m.to_owned(),
                                    }
                                })
                            })
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_else(|| details.to_string());
                message.push_str(&redact(&rendered, secrets));
            }
            message
        },
        request_id: request_id
            .or_else(|| field("request_id"))
            .or_else(|| field("requestId")),
    }
    .into())
}
pub async fn json_response(request: RequestBuilder, secrets: &[String]) -> Result<Value> {
    let response = checked_response(request, secrets).await?;
    let bytes = response
        .bytes()
        .await
        .context("Не удалось прочитать ответ")?;
    if bytes.is_empty() {
        return Ok(json!({"status":"ok"}));
    }
    serde_json::from_slice(&bytes).context("Сервис вернул некорректный JSON")
}
pub async fn bytes_response(request: RequestBuilder, secrets: &[String]) -> Result<Vec<u8>> {
    let response = checked_response(request, secrets).await?;
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| redact(value, secrets));
    Ok(response
        .bytes()
        .await
        .map_err(|_| ApiFailure {
            status: None,
            code: Some("TRANSPORT_ERROR".into()),
            message: "Не удалось получить файл: соединение прервано или превышено время ожидания"
                .into(),
            request_id,
        })?
        .to_vec())
}
pub fn read_text(path: &str) -> Result<String> {
    if path == "-" {
        let mut value = String::new();
        std::io::stdin().read_to_string(&mut value)?;
        Ok(value)
    } else {
        std::fs::read_to_string(path).context("Не удалось прочитать входной файл")
    }
}
pub fn text_input(inline: Option<String>, file: Option<String>) -> Result<Option<String>> {
    match (inline, file) {
        (Some(_), Some(_)) => bail!("Inline и --from-file несовместимы"),
        (_, Some(path)) => Ok(Some(read_text(&path)?)),
        (value, None) => Ok(value),
    }
}
pub fn check_destination(path: &Path, overwrite: bool) -> Result<()> {
    if path.exists() && !overwrite {
        bail!("Файл уже существует; используйте --overwrite");
    }
    Ok(())
}
pub fn save_download(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    check_destination(path, overwrite)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if overwrite {
        temp.persist(path).map_err(|e| e.error)?;
    } else {
        temp.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
}

/// Keep help/version behavior while making parser failures safe and machine-readable.
pub fn parse_error(error: clap::Error) -> std::process::ExitCode {
    if !error.use_stderr() {
        let _ = error.print();
        return std::process::ExitCode::SUCCESS;
    }
    let args = std::env::args().collect::<Vec<_>>();
    let json = args
        .windows(2)
        .any(|v| v[0] == "--error-format" && v[1] == "json")
        || args.iter().any(|v| v == "--error-format=json");
    let mut secrets = Vec::new();
    for flag in ["--token", "--value", "--password", "--secret", "--variable"] {
        for (index, arg) in args.iter().enumerate() {
            if arg == flag {
                if let Some(value) = args.get(index + 1) {
                    secrets.push(value.clone());
                }
            }
            if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
                secrets.push(value.to_string());
            }
        }
    }
    if json {
        report(
            &ApiFailure {
                status: None,
                code: Some("CLI_USAGE".into()),
                message: format!("Некорректные аргументы CLI ({:?})", error.kind()),
                request_id: None,
            }
            .into(),
            ErrorFormat::Json,
            &secrets,
        );
    } else {
        eprint!("{}", redact(&error.to_string(), &secrets));
    }
    std::process::ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_does_not_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact");
        save_download(&path, b"old", false).unwrap();
        assert!(save_download(&path, b"new", false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        save_download(&path, b"new", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
    #[test]
    fn inline_and_file_are_exclusive() {
        assert!(text_input(Some("x".into()), Some("-".into())).is_err());
    }
    #[test]
    fn credentials_are_redacted() {
        assert_eq!(redact("bad secret", &["secret".into()]), "bad [REDACTED]");
        assert_eq!(redact("bad secret", &["secret\n".into()]), "bad [REDACTED]");
    }
}
