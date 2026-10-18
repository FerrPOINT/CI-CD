//! Child of task_git. Existing CLI -> Fleet request SAT -> sealed Git files -> Smart HTTP.
use super::*;
use reqwest::Method;
use std::time::Duration;

fn origin_url(origin: &str) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(origin).context("Некорректный Fleet origin")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "Нужен точный Fleet origin"
    );
    Ok(url)
}

fn uuid_field(value: &Value, key: &str) -> anyhow::Result<uuid::Uuid> {
    let raw = field(value, key)?;
    let id = uuid::Uuid::parse_str(raw)?;
    ensure!(
        !id.is_nil() && id.to_string() == raw,
        "Некорректная identity"
    );
    Ok(id)
}

fn material_path(access: &Path, raw: &str, name: &str) -> anyhow::Result<PathBuf> {
    let root = access
        .parent()
        .context("Нет private run directory")?
        .canonicalize()?;
    let path = PathBuf::from(raw);
    ensure!(
        path.is_absolute() && path.file_name().and_then(|v| v.to_str()) == Some(name),
        "Некорректный private material path"
    );
    let slot = path.parent().context("Нет private slot")?;
    let hash = slot
        .file_name()
        .and_then(|v| v.to_str())
        .context("Нет slot hash")?;
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Некорректный slot hash"
    );
    let expected = root.join("developer-git").join(hash).join(name);
    ensure!(path == expected, "Материал другого run");
    for component in [
        root.join("developer-git"),
        expected.parent().unwrap().to_path_buf(),
        expected.clone(),
    ] {
        ensure!(
            !fs::symlink_metadata(&component)?.file_type().is_symlink(),
            "Private material symlink запрещён"
        );
    }
    Ok(expected)
}

async fn bounded_json(mut response: reqwest::Response) -> anyhow::Result<Value> {
    ensure!(
        response.status().is_success(),
        "Fleet не разрешил Git preparation; без fallback"
    );
    ensure!(
        response
            .headers()
            .get_all(reqwest::header::CACHE_CONTROL)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .any(|v| v.trim().eq_ignore_ascii_case("no-store")),
        "Fleet response должен быть no-store"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Fleet readback неизвестен")?
    {
        ensure!(
            bytes.len() + chunk.len() <= 16_384,
            "Слишком большой Fleet readback"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).context("Некорректный Fleet readback")
}

async fn prepare(access_file: &Path, request: &Value) -> anyhow::Result<(Value, PathBuf, PathBuf)> {
    // Re-open pointer on EVERY invocation: worker rotation never requires a CLI-cached PAT.
    let pointer: Value = serde_json::from_str(&read_private(access_file, 16_384)?)?;
    ensure!(pointer["version"] == 1, "Неподдерживаемый access pointer");
    let run = uuid_field(&pointer, "runId")?;
    let origin = origin_url(field(&pointer, "fleetOrigin")?)?;
    let access_context = material_path(
        access_file,
        field(&pointer["material"], "contextFile")?,
        "context.json",
    )?;
    let access_token = material_path(
        access_file,
        field(&pointer["material"], "tokenFile")?,
        "child.token",
    )?;
    let access: Value = serde_json::from_str(&read_private(&access_context, 16_384)?)?;
    let operation = uuid_field(&access, "operationId")?;
    ensure!(
        uuid_field(&access, "runId")? == run,
        "Access child другого run"
    );
    let token = read_private(&access_token, 128)?;
    ensure!(
        ["sdlc_pat_", "pdlc_pat_"]
            .iter()
            .any(|prefix| token.starts_with(*prefix))
            && token.bytes().all(|b| b.is_ascii_graphic()),
        "Нужен Fleet request child"
    );
    // Existing Base CLI transport enforces trusted HTTP origins and disables redirects.
    // Explicit nonempty child prevents platform-token/operator fallback.
    let client = pdlc_cli_core::ApiClient::new(
        &format!("{origin}api/v1"),
        Some(&token),
        Duration::from_secs(60),
    )?;
    let response = client
        .request_builder(
            Method::POST,
            &format!("/pm-drafts/{operation}/delivery/runs/{run}/repository/prepare"),
        )
        .json(request)
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("Fleet preparation outcome неизвестен; сохраните exact intent")
        })?;
    let material = bounded_json(response).await?;
    let context_file = material_path(
        access_file,
        field(&material, "contextFile")?,
        "context.json",
    )?;
    let token_file = material_path(access_file, field(&material, "tokenFile")?, "child.token")?;
    let context: Value = serde_json::from_str(&read_private(&context_file, 16_384)?)?;
    ensure!(
        context["runId"] == access["runId"]
            && context["chatId"] == access["chatId"]
            && context["machineSubject"] == access["machineSubject"]
            && context["intent"]["binding"]["assignmentId"] == access["assignmentId"]
            && context["intent"]["binding"]["executionId"] == access["executionId"]
            && context["intent"]["binding"]["assignmentHash"] == access["assignmentHash"]
            && context["intent"]["binding"]["fencingToken"] == access["fencingToken"]
            && context["source"]["owner_subject"] == access["ownerSubject"]
            && context["source"]["task_id"] == access["taskId"]
            && context["source"]["project_id"] == access["projectId"],
        "Fleet material scope изменился"
    );
    Ok((context, context_file, token_file))
}

async fn commit(checkout: &Path) -> anyhow::Result<String> {
    let output = git()
        .arg("-C")
        .arg(checkout)
        .args([
            "-c",
            "core.fsmonitor=false",
            "rev-parse",
            "--verify",
            "HEAD^{commit}",
        ])
        .output()
        .await?;
    ensure!(output.status.success(), "Checkout не содержит commit");
    let oid = String::from_utf8(output.stdout)?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    ensure!(super::oid(&oid), "Некорректный commit OID");
    Ok(oid)
}

async fn ancestor(checkout: &Path, base: &str, head: &str) -> anyhow::Result<()> {
    ensure!(oid(base) && oid(head), "Некорректный base/HEAD");
    ensure!(
        git()
            .arg("-C")
            .arg(checkout)
            .args([
                "-c",
                "core.fsmonitor=false",
                "merge-base",
                "--is-ancestor",
                base,
                head
            ])
            .output()
            .await?
            .status
            .success(),
        "HEAD не происходит от immutable Tracker base"
    );
    Ok(())
}

/// Runs AFTER the existing mandatory first Workflow step and a real local commit.
/// Does not push; existing repository push-candidate consumes the returned private files.
pub(crate) async fn candidate(
    access_file: PathBuf,
    checkout: PathBuf,
    old_oid: String,
) -> anyhow::Result<Value> {
    tokio::time::timeout(Duration::from_secs(90),async {
        ensure!(oid(&old_oid),"Нужен exact old OID (40 нулей только для absent CAS)");
        let head=commit(&checkout).await?;
        ensure!(head!=old_oid,"Нет нового commit");
        let request=json!({"operation":"candidate","oldOid":old_oid,"newOid":head});
        let (context,context_file,token_file)=prepare(&access_file,&request).await?;
        ensure!(context["oldOid"]==request["oldOid"] && context["newOid"]==request["newOid"],"OID CAS изменился");
        ancestor(&checkout,field(&context["source"],"base_commit")?,&head).await?;
        ensure!(commit(&checkout).await?==head,"HEAD изменился во время preparation");
        Ok(json!({"state":"candidate_prepared","context_file":context_file,"child_token_file":token_file,
            "old_oid":old_oid,"new_oid":head,"business_accepted":false}))
    }).await.context("Preparation timeout; без автоматического resend")?
}

fn checkout_location(checkout: &Path) -> anyhow::Result<()> {
    ensure!(checkout.is_absolute(), "Нужен абсолютный checkout path");
    let parent = checkout.parent().context("Нет checkout parent")?;
    ensure!(
        parent.canonicalize()? == parent,
        "Checkout parent должен быть canonical"
    );
    Ok(())
}

// Only metadata lookup sees normal user/system config; transport stays isolated.
fn author_git() -> Command {
    let mut command = git();
    for name in [
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        match std::env::var_os(name) {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    for (name, value) in std::env::vars_os() {
        if name.to_str().is_some_and(|name| {
            name.starts_with("GIT_CONFIG_KEY_") || name.starts_with("GIT_CONFIG_VALUE_")
        }) {
            command.env(name, value);
        }
    }
    command
}

async fn default_author(checkout: &Path, config_git: impl Fn() -> Command) -> anyhow::Result<()> {
    for (key, fallback) in [
        ("user.name", "Fleet Control Developer"),
        ("user.email", "developer@fleet-control.local"),
    ] {
        let configured = config_git()
            .arg("-C")
            .arg(checkout)
            .args(["config", "--get", key])
            .output()
            .await?;
        match configured.status.code() {
            Some(0) => continue,
            Some(1) => (),
            _ => anyhow::bail!("Git author config недоступен; без замены identity"),
        }
        ensure!(
            git()
                .arg("-C")
                .arg(checkout)
                .args(["config", "--local", key, fallback])
                .output()
                .await?
                .status
                .success(),
            "Local Git author default не подтверждён"
        );
    }
    Ok(())
}

async fn reuse_checkout(checkout: &Path, base: &str, source: &[u8]) -> anyhow::Result<Value> {
    ensure!(
        !fs::symlink_metadata(checkout)?.file_type().is_symlink(),
        "Checkout symlink запрещён"
    );
    let git_dir = checkout.join(".git");
    ensure!(
        fs::symlink_metadata(&git_dir)?.is_dir(),
        "Нужен исходный собственный checkout, не worktree/link"
    );
    ensure!(
        read_private(&git_dir.join("forge-source.json"), 16_384)?.as_bytes() == source,
        "Существующий checkout другой/незавершённый; без reset/overwrite"
    );
    ancestor(checkout, base, &commit(checkout).await?).await?;
    Ok(json!({"state":"checkout_reused","checkout":checkout,"base_oid":base}))
}

async fn checkout_inner(
    origin: &str,
    access_file: &Path,
    checkout: &Path,
) -> anyhow::Result<Value> {
    checkout_location(checkout)?;
    let (context, context_file, token_file) =
        prepare(access_file, &json!({"operation":"checkout"})).await?;
    let base = field(&context["source"], "base_commit")?;
    ensure!(
        context["newOid"] == base && context["oldOid"] == "0".repeat(40),
        "Checkout grant не соответствует base"
    );
    let (url, reference, _, _) = inputs(origin, &context)?;
    let source = serde_json::to_vec(&context["source"])?;
    if fs::symlink_metadata(checkout).is_ok() {
        return reuse_checkout(checkout, base, &source).await;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(checkout)?;
    ensure!(
        git()
            .args(["init", "--object-format=sha1", "--template="])
            .arg(checkout)
            .output()
            .await?
            .status
            .success(),
        "Checkout init не подтверждён"
    );
    let git_dir = checkout.join(".git");
    private_file(&git_dir.join("forge-checkout-attempt"), &source)?;
    let token = read_private(&token_file, 128)?;
    ensure!(
        ["sdlc_pat_", "pdlc_pat_"]
            .iter()
            .any(|prefix| token.starts_with(*prefix))
            && token.bytes().all(|b| b.is_ascii_graphic()),
        "Нужен Git child"
    );
    let compact = serde_json::to_string(&context)?;
    ensure!(compact.is_ascii(), "Git context header должен быть ASCII");
    let temporary = tempfile::tempdir()?;
    let config = temporary.path().join("private-config");
    private_file(&config,format!("[http]\n\textraHeader = \"Authorization: Bearer {}\"\n\textraHeader = \"X-Forge-Task-Git-Context: {}\"\n\tfollowRedirects = false\n\tproxy =\n[credential]\n\thelper =\n",escaped(&token),escaped(&compact)).as_bytes())?;
    // Real registered Git bytes, not an empty worktree or caller-provided tree.
    ensure!(
        git()
            .arg("-C")
            .arg(checkout)
            .arg("-c")
            .arg(format!("include.path={}", config.display()))
            .args([
                "-c",
                "core.hooksPath=",
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                &url,
                &format!("{base}:refs/fleet/immutable-base")
            ])
            .output()
            .await?
            .status
            .success(),
        "Fetch не подтверждён; partial checkout сохранён, без reset/retry"
    );
    let branch = reference
        .strip_prefix("refs/heads/")
        .context("Некорректный candidate ref")?;
    ensure!(
        git()
            .arg("-C")
            .arg(checkout)
            .args(["-c", "core.hooksPath=", "checkout", "-b", branch, base])
            .output()
            .await?
            .status
            .success(),
        "Checkout не подтверждён; без reset/retry"
    );
    ensure!(
        commit(checkout).await? == base,
        "Checkout OID отличается от Tracker base"
    );
    default_author(checkout, author_git).await?;
    private_file(&git_dir.join("forge-source.json"), &source)?;
    Ok(
        json!({"state":"checkout_prepared","checkout":checkout,"base_oid":base,"context_file":context_file}),
    )
}

/// Only fetch/checkout here. The existing Git grant also permits exact absent->base
/// candidate CAS; this command never invokes receive-pack and is NOT a read-only grant.
pub(crate) async fn checkout(
    origin: &str,
    access_file: PathBuf,
    checkout: PathBuf,
) -> anyhow::Result<Value> {
    tokio::time::timeout(
        Duration::from_secs(300),
        checkout_inner(origin, &access_file, &checkout),
    )
    .await
    .context("Checkout timeout; partial checkout сохранён без автоматического retry")?
}

#[cfg(test)]
#[path = "task_git_author_tests.rs"]
mod author_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn normal_http_uses_only_fleet_child_and_rejects_cross_run_readback() {
        use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        #[derive(Clone)]
        struct Mock {
            material: Value,
            calls: Arc<AtomicUsize>,
        }
        async fn peer(
            State(state): State<Mock>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> ([(String, String); 1], Json<Value>) {
            assert_eq!(
                headers["authorization"],
                "Bearer sdlc_pat_request_fixture_1234567890123456789012345"
            );
            assert_eq!(body, json!({"operation":"checkout"}));
            state.calls.fetch_add(1, Ordering::SeqCst);
            (
                [("cache-control".into(), "no-store".into())],
                Json(state.material),
            )
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let access_slot = root.join("developer-git").join("a".repeat(64));
        let git_slot = root.join("developer-git").join("b".repeat(64));
        fs::create_dir_all(&access_slot).unwrap();
        fs::create_dir_all(&git_slot).unwrap();
        let id = "00000000-0000-0000-0000-000000000001";
        let access = json!({"operationId":id,"runId":id,"chatId":id,"machineSubject":"machine","assignmentId":id,
            "executionId":id,"assignmentHash":"a".repeat(64),"fencingToken":1,"ownerSubject":"owner","taskId":id,"projectId":id});
        let context = json!({"runId":id,"chatId":id,"machineSubject":"machine",
            "intent":{"binding":{"assignmentId":id,"executionId":id,"assignmentHash":"a".repeat(64),"fencingToken":1}},
            "source":{"owner_subject":"owner","task_id":id,"project_id":id}});
        private_file(
            &access_slot.join("context.json"),
            serde_json::to_string(&access).unwrap().as_bytes(),
        )
        .unwrap();
        private_file(
            &access_slot.join("child.token"),
            b"sdlc_pat_request_fixture_1234567890123456789012345",
        )
        .unwrap();
        private_file(
            &git_slot.join("context.json"),
            serde_json::to_string(&context).unwrap().as_bytes(),
        )
        .unwrap();
        private_file(
            &git_slot.join("child.token"),
            b"sdlc_pat_git_fixture_12345678901234567890123456789",
        )
        .unwrap();
        let material = json!({"contextFile":git_slot.join("context.json"),"tokenFile":git_slot.join("child.token")});
        let calls = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                &format!("/api/v1/pm-drafts/{id}/delivery/runs/{id}/repository/prepare"),
                post(peer),
            )
            .with_state(Mock {
                material,
                calls: calls.clone(),
            });
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let access_file = root.join("fleet-git-access.json");
        private_file(&access_file,serde_json::to_string(&json!({"version":1,"runId":id,"fleetOrigin":origin,
            "material":{"contextFile":access_slot.join("context.json"),"tokenFile":access_slot.join("child.token")}})).unwrap().as_bytes()).unwrap();
        assert!(
            prepare(&access_file, &json!({"operation":"checkout"}))
                .await
                .is_ok()
        );
        let mut foreign = context;
        foreign["runId"] = json!("00000000-0000-0000-0000-000000000002");
        fs::remove_file(git_slot.join("context.json")).unwrap();
        private_file(
            &git_slot.join("context.json"),
            serde_json::to_string(&foreign).unwrap().as_bytes(),
        )
        .unwrap();
        assert!(
            prepare(&access_file, &json!({"operation":"checkout"}))
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        task.abort();
        let _ = task.await;
    }
    #[test]
    fn no_origin_escape_or_identity_alias() {
        assert!(origin_url("https://fleet.invalid").is_ok());
        for url in [
            "file:///repo",
            "https://u@fleet.invalid",
            "https://fleet.invalid/other",
            "https://fleet.invalid/?key=x",
        ] {
            assert!(origin_url(url).is_err());
        }
        assert!(
            uuid_field(
                &json!({"runId":"00000000-0000-0000-0000-000000000001"}),
                "runId"
            )
            .is_ok()
        );
        assert!(
            uuid_field(
                &json!({"runId":"00000000-0000-0000-0000-000000000000"}),
                "runId"
            )
            .is_err()
        );
    }
    #[test]
    fn no_cross_run_material_or_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let slot = root.join("developer-git").join("a".repeat(64));
        fs::create_dir_all(&slot).unwrap();
        let file = slot.join("context.json");
        private_file(&file, b"{}").unwrap();
        let access = root.join("fleet-git-access.json");
        assert!(material_path(&access, file.to_str().unwrap(), "context.json").is_ok());
        assert!(material_path(&access, file.to_str().unwrap(), "child.token").is_err());
        assert!(
            material_path(&access, "/other/developer-git/context.json", "context.json").is_err()
        );
        #[cfg(unix)]
        {
            fs::remove_file(&file).unwrap();
            std::os::unix::fs::symlink("/etc/passwd", &file).unwrap();
            assert!(material_path(&access, file.to_str().unwrap(), "context.json").is_err());
        }
    }
}
