use std::{path::PathBuf, time::Duration};

use clap::{Parser, Subcommand, ValueEnum};
use reqwest::{RequestBuilder, Url, header};
use serde_json::{Map, Value, json};

#[derive(Parser)]
#[command(name = "cicd", about = "Forge CI/CD control-plane CLI")]
struct Cli {
    #[arg(long, env = "CICD_API_URL", default_value = "http://127.0.0.1:22801")]
    api_url: String,
    #[arg(long, env = "CICD_API_TOKEN")]
    token: Option<String>,
    #[arg(long, env = "CICD_TIMEOUT_SECONDS", default_value_t = 60)]
    timeout_seconds: u64,
    #[arg(
        long,
        value_enum,
        env = "CICD_OUTPUT",
        default_value_t = OutputFormat::Json
    )]
    output: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Json,
    Table,
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
    Runner {
        #[command(subcommand)]
        command: RunnerCommand,
    },
    Secret {
        #[command(subcommand)]
        command: SecretCommand,
    },
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommand,
    },
    Environment {
        #[command(subcommand)]
        command: EnvironmentCommand,
    },
    Deployment {
        #[command(subcommand)]
        command: DeploymentCommand,
    },
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommand,
    },
    Webhook {
        #[command(subcommand)]
        command: WebhookCommand,
    },
    Notification {
        #[command(subcommand)]
        command: NotificationCommand,
    },
    Outbox {
        #[command(subcommand)]
        command: OutboxCommand,
    },
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
    Member {
        #[command(subcommand)]
        command: MemberCommand,
    },
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    List {
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        offset: Option<u32>,
    },
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
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        offset: Option<u32>,
    },
    Run {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "main")]
        git_ref: String,
        #[arg(long)]
        idempotency_key: Option<String>,
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
        #[arg(long)]
        attempt: Option<String>,
    },
    Log {
        #[arg(long)]
        id: String,
        #[arg(long)]
        message: String,
    },
    Attempts {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum RunnerCommand {
    List,
    Register {
        #[arg(long)]
        name: String,
        #[arg(long = "tag", value_delimiter = ',')]
        tags: Vec<String>,
    },
    Heartbeat {
        #[arg(long)]
        id: String,
        #[arg(long)]
        status: Option<String>,
    },
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum SecretCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Set {
        #[arg(long)]
        project: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
    },
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum ArtifactCommand {
    List {
        #[arg(long)]
        job: String,
    },
    Upload {
        #[arg(long)]
        job: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "application/octet-stream")]
        content_type: String,
    },
    Download {
        #[arg(long)]
        id: String,
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
enum EnvironmentCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        protected: bool,
        #[arg(long)]
        required_approvals: Option<i32>,
    },
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        protected: Option<bool>,
        #[arg(long)]
        required_approvals: Option<i32>,
    },
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum DeploymentCommand {
    List {
        #[arg(long)]
        environment: String,
    },
    Create {
        #[arg(long)]
        environment: String,
        #[arg(long)]
        git_ref: String,
        #[arg(long)]
        pipeline: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    Approvals {
        #[arg(long)]
        id: String,
    },
    Approve {
        #[arg(long)]
        id: String,
        #[arg(long)]
        actor: Option<String>,
        #[arg(long)]
        comment: Option<String>,
    },
    Reject {
        #[arg(long)]
        id: String,
        #[arg(long)]
        actor: Option<String>,
        #[arg(long)]
        comment: Option<String>,
    },
    Rollback {
        #[arg(long)]
        id: String,
        #[arg(long)]
        git_ref: Option<String>,
    },
}

#[derive(Subcommand)]
enum ScheduleCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        cron: String,
        #[arg(long, default_value = "main")]
        git_ref: String,
        #[arg(long)]
        enabled: Option<bool>,
    },
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        cron: String,
        #[arg(long)]
        git_ref: String,
        #[arg(long)]
        enabled: Option<bool>,
    },
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum WebhookCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        url: String,
        #[arg(long = "event", value_delimiter = ',')]
        events: Vec<String>,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        secret: Option<String>,
    },
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum NotificationCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Replace {
        #[arg(long)]
        project: String,
        #[arg(long = "config", value_name = "CHANNEL=TARGET")]
        configs: Vec<String>,
    },
    Events {
        #[arg(long)]
        project: String,
        #[arg(long)]
        limit: Option<u16>,
    },
}

#[derive(Subcommand)]
enum OutboxCommand {
    List {
        #[arg(long)]
        project: String,
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        channel: Option<String>,
    },
    Show {
        #[arg(long)]
        id: String,
    },
    Requeue {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum ReportCommand {
    Summary {
        #[arg(long)]
        project: String,
    },
}

#[derive(Subcommand)]
enum AuditCommand {
    List,
}

#[derive(Subcommand)]
enum UserCommand {
    List,
    Create {
        #[arg(long)]
        username: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        password: Option<String>,
    },
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        username: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        password: Option<String>,
    },
}

#[derive(Subcommand)]
enum MemberCommand {
    List {
        #[arg(long)]
        project: String,
    },
    Upsert {
        #[arg(long)]
        project: String,
        #[arg(long)]
        user: String,
        #[arg(long)]
        role: String,
    },
    Remove {
        #[arg(long)]
        project: String,
        #[arg(long)]
        user: String,
    },
}

#[derive(Subcommand)]
enum TokenCommand {
    List,
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long = "scope", value_delimiter = ',')]
        scopes: Vec<String>,
        #[arg(long)]
        expires_in_days: Option<i32>,
    },
    Revoke {
        #[arg(long)]
        id: String,
    },
}

struct ApiClient {
    base: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl ApiClient {
    fn new(api_url: String, token: Option<String>, timeout: Duration) -> anyhow::Result<Self> {
        Ok(Self {
            base: api_url.trim_end_matches('/').to_string(),
            token: token.and_then(|token| {
                let token = token.trim().to_string();
                (!token.is_empty()).then_some(token)
            }),
            client: reqwest::Client::builder().timeout(timeout).build()?,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base, path)
    }

    fn endpoint_with_query(
        &self,
        path: &str,
        params: &[(&str, Option<String>)],
    ) -> anyhow::Result<Url> {
        let mut url = Url::parse(&self.endpoint(path))?;
        {
            let mut query = url.query_pairs_mut();
            for (key, value) in params {
                if let Some(value) = value.as_deref().filter(|value| !value.is_empty()) {
                    query.append_pair(key, value);
                }
            }
        }
        Ok(url)
    }

    fn auth(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.auth(self.client.get(self.endpoint(path)))
    }

    fn get_query(
        &self,
        path: &str,
        params: &[(&str, Option<String>)],
    ) -> anyhow::Result<RequestBuilder> {
        Ok(self.auth(self.client.get(self.endpoint_with_query(path, params)?)))
    }

    fn post(&self, path: &str) -> RequestBuilder {
        self.auth(self.client.post(self.endpoint(path)))
    }

    fn patch(&self, path: &str) -> RequestBuilder {
        self.auth(self.client.patch(self.endpoint(path)))
    }

    fn put(&self, path: &str) -> RequestBuilder {
        self.auth(self.client.put(self.endpoint(path)))
    }

    fn delete(&self, path: &str) -> RequestBuilder {
        self.auth(self.client.delete(self.endpoint(path)))
    }

    async fn json(&self, request: RequestBuilder) -> anyhow::Result<Value> {
        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("API returned {status}: {body}");
        }
        if body.trim().is_empty() {
            return Ok(json!({}));
        }
        Ok(serde_json::from_str(&body)?)
    }

    async fn download(&self, request: RequestBuilder) -> anyhow::Result<Vec<u8>> {
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("API returned {status}: {body}");
        }
        Ok(response.bytes().await?.to_vec())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.timeout_seconds == 0 {
        anyhow::bail!("--timeout-seconds must be greater than 0");
    }
    let output = cli.output;
    let timeout = Duration::from_secs(cli.timeout_seconds);
    let api = ApiClient::new(cli.api_url, cli.token, timeout)?;
    let value = execute(&api, cli.command).await?;
    print_output(&value, output)?;
    Ok(())
}

async fn execute(api: &ApiClient, command: Command) -> anyhow::Result<Value> {
    match command {
        Command::Project { command } => project(api, command).await,
        Command::Pipeline { command } => pipeline(api, command).await,
        Command::Job { command } => job(api, command).await,
        Command::Runner { command } => runner(api, command).await,
        Command::Secret { command } => secret(api, command).await,
        Command::Artifact { command } => artifact(api, command).await,
        Command::Environment { command } => environment(api, command).await,
        Command::Deployment { command } => deployment(api, command).await,
        Command::Schedule { command } => schedule(api, command).await,
        Command::Webhook { command } => webhook(api, command).await,
        Command::Notification { command } => notification(api, command).await,
        Command::Outbox { command } => outbox(api, command).await,
        Command::Report { command } => report(api, command).await,
        Command::Audit { command } => audit(api, command).await,
        Command::User { command } => user(api, command).await,
        Command::Member { command } => member(api, command).await,
        Command::Token { command } => token(api, command).await,
    }
}

async fn project(api: &ApiClient, command: ProjectCommand) -> anyhow::Result<Value> {
    match command {
        ProjectCommand::List { limit, offset } => {
            let request = api.get_query(
                "/projects",
                &[
                    ("limit", limit.map(|value| value.to_string())),
                    ("offset", offset.map(|value| value.to_string())),
                ],
            )?;
            api.json(request).await
        }
        ProjectCommand::Create {
            name,
            repository_url,
            branch,
        } => {
            api.json(api.post("/projects").json(&json!({
                "name": name,
                "repository_url": repository_url,
                "default_branch": branch,
            })))
            .await
        }
    }
}

async fn pipeline(api: &ApiClient, command: PipelineCommand) -> anyhow::Result<Value> {
    match command {
        PipelineCommand::List {
            project,
            limit,
            offset,
        } => {
            let request = api.get_query(
                &format!("/projects/{project}/pipelines"),
                &[
                    ("limit", limit.map(|value| value.to_string())),
                    ("offset", offset.map(|value| value.to_string())),
                ],
            )?;
            api.json(request).await
        }
        PipelineCommand::Run {
            project,
            git_ref,
            idempotency_key,
        } => {
            let idempotency_key =
                idempotency_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            api.json(
                api.post(&format!("/projects/{project}/pipelines"))
                    .header("Idempotency-Key", idempotency_key)
                    .json(&json!({"git_ref": git_ref})),
            )
            .await
        }
        PipelineCommand::Show { id } => api.json(api.get(&format!("/pipelines/{id}"))).await,
    }
}

async fn job(api: &ApiClient, command: JobCommand) -> anyhow::Result<Value> {
    match command {
        JobCommand::Start { id } => set_job_status(api, &id, "running").await,
        JobCommand::Pass { id } => set_job_status(api, &id, "success").await,
        JobCommand::Fail { id } => set_job_status(api, &id, "failed").await,
        JobCommand::Logs { id, attempt } => match attempt {
            Some(attempt) => {
                api.json(api.get(&format!("/jobs/{id}/attempts/{attempt}/logs")))
                    .await
            }
            None => api.json(api.get(&format!("/jobs/{id}/logs"))).await,
        },
        JobCommand::Log { id, message } => {
            api.json(
                api.post(&format!("/jobs/{id}/logs"))
                    .json(&json!({"message": message})),
            )
            .await
        }
        JobCommand::Attempts { id } => api.json(api.get(&format!("/jobs/{id}/attempts"))).await,
    }
}

async fn set_job_status(api: &ApiClient, id: &str, status: &str) -> anyhow::Result<Value> {
    api.json(
        api.post(&format!("/jobs/{id}/status"))
            .json(&json!({"status": status})),
    )
    .await
}

async fn runner(api: &ApiClient, command: RunnerCommand) -> anyhow::Result<Value> {
    match command {
        RunnerCommand::List => api.json(api.get("/runners")).await,
        RunnerCommand::Register { name, tags } => {
            api.json(api.post("/runners").json(&json!({
                "name": name,
                "tags": clean_values(tags),
            })))
            .await
        }
        RunnerCommand::Heartbeat { id, status } => {
            api.json(
                api.post(&format!("/runners/{id}/heartbeat"))
                    .json(&json!({"status": status})),
            )
            .await
        }
        RunnerCommand::Delete { id } => api.json(api.delete(&format!("/runners/{id}"))).await,
    }
}

async fn secret(api: &ApiClient, command: SecretCommand) -> anyhow::Result<Value> {
    match command {
        SecretCommand::List { project } => {
            api.json(api.get(&format!("/projects/{project}/secrets")))
                .await
        }
        SecretCommand::Set {
            project,
            key,
            value,
        } => {
            api.json(
                api.post(&format!("/projects/{project}/secrets"))
                    .json(&json!({"key": key, "value": value})),
            )
            .await
        }
        SecretCommand::Delete { id } => api.json(api.delete(&format!("/secrets/{id}"))).await,
    }
}

async fn artifact(api: &ApiClient, command: ArtifactCommand) -> anyhow::Result<Value> {
    match command {
        ArtifactCommand::List { job } => api.json(api.get(&format!("/jobs/{job}/artifacts"))).await,
        ArtifactCommand::Upload {
            job,
            file,
            name,
            content_type,
        } => {
            let bytes = std::fs::read(&file)?;
            let artifact_name = name.unwrap_or_else(|| {
                file.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("artifact.bin")
                    .to_string()
            });
            api.json(
                api.post(&format!("/jobs/{job}/artifacts"))
                    .header("X-Artifact-Name", artifact_name)
                    .header(header::CONTENT_TYPE, content_type)
                    .body(bytes),
            )
            .await
        }
        ArtifactCommand::Download { id, output } => {
            let bytes = api
                .download(api.get(&format!("/artifacts/{id}/download")))
                .await?;
            std::fs::write(&output, &bytes)?;
            Ok(json!({"saved": output.display().to_string(), "bytes": bytes.len()}))
        }
    }
}

async fn environment(api: &ApiClient, command: EnvironmentCommand) -> anyhow::Result<Value> {
    match command {
        EnvironmentCommand::List { project } => {
            api.json(api.get(&format!("/projects/{project}/environments")))
                .await
        }
        EnvironmentCommand::Create {
            project,
            name,
            url,
            protected,
            required_approvals,
        } => {
            api.json(
                api.post(&format!("/projects/{project}/environments"))
                    .json(&json!({
                        "name": name,
                        "url": url,
                        "protected": protected,
                        "required_approvals": required_approvals,
                    })),
            )
            .await
        }
        EnvironmentCommand::Update {
            id,
            name,
            url,
            status,
            protected,
            required_approvals,
        } => {
            let mut body = Map::new();
            insert_optional(&mut body, "name", name);
            insert_optional(&mut body, "url", url);
            insert_optional(&mut body, "status", status);
            insert_optional_bool(&mut body, "protected", protected);
            insert_optional_i32(&mut body, "required_approvals", required_approvals);
            api.json(
                api.patch(&format!("/environments/{id}"))
                    .json(&Value::Object(body)),
            )
            .await
        }
        EnvironmentCommand::Delete { id } => {
            api.json(api.delete(&format!("/environments/{id}"))).await
        }
    }
}

async fn deployment(api: &ApiClient, command: DeploymentCommand) -> anyhow::Result<Value> {
    match command {
        DeploymentCommand::List { environment } => {
            api.json(api.get(&format!("/environments/{environment}/deployments")))
                .await
        }
        DeploymentCommand::Create {
            environment,
            git_ref,
            pipeline,
            status,
        } => {
            let mut body = Map::new();
            body.insert("git_ref".into(), Value::String(git_ref));
            insert_optional(&mut body, "pipeline_id", pipeline);
            insert_optional(&mut body, "status", status);
            api.json(
                api.post(&format!("/environments/{environment}/deployments"))
                    .json(&Value::Object(body)),
            )
            .await
        }
        DeploymentCommand::Approvals { id } => {
            api.json(api.get(&format!("/deployments/{id}/approvals")))
                .await
        }
        DeploymentCommand::Approve { id, actor, comment } => {
            deployment_approval(api, &id, "approved", actor, comment).await
        }
        DeploymentCommand::Reject { id, actor, comment } => {
            deployment_approval(api, &id, "rejected", actor, comment).await
        }
        DeploymentCommand::Rollback { id, git_ref } => {
            let mut body = Map::new();
            insert_optional(&mut body, "git_ref", git_ref);
            api.json(
                api.post(&format!("/deployments/{id}/rollback"))
                    .json(&Value::Object(body)),
            )
            .await
        }
    }
}

async fn deployment_approval(
    api: &ApiClient,
    id: &str,
    decision: &str,
    actor: Option<String>,
    comment: Option<String>,
) -> anyhow::Result<Value> {
    let mut body = Map::new();
    body.insert("decision".into(), Value::String(decision.to_owned()));
    insert_optional(&mut body, "actor", actor);
    insert_optional(&mut body, "comment", comment);
    api.json(
        api.post(&format!("/deployments/{id}/approvals"))
            .json(&Value::Object(body)),
    )
    .await
}

async fn schedule(api: &ApiClient, command: ScheduleCommand) -> anyhow::Result<Value> {
    match command {
        ScheduleCommand::List { project } => {
            api.json(api.get(&format!("/projects/{project}/schedules")))
                .await
        }
        ScheduleCommand::Create {
            project,
            cron,
            git_ref,
            enabled,
        } => {
            api.json(
                api.post(&format!("/projects/{project}/schedules"))
                    .json(&json!({"cron": cron, "git_ref": git_ref, "enabled": enabled})),
            )
            .await
        }
        ScheduleCommand::Update {
            id,
            cron,
            git_ref,
            enabled,
        } => {
            api.json(
                api.patch(&format!("/schedules/{id}"))
                    .json(&json!({"cron": cron, "git_ref": git_ref, "enabled": enabled})),
            )
            .await
        }
        ScheduleCommand::Delete { id } => api.json(api.delete(&format!("/schedules/{id}"))).await,
    }
}

async fn webhook(api: &ApiClient, command: WebhookCommand) -> anyhow::Result<Value> {
    match command {
        WebhookCommand::List { project } => {
            api.json(api.get(&format!("/projects/{project}/webhooks")))
                .await
        }
        WebhookCommand::Create {
            project,
            url,
            events,
            enabled,
            secret,
        } => {
            api.json(
                api.post(&format!("/projects/{project}/webhooks"))
                    .json(&json!({
                        "url": url,
                        "events": clean_values(events),
                        "enabled": enabled,
                        "secret": secret,
                    })),
            )
            .await
        }
        WebhookCommand::Delete { id } => api.json(api.delete(&format!("/webhooks/{id}"))).await,
    }
}

async fn notification(api: &ApiClient, command: NotificationCommand) -> anyhow::Result<Value> {
    match command {
        NotificationCommand::List { project } => {
            api.json(api.get(&format!("/projects/{project}/notifications")))
                .await
        }
        NotificationCommand::Replace { project, configs } => {
            let body = configs
                .into_iter()
                .map(parse_notification_config)
                .collect::<anyhow::Result<Vec<_>>>()?;
            api.json(
                api.put(&format!("/projects/{project}/notifications"))
                    .json(&body),
            )
            .await
        }
        NotificationCommand::Events { project, limit } => {
            let request = api.get_query(
                &format!("/projects/{project}/notification-events"),
                &[("limit", limit.map(|value| value.to_string()))],
            )?;
            api.json(request).await
        }
    }
}

async fn outbox(api: &ApiClient, command: OutboxCommand) -> anyhow::Result<Value> {
    match command {
        OutboxCommand::List {
            project,
            limit,
            status,
            channel,
        } => {
            let request = api.get_query(
                &format!("/projects/{project}/outbox-deliveries"),
                &[
                    ("limit", limit.map(|value| value.to_string())),
                    ("status", status),
                    ("channel", channel),
                ],
            )?;
            api.json(request).await
        }
        OutboxCommand::Show { id } => api.json(api.get(&format!("/outbox-deliveries/{id}"))).await,
        OutboxCommand::Requeue { id } => {
            api.json(api.post(&format!("/outbox-deliveries/{id}/requeue")))
                .await
        }
    }
}

async fn report(api: &ApiClient, command: ReportCommand) -> anyhow::Result<Value> {
    match command {
        ReportCommand::Summary { project } => {
            api.json(api.get(&format!("/projects/{project}/reports/summary")))
                .await
        }
    }
}

async fn audit(api: &ApiClient, command: AuditCommand) -> anyhow::Result<Value> {
    match command {
        AuditCommand::List => api.json(api.get("/audit-log")).await,
    }
}

async fn user(api: &ApiClient, command: UserCommand) -> anyhow::Result<Value> {
    match command {
        UserCommand::List => api.json(api.get("/users")).await,
        UserCommand::Create {
            username,
            role,
            enabled,
            password,
        } => {
            api.json(api.post("/users").json(&json!({
                "username": username,
                "role": role,
                "enabled": enabled,
                "password": password,
            })))
            .await
        }
        UserCommand::Update {
            id,
            username,
            role,
            enabled,
            password,
        } => {
            api.json(api.patch(&format!("/users/{id}")).json(&json!({
                "username": username,
                "role": role,
                "enabled": enabled,
                "password": password,
            })))
            .await
        }
    }
}

async fn member(api: &ApiClient, command: MemberCommand) -> anyhow::Result<Value> {
    match command {
        MemberCommand::List { project } => {
            api.json(api.get(&format!("/projects/{project}/memberships")))
                .await
        }
        MemberCommand::Upsert {
            project,
            user,
            role,
        } => {
            api.json(
                api.post(&format!("/projects/{project}/memberships"))
                    .json(&json!({"user_id": user, "role": role})),
            )
            .await
        }
        MemberCommand::Remove { project, user } => {
            api.json(api.delete(&format!("/projects/{project}/memberships/{user}")))
                .await
        }
    }
}

async fn token(api: &ApiClient, command: TokenCommand) -> anyhow::Result<Value> {
    match command {
        TokenCommand::List => api.json(api.get("/api-tokens")).await,
        TokenCommand::Create {
            name,
            user,
            project,
            scopes,
            expires_in_days,
        } => {
            let mut body = Map::new();
            body.insert("name".into(), Value::String(name));
            insert_optional(&mut body, "user_id", user);
            insert_optional(&mut body, "project_id", project);
            insert_optional_i32(&mut body, "expires_in_days", expires_in_days);
            let scopes = clean_values(scopes);
            if !scopes.is_empty() {
                body.insert(
                    "scopes".into(),
                    Value::Array(scopes.into_iter().map(Value::String).collect()),
                );
            }
            api.json(api.post("/api-tokens").json(&Value::Object(body)))
                .await
        }
        TokenCommand::Revoke { id } => api.json(api.delete(&format!("/api-tokens/{id}"))).await,
    }
}

fn insert_optional(body: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(key.to_string(), Value::String(value));
    }
}

fn insert_optional_i32(body: &mut Map<String, Value>, key: &str, value: Option<i32>) {
    if let Some(value) = value {
        body.insert(key.to_string(), Value::Number(value.into()));
    }
}

fn insert_optional_bool(body: &mut Map<String, Value>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        body.insert(key.to_string(), Value::Bool(value));
    }
}

fn clean_values(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_notification_config(raw: String) -> anyhow::Result<Value> {
    let Some((channel, target)) = raw.split_once('=') else {
        anyhow::bail!("notification config must use CHANNEL=TARGET");
    };
    let channel = channel.trim();
    let target = target.trim();
    if channel.is_empty() || target.is_empty() {
        anyhow::bail!("notification channel and target are required");
    }
    Ok(json!({"channel": channel, "target": target, "enabled": true}))
}

fn print_output(value: &Value, format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
        OutputFormat::Table => print_table(value),
    }
    Ok(())
}

fn print_table(value: &Value) {
    match value {
        Value::Array(items) => print_array_table(items),
        Value::Object(map) => {
            for (key, value) in map {
                println!("{key}\t{}", table_cell(value));
            }
        }
        value => println!("{}", table_cell(value)),
    }
}

fn print_array_table(items: &[Value]) {
    if items.is_empty() {
        return;
    }
    let Some(columns) = items.iter().find_map(|item| {
        item.as_object()
            .map(|map| map.keys().cloned().collect::<Vec<_>>())
    }) else {
        for item in items {
            println!("{}", table_cell(item));
        }
        return;
    };
    println!("{}", columns.join("\t"));
    for item in items {
        let Some(map) = item.as_object() else {
            println!("{}", table_cell(item));
            continue;
        };
        let row = columns
            .iter()
            .map(|column| map.get(column).map(table_cell).unwrap_or_default())
            .collect::<Vec<_>>();
        println!("{}", row.join("\t"));
    }
}

fn table_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| String::new())
        }
    }
}
