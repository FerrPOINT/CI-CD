//! Parsing and early-dispatch only. No HTTP, Docker, Git command, or credential issuance.
use super::*;

#[test]
fn prepare_checkout_parses_explicit_private_material_and_checkout_paths() {
    let cli = Cli::try_parse_from([
        "cicd",
        "repository",
        "prepare-checkout",
        "--access-file",
        "private/access.json",
        "--checkout",
        "project-checkout",
    ])
    .unwrap();
    match cli.command {
        Command::Work(commands::WorkCommand::Repository {
            command:
                commands::RepositoryCommand::PrepareCheckout {
                    access_file,
                    checkout,
                },
        }) => {
            assert_eq!(access_file, PathBuf::from("private/access.json"));
            assert_eq!(checkout, PathBuf::from("project-checkout"));
        }
        _ => panic!("prepare-checkout must belong to the repository group"),
    }
}

#[test]
fn prepare_candidate_parses_exact_cas_old_and_no_caller_new_oid() {
    let old = "a".repeat(40);
    let cli = Cli::try_parse_from([
        "cicd",
        "repository",
        "prepare-candidate",
        "--access-file",
        "private/access.json",
        "--checkout",
        "project-checkout",
        "--old-oid",
        &old,
    ])
    .unwrap();
    match cli.command {
        Command::Work(commands::WorkCommand::Repository {
            command:
                commands::RepositoryCommand::PrepareCandidate {
                    access_file,
                    checkout,
                    old_oid,
                },
        }) => {
            assert_eq!(access_file, PathBuf::from("private/access.json"));
            assert_eq!(checkout, PathBuf::from("project-checkout"));
            assert_eq!(old_oid, old);
        }
        _ => panic!("prepare-candidate must belong to the repository group"),
    }
}

#[test]
fn prepare_requires_paths_and_candidate_old_oid_without_authority_overrides() {
    for args in [
        vec!["cicd", "repository", "prepare-checkout"],
        vec![
            "cicd",
            "repository",
            "prepare-checkout",
            "--access-file",
            "access.json",
        ],
        vec![
            "cicd",
            "repository",
            "prepare-candidate",
            "--access-file",
            "access.json",
            "--checkout",
            "repo",
        ],
        vec![
            "cicd",
            "repository",
            "prepare-candidate",
            "--access-file",
            "access.json",
            "--checkout",
            "repo",
            "--old-oid",
            "HEAD",
            "--new-oid",
            "untrusted",
        ],
        vec![
            "cicd",
            "repository",
            "prepare-checkout",
            "--access-file",
            "access.json",
            "--checkout",
            "repo",
            "--machine-subject",
            "operator",
        ],
        vec![
            "cicd",
            "prepare-checkout",
            "--access-file",
            "access.json",
            "--checkout",
            "repo",
        ],
        vec![
            "cicd",
            "prepare-candidate",
            "--access-file",
            "access.json",
            "--checkout",
            "repo",
            "--old-oid",
            "HEAD",
        ],
        vec![
            "cicd",
            "work",
            "repository",
            "prepare-checkout",
            "--access-file",
            "access.json",
            "--checkout",
            "repo",
        ],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[tokio::test]
async fn checkout_dispatch_uses_private_access_not_general_forge_client() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let command = Command::Work(commands::WorkCommand::Repository {
        command: commands::RepositoryCommand::PrepareCheckout {
            access_file: root.join("missing-private-access.json"),
            checkout: root.join("checkout"),
        },
    });
    // A general ApiClient would fail on this URL/token configuration first.
    // The real early dispatcher instead reaches the private-file reader.
    let error = private_git(&command, "not-a-service-url")
        .await
        .unwrap()
        .unwrap_err();
    assert!(format!("{error:#}").contains("Private Git input недоступен"));
    assert!(error.downcast_ref::<CliError>().is_none());
    assert!(!root.join("checkout").exists());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
}

#[tokio::test]
async fn candidate_dispatch_rejects_invalid_cas_before_any_git_or_http_effect() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let command = Command::Work(commands::WorkCommand::Repository {
        command: commands::RepositoryCommand::PrepareCandidate {
            access_file: root.join("missing-private-access.json"),
            checkout: root.join("missing-checkout"),
            old_oid: "HEAD".into(),
        },
    });
    let error = private_git(&command, "not-a-service-url")
        .await
        .unwrap()
        .unwrap_err();
    assert!(format!("{error:#}").contains("Нужен exact old OID"));
    assert!(error.downcast_ref::<CliError>().is_none());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
}

#[tokio::test]
async fn push_and_readback_need_private_material_not_a_global_token() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let context = root.join("missing-context.json");
    let child = root.join("missing-child.token");
    let checkout = root.join("missing-checkout");
    for readback in [false, true] {
        let mut args = vec![
            "cicd",
            "repository",
            "push-candidate",
            "--context-file",
            context.to_str().unwrap(),
            "--child-token-file",
            child.to_str().unwrap(),
            "--checkout",
            checkout.to_str().unwrap(),
        ];
        if readback {
            args.push("--readback");
        }
        let mut cli = Cli::try_parse_from(args).unwrap();
        // No process-environment mutation: ignore any inherited test-host token.
        cli.token = None;
        assert!(cli.token.is_none());
        match &cli.command {
            Command::Work(commands::WorkCommand::Repository {
                command:
                    commands::RepositoryCommand::PushCandidate {
                        context_file,
                        child_token_file,
                        checkout: parsed_checkout,
                        readback: parsed_readback,
                    },
            }) => {
                assert_eq!(context_file, &context);
                assert_eq!(child_token_file, &child);
                assert_eq!(parsed_checkout, &checkout);
                assert_eq!(*parsed_readback, readback);
            }
            _ => panic!("push-candidate must remain in the repository group"),
        }
        let error = private_git(&cli.command, &cli.api_url)
            .await
            .unwrap()
            .unwrap_err();
        // Real unchanged task_git::run reaches the private-file reader, not
        // ApiClient's missing-global-token guard. No Git/HTTP operation occurs.
        assert!(format!("{error:#}").contains("Private Git input недоступен"));
        assert!(error.downcast_ref::<CliError>().is_none());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn unrelated_commands_do_not_gain_private_child_auth_bypass() {
    for args in [
        vec!["cicd", "project", "list"],
        vec!["cicd", "repository", "list"],
        vec!["cicd", "completions", "bash"],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(
            private_git(&cli.command, "not-a-service-url")
                .await
                .is_none()
        );
    }
}

#[test]
fn private_push_still_requires_context_child_and_checkout_for_readback_too() {
    for readback in [false, true] {
        for missing in ["--context-file", "--child-token-file", "--checkout"] {
            let mut args = vec!["cicd", "repository", "push-candidate"];
            for (flag, value) in [
                ("--context-file", "context.json"),
                ("--child-token-file", "child.token"),
                ("--checkout", "checkout"),
            ] {
                if flag != missing {
                    args.extend([flag, value]);
                }
            }
            if readback {
                args.push("--readback");
            }
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
}
