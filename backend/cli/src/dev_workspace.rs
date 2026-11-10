//! Workspace commands; original operation UUIDs are explicit and reusable.
use super::ApiClient;
use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use uuid::Uuid;

#[derive(Args)]
pub struct Mutation {
    #[arg(long)]
    workspace: Uuid,
    #[arg(long)]
    operation_id: Uuid,
    #[arg(long)]
    expected_version: i64,
}
#[derive(Subcommand)]
pub enum Command {
    List {
        #[arg(long)]
        project: Uuid,
    },
    Profiles {
        #[arg(long)]
        project: Uuid,
    },
    Show {
        #[arg(long)]
        workspace: Uuid,
    },
    Register {
        #[arg(long)]
        project: Uuid,
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        operation_id: Uuid,
        #[arg(long)]
        target: Uuid,
        #[arg(long)]
        name: String,
    },
    Create {
        #[arg(long)]
        project: Uuid,
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        operation_id: Uuid,
        #[arg(long)]
        target: Uuid,
        #[arg(long)]
        name: String,
    },
    Observe {
        #[command(flatten)]
        mutation: Mutation,
    },
    Start {
        #[command(flatten)]
        mutation: Mutation,
    },
    Stop {
        #[command(flatten)]
        mutation: Mutation,
    },
    BeginSession {
        #[command(flatten)]
        mutation: Mutation,
        #[arg(long)]
        task: String,
    },
    EndSession {
        #[command(flatten)]
        mutation: Mutation,
        #[arg(long)]
        session: Uuid,
    },
    RecoverSession {
        #[command(flatten)]
        mutation: Mutation,
        #[arg(long)]
        session: Uuid,
    },
    Prepare {
        #[command(flatten)]
        mutation: Mutation,
        #[arg(long)]
        root: PathBuf,
        #[arg(long="source",required=true,value_parser=super::commands::parse_variable)]
        sources: Vec<(String, String)>,
        #[arg(long, default_value = "pdlc-dev-workspace-operator")]
        source_helper: PathBuf,
        #[arg(long, hide = true)]
        request_only: bool,
    },
    Check {
        #[command(flatten)]
        mutation: Mutation,
        #[arg(long)]
        recipe: String,
        #[arg(long)]
        commit: String,
    },
    Build {
        #[command(flatten)]
        mutation: Mutation,
        #[arg(long)]
        commit: String,
    },
    Operations {
        #[arg(long)]
        workspace: Uuid,
    },
    Operation {
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        operation_id: Uuid,
    },
    Reconcile {
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        operation_id: Uuid,
    },
    Wait {
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        operation_id: Uuid,
        #[arg(long, default_value_t = 600)]
        timeout_seconds: u64,
    },
}
fn path(workspace: Uuid) -> String {
    format!("/development-workspaces/{workspace}")
}
fn operation_request(mutation: &Mutation, command: Value) -> Result<Value> {
    ensure!(
        !mutation.operation_id.is_nil() && mutation.expected_version > 0,
        "Нужны исходный UUID операции и текущая версия workspace"
    );
    Ok(
        json!({"operation_id":mutation.operation_id,"expected_version":mutation.expected_version,"command":command}),
    )
}
async fn mutate(api: &ApiClient, mutation: Mutation, command: Value) -> Result<Value> {
    let body = operation_request(&mutation, command)?;
    submit(api, mutation, body).await
}
async fn submit(api: &ApiClient, mutation: Mutation, body: Value) -> Result<Value> {
    // Never invent a replacement key after a lost response. The caller can
    // read this exact UUID using `operation`, `wait` or `reconcile`.
    api.json(
        api.post(&format!("{}/operations", path(mutation.workspace)))
            .json(&body),
    )
    .await
    .with_context(|| {
        format!(
            "Результат операции {} нужно прочитать по исходному ключу",
            mutation.operation_id
        )
    })
}
pub async fn execute(api: &ApiClient, command: Command) -> Result<Value> {
    match command {
        Command::List { project } => {
            api.json(api.get(&format!("/projects/{project}/development-workspaces")))
                .await
        }
        Command::Profiles { project } => {
            api.json(api.get(&format!(
                "/projects/{project}/development-workspace-profiles"
            )))
            .await
        }
        Command::Show { workspace } => api.json(api.get(&path(workspace))).await,
        Command::Register {
            project,
            workspace,
            operation_id,
            target,
            name,
        } => register(api, project, workspace, operation_id, target, name, false).await,
        Command::Create {
            project,
            workspace,
            operation_id,
            target,
            name,
        } => register(api, project, workspace, operation_id, target, name, true).await,
        Command::Observe { mutation } => mutate(api, mutation, json!({"action":"observe"})).await,
        Command::Start { mutation } => mutate(api, mutation, json!({"action":"start"})).await,
        Command::Stop { mutation } => mutate(api, mutation, json!({"action":"stop"})).await,
        Command::BeginSession { mutation, task } => {
            mutate(api, mutation, json!({"action":"begin_session","task":task})).await
        }
        Command::EndSession { mutation, session } => {
            mutate(
                api,
                mutation,
                json!({"action":"end_session","session_id":session}),
            )
            .await
        }
        Command::RecoverSession { mutation, session } => {
            mutate(
                api,
                mutation,
                json!({"action":"recover_session","session_id":session}),
            )
            .await
        }
        Command::Check {
            mutation,
            recipe,
            commit,
        } => {
            mutate(
                api,
                mutation,
                json!({"action":"check","recipe":recipe,"source_commit":commit}),
            )
            .await
        }
        Command::Build { mutation, commit } => {
            mutate(
                api,
                mutation,
                json!({"action":"build","source_commit":commit}),
            )
            .await
        }
        Command::Prepare {
            mutation,
            root,
            sources,
            source_helper,
            request_only,
        } => {
            let body = prepare(api, &mutation, root, sources, source_helper).await?;
            if request_only {
                Ok(body)
            } else {
                submit(api, mutation, body).await
            }
        }
        Command::Operations { workspace } => {
            api.json(api.get(&format!("{}/operations", path(workspace))))
                .await
        }
        Command::Operation {
            workspace,
            operation_id,
        } => {
            api.json(api.get(&format!("{}/operations/{operation_id}", path(workspace))))
                .await
        }
        Command::Reconcile {
            workspace,
            operation_id,
        } => {
            api.json(api.post(&format!(
                "{}/operations/{operation_id}/reconcile",
                path(workspace)
            )))
            .await
        }
        Command::Wait {
            workspace,
            operation_id,
            timeout_seconds,
        } => {
            ensure!(
                (1..=86400).contains(&timeout_seconds),
                "Таймаут должен быть от 1 до 86400 секунд"
            );
            let until = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
            loop {
                let result = api
                    .json(api.get(&format!("{}/operations/{operation_id}", path(workspace))))
                    .await?;
                if matches!(
                    result["state"].as_str(),
                    Some("succeeded" | "failed" | "unknown")
                ) {
                    return Ok(result);
                }
                if tokio::time::Instant::now() >= until {
                    bail!(
                        "Операция {operation_id} ещё не подтверждена; продолжите чтение по тому же ключу"
                    );
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
fn validate_machine_source_root(root: &Path) -> Result<()> {
    let root = root
        .canonicalize()
        .context("Корень workspace агента недоступен")?;
    ensure!(root.is_dir(), "Корень workspace агента должен быть каталогом");
    for directory in root.ancestors() {
        match directory.join(".git").symlink_metadata() {
            Ok(_) => bail!("Передайте корень workspace агента, расположенный вне Git checkout"),
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => return Err(error).context("Не удалось проверить корень workspace агента"),
        }
    }
    Ok(())
}

async fn prepare(
    api: &ApiClient,
    mutation: &Mutation,
    root: PathBuf,
    sources: Vec<(String, String)>,
    helper: PathBuf,
) -> Result<Value> {
    ensure!(
        root.is_absolute() && !root.is_symlink(),
        "Нужен абсолютный локальный каталог workspace"
    );
    let detail = api.json(api.get(&path(mutation.workspace))).await?;
    let machine = detail["session"]["machine_assignment"].is_object();
    if machine {
        ensure!(
            std::env::var("FLEET_CLI_PROTOCOL").as_deref() == Ok("configured-cli-v1")
                && std::env::var("FLEET_AGENT_ID").ok().as_deref()
                    == detail["session"]["machine_assignment"]["agent_id"].as_str()
                && std::env::var("CICD_API_TOKEN").is_ok_and(|s| !s.trim().is_empty()),
            "Подготовка агента требует настроенной identity текущего исполнителя"
        );
        validate_machine_source_root(&root)?;
    }
    let workspace = &detail["workspace"];
    let project = workspace["project_id"]
        .as_str()
        .context("В ответе отсутствует проект")?;
    let choices = api
        .json(api.get(&format!(
            "/projects/{project}/development-workspace-profiles"
        )))
        .await?;
    let target = choices["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["id"] == workspace["catalog_target_id"])
        })
        .context("Опубликованный профиль недоступен")?;
    let bindings = target["repositories"]
        .as_array()
        .context("Отсутствуют связи репозиториев")?;
    ensure!(
        sources.len() == bindings.len(),
        "Явно выберите commit каждого репозитория"
    );
    let mut selected = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut server_sources = Vec::new();
    for (key, commit) in sources {
        ensure!(
            commit.len() == 40
                && commit
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Требуется полный commit SHA"
        );
        let binding = bindings
            .iter()
            .find(|binding| binding["key"] == key)
            .context("Репозиторий отсутствует в профиле")?;
        ensure!(seen.insert(key.clone()), "Повторный ключ репозитория");
        selected.push(json!({"key":key,"commit":commit,"url":binding["clone_url"]}));
        server_sources
            .push(json!({"key":key,"commit":commit,"repository_id":binding["repository_id"]}));
    }
    let mut profile = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(&mut profile, &workspace["profile"])?;
    profile.flush()?;
    let request = json!({"root":root,"repositories":selected,"operationId":mutation.operation_id});
    let workspace_id = mutation.workspace;
    let output = tokio::task::spawn_blocking(move || -> Result<std::process::Output> {
        let mut command = std::process::Command::new(helper);
        if machine {
            // Keep the permanent credential helper configured before the run.
            // Only a nonsecret selector is added for this exact preparation;
            // neither the token nor an agent's previous workspace enters argv.
            let count = std::env::var("GIT_CONFIG_COUNT")
                .unwrap_or_else(|_| "0".into())
                .parse::<usize>()
                .context("Некорректная настройка Git агента")?;
            ensure!(count <= 32, "Слишком много настроек Git агента");
            command
                .env("GIT_CONFIG_COUNT", (count + 1).to_string())
                .env(format!("GIT_CONFIG_KEY_{count}"), "credential.username")
                .env(
                    format!("GIT_CONFIG_VALUE_{count}"),
                    format!("workspace-{workspace_id}"),
                );
        } else {
            command.env_remove("CICD_API_TOKEN");
        }
        let mut child = command
            .arg("prepare-sources")
            .arg("--profile")
            .arg(profile.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Установите Base workspace source helper на машине разработчика")?;
        child
            .stdin
            .take()
            .context("stdin недоступен")?
            .write_all(&serde_json::to_vec(&request)?)?;
        Ok(child.wait_with_output()?)
    })
    .await??;
    ensure!(
        output.status.success() && output.stdout.len() <= 128 * 1024,
        "Подготовка исходников не подтверждена; существующие файлы сохраняются"
    );
    let report: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        report["operationId"] == mutation.operation_id.to_string()
            && report["workingTreeTransferred"] == false,
        "Неверная квитанция локальной подготовки"
    );
    std::fs::create_dir_all(&root)?;
    let machine_file = root.join(".pdlc-workspace-machine.json");
    ensure!(
        !machine_file.is_symlink(),
        "Некорректная локальная identity"
    );
    let machine = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&machine_file)
    {
        Ok(mut file) => {
            let id = Uuid::new_v4();
            file.write_all(serde_json::to_string(&json!({"machine_id":id}))?.as_bytes())?;
            file.sync_all()?;
            id
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let bytes = std::fs::read(&machine_file)?;
            ensure!(bytes.len() < 1024, "Некорректная локальная identity");
            serde_json::from_slice::<Value>(&bytes)?["machine_id"]
                .as_str()
                .context("Отсутствует machine identity")?
                .parse::<Uuid>()?
        }
        Err(error) => return Err(error.into()),
    };
    let repositories = report["repositories"]
        .as_array()
        .context("Отсутствуют результаты подготовки")?
        .iter()
        .map(|item| json!({"key":item["key"],"commit":item["commit"],"dirty":item["dirty"]}))
        .collect::<Vec<_>>();
    operation_request(
        mutation,
        json!({"action":"prepare","sources":server_sources,"local":{"machine_id":machine,"profile_hash":workspace["profile_hash"],"repositories":repositories,"tools":report["tools"]}}),
    )
}

#[cfg(test)]
mod tests {
    use super::validate_machine_source_root;

    #[test]
    fn machine_prepare_rejects_roots_inside_a_git_checkout() {
        let root = tempfile::tempdir().unwrap();
        let checkout = root.path().join("checkout");
        let nested = checkout.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../metadata\n").unwrap();

        assert!(validate_machine_source_root(root.path()).is_ok());
        assert!(validate_machine_source_root(&checkout).is_err());
        assert!(validate_machine_source_root(&nested).is_err());
    }
}

#[allow(clippy::too_many_arguments)]
async fn register(
    api: &ApiClient,
    project: Uuid,
    workspace: Uuid,
    operation_id: Uuid,
    target: Uuid,
    name: String,
    create: bool,
) -> Result<Value> {
    api.json(api.post(&format!("/projects/{project}/development-workspaces")).json(&json!({"id":workspace,"operation_id":operation_id,"catalog_target_id":target,"name":name,"create":create}))).await
}
