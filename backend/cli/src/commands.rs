use super::{ApiClient, support};
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::time::Duration;

pub fn parse_variable(raw: &str) -> std::result::Result<(String, String), String> {
    let (key, value) = raw.split_once('=').ok_or("Ожидается KEY=VALUE")?;
    if key.is_empty() {
        return Err("Пустое имя переменной".into());
    }
    Ok((key.to_string(), value.to_string()))
}
fn enc(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}
#[derive(Args)]
pub struct Page {
    #[arg(long)]
    limit: Option<u32>,
    #[arg(long)]
    offset: Option<u32>,
    #[arg(long)]
    search: Option<String>,
}
impl Page {
    fn pairs(self) -> Vec<(&'static str, Option<String>)> {
        vec![
            ("limit", self.limit.map(|v| v.to_string())),
            ("offset", self.offset.map(|v| v.to_string())),
            ("search", self.search),
        ]
    }
}
#[derive(Subcommand)]
pub enum WorkCommand {
    Repository {
        #[command(subcommand)]
        command: RepositoryCommand,
    },
    Pr {
        #[command(subcommand)]
        command: PrCommand,
    },
}
#[derive(Subcommand)]
pub enum RepositoryCommand {
    List,
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        visibility: Option<String>,
    },
    Delete {
        #[arg(long)]
        name: String,
    },
    Refs {
        #[arg(long)]
        repo: String,
        #[command(flatten)]
        page: Page,
        #[arg(long)]
        kind: Option<String>,
    },
    Commits {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
    },
    Tree {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        git_ref: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[command(flatten)]
        page: Page,
    },
    Blob {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        git_ref: Option<String>,
        #[arg(long)]
        path: String,
    },
    Tags {
        #[arg(long)]
        repo: String,
        #[command(flatten)]
        page: Page,
    },
    Compare {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
}
#[derive(Subcommand)]
pub enum PrCommand {
    List {
        #[arg(long)]
        repo: String,
        #[command(flatten)]
        page: Page,
        #[arg(long)]
        status: Option<String>,
    },
    Get {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        number: u32,
    },
    Create {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        source_branch: String,
        #[arg(long)]
        target_branch: String,
        #[arg(long, conflicts_with = "from_file")]
        description: Option<String>,
        #[arg(long)]
        from_file: Option<String>,
    },
    Merge {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        number: u32,
    },
    Close {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        number: u32,
    },
    Reopen {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        number: u32,
    },
}
async fn get(api: &ApiClient, path: &str, pairs: Vec<(&str, Option<String>)>) -> Result<Value> {
    api.json(api.get_query(path, &pairs)?).await
}
async fn action(api: &ApiClient, repo: &str, number: u32, action: &str) -> Result<Value> {
    api.json(
        api.post(&format!("/repos/{}/pulls/{number}/action", enc(repo)))
            .json(&json!({"action":action})),
    )
    .await
}
pub async fn execute(api: &ApiClient, command: WorkCommand) -> Result<Value> {
    match command {
        WorkCommand::Repository {command}=>match command {
            RepositoryCommand::List=>api.json(api.get("/repositories")).await,
            RepositoryCommand::Create {name,visibility}=>api.json(api.post("/repositories").json(&json!({"name":name,"visibility":visibility}))).await,
            RepositoryCommand::Delete {name}=>api.json(api.delete(&format!("/repositories/{}",enc(&name)))).await,
            RepositoryCommand::Refs {repo,page,kind}=>{let mut pairs=page.pairs();pairs.push(("kind",kind));get(api,&format!("/repos/{}/refs",enc(&repo)),pairs).await},
            RepositoryCommand::Commits {repo,branch,limit,offset}=>get(api,&format!("/repos/{}/commits",enc(&repo)),vec![("branch",branch),("limit",limit.map(|v|v.to_string())),("offset",offset.map(|v|v.to_string()))]).await,
            RepositoryCommand::Tree {repo,git_ref,path,page}=>{let mut pairs=page.pairs();pairs.extend([("ref",git_ref),("path",path)]);get(api,&format!("/repos/{}/tree",enc(&repo)),pairs).await},
            RepositoryCommand::Blob {repo,git_ref,path}=>get(api,&format!("/repos/{}/blob",enc(&repo)),vec![("ref",git_ref),("path",Some(path))]).await,
            RepositoryCommand::Tags {repo,page}=>get(api,&format!("/repos/{}/tags",enc(&repo)),page.pairs()).await,
            RepositoryCommand::Compare {repo,from,to}=>get(api,&format!("/repos/{}/compare",enc(&repo)),vec![("from",Some(from)),("to",Some(to))]).await,
        },
        WorkCommand::Pr {command}=>match command {
            PrCommand::List {repo,page,status}=>{let mut pairs=page.pairs();pairs.push(("status",status));get(api,&format!("/repos/{}/pulls/page",enc(&repo)),pairs).await},
            PrCommand::Get {repo,number}=>api.json(api.get(&format!("/repos/{}/pulls/{number}",enc(&repo)))).await,
            PrCommand::Create {repo,title,source_branch,target_branch,description,from_file}=>api.json(api.post(&format!("/repos/{}/pulls",enc(&repo))).json(&json!({"repository_name":repo,"title":title,"source_branch":source_branch,"target_branch":target_branch,"description":support::text_input(description,from_file)?}))).await,
            PrCommand::Merge {repo,number}=>action(api,&repo,number,"merge").await,
            PrCommand::Close {repo,number}=>action(api,&repo,number,"close").await,
            PrCommand::Reopen {repo,number}=>action(api,&repo,number,"reopen").await,
        }
    }
}
pub async fn wait_pipeline(
    api: &ApiClient,
    id: &str,
    timeout: u64,
    interval: u64,
) -> Result<Value> {
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(timeout))
        .ok_or_else(|| anyhow::anyhow!("Слишком большое время ожидания"))?;
    loop {
        let request = api
            .get(&format!("/pipelines/{}", enc(id)))
            .timeout(deadline.saturating_duration_since(tokio::time::Instant::now()));
        let result = tokio::time::timeout_at(deadline, api.json(request)).await;
        let value = match result {
            Ok(value) => value?,
            Err(_) => {
                return Err(support::ApiFailure {
                    status: None,
                    code: Some("WAIT_TIMEOUT".into()),
                    message: "Превышено время ожидания; pipeline не отменён".into(),
                    request_id: None,
                }
                .into());
            }
        };
        match value
            .get("pipeline")
            .and_then(|p| p.get("status"))
            .and_then(Value::as_str)
        {
            Some("success" | "failed" | "canceled") => return Ok(value),
            Some("queued" | "running") => {}
            _ => bail!("Неизвестный статус pipeline"),
        }
        tokio::time::sleep(std::cmp::min(
            Duration::from_secs(interval),
            deadline.saturating_duration_since(tokio::time::Instant::now()),
        ))
        .await;
        if tokio::time::Instant::now() >= deadline {
            return Err(support::ApiFailure {
                status: None,
                code: Some("WAIT_TIMEOUT".into()),
                message: "Превышено время ожидания; pipeline не отменён".into(),
                request_id: None,
            }
            .into());
        }
    }
}
