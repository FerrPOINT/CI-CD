//! Persistent project environments, distinct from CI attempt workspaces.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Degraded,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceAction {
    Register {},
    Create {},
    Prepare {
        sources: Vec<SelectedSource>,
        local: LocalPreparation,
    },
    Observe {},
    Start {},
    Stop {},
    BeginSession {
        task: String,
    },
    EndSession {
        session_id: Uuid,
    },
    RecoverSession {
        session_id: Uuid,
    },
    Check {
        recipe: String,
        source_commit: String,
    },
    Build {
        source_commit: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectedSource {
    pub key: String,
    pub repository_id: Uuid,
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalPreparation {
    pub machine_id: Uuid,
    pub profile_hash: String,
    pub repositories: Vec<LocalRepository>,
    pub tools: Vec<LocalTool>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalTool {
    pub name: String,
    pub version: String,
    pub requirement: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalRepository {
    pub key: String,
    pub commit: String,
    pub dirty: bool,
}

impl WorkspaceAction {
    pub fn requires_free_workspace(&self) -> bool {
        matches!(
            self,
            Self::Register {} | Self::Create {} | Self::Stop {} | Self::BeginSession { .. }
        )
    }

    pub fn requires_session(&self) -> bool {
        matches!(
            self,
            Self::Prepare { .. }
                | Self::Check { .. }
                | Self::Build { .. }
                | Self::EndSession { .. }
        )
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let key = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"-_.:".contains(&value))
        };
        let commit = |value: &str| {
            value.len() == 40
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        match self {
            Self::BeginSession { task } if !key(task) => Err("invalid_task_reference"),
            Self::EndSession { session_id } if session_id.is_nil() => {
                Err("invalid_session_identity")
            }
            Self::RecoverSession { session_id } if session_id.is_nil() => {
                Err("invalid_session_identity")
            }
            Self::Check {
                recipe,
                source_commit,
            } if !key(recipe) || !commit(source_commit) => Err("invalid_check_selection"),
            Self::Build { source_commit } if !commit(source_commit) => Err("invalid_build_source"),
            Self::Prepare { sources, local } => {
                if local.machine_id.is_nil()
                    || local.tools.len() > 32
                    || local.tools.iter().any(|tool| {
                        !key(&tool.name) || tool.version.len() > 64 || tool.requirement.len() > 64
                    })
                    || local.profile_hash.len() != 64
                    || !local.profile_hash.bytes().all(|c| c.is_ascii_hexdigit())
                    || local.repositories.len() != sources.len()
                    || sources.iter().any(|source| {
                        local
                            .repositories
                            .iter()
                            .filter(|item| item.key == source.key && item.commit == source.commit)
                            .count()
                            != 1
                    })
                {
                    return Err("invalid_local_preparation_report");
                }
                let mut keys = std::collections::BTreeSet::new();
                if sources.is_empty()
                    || sources.len() > 16
                    || sources.iter().any(|source| {
                        !key(&source.key)
                            || source.repository_id.is_nil()
                            || !commit(&source.commit)
                            || !keys.insert(&source.key)
                    })
                {
                    Err("invalid_explicit_sources")
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_needs_free_workspace_but_checks_need_the_current_session() {
        assert!(WorkspaceAction::Stop {}.requires_free_workspace());
        assert!(!WorkspaceAction::Observe {}.requires_free_workspace());
        assert!(
            WorkspaceAction::Check {
                recipe: "unit".into(),
                source_commit: "a".repeat(40)
            }
            .requires_session()
        );
    }

    #[test]
    fn sources_are_explicit_complete_and_disjoint_by_repository_key() {
        let source = SelectedSource {
            key: "app".into(),
            repository_id: Uuid::from_u128(1),
            commit: "a".repeat(40),
        };
        let local = LocalPreparation {
            machine_id: Uuid::from_u128(2),
            profile_hash: "a".repeat(64),
            tools: vec![],
            repositories: vec![LocalRepository {
                key: "app".into(),
                commit: "a".repeat(40),
                dirty: true,
            }],
        };
        assert!(
            WorkspaceAction::Prepare {
                sources: vec![source.clone()],
                local: local.clone()
            }
            .validate()
            .is_ok()
        );
        assert!(
            WorkspaceAction::Prepare {
                sources: vec![source.clone(), source],
                local
            }
            .validate()
            .is_err()
        );
        assert!(
            WorkspaceAction::Build {
                source_commit: "main".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_str::<WorkspaceAction>(r#"{"action":"stop","force":true}"#).is_err()
        );
    }
}
