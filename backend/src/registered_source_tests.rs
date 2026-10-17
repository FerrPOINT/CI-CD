//! Owned synthetic native Git fixtures only; no hosted repository, DB or live auth evidence.
use super::*;
use serde_json::json;

fn query() -> RepositoryQuery {
    serde_json::from_value(
        json!({"registry_instance_id":Uuid::new_v4(),"namespace_id":Uuid::new_v4(),
            "base_ref":"refs/heads/main","base_commit":"a".repeat(40)}),
    )
    .unwrap()
}

#[test]
fn old_reader_query_remains_metadata_only_and_source_requires_both_fields() {
    let metadata = json!({"registry_instance_id":Uuid::new_v4(),"namespace_id":Uuid::new_v4()});
    assert!(
        serde_json::from_value::<crate::repository_catalog::NamespaceQuery>(metadata.clone())
            .is_ok()
    );
    assert!(serde_json::from_value::<RepositoryQuery>(metadata.clone()).is_err());
    let mut source = metadata;
    source["base_ref"] = json!("refs/heads/main");
    assert!(serde_json::from_value::<RepositoryQuery>(source.clone()).is_err());
    source["base_commit"] = json!("a".repeat(40));
    assert!(
        serde_json::from_value::<RepositoryQuery>(source.clone())
            .unwrap()
            .source()
            .is_ok()
    );
    source.as_object_mut().unwrap().remove("base_ref");
    assert!(serde_json::from_value::<RepositoryQuery>(source).is_err());
}

#[test]
fn source_query_is_exact_branch_sha_not_a_caller_observation_or_path() {
    let mut query = query();
    query.base_ref = "refs/heads/main".into();
    query.base_commit = "a".repeat(40);
    for reference in [
        "main",
        "HEAD",
        "refs/tags/main",
        "refs/heads/main~1",
        "refs/heads/../x",
        "refs/heads/a.lock",
        "refs/heads/a\nb",
    ] {
        query.base_ref = reference.into();
        assert!(query.source().is_err(), "{reference}");
    }
    query.base_ref = "refs/heads/main".into();
    for commit in [
        "A".repeat(40),
        "a".repeat(39),
        "a".repeat(64),
        "main".into(),
    ] {
        query.base_commit = commit;
        assert!(query.source().is_err());
    }
    let raw = json!({"registry_instance_id":Uuid::new_v4(),"namespace_id":Uuid::new_v4(),
        "base_ref":"refs/heads/main","base_commit":"a".repeat(40)});
    for field in [
        "storage_name",
        "default_ref",
        "protected_refs",
        "verified",
        "source_hash",
    ] {
        let mut changed = raw.clone();
        changed[field] = json!(true);
        assert!(serde_json::from_value::<RepositoryQuery>(changed).is_err());
    }
}

#[test]
fn all_project_defaults_and_protected_refs_are_retained_without_head_selection() {
    let refs = protected_refs(
        "refs/heads/main",
        vec![
            ("release".into(), vec!["stable".into(), "main".into()]),
            (
                "next".into(),
                vec!["stable".into(), "future/not-created".into()],
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        refs,
        vec![
            "refs/heads/future/not-created",
            "refs/heads/main",
            "refs/heads/next",
            "refs/heads/release",
            "refs/heads/stable"
        ]
    );
}

#[test]
fn malformed_or_truncated_policy_never_becomes_an_empty_success() {
    assert!(protected_refs("HEAD", vec![]).is_err());
    assert!(protected_refs("refs/heads/main", vec![("bad name".into(), vec![])]).is_err());
    assert!(
        protected_refs(
            "refs/heads/main",
            vec![("main".into(), vec!["../escape".into()])]
        )
        .is_err()
    );
    assert!(protected_refs("refs/heads/main", vec![("main".into(), vec![]); 257]).is_err());
    assert!(
        protected_refs(
            "refs/heads/main",
            vec![(
                "main".into(),
                (0..256).map(|n| format!("protected-{n}")).collect()
            )]
        )
        .is_err()
    );
}

#[test]
fn catalog_source_requires_exact_active_namespace_and_ready_hosted_identity() {
    let namespace = query().namespace();
    let group = Uuid::new_v4();
    let command: OwnerCommand =
        serde_json::from_value(json!({"schema_version":1,"namespace":namespace,
        "resource":{"kind":"git_group","instance_id":Uuid::new_v4(),"resource_id":group},
        "operation_id":Uuid::new_v4(),"generation":1,"state":"active"}))
        .unwrap();
    validate_catalog(&command, &namespace, group, "hosted", true).unwrap();
    assert!(validate_catalog(&command, &query().namespace(), group, "hosted", true).is_err());
    assert!(validate_catalog(&command, &namespace, Uuid::new_v4(), "hosted", true).is_err());
    assert!(validate_catalog(&command, &namespace, group, "external", true).is_err());
    assert!(validate_catalog(&command, &namespace, group, "hosted", false).is_err());
    let mut archived = command;
    archived.state = "archived".into();
    assert!(validate_catalog(&archived, &namespace, group, "hosted", true).is_err());
}

#[test]
fn old_catalog_wire_shape_is_unchanged_and_source_has_no_git_grant() {
    let old = crate::repository_catalog::VerifiedRepository {
        namespace: query().namespace(),
        forge_instance_id: Uuid::new_v4(),
        repository_id: Uuid::new_v4(),
        public_name: "fixture/repo".into(),
        kind: "hosted".into(),
    };
    let raw = serde_json::to_value(&old).unwrap();
    assert_eq!(
        raw,
        json!({"namespace":old.namespace,"forge_instance_id":old.forge_instance_id,
            "repository_id":old.repository_id,"public_name":old.public_name,"kind":"hosted"})
    );
    let source = VerifiedSource {
        namespace: query().namespace(),
        forge_instance_id: Uuid::new_v4(),
        repository_id: Uuid::new_v4(),
        public_name: "fixture/repo".into(),
        base_ref: "refs/heads/main".into(),
        base_commit: "a".repeat(40),
        default_ref: "refs/heads/main".into(),
        protected_refs: vec!["refs/heads/main".into()],
        observed_at: Utc::now(),
        delivery_configuration: None,
    };
    let value = serde_json::to_value(source).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 9);
    for field in [
        "token",
        "candidate_ref",
        "git_write",
        "dispatch_allowed",
        "effective_admin",
    ] {
        assert!(value.get(field).is_none());
    }
}

struct BareFixture(std::path::PathBuf);
impl BareFixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("forge-source-reader-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        fixture.git(&["init", "--bare", "--quiet", "--initial-branch=main"]);
        fixture
    }
    fn git(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.0)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Source fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Source fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    fn commit(&self) -> String {
        let tree = self.git(&["hash-object", "-t", "tree", "-w", "--stdin"]);
        self.git(&["commit-tree", &tree, "-m", "source fixture"])
    }
}
impl Drop for BareFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn native_exact_branch_ignores_same_named_tag_and_does_not_change_refs() {
    let fixture = BareFixture::new();
    let commit = fixture.commit();
    fixture.git(&["update-ref", "refs/heads/main", &commit]);
    let other = fixture.git(&["hash-object", "-t", "blob", "-w", "--stdin"]);
    fixture.git(&["update-ref", "refs/tags/main", &other]);
    let before = fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
    let expected = ExpectedSource {
        reference: "refs/heads/main".into(),
        commit: commit.clone(),
    };
    let (observed, default) = native_source(&fixture.0, &expected).await.unwrap();
    assert_eq!(observed, commit);
    assert_eq!(default, "refs/heads/main");
    assert_eq!(
        before,
        fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"])
    );
}

#[tokio::test]
async fn native_missing_branch_or_wrong_sha_has_no_head_tag_fallback() {
    let fixture = BareFixture::new();
    let commit = fixture.commit();
    fixture.git(&["update-ref", "refs/heads/main", &commit]);
    fixture.git(&["update-ref", "refs/tags/release", &commit]);
    assert!(
        native_source(
            &fixture.0,
            &ExpectedSource {
                reference: "refs/heads/release".into(),
                commit: commit.clone()
            }
        )
        .await
        .is_err()
    );
    let error = native_source(
        &fixture.0,
        &ExpectedSource {
            reference: "refs/heads/main".into(),
            commit: "f".repeat(40),
        },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn native_detached_head_is_not_a_guessed_default() {
    let fixture = BareFixture::new();
    let commit = fixture.commit();
    fixture.git(&["update-ref", "refs/heads/main", &commit]);
    fixture.git(&["update-ref", "--no-deref", "HEAD", &commit]);
    assert!(
        native_source(
            &fixture.0,
            &ExpectedSource {
                reference: "refs/heads/main".into(),
                commit
            }
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn native_branch_must_be_a_commit_not_just_a_known_object_sha() {
    let fixture = BareFixture::new();
    let blob = fixture.git(&["hash-object", "-t", "blob", "-w", "--stdin"]);
    std::fs::write(
        fixture.0.join("refs/heads/not-a-commit"),
        format!("{blob}\n"),
    )
    .unwrap();
    assert!(
        native_source(
            &fixture.0,
            &ExpectedSource {
                reference: "refs/heads/not-a-commit".into(),
                commit: blob
            }
        )
        .await
        .is_err()
    );
}
