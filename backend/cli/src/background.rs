//! Read-only source commands; runtime settings remain in the existing CLI environment.
use crate::ApiClient;
use clap::Subcommand;
use serde_json::Value;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum Command {
    Tree {
        #[arg(long)]
        repository: Uuid,
        #[arg(long)]
        branch: String,
        #[arg(long, default_value = "")]
        path: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        offset: Option<usize>,
    },
    Blob {
        #[arg(long)]
        repository: Uuid,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        path: String,
    },
}

pub async fn execute(api: &ApiClient, run: Uuid, command: Command) -> anyhow::Result<Value> {
    if run.is_nil() {
        anyhow::bail!("background run UUID required");
    }
    let (repository, branch, path, kind, limit, offset) = match command {
        Command::Tree {
            repository,
            branch,
            path,
            limit,
            offset,
        } => {
            if !(1..=200).contains(&limit) {
                anyhow::bail!("tree limit must be 1..200");
            }
            (repository, branch, path, "tree", Some(limit), offset)
        }
        Command::Blob {
            repository,
            branch,
            path,
        } => (repository, branch, path, "blob", None, None),
    };
    if repository.is_nil() {
        anyhow::bail!("repository UUID required");
    }
    let request = api.get_query(
        &format!("/background/runs/{run}/repositories/{repository}/{kind}"),
        &[
            ("branch", Some(branch.clone())),
            ("path", Some(path)),
            ("limit", limit.map(|value| value.to_string())),
            ("offset", offset.map(|value| value.to_string())),
        ],
    )?;
    let result = api.json(request).await?;
    if result["source"]["repository"].as_str() != Some(repository.to_string().as_str())
        || result["source"]["branch"].as_str() != Some(branch.as_str())
        || !result["source"]["sha"].as_str().is_some_and(|value| {
            value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        anyhow::bail!("background source response identity changed");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    #[test]
    fn background_surface_has_only_source_reads() {
        let run = uuid::Uuid::new_v4().to_string();
        let repository = uuid::Uuid::new_v4().to_string();
        assert!(
            crate::Cli::try_parse_from([
                "cicd",
                "background",
                "--run-id",
                &run,
                "tree",
                "--repository",
                &repository,
                "--branch",
                "refs/heads/main"
            ])
            .is_ok()
        );
        for command in ["push", "checkout", "pipeline", "merge", "write", "create"] {
            assert!(
                crate::Cli::try_parse_from(["cicd", "background", "--run-id", &run, command])
                    .is_err()
            );
        }
    }
}
