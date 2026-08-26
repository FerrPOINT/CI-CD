use clap::{Parser, Subcommand};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "cicd", about = "Forge CI/CD control-plane CLI")]
struct Cli {
    #[arg(long, env = "CICD_API_URL", default_value = "http://127.0.0.1:22801")]
    api_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Pipeline {
        #[command(subcommand)]
        command: PipelineCommand,
    },
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    List,
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        repository_url: String,
        #[arg(long, default_value = "main")]
        branch: String,
    },
}

#[derive(Subcommand)]
enum PipelineCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Run {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "main")]
        git_ref: String,
    },
    Show {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum JobCommand {
    Start {
        #[arg(long)]
        id: String,
    },
    Pass {
        #[arg(long)]
        id: String,
    },
    Fail {
        #[arg(long)]
        id: String,
    },
    Logs {
        #[arg(long)]
        id: String,
    },
    Log {
        #[arg(long)]
        id: String,
        #[arg(long)]
        message: String,
    },
}

async fn request(request: reqwest::RequestBuilder) -> anyhow::Result<Value> {
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("API returned {status}: {body}");
    }
    Ok(serde_json::from_str(&body)?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let base = cli.api_url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let value =
        match cli.command {
            Command::Project {
                command: ProjectCommand::List,
            } => request(client.get(format!("{base}/api/v1/projects"))).await?,
            Command::Project {
                command:
                    ProjectCommand::Create {
                        name,
                        repository_url,
                        branch,
                    },
            } => request(client.post(format!("{base}/api/v1/projects")).json(
                &json!({"name": name, "repository_url": repository_url, "default_branch": branch}),
            ))
            .await?,
            Command::Pipeline {
                command: PipelineCommand::List { project },
            } => request(client.get(format!("{base}/api/v1/projects/{project}/pipelines"))).await?,
            Command::Pipeline {
                command: PipelineCommand::Run { project, git_ref },
            } => {
                request(
                    client
                        .post(format!("{base}/api/v1/projects/{project}/pipelines"))
                        .json(&json!({"git_ref": git_ref})),
                )
                .await?
            }
            Command::Pipeline {
                command: PipelineCommand::Show { id },
            } => request(client.get(format!("{base}/api/v1/pipelines/{id}"))).await?,
            Command::Job { command } => match command {
                JobCommand::Start { id } => {
                    request(
                        client
                            .post(format!("{base}/api/v1/jobs/{id}/status"))
                            .json(&json!({"status": "running"})),
                    )
                    .await?
                }
                JobCommand::Pass { id } => {
                    request(
                        client
                            .post(format!("{base}/api/v1/jobs/{id}/status"))
                            .json(&json!({"status": "success"})),
                    )
                    .await?
                }
                JobCommand::Fail { id } => {
                    request(
                        client
                            .post(format!("{base}/api/v1/jobs/{id}/status"))
                            .json(&json!({"status": "failed"})),
                    )
                    .await?
                }
                JobCommand::Logs { id } => {
                    request(client.get(format!("{base}/api/v1/jobs/{id}/logs"))).await?
                }
                JobCommand::Log { id, message } => {
                    request(
                        client
                            .post(format!("{base}/api/v1/jobs/{id}/logs"))
                            .json(&json!({"message": message})),
                    )
                    .await?
                }
            },
        };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
