use std::{env, fmt, path::PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use thiserror::Error;

pub const DEFAULT_BIND: &str = "0.0.0.0:22801";
pub const DEFAULT_GIT_ROOT: &str = "/var/lib/forge/git";
pub const DEFAULT_ARTIFACTS_ROOT: &str = "/var/lib/forge/artifacts";
pub const DEFAULT_ARTIFACT_RETENTION_DAYS: i64 = 30;
pub const MAX_ARTIFACT_RETENTION_DAYS: i64 = 3650;
pub const DEFAULT_RUNNER_QUEUE_TIMEOUT_SECONDS: i64 = 86_400;
pub const MAX_RUNNER_QUEUE_TIMEOUT_SECONDS: i64 = 2_592_000;
pub const INSECURE_GIT_INTERNAL_TOKEN: &str = "forge-internal-dev-token";

const TEST_DATABASE_URL: &str = "postgresql://cicd-test-placeholder";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub database: DatabaseConfig,
    pub http: HttpConfig,
    pub git: GitRuntimeConfig,
    pub artifacts: ArtifactsConfig,
    pub runner: RunnerConfig,
    pub auth: AuthConfig,
    pub secrets: SecretsConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub url: String,
    pub migrations_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpConfig {
    pub bind: String,
    pub cors_allowed_origins: Option<String>,
    pub auth_cookie_secure: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GitRuntimeConfig {
    pub root: PathBuf,
    pub token: Option<String>,
    pub internal_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactsConfig {
    pub root: PathBuf,
    pub retention_days: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerConfig {
    pub mode: RunnerMode,
    pub embedded_enabled: bool,
    pub queue_timeout_seconds: Option<i64>,
    pub keep_workspace: bool,
    pub registration_token: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerMode {
    Docker,
    HostShell,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub secret: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretsConfig {
    pub key: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{variable}: {message}")]
pub struct ConfigError {
    pub variable: &'static str,
    pub message: String,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_source(|name| env::var(name).ok(), true)
    }

    pub fn from_env_for_app() -> Result<Self, ConfigError> {
        Self::from_env_source(|name| env::var(name).ok(), false)
    }

    pub fn from_env_source<F>(mut get: F, require_database_url: bool) -> Result<Self, ConfigError>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let database_url = match optional_trimmed(get("CICD_DATABASE_URL")) {
            Some(value) => value,
            None if require_database_url => {
                return Err(ConfigError::missing("CICD_DATABASE_URL"));
            }
            None => TEST_DATABASE_URL.to_string(),
        };
        let migrations_dir = path_from_env(get("CICD_MIGRATIONS_DIR"), default_migrations_dir());
        let http = HttpConfig {
            bind: string_from_env(get("CICD_BIND"), DEFAULT_BIND),
            cors_allowed_origins: optional_trimmed(get("CICD_CORS_ALLOWED_ORIGINS")),
            auth_cookie_secure: bool_from_env(
                "CICD_AUTH_COOKIE_SECURE",
                get("CICD_AUTH_COOKIE_SECURE"),
                false,
            )?,
        };
        let git = GitRuntimeConfig {
            root: path_from_env(get("CICD_GIT_ROOT"), PathBuf::from(DEFAULT_GIT_ROOT)),
            token: optional_secret_value(get("CICD_GIT_TOKEN")),
            internal_token: git_internal_token_from_env(get("CICD_GIT_INTERNAL_TOKEN"))?,
        };
        let artifacts = artifacts_config_from_source(&mut get)?;
        let runner = RunnerConfig {
            mode: runner_mode_from_env(get("CICD_RUNNER_MODE"))?,
            embedded_enabled: bool_from_env(
                "CICD_EMBEDDED_RUNNER_ENABLED",
                get("CICD_EMBEDDED_RUNNER_ENABLED"),
                true,
            )?,
            queue_timeout_seconds: runner_queue_timeout_seconds_from_env(get(
                "CICD_RUNNER_QUEUE_TIMEOUT_SECONDS",
            ))?,
            keep_workspace: bool_from_env(
                "CICD_RUNNER_KEEP_WORKSPACE",
                get("CICD_RUNNER_KEEP_WORKSPACE"),
                false,
            )?,
            registration_token: optional_secret_value(get("CICD_RUNNER_REGISTRATION_TOKEN")),
        };
        let auth = AuthConfig {
            secret: optional_secret_value(get("CICD_AUTH_SECRET")),
        };
        let secrets = SecretsConfig {
            key: secrets_key_from_env(get("CICD_SECRETS_KEY"))?,
        };
        Ok(Self {
            database: DatabaseConfig {
                url: database_url,
                migrations_dir,
            },
            http,
            git,
            artifacts,
            runner,
            auth,
            secrets,
        })
    }

    pub fn test_default() -> Self {
        Self {
            database: DatabaseConfig {
                url: TEST_DATABASE_URL.to_string(),
                migrations_dir: default_migrations_dir(),
            },
            http: HttpConfig {
                bind: DEFAULT_BIND.to_string(),
                cors_allowed_origins: None,
                auth_cookie_secure: false,
            },
            git: GitRuntimeConfig {
                root: crate::git_host::GitConfig::default().root,
                token: None,
                internal_token: None,
            },
            artifacts: ArtifactsConfig {
                root: PathBuf::from(DEFAULT_ARTIFACTS_ROOT),
                retention_days: DEFAULT_ARTIFACT_RETENTION_DAYS,
            },
            runner: RunnerConfig {
                mode: RunnerMode::Docker,
                embedded_enabled: true,
                queue_timeout_seconds: Some(DEFAULT_RUNNER_QUEUE_TIMEOUT_SECONDS),
                keep_workspace: false,
                registration_token: None,
            },
            auth: AuthConfig { secret: None },
            secrets: SecretsConfig { key: None },
        }
    }

    pub fn with_git_config(mut self, git: crate::git_host::GitConfig) -> Self {
        self.git = GitRuntimeConfig {
            root: git.root,
            token: git.token,
            internal_token: git.internal_token,
        };
        self
    }

    pub fn with_auth_secret(mut self, auth_secret: Option<String>) -> Self {
        self.auth.secret = optional_secret_value(auth_secret);
        self
    }
}

impl GitRuntimeConfig {
    pub fn to_git_config(&self) -> crate::git_host::GitConfig {
        crate::git_host::GitConfig {
            root: self.root.clone(),
            token: self.token.clone(),
            internal_token: self.internal_token.clone(),
        }
    }
}

impl ConfigError {
    pub fn missing(variable: &'static str) -> Self {
        Self {
            variable,
            message: "is required".to_string(),
        }
    }

    pub fn invalid(variable: &'static str, message: impl Into<String>) -> Self {
        Self {
            variable,
            message: message.into(),
        }
    }

    pub fn into_io_error(self) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, self.to_string())
    }
}

impl fmt::Debug for GitRuntimeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitRuntimeConfig")
            .field("root", &self.root)
            .field("token", &redacted(self.token.as_ref()))
            .field("internal_token", &redacted(self.internal_token.as_ref()))
            .finish()
    }
}

impl fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthConfig")
            .field("secret", &redacted(self.secret.as_ref()))
            .finish()
    }
}

impl fmt::Debug for SecretsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretsConfig")
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

pub fn default_migrations_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

pub fn migrations_dir_from_env() -> PathBuf {
    path_from_env(
        env::var("CICD_MIGRATIONS_DIR").ok(),
        default_migrations_dir(),
    )
}

pub fn artifacts_config_from_env() -> Result<ArtifactsConfig, ConfigError> {
    let mut get = |name: &str| env::var(name).ok();
    artifacts_config_from_source(&mut get)
}

pub fn secrets_config_from_env() -> Result<SecretsConfig, ConfigError> {
    Ok(SecretsConfig {
        key: secrets_key_from_env(env::var("CICD_SECRETS_KEY").ok())?,
    })
}

pub fn bool_from_env(
    variable: &'static str,
    raw: Option<String>,
    default: bool,
) -> Result<bool, ConfigError> {
    let Some(value) = optional_trimmed(raw).map(|value| value.to_ascii_lowercase()) else {
        return Ok(default);
    };
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ConfigError::invalid(
            variable,
            "must be one of true/false, 1/0, yes/no or on/off",
        )),
    }
}

pub fn runner_mode_from_env(raw: Option<String>) -> Result<RunnerMode, ConfigError> {
    let Some(value) = optional_trimmed(raw).map(|value| value.to_ascii_lowercase()) else {
        return Ok(RunnerMode::Docker);
    };
    match value.as_str() {
        "docker" => Ok(RunnerMode::Docker),
        "host" => Ok(RunnerMode::HostShell),
        _ => Err(ConfigError::invalid(
            "CICD_RUNNER_MODE",
            "must be either docker or host",
        )),
    }
}

pub fn runner_queue_timeout_seconds_from_env(
    raw: Option<String>,
) -> Result<Option<i64>, ConfigError> {
    let Some(value) = optional_trimmed(raw) else {
        return Ok(Some(DEFAULT_RUNNER_QUEUE_TIMEOUT_SECONDS));
    };
    let seconds = value.parse::<i64>().map_err(|_| {
        ConfigError::invalid(
            "CICD_RUNNER_QUEUE_TIMEOUT_SECONDS",
            "must be an integer number of seconds",
        )
    })?;
    if seconds == 0 {
        return Ok(None);
    }
    if !(1..=MAX_RUNNER_QUEUE_TIMEOUT_SECONDS).contains(&seconds) {
        return Err(ConfigError::invalid(
            "CICD_RUNNER_QUEUE_TIMEOUT_SECONDS",
            format!("must be 0 or 1..={MAX_RUNNER_QUEUE_TIMEOUT_SECONDS}"),
        ));
    }
    Ok(Some(seconds))
}

pub fn artifact_retention_days_from_env(raw: Option<String>) -> Result<i64, ConfigError> {
    let Some(value) = optional_trimmed(raw) else {
        return Ok(DEFAULT_ARTIFACT_RETENTION_DAYS);
    };
    let days: i64 = value.parse().map_err(|_| {
        ConfigError::invalid(
            "CICD_ARTIFACT_RETENTION_DAYS",
            "must be an integer number of days",
        )
    })?;
    if !(1..=MAX_ARTIFACT_RETENTION_DAYS).contains(&days) {
        return Err(ConfigError::invalid(
            "CICD_ARTIFACT_RETENTION_DAYS",
            format!("must be 1..={MAX_ARTIFACT_RETENTION_DAYS}"),
        ));
    }
    Ok(days)
}

pub fn secrets_key_from_env(raw: Option<String>) -> Result<Option<[u8; 32]>, ConfigError> {
    let Some(configured) = optional_secret_value(raw) else {
        return Ok(None);
    };
    let decoded = BASE64
        .decode(configured.trim())
        .map_err(|_| ConfigError::invalid("CICD_SECRETS_KEY", "must be base64-encoded 32 bytes"))?;
    decoded
        .try_into()
        .map(Some)
        .map_err(|_| ConfigError::invalid("CICD_SECRETS_KEY", "must be base64-encoded 32 bytes"))
}

pub fn optional_secret_value(raw: Option<String>) -> Option<String> {
    optional_trimmed(raw)
}

fn artifacts_config_from_source<F>(get: &mut F) -> Result<ArtifactsConfig, ConfigError>
where
    F: FnMut(&str) -> Option<String>,
{
    Ok(ArtifactsConfig {
        root: path_from_env(
            get("CICD_ARTIFACTS_DIR"),
            PathBuf::from(DEFAULT_ARTIFACTS_ROOT),
        ),
        retention_days: artifact_retention_days_from_env(get("CICD_ARTIFACT_RETENTION_DAYS"))?,
    })
}

fn git_internal_token_from_env(raw: Option<String>) -> Result<Option<String>, ConfigError> {
    let token = optional_secret_value(raw);
    if token.as_deref() == Some(INSECURE_GIT_INTERNAL_TOKEN) {
        return Err(ConfigError::invalid(
            "CICD_GIT_INTERNAL_TOKEN",
            "uses the removed insecure development default; generate a unique value or leave it blank only for isolated local development",
        ));
    }
    Ok(token)
}

fn string_from_env(raw: Option<String>, default: &str) -> String {
    optional_trimmed(raw).unwrap_or_else(|| default.to_string())
}

fn path_from_env(raw: Option<String>, default: PathBuf) -> PathBuf {
    optional_trimmed(raw).map(PathBuf::from).unwrap_or(default)
}

fn optional_trimmed(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn redacted<T>(value: Option<T>) -> Option<&'static str> {
    value.map(|_| "<redacted>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_requires_database_url_for_server_startup() {
        let error = RuntimeConfig::from_env_source(|_| None, true).unwrap_err();
        assert_eq!(error.variable, "CICD_DATABASE_URL");
    }

    #[test]
    fn runtime_config_uses_typed_defaults_for_app_harnesses() {
        let config = RuntimeConfig::from_env_source(|_| None, false).unwrap();
        assert_eq!(config.database.url, TEST_DATABASE_URL);
        assert_eq!(config.http.bind, DEFAULT_BIND);
        assert_eq!(config.git.root, PathBuf::from(DEFAULT_GIT_ROOT));
        assert_eq!(
            config.runner.queue_timeout_seconds,
            Some(DEFAULT_RUNNER_QUEUE_TIMEOUT_SECONDS)
        );
        assert!(config.runner.embedded_enabled);
        assert!(!config.runner.keep_workspace);
    }

    #[test]
    fn runtime_config_normalizes_all_runtime_groups() {
        let key = BASE64.encode([5_u8; 32]);
        let config = RuntimeConfig::from_env_source(
            |name| match name {
                "CICD_DATABASE_URL" => Some(" postgres://db ".into()),
                "CICD_MIGRATIONS_DIR" => Some(" ./migrations ".into()),
                "CICD_BIND" => Some(" 127.0.0.1:8080 ".into()),
                "CICD_CORS_ALLOWED_ORIGINS" => Some(" https://ci.example ".into()),
                "CICD_AUTH_COOKIE_SECURE" => Some(" yes ".into()),
                "CICD_GIT_ROOT" => Some(" ./git ".into()),
                "CICD_GIT_TOKEN" => Some(" git-token ".into()),
                "CICD_GIT_INTERNAL_TOKEN" => Some(" internal-token ".into()),
                "CICD_ARTIFACTS_DIR" => Some(" ./artifacts ".into()),
                "CICD_ARTIFACT_RETENTION_DAYS" => Some(" 45 ".into()),
                "CICD_RUNNER_MODE" => Some(" host ".into()),
                "CICD_EMBEDDED_RUNNER_ENABLED" => Some(" off ".into()),
                "CICD_RUNNER_QUEUE_TIMEOUT_SECONDS" => Some(" 0 ".into()),
                "CICD_RUNNER_KEEP_WORKSPACE" => Some(" true ".into()),
                "CICD_RUNNER_REGISTRATION_TOKEN" => Some(" reg-token ".into()),
                "CICD_AUTH_SECRET" => Some(" auth-secret ".into()),
                "CICD_SECRETS_KEY" => Some(key.clone()),
                _ => None,
            },
            true,
        )
        .unwrap();
        assert_eq!(config.database.url, "postgres://db");
        assert_eq!(
            config.database.migrations_dir,
            PathBuf::from("./migrations")
        );
        assert_eq!(config.http.bind, "127.0.0.1:8080");
        assert_eq!(
            config.http.cors_allowed_origins.as_deref(),
            Some("https://ci.example")
        );
        assert!(config.http.auth_cookie_secure);
        assert_eq!(config.git.root, PathBuf::from("./git"));
        assert_eq!(config.git.token.as_deref(), Some("git-token"));
        assert_eq!(config.git.internal_token.as_deref(), Some("internal-token"));
        assert_eq!(config.artifacts.root, PathBuf::from("./artifacts"));
        assert_eq!(config.artifacts.retention_days, 45);
        assert_eq!(config.runner.mode, RunnerMode::HostShell);
        assert!(!config.runner.embedded_enabled);
        assert_eq!(config.runner.queue_timeout_seconds, None);
        assert!(config.runner.keep_workspace);
        assert_eq!(
            config.runner.registration_token.as_deref(),
            Some("reg-token")
        );
        assert_eq!(config.auth.secret.as_deref(), Some("auth-secret"));
        assert_eq!(config.secrets.key, Some([5_u8; 32]));
    }

    #[test]
    fn runtime_config_rejects_dangerous_or_invalid_values() {
        assert!(
            RuntimeConfig::from_env_source(
                |name| (name == "CICD_GIT_INTERNAL_TOKEN")
                    .then(|| INSECURE_GIT_INTERNAL_TOKEN.to_string()),
                false,
            )
            .unwrap_err()
            .to_string()
            .contains("CICD_GIT_INTERNAL_TOKEN")
        );
        assert!(runner_mode_from_env(Some("podman".into())).is_err());
        assert!(bool_from_env("CICD_TEST_BOOL", Some("maybe".into()), false).is_err());
        assert!(runner_queue_timeout_seconds_from_env(Some("-1".into())).is_err());
        assert!(artifact_retention_days_from_env(Some("0".into())).is_err());
        assert!(secrets_key_from_env(Some("not-base64".into())).is_err());
    }
}
