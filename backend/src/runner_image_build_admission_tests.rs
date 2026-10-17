//! Pure immutable-plan/row projection tests. No DB/task authority, Docker or credentials.
use super::{ImageBuildPlan, JobRow, planned_image_build};
use crate::domain::image_build::ImageBuildSpec;
use axum::http::StatusCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn plan_hash(snapshot: &mut ImageBuildPlan) {
    snapshot.plan_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&snapshot.plan).unwrap())
    );
}

fn fixture() -> (JobRow, ImageBuildPlan, ImageBuildSpec) {
    let spec = ImageBuildSpec {
        context: ".".into(),
        dockerfile: "deploy/Dockerfile".into(),
        target: Some("runtime".into()),
        descriptor_artifact: "delivery/candidate.json".into(),
        readable_schema_versions: vec![1, 2],
    };
    let job = JobRow {
        id: Uuid::new_v4(),
        stage_id: Uuid::new_v4(),
        name: "candidate".into(),
        image: "forge:backend-image-build".into(),
        command: "forge:image-build".into(),
        required_secrets: vec![],
        artifact_paths: vec![spec.descriptor_artifact.clone()],
        status: "running".into(),
        pipeline_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        stage_name: "package".into(),
        git_ref: "main".into(),
        commit_sha: Some("a".repeat(40)),
        project_name: "synthetic-image-build".into(),
    };
    let raw = "version: 1\njobs:\n  candidate:\n    stage: package\n    image_build:\n      context: '.'\n      dockerfile: deploy/Dockerfile\n      target: runtime\n      descriptor_artifact: delivery/candidate.json\n      readable_schema_versions: [1, 2]\n".to_owned();
    let mut snapshot = ImageBuildPlan {
        plan: json!({"format":"v1-dag","version":1,"parser_version":"forge-dsl/1.0.0",
            "config_source":"repository","git_ref":"main","resolved_commit_sha":job.commit_sha,
            "jobs":[
                {"key":"prepare","stage":"prepare","stage_position":0,"position":0,
                 "image":"alpine:3.20","commands":["true"],"command":"true",
                 "required_tags":[],"required_secrets":[],"artifact_paths":[],
                 "timeout_seconds":null,"allow_failure":false,"needs":[]},
                {"key":job.name,"stage":job.stage_name,"stage_position":1,"position":0,
                 "image":job.image,"commands":[],"command":job.command,
                 "required_tags":[],"required_secrets":[],"artifact_paths":job.artifact_paths,
                 "timeout_seconds":null,"allow_failure":false,"needs":["prepare"],"image_build":spec}
            ],"dependencies":[{"from":"prepare","to":"candidate"}]}),
        plan_sha256: String::new(),
        config_sha256: format!("{:x}", Sha256::digest(raw.as_bytes())),
        raw_config: raw,
        config_source: "repository".into(),
        resolved_commit_sha: job.commit_sha.clone(),
        stage_position: 1,
        job_position: 0,
    };
    plan_hash(&mut snapshot);
    (job, snapshot, spec)
}

fn entry(snapshot: &mut ImageBuildPlan) -> &mut Value {
    &mut snapshot.plan["jobs"][1]
}

fn denied(job: &JobRow, snapshot: &ImageBuildPlan, case: &str) {
    match planned_image_build(job, snapshot) {
        Err(error) => assert_eq!(error.status, StatusCode::CONFLICT, "{case}"),
        Ok(_) => panic!("image build projection unexpectedly admitted: {case}"),
    }
}

#[test]
fn image_build_admission_accepts_exact_repository_projection_without_mutating_snapshot() {
    let (job, snapshot, expected) = fixture();
    let before = serde_json::to_vec(&snapshot.plan).unwrap();
    let accepted = planned_image_build(&job, &snapshot).unwrap().unwrap();
    assert_eq!(accepted, expected);
    assert_eq!(serde_json::to_vec(&snapshot.plan).unwrap(), before);
    assert_eq!(
        job.artifact_paths,
        vec![String::from("delivery/candidate.json")]
    );
}

#[test]
fn image_build_admission_ordinary_commands_never_become_a_forge_action() {
    for command in [
        "true",
        "printf '%s' forge:image-build",
        "forge:image-build-extra",
    ] {
        let (mut job, mut snapshot, _) = fixture();
        job.command = command.into();
        job.image = "alpine:3.20".into();
        entry(&mut snapshot)
            .as_object_mut()
            .unwrap()
            .remove("image_build");
        assert!(planned_image_build(&job, &snapshot).unwrap().is_none());
        snapshot.plan = json!({"format":"legacy-stages","jobs":[]});
        assert!(planned_image_build(&job, &snapshot).unwrap().is_none());
    }
}

#[test]
fn image_build_admission_either_sentinel_requires_an_actual_typed_entry() {
    for sentinel in ["command", "image", "both"] {
        let (mut job, mut snapshot, _) = fixture();
        if sentinel == "command" {
            job.image = "alpine:3.20".into();
        }
        if sentinel == "image" {
            job.command = "true".into();
        }
        entry(&mut snapshot)
            .as_object_mut()
            .unwrap()
            .remove("image_build");
        denied(&job, &snapshot, sentinel);
        snapshot.plan["jobs"] = json!([]);
        denied(&job, &snapshot, "missing planned position");
    }
    let (job, mut snapshot, _) = fixture();
    entry(&mut snapshot)["image_build"] = Value::Null;
    plan_hash(&mut snapshot);
    denied(&job, &snapshot, "explicit null is not a typed build");
}

#[test]
fn image_build_admission_rejects_plan_and_raw_config_hash_drift() {
    let (job, mut snapshot, _) = fixture();
    entry(&mut snapshot)["image_build"]["target"] = json!("another-valid-target");
    denied(&job, &snapshot, "valid spec changed without plan hash");
    let (job, mut snapshot, _) = fixture();
    snapshot.plan_sha256 = "b".repeat(64);
    denied(&job, &snapshot, "plan digest differs");
    let (job, mut snapshot, _) = fixture();
    snapshot.raw_config.push_str("# changed raw bytes\n");
    denied(&job, &snapshot, "raw config bytes differ");
    let (job, mut snapshot, _) = fixture();
    snapshot.config_sha256 = "b".repeat(64);
    denied(&job, &snapshot, "raw config digest differs");
}

#[test]
fn image_build_admission_rejects_source_projection_drift_even_with_matching_plan_digest() {
    type Mutation = fn(&mut JobRow, &mut ImageBuildPlan);
    let cases: [(&str, Mutation); 9] = [
        ("snapshot is manual", |_, s| {
            s.config_source = "manual".into()
        }),
        ("plan is manual", |_, s| {
            s.plan["config_source"] = json!("manual")
        }),
        ("wrong plan format", |_, s| {
            s.plan["format"] = json!("legacy-stages")
        }),
        ("wrong DSL version", |_, s| s.plan["version"] = json!(2)),
        ("job commit differs", |j, _| {
            j.commit_sha = Some("b".repeat(40))
        }),
        ("job commit absent", |j, _| j.commit_sha = None),
        ("snapshot commit differs", |_, s| {
            s.resolved_commit_sha = Some("b".repeat(40))
        }),
        ("snapshot commit absent", |_, s| {
            s.resolved_commit_sha = None
        }),
        ("plan commit differs", |_, s| {
            s.plan["resolved_commit_sha"] = json!("b".repeat(40))
        }),
    ];
    for (case, mutate) in cases {
        let (mut job, mut snapshot, _) = fixture();
        mutate(&mut job, &mut snapshot);
        plan_hash(&mut snapshot);
        denied(&job, &snapshot, case);
    }
}

#[test]
fn image_build_admission_rejects_wrong_selected_row_or_sentinel_projection() {
    type Mutation = fn(&mut JobRow, &mut ImageBuildPlan);
    let cases: [(&str, Mutation); 10] = [
        ("job name changed", |j, _| j.name = "neighbor".into()),
        ("job stage changed", |j, _| j.stage_name = "neighbor".into()),
        ("job command changed", |j, _| j.command = "true".into()),
        ("job image changed", |j, _| j.image = "alpine:3.20".into()),
        ("planned key changed", |_, s| {
            entry(s)["key"] = json!("neighbor")
        }),
        ("planned stage changed", |_, s| {
            entry(s)["stage"] = json!("neighbor")
        }),
        ("planned command changed", |_, s| {
            entry(s)["command"] = json!("true")
        }),
        ("planned image changed", |_, s| {
            entry(s)["image"] = json!("alpine:3.20")
        }),
        ("wrong stage position", |_, s| s.stage_position = 2),
        ("wrong job position", |_, s| s.job_position = 1),
    ];
    for (case, mutate) in cases {
        let (mut job, mut snapshot, _) = fixture();
        mutate(&mut job, &mut snapshot);
        plan_hash(&mut snapshot);
        denied(&job, &snapshot, case);
    }
}

#[test]
fn image_build_admission_rejects_commands_secrets_tags_and_failure_override() {
    for (field, value) in [
        ("commands", json!(["true"])),
        ("commands", Value::Null),
        ("required_secrets", json!(["SYNTHETIC_SECRET_NAME"])),
        ("required_secrets", Value::Null),
        ("required_tags", json!(["external"])),
        ("required_tags", Value::Null),
        ("allow_failure", json!(true)),
        ("allow_failure", Value::Null),
    ] {
        let (job, mut snapshot, _) = fixture();
        entry(&mut snapshot)[field] = value;
        plan_hash(&mut snapshot);
        denied(&job, &snapshot, field);
    }
    let (mut job, snapshot, _) = fixture();
    job.required_secrets.push("SYNTHETIC_SECRET_NAME".into());
    denied(
        &job,
        &snapshot,
        "persisted job secret differs from secret-free plan",
    );
}

#[test]
fn image_build_admission_artifact_collection_is_exactly_the_descriptor() {
    for paths in [
        json!([]),
        json!(["other.json"]),
        json!(["delivery/candidate.json", "extra.txt"]),
        json!(["delivery/candidate.json", "delivery/candidate.json"]),
        Value::Null,
    ] {
        let (job, mut snapshot, _) = fixture();
        entry(&mut snapshot)["artifact_paths"] = paths;
        plan_hash(&mut snapshot);
        denied(&job, &snapshot, "planned artifact override");
    }
    for paths in [
        vec![],
        vec!["other.json".into()],
        vec!["delivery/candidate.json".into(), "extra.txt".into()],
    ] {
        let (mut job, snapshot, _) = fixture();
        job.artifact_paths = paths;
        denied(&job, &snapshot, "persisted artifact override");
    }
}

#[test]
fn image_build_admission_preserves_closed_spec_validation_at_runtime_boundary() {
    for (field, value) in [
        ("daemon", json!("forbidden")),
        ("image", json!("caller:latest")),
        ("context", json!("../outside")),
        ("dockerfile", json!("../Dockerfile")),
        ("descriptor_artifact", json!("/outside.json")),
        ("target", json!("--allow=security.insecure")),
        ("readable_schema_versions", json!([])),
        ("readable_schema_versions", json!([1, 1])),
    ] {
        let (job, mut snapshot, _) = fixture();
        entry(&mut snapshot)["image_build"][field] = value;
        plan_hash(&mut snapshot);
        denied(&job, &snapshot, field);
    }
}
