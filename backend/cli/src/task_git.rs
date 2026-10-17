//! One sealed candidate push, or readback only after an unknown outcome. No mint/retry.
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;

fn private_file(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("Не удалось создать private Git file")?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn read_private(path: &Path, maximum: u64) -> anyhow::Result<String> {
    let metadata = fs::symlink_metadata(path).context("Private Git input недоступен")?;
    ensure!(
        metadata.is_file() && metadata.len() <= maximum,
        "Некорректный private Git input"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "Private Git input доступен другим пользователям"
        );
    }
    fs::read_to_string(path).context("Некорректный private Git input")
}

fn git() -> Command {
    let mut git = Command::new("git");
    // No inherited tracing, config injection, proxies, askpass or bearer environment.
    git.env_clear();
    for name in ["PATH", "SystemRoot", "WINDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(name) {
            git.env(name, value);
        }
    }
    git.env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    git
}

fn field<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value[key].as_str().context("Некорректный Git context")
}

fn oid(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn escaped(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn inputs(origin: &str, context: &Value) -> anyhow::Result<(String, String, String, String)> {
    let mut url = reqwest::Url::parse(origin).context("Некорректный Forge origin")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "Нужен точный Forge origin"
    );
    let repository = field(context, "repository")?;
    let parts: Vec<_> = repository.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|p| !p.is_empty()
                && p.len() <= 100
                && !p.starts_with('.')
                && !p.contains("..")
                && !p.ends_with(".git")
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))),
        "Некорректный repository"
    );
    let source = &context["source"];
    let task = uuid::Uuid::parse_str(field(source, "task_id")?)?;
    ensure!(!task.is_nil(), "Некорректный task");
    let reference = field(source, "candidate_ref")?;
    ensure!(
        reference == format!("refs/heads/sdlc/{task}"),
        "Некорректный candidate ref"
    );
    let old = field(context, "oldOid")?;
    let new = field(context, "newOid")?;
    ensure!(
        oid(old) && oid(new) && new.bytes().any(|c| c != b'0') && old != new,
        "Некорректный OID CAS"
    );
    url.set_path(&format!("/git/{repository}.git"));
    Ok((url.to_string(), reference.into(), old.into(), new.into()))
}

pub(crate) async fn run(
    origin: &str,
    context_file: PathBuf,
    token_file: PathBuf,
    checkout: PathBuf,
    readback: bool,
) -> anyhow::Result<Value> {
    let text = read_private(&context_file, 16_384)?;
    let context: Value = serde_json::from_str(&text).context("Некорректный Git context")?;
    let compact = serde_json::to_string(&context)?;
    ensure!(compact.is_ascii(), "Git context header должен быть ASCII");
    let (url, reference, old, new) = inputs(origin, &context)?;
    let token = read_private(&token_file, 128)?;
    let token = token.trim_end_matches(['\r', '\n']);
    ensure!(
        !token.is_empty() && token.bytes().all(|c| c.is_ascii_graphic()),
        "Нужен private child PAT file"
    );
    let temporary = tempfile::tempdir()?;
    let config = temporary.path().join("private-config");
    private_file(&config,format!("[http]\n\textraHeader = \"Authorization: Bearer {}\"\n\textraHeader = \"X-Forge-Task-Git-Context: {}\"\n\tfollowRedirects = false\n\tproxy =\n[credential]\n\thelper =\n",escaped(token),escaped(&compact)).as_bytes())?;
    let bare = temporary.path().join("candidate.git");
    ensure!(
        git()
            .args(["init", "--bare", "--object-format=sha1"])
            .arg(&bare)
            .output()
            .await?
            .status
            .success(),
        "Temporary Git context недоступен"
    );
    if !readback {
        // Only objects are shared; never inherit working repository config, hooks or URL rewrites.
        let objects = git()
            .arg("-C")
            .arg(&checkout)
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "objects",
            ])
            .output()
            .await?;
        ensure!(objects.status.success(), "Исходные Git objects недоступны");
        let path = PathBuf::from(String::from_utf8(objects.stdout)?.trim_end_matches(['\r', '\n']))
            .canonicalize()?;
        let path = path.to_str().context("Git object path не UTF-8")?;
        ensure!(!path.contains(['\n', '\r']), "Некорректный Git object path");
        private_file(
            &bare.join("objects/info/alternates"),
            format!("{path}\n").as_bytes(),
        )?;
        // Durable single-attempt marker; unknown native outcome never permits automatic resend.
        let marker = context_file.with_extension("git-push-attempt");
        private_file(&marker, compact.as_bytes())
            .context("Push уже отмечен/не может быть записан; только --readback")?;
    }
    let mut command = git();
    command
        .arg("--git-dir")
        .arg(&bare)
        .arg("-c")
        .arg(format!("include.path={}", config.display()))
        .arg("-c")
        .arg(format!(
            "core.hooksPath={}",
            temporary.path().join("no-hooks").display()
        ));
    if readback {
        command.args(["ls-remote", "--refs", "--heads", &url, &reference]);
    } else {
        // Empty old expectation means absent ref, not all-zero object syntax in force-with-lease.
        let expected = if old.bytes().all(|c| c == b'0') {
            ""
        } else {
            &old
        };
        command
            .arg("push")
            .arg("--porcelain")
            .arg(format!("--force-with-lease={reference}:{expected}"))
            .arg(&url)
            .arg(format!("{new}:{reference}"));
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(300), command.output())
        .await
        .context("Git transport timeout; STOP, только --readback")?
        .context("Git transport outcome неизвестен; STOP, только --readback")?;
    ensure!(
        output.status.success(),
        "Git не подтвердил результат; STOP, только --readback без resend/remint"
    );
    if readback {
        let observed = String::from_utf8(output.stdout).context("Некорректный Git readback")?;
        let expected = format!("{new}\t{reference}\n");
        Ok(
            json!({"state":if observed==expected {"candidate_observed"} else {"held"},"business_accepted":false,"deployment_accepted":false}),
        )
    } else {
        Ok(
            json!({"state":"native_push_reported_success","readback_required":true,"business_accepted":false,"deployment_accepted":false}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Value {
        json!({"repository":"fixture/repo","source":{"task_id":"00000000-0000-0000-0000-000000000001","candidate_ref":"refs/heads/sdlc/00000000-0000-0000-0000-000000000001"},"oldOid":"0".repeat(40),"newOid":"a".repeat(40)})
    }
    #[test]
    fn only_fixed_candidate_and_plain_origin() {
        let mut c = context();
        assert!(inputs("https://forge.invalid", &c).is_ok());
        for url in [
            "https://user@forge.invalid",
            "https://forge.invalid/?token=x",
            "https://forge.invalid/path",
            "file:///tmp/repo",
        ] {
            assert!(inputs(url, &c).is_err());
        }
        c["source"]["candidate_ref"] = json!("refs/heads/main");
        assert!(inputs("https://forge.invalid", &c).is_err());
    }
    #[test]
    fn no_delete_no_replay_oid_no_repository_escape() {
        let c = context();
        for (key, value) in [
            ("newOid", "0".repeat(40)),
            ("repository", "other/../repo".into()),
            ("newOid", "HEAD".into()),
        ] {
            let mut changed = c.clone();
            changed[key] = json!(value);
            assert!(inputs("https://forge.invalid", &changed).is_err());
        }
    }
    #[test]
    fn durable_attempt_is_create_new_not_overwritten() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("attempt");
        private_file(&path, b"original").unwrap();
        assert!(private_file(&path, b"changed").is_err());
        assert_eq!(fs::read(path).unwrap(), b"original");
    }
}
