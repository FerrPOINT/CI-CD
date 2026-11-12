//! Current Tracker assignment, independent from native runs and CI workspaces.
use crate::api::ApiError;
use cicd_domain::development_workspace::WorkspaceAction;
use pdlc_shared::resource_context::NamespaceRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceAssignment {
    pub task_id: Uuid,
    pub assignment_id: Uuid,
    pub execution_id: Uuid,
    pub agent_id: Uuid,
    pub version: i64,
    pub machine_subject: Uuid,
    pub requirement_revision: i64,
    pub stage: String,
    pub source: Value,
}

pub(super) struct WorkspacePreparation {
    commit: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AssignmentAccess {
    Active,
    Preparing,
    Reading,
    Closing,
}

impl AssignmentAccess {
    pub fn for_action(action: &WorkspaceAction) -> Self {
        match action {
            WorkspaceAction::BeginSession { .. } | WorkspaceAction::Prepare { .. } => {
                Self::Preparing
            }
            WorkspaceAction::EndSession { .. } => Self::Closing,
            _ => Self::Active,
        }
    }
}

impl WorkspacePreparation {
    pub fn require_source(&self, action: &WorkspaceAction) -> Result<(), ApiError> {
        if let WorkspaceAction::Prepare { sources, .. } = action
            && (sources.len() != 1 || sources[0].commit != self.commit)
        {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }
}

impl WorkspaceAssignment {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn read(
        context: &Value,
        namespace: &NamespaceRef,
        tracker: Uuid,
        project: Uuid,
        task: Uuid,
        subject: Uuid,
        repository: Uuid,
        workspace: Uuid,
        access: AssignmentAccess,
    ) -> Result<(Self, Option<WorkspacePreparation>), ApiError> {
        let denied = ApiError::forbidden;
        let execution = &context["execution"];
        let assignment = &execution["assignment"];
        let source = &execution["source_binding"];
        let preparation = if execution["status"] == "InProgress"
            || (access == AssignmentAccess::Preparing && execution["stage"] == "Deployment" && execution["status"] == "Ready")
            || (access == AssignmentAccess::Closing && execution["status"] == "Ready")
            || (matches!(
                access,
                AssignmentAccess::Reading | AssignmentAccess::Closing
            ) && execution["status"] == "Completed")
        {
            None
        } else if matches!(
            access,
            AssignmentAccess::Preparing | AssignmentAccess::Reading
        ) && execution["status"] == "Ready"
        {
            if !matches!(execution["stage"].as_str(), Some("Development" | "Rework"))
                || execution["workspace_preparation_authorized"] != true {
                return Err(denied());
            }
            Some(WorkspacePreparation {
                commit: execution["workspace_preparation_commit"]
                    .as_str()
                    .filter(|sha| commit_sha(sha))
                    .filter(|sha| execution["stage"] == "Rework" || source["base_commit"] == *sha)
                    .ok_or_else(denied)?
                    .into(),
            })
        } else {
            return Err(denied());
        };
        if context["schema_version"] != 1
            || context["namespace"] != serde_json::to_value(namespace).map_err(|_| denied())?
            || context["tracker_instance_id"] != tracker.to_string()
            || context["project_id"] != project.to_string()
            || context["task_id"] != task.to_string()
            || context["state"] != "active"
            || execution["interaction_protocol"] != "configured-cli-v1"
            || execution["root_task_id"] != task.to_string()
            || !matches!(execution["stage"].as_str(), Some("Development" | "Rework" | "Deployment"))
            || assignment["machine_subject"] != subject.to_string()
            || subject.is_nil()
            || task.is_nil()
            || source["contract_version"] != 1
            || source["task_id"] != task.to_string()
            || source["root_task_id"] != task.to_string()
            || source["project_id"] != project.to_string()
            || source["tracker_instance_id"] != tracker.to_string()
            || source["repository_id"] != repository.to_string()
            || source["development_workspace_id"] != workspace.to_string()
            || source["namespace"] != context["namespace"]
            || source["requirement_revision"] != execution["requirement_revision"]
            || !source["base_ref"]
                .as_str()
                .is_some_and(crate::registered_source::full_branch)
            || !source["candidate_ref"]
                .as_str()
                .is_some_and(crate::registered_source::full_branch)
            || source["base_ref"] == source["candidate_ref"]
            || !source["base_commit"].as_str().is_some_and(commit_sha)
        {
            return Err(denied());
        }
        let id = |key: &str| -> Result<Uuid, ApiError> {
            let raw = assignment[key].as_str().ok_or_else(denied)?;
            Uuid::parse_str(raw)
                .ok()
                .filter(|id| !id.is_nil() && id.to_string() == raw)
                .ok_or_else(denied)
        };
        let version = |value: &Value| -> Result<i64, ApiError> {
            value
                .as_i64()
                .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
                .ok_or_else(denied)
        };
        Ok((
            Self {
                task_id: task,
                assignment_id: id("assignment_id")?,
                execution_id: id("execution_id")?,
                agent_id: id("agent_id")?,
                version: version(&assignment["version"])?,
                machine_subject: subject,
                requirement_revision: version(&execution["requirement_revision"])?,
                stage: execution["stage"].as_str().ok_or_else(denied)?.to_owned(),
                source: source.clone(),
            },
            preparation,
        ))
    }

    pub(super) fn is_developer(&self) -> bool {
        matches!(self.stage.as_str(), "Development" | "Rework")
    }

    pub(super) fn require_action(&self, action: &WorkspaceAction, profile: &Value) -> Result<(), ApiError> {
        let allowed = match action {
            WorkspaceAction::Check { recipe, source_pipeline_id, .. } => profile["checks"]
                .as_array().is_some_and(|checks| checks.iter().any(|check| {
                    check["key"] == *recipe && if self.is_developer() {
                        source_pipeline_id.is_none() && check["requiresRuntime"] == false
                            && (check["executor"] == "forge" || (check["executor"] == "forge-build"
                                && check["kind"] == "build" && check.get("commands").is_none()))
                    } else {
                        self.stage == "Deployment" && source_pipeline_id.is_some_and(|id| !id.is_nil())
                            && check["executor"] == "forge-delivery"
                    }
                })),
            WorkspaceAction::BeginSession { .. } | WorkspaceAction::EndSession { .. } => self.is_developer() || self.stage == "Deployment",
            WorkspaceAction::Prepare { .. } => self.is_developer(),
            _ => false,
        };
        if allowed { Ok(()) } else { Err(ApiError::forbidden()) }
    }

    pub(super) fn require_same(&self, saved: &Self) -> Result<(), ApiError> {
        if self == saved {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
}

fn commit_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn machine_action(action: &WorkspaceAction) -> bool {
    matches!(
        action,
        WorkspaceAction::BeginSession { .. }
            | WorkspaceAction::EndSession { .. }
            | WorkspaceAction::Prepare { .. }
            | WorkspaceAction::Check { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context(
        namespace: &NamespaceRef,
        tracker: Uuid,
        project: Uuid,
        task: Uuid,
        actor: Uuid,
        repository: Uuid,
        workspace: Uuid,
    ) -> Value {
        json!({"schema_version":1,"namespace":namespace,"tracker_instance_id":tracker,
            "project_id":project,"task_id":task,"state":"active",
            "execution":{"interaction_protocol":"configured-cli-v1","root_task_id":task,
                "stage":"Development","status":"InProgress","requirement_revision":3,
                "source_binding":{"contract_version":1,"task_id":task,"root_task_id":task,
                    "development_workspace_id":workspace,
                    "project_id":project,"tracker_instance_id":tracker,"repository_id":repository,
                    "forge_instance_id":Uuid::new_v4(),"namespace":namespace,"requirement_revision":3,
                    "base_ref":"refs/heads/main","base_commit":"a".repeat(40),
                    "candidate_ref":format!("refs/heads/workspace/{task}")},
                "assignment":{"assignment_id":Uuid::new_v4(),"execution_id":Uuid::new_v4(),
                    "agent_id":Uuid::new_v4(),"version":7,"machine_subject":actor}}})
    }

    #[test]
    fn current_assignment_accepts_initial_and_rework_and_rejects_foreign_or_stale_context() {
        let namespace = NamespaceRef {
            registry_instance_id: Uuid::new_v4(),
            namespace_id: Uuid::new_v4(),
        };
        let (tracker, project, task, actor) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let repository = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let initial = context(
            &namespace, tracker, project, task, actor, repository, workspace,
        );
        let read = |v: &Value| {
            WorkspaceAssignment::read(
                v,
                &namespace,
                tracker,
                project,
                task,
                actor,
                repository,
                workspace,
                AssignmentAccess::Active,
            )
            .map(|(assignment, _)| assignment)
        };
        let saved = read(&initial).unwrap();
        saved.require_same(&saved).unwrap();
        let mut completed = initial.clone();
        completed["execution"]["status"] = json!("Completed");
        for access in [
            AssignmentAccess::Active,
            AssignmentAccess::Preparing,
            AssignmentAccess::Reading,
            AssignmentAccess::Closing,
        ] {
            let result = WorkspaceAssignment::read(
                &completed, &namespace, tracker, project, task, actor, repository, workspace,
                access,
            );
            if matches!(
                access,
                AssignmentAccess::Reading | AssignmentAccess::Closing
            ) {
                let (closing, deadline) = result.unwrap();
                closing.require_same(&saved).unwrap();
                assert!(deadline.is_none());
            } else {
                assert!(result.is_err());
            }
        }
        let mut rework = initial.clone();
        rework["execution"]["stage"] = json!("Rework");
        let repaired = read(&rework).unwrap();
        assert!(repaired.require_same(&saved).is_err());
        let mut reserved = initial.clone();
        reserved["execution"]["status"] = json!("Ready");
        let read_preparation = |v: &Value| {
            WorkspaceAssignment::read(
                v,
                &namespace,
                tracker,
                project,
                task,
                actor,
                repository,
                workspace,
                AssignmentAccess::Preparing,
            )
        };
        assert!(read_preparation(&reserved).is_err());
        let (closing, deadline) = WorkspaceAssignment::read(
            &reserved,
            &namespace,
            tracker,
            project,
            task,
            actor,
            repository,
            workspace,
            AssignmentAccess::Closing,
        )
        .unwrap();
        closing.require_same(&saved).unwrap();
        assert!(deadline.is_none());
        reserved["execution"]["workspace_preparation_expires_at"] =
            json!(chrono::Utc::now() + chrono::Duration::seconds(30));
        reserved["execution"]["workspace_preparation_commit"] = json!("a".repeat(40));
        assert!(read(&reserved).is_err());
        assert!(read_preparation(&reserved).is_err(), "an old expiry is not authorization");
        reserved["execution"]["workspace_preparation_authorized"] = json!(true);
        reserved["execution"]["workspace_preparation_expires_at"] = Value::Null;
        let (preparing, deadline) = read_preparation(&reserved).unwrap();
        preparing.require_same(&saved).unwrap();
        assert!(deadline.is_some());
        reserved["execution"]["workspace_preparation_expires_at"] =
            json!(chrono::Utc::now() - chrono::Duration::seconds(1));
        assert!(read_preparation(&reserved).is_ok(), "historical expiry is not an ownership deadline");
        reserved["execution"]["workspace_preparation_authorized"] = json!(false);
        assert!(read_preparation(&reserved).is_err());
        for (pointer, value) in [
            ("/schema_version", json!(2)),
            ("/tracker_instance_id", json!(Uuid::new_v4())),
            ("/namespace/namespace_id", json!(Uuid::new_v4())),
            ("/namespace/registry_instance_id", json!(Uuid::new_v4())),
            ("/project_id", json!(Uuid::new_v4())),
            ("/task_id", json!(Uuid::new_v4())),
            ("/state", json!("disabled")),
            ("/execution", Value::Null),
            ("/execution/root_task_id", json!(Uuid::new_v4())),
            ("/execution/interaction_protocol", json!("legacy")),
            ("/execution/status", json!("Completed")),
            ("/execution/stage", json!("Testing")),
            (
                "/execution/assignment/machine_subject",
                json!(Uuid::new_v4()),
            ),
            ("/execution/assignment/agent_id", json!(Uuid::nil())),
            ("/execution/assignment/version", json!(0)),
            (
                "/execution/assignment/version",
                json!(9_007_199_254_740_992_i64),
            ),
            ("/execution/requirement_revision", Value::Null),
            ("/execution/source_binding", Value::Null),
            (
                "/execution/source_binding/development_workspace_id",
                Value::Null,
            ),
            (
                "/execution/source_binding/development_workspace_id",
                json!(Uuid::new_v4()),
            ),
            (
                "/execution/source_binding/repository_id",
                json!(Uuid::new_v4()),
            ),
            ("/execution/source_binding/task_id", json!(Uuid::new_v4())),
            ("/execution/source_binding/requirement_revision", json!(4)),
            ("/execution/source_binding/base_commit", json!("main")),
            (
                "/execution/source_binding/candidate_ref",
                json!("refs/heads/main"),
            ),
            (
                "/execution/source_binding/candidate_ref",
                json!("refs/heads/../other"),
            ),
        ] {
            let mut changed = initial.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(read(&changed).is_err(), "accepted {pointer}");
        }
        for pointer in [
            "/execution/assignment/assignment_id",
            "/execution/assignment/execution_id",
            "/execution/assignment/agent_id",
            "/execution/assignment/version",
            "/execution/requirement_revision",
        ] {
            let mut changed = initial.clone();
            *changed.pointer_mut(pointer).unwrap() = if pointer.ends_with("version") {
                json!(8)
            } else if pointer.ends_with("revision") {
                json!(4)
            } else {
                json!(Uuid::new_v4())
            };
            if pointer.ends_with("revision") {
                changed["execution"]["source_binding"]["requirement_revision"] = json!(4);
            }
            assert!(
                read(&changed).unwrap().require_same(&saved).is_err(),
                "replayed {pointer}"
            );
        }
    }

    #[test]
    fn deployment_assignment_uses_its_own_session_and_only_published_delivery_checks() {
        let namespace = NamespaceRef { registry_instance_id:Uuid::new_v4(), namespace_id:Uuid::new_v4() };
        let (tracker, project, task, actor, repository, workspace) =
            (Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4(),Uuid::new_v4());
        let mut value = context(&namespace,tracker,project,task,actor,repository,workspace);
        let developer = WorkspaceAssignment::read(&value,&namespace,tracker,project,task,actor,repository,workspace,AssignmentAccess::Active).unwrap().0;
        value["execution"]["stage"] = json!("Deployment");
        let read = |value: &Value, access| WorkspaceAssignment::read(value,&namespace,tracker,project,task,actor,repository,workspace,access);
        let deployment = read(&value,AssignmentAccess::Active).unwrap().0;
        assert!(!deployment.is_developer());
        assert!(deployment.require_same(&developer).is_err());
        let profile = json!({"checks":[{"key":"unit","executor":"forge","requiresRuntime":false},
            {"key":"release","executor":"forge-delivery","requiresRuntime":true},
            {"key":"publish","kind":"build","executor":"forge-build","requiresRuntime":false}]});
        let release = WorkspaceAction::Check {recipe:"release".into(),source_commit:"a".repeat(40),source_pipeline_id:Some(Uuid::new_v4())};
        assert!(deployment.require_action(&release,&profile).is_ok());
        assert!(developer.require_action(&release,&profile).is_err());
        let publication = WorkspaceAction::Check {recipe:"publish".into(),source_commit:"a".repeat(40),source_pipeline_id:None};
        assert!(developer.require_action(&publication,&profile).is_ok());
        assert!(deployment.require_action(&publication,&profile).is_err());
        let mut runtime_profile = profile.clone(); runtime_profile["checks"][2]["requiresRuntime"] = json!(true);
        assert!(developer.require_action(&publication,&runtime_profile).is_err());
        for (recipe, source_pipeline_id) in [("release",None),("release",Some(Uuid::nil())),("unit",None),("unit",Some(Uuid::new_v4())),("unknown",Some(Uuid::new_v4()))] {
            assert!(deployment.require_action(&WorkspaceAction::Check {recipe:recipe.into(),source_commit:"a".repeat(40),source_pipeline_id},&profile).is_err());
        }
        assert!(deployment.require_action(&WorkspaceAction::BeginSession{task:task.to_string()},&profile).is_ok());
        assert!(deployment.require_action(&WorkspaceAction::Start{},&profile).is_err());
        for (pointer, replacement) in [("/execution/assignment/machine_subject",json!(Uuid::new_v4())),
            ("/execution/source_binding/development_workspace_id",json!(Uuid::new_v4())),
            ("/execution/source_binding/repository_id",json!(Uuid::new_v4()))] {
            let mut foreign=value.clone(); *foreign.pointer_mut(pointer).unwrap()=replacement;
            assert!(read(&foreign,AssignmentAccess::Active).is_err());
        }
        value["execution"]["status"]=json!("Completed");
        assert!(read(&value,AssignmentAccess::Active).is_err());
        assert!(read(&value,AssignmentAccess::Reading).is_ok());
        value["execution"]["status"]=json!("Ready");
        assert!(read(&value,AssignmentAccess::Preparing).unwrap().1.is_none(), "DevOps BeginSession has no Developer preparation grant");
        value["execution"]["workspace_preparation_authorized"]=json!(true);
        value["execution"]["workspace_preparation_commit"]=json!("a".repeat(40));
        assert!(read(&value,AssignmentAccess::Preparing).is_ok());
        assert!(read(&value,AssignmentAccess::Active).is_err());
        let prepare=WorkspaceAction::Prepare { sources:vec![], local:cicd_domain::development_workspace::LocalPreparation {
            machine_id:Uuid::new_v4(),profile_hash:"a".repeat(64),repositories:vec![],tools:vec![] } };
        assert!(deployment.require_action(&prepare,&profile).is_err());
    }

    #[test]
    fn machine_actions_do_not_include_workspace_operator_actions() {
        assert!(machine_action(&WorkspaceAction::BeginSession {
            task: "R1-7".into()
        }));
        assert!(machine_action(&WorkspaceAction::EndSession {
            session_id: Uuid::new_v4()
        }));
        assert!(machine_action(&WorkspaceAction::Check {
            recipe: "backend-unit".into(),
            source_commit: "a".repeat(40),
            source_pipeline_id: None,
        }));
        for action in [
            WorkspaceAction::Register {},
            WorkspaceAction::Create {},
            WorkspaceAction::Observe {},
            WorkspaceAction::Start {},
            WorkspaceAction::Stop {},
            WorkspaceAction::RecoverSession {
                session_id: Uuid::new_v4(),
            },
            WorkspaceAction::Build {
                source_commit: "a".repeat(40),
            },
        ] {
            assert!(!machine_action(&action));
        }
    }
}
