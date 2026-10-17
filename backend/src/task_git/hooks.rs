//! Per-request private hook files, held by the existing detached native owner.
use super::{Authorization, CandidateCheck};
use crate::api::ApiError;
use std::{fs, io::Write, path::Path};

fn error() -> ApiError {
    ApiError::service_unavailable("Private task Git hook unavailable")
}

fn quote(path: &Path) -> Result<String, ApiError> {
    Ok(format!(
        "'{}'",
        path.to_str().ok_or_else(error)?.replace('\'', "'\\''")
    ))
}

fn write(path: &Path, data: &[u8], executable: bool) -> Result<(), ApiError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o700 } else { 0o600 });
    }
    #[cfg(not(unix))]
    let _ = executable;
    options
        .open(path)
        .and_then(|mut f| {
            f.write_all(data)?;
            f.sync_all()
        })
        .map_err(|_| error())
}

// Includes exact-line checking again at reference-transaction/prepared, after custom hooks.
const CHECK: &str = r#"set -eu
umask 077
export GIT_NO_REPLACE_OBJECTS=1 GIT_NO_LAZY_FETCH=1 GIT_OPTIONAL_LOCKS=0
cat > "$dir/task-updates"
cmp -s "$dir/task-updates" "$dir/expected-update" || exit 1
read -r old new ref < "$dir/expected-update"
default=$(git symbolic-ref --quiet HEAD) || exit 1
test "$default" != "$ref" || exit 1
test "$(git cat-file -t "$new")" = commit || exit 1
base=$(cat "$dir/base")
git merge-base --is-ancestor "$base" "$new" || exit 1
if test "$old" = 0000000000000000000000000000000000000000; then
    if git show-ref --verify --quiet "$ref"; then exit 1; else test "$?" -eq 1 || exit 1; fi
else
    test "$(git rev-parse --verify "$ref")" = "$old" || exit 1
    git merge-base --is-ancestor "$old" "$new" || exit 1
fi
curl --disable --silent --fail --max-time 3 --max-redirs 0 --noproxy '*' --proto '=http' \
    --header @"$dir/child-headers" --header 'content-type: application/json' \
    --data-binary @"$dir/context.json" "$callback" >/dev/null 2>&1 || exit 1
"#;

pub(crate) fn install(
    directory: &Path,
    authorization: &Authorization,
    internal: &str,
    port: u16,
) -> Result<(), ApiError> {
    if internal.is_empty()
        || !internal.bytes().all(|b| b.is_ascii_graphic())
        || !authorization.token.bytes().all(|b| b.is_ascii_graphic())
        || port == 0
    {
        return Err(error());
    }
    let context = &authorization.context;
    context.validate()?;
    let saved_pre = directory.join("protected-pre-receive");
    fs::rename(directory.join("pre-receive"), &saved_pre).map_err(|_| error())?;
    let original_transaction = directory.join("reference-transaction");
    let saved_transaction = directory.join("original-reference-transaction");
    let has_transaction = original_transaction.exists();
    if has_transaction {
        fs::rename(&original_transaction, &saved_transaction).map_err(|_| error())?;
    }
    // Never invoke a legacy pipeline notification (or custom post hook) for this variant.
    let post = directory.join("post-receive");
    if post.exists() {
        fs::remove_file(&post).map_err(|_| error())?;
    }
    write(&post, b"#!/bin/sh\ncat >/dev/null\nexit 0\n", true)?;
    write(
        &directory.join("expected-update"),
        format!(
            "{} {} {}\n",
            context.old_oid, context.new_oid, context.source.candidate_ref
        )
        .as_bytes(),
        false,
    )?;
    write(
        &directory.join("base"),
        context.source.base_commit.as_bytes(),
        false,
    )?;
    let bytes = serde_json::to_vec(&CandidateCheck {
        task_candidate: context.clone(),
    })
    .map_err(|_| error())?;
    write(&directory.join("context.json"), &bytes, false)?;
    write(
        &directory.join("child-headers"),
        format!(
            "Authorization: Bearer {}\nx-internal-token: {}\n",
            authorization.token, internal
        )
        .as_bytes(),
        false,
    )?;
    let pre = format!(
        "#!/bin/sh\nset -eu\numask 077\ndir={}\ncallback='http://127.0.0.1:{port}/api/v1/internal/git-push'\ncat > \"$dir/input\"\n{} < \"$dir/input\"\n{} < \"$dir/input\"\n",
        quote(directory)?,
        quote(&saved_pre)?,
        quote(&directory.join("task-check"))?
    );
    write(&directory.join("pre-receive"), pre.as_bytes(), true)?;
    let check = format!(
        "#!/bin/sh\ndir={}\ncallback='http://127.0.0.1:{port}/api/v1/internal/git-push'\n{CHECK}",
        quote(directory)?
    );
    write(&directory.join("task-check"), check.as_bytes(), true)?;
    let delegate = if has_transaction {
        format!(
            "{} \"$@\" < \"$dir/transaction-input\"\n",
            quote(&saved_transaction)?
        )
    } else {
        String::new()
    };
    let transaction = format!(
        "#!/bin/sh\nset -eu\numask 077\ndir={}\ncat > \"$dir/transaction-input\"\n{delegate}if test \"$1\" = prepared; then\n    {} < \"$dir/transaction-input\"\nfi\n",
        quote(directory)?,
        quote(&directory.join("task-check"))?
    );
    write(&original_transaction, transaction.as_bytes(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_uses_private_header_file_and_no_pipeline_notification() {
        assert!(CHECK.contains("--header @\"$dir/child-headers\""));
        assert!(!CHECK.contains("Bearer"));
        assert!(!CHECK.contains("export token"));
        assert!(CHECK.contains("cmp -s"));
        assert!(CHECK.contains("merge-base --is-ancestor"));
        assert!(CHECK.contains("symbolic-ref --quiet HEAD"));
    }

    #[cfg(unix)]
    mod native {
        use super::*;
        use std::{
            path::PathBuf,
            process::{Command, Output},
        };
        use uuid::Uuid;

        struct Fixture {
            root: PathBuf,
            source: PathBuf,
            bare: PathBuf,
            hooks: PathBuf,
            authorization: Authorization,
        }
        impl Fixture {
            fn git(&self, args: &[&str]) -> Output {
                Command::new("git")
                    .current_dir(&self.source)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env(
                        "PATH",
                        format!(
                            "{}:{}",
                            self.root.join("bin").display(),
                            std::env::var("PATH").unwrap()
                        ),
                    )
                    .args(args)
                    .output()
                    .unwrap()
            }
            fn new() -> Self {
                Self::with_seeded_candidate(false)
            }
            fn with_seeded_candidate(seed_candidate: bool) -> Self {
                let root = std::env::temp_dir().join(format!("forge-task-hook-{}", Uuid::new_v4()));
                let source = root.join("source");
                let bare = root.join("repo.git");
                let hooks = root.join("private-hooks");
                for dir in [&source, &hooks, &root.join("bin")] {
                    fs::create_dir_all(dir).unwrap();
                }
                let (context, _, _, _) = crate::task_git::tests::fixture();
                let mut f = Self {
                    root,
                    source,
                    bare,
                    hooks,
                    authorization: Authorization {
                        context,
                        token: "sdlc_pat_fixture_child".into(),
                    },
                };
                for args in [
                    &["init", "--initial-branch=main"][..],
                    &["config", "user.email", "fixture@example.invalid"],
                    &["config", "user.name", "Fixture"],
                ] {
                    assert!(f.git(args).status.success());
                }
                fs::write(f.source.join("data"), "first").unwrap();
                assert!(f.git(&["add", "data"]).status.success());
                assert!(f.git(&["commit", "-m", "first"]).status.success());
                f.authorization.context.source.base_commit =
                    String::from_utf8(f.git(&["rev-parse", "HEAD"]).stdout)
                        .unwrap()
                        .trim()
                        .into();
                f.authorization.context.intent.source_commit =
                    f.authorization.context.source.base_commit.clone();
                assert!(
                    f.git(&["clone", "--bare", ".", f.bare.to_str().unwrap()])
                        .status
                        .success()
                );
                fs::write(f.source.join("data"), "second").unwrap();
                assert!(f.git(&["commit", "-am", "second"]).status.success());
                f.authorization.context.new_oid =
                    String::from_utf8(f.git(&["rev-parse", "HEAD"]).stdout)
                        .unwrap()
                        .trim()
                        .into();
                crate::task_git::tests::seal(&mut f.authorization.context.source);
                write(
                    &f.hooks.join("pre-receive"),
                    b"#!/bin/sh\ncat >/dev/null\n",
                    true,
                )
                .unwrap();
                write(
                    &f.hooks.join("post-receive"),
                    format!(
                        "#!/bin/sh\ntouch {}\n",
                        quote(&f.root.join("ordinary-pipeline")).unwrap()
                    )
                    .as_bytes(),
                    true,
                )
                .unwrap();
                let curl = format!(
                    "#!/bin/sh\ntest ! -e {} || exit 1\nprintf 'checked\\n' >> {}\n",
                    quote(&f.root.join("revoked")).unwrap(),
                    quote(&f.root.join("checks")).unwrap()
                );
                write(&f.root.join("bin/curl"), curl.as_bytes(), true).unwrap();
                if seed_candidate {
                    let old_main = f.reference("refs/heads/main").unwrap();
                    assert!(
                        f.git(&[
                            "--git-dir",
                            f.bare.to_str().unwrap(),
                            "update-ref",
                            &f.authorization.context.source.candidate_ref,
                            &old_main
                        ])
                        .status
                        .success()
                    );
                }
                install(&f.hooks, &f.authorization, "fixture-internal", 22801).unwrap();
                assert!(
                    f.git(&[
                        "--git-dir",
                        f.bare.to_str().unwrap(),
                        "config",
                        "core.hooksPath",
                        f.hooks.to_str().unwrap()
                    ])
                    .status
                    .success()
                );
                f
            }
            fn push(&self, extra: &[&str]) -> Output {
                let mut args = vec!["push", self.bare.to_str().unwrap()];
                args.extend_from_slice(extra);
                self.git(&args)
            }
            fn reference(&self, reference: &str) -> Option<String> {
                let output = self.git(&[
                    "--git-dir",
                    self.bare.to_str().unwrap(),
                    "rev-parse",
                    "--verify",
                    reference,
                ]);
                output
                    .status
                    .success()
                    .then(|| String::from_utf8(output.stdout).unwrap().trim().into())
            }
            fn candidate(&self) -> String {
                format!(
                    "{}:{}",
                    self.authorization.context.new_oid,
                    self.authorization.context.source.candidate_ref
                )
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        #[test]
        fn native_exact_candidate_rechecks_prepared_and_does_not_enqueue_ordinary_pipeline() {
            let f = Fixture::new();
            assert!(f.push(&[&f.candidate()]).status.success());
            assert_eq!(
                f.reference(&f.authorization.context.source.candidate_ref)
                    .as_deref(),
                Some(f.authorization.context.new_oid.as_str())
            );
            assert_eq!(
                fs::read_to_string(f.root.join("checks"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
            assert!(!f.root.join("ordinary-pipeline").exists());
            let headers = fs::read_to_string(f.hooks.join("child-headers")).unwrap();
            for script in [
                "pre-receive",
                "task-check",
                "reference-transaction",
                "post-receive",
            ] {
                let text = fs::read_to_string(f.hooks.join(script)).unwrap();
                assert!(!text.contains(&f.authorization.token));
                assert!(!text.contains(&headers));
            }
        }
        #[test]
        fn native_foreign_multiref_delete_and_wrong_cas_cannot_change_refs() {
            let f = Fixture::new();
            let candidate = f.candidate();
            let old_main = f.reference("refs/heads/main").unwrap();
            for refs in [
                vec!["HEAD:refs/heads/main"],
                vec![&candidate, "HEAD:refs/heads/foreign"],
                vec![":refs/heads/main"],
            ] {
                assert!(!f.push(&refs).status.success());
                assert!(
                    f.reference(&f.authorization.context.source.candidate_ref)
                        .is_none()
                );
                assert_eq!(
                    f.reference("refs/heads/main").as_deref(),
                    Some(old_main.as_str())
                );
            }
            let f = Fixture::with_seeded_candidate(true);
            let candidate = f.candidate();
            let old_main = f.reference("refs/heads/main").unwrap();
            assert!(!f.push(&[&candidate]).status.success());
            assert_eq!(
                f.reference(&f.authorization.context.source.candidate_ref)
                    .as_deref(),
                Some(old_main.as_str())
            );
        }
        #[test]
        fn native_fresh_check_failure_and_original_pre_receive_denial_are_closed() {
            let f = Fixture::new();
            fs::write(f.root.join("revoked"), "held").unwrap();
            assert!(!f.push(&[&f.candidate()]).status.success());
            assert!(
                f.reference(&f.authorization.context.source.candidate_ref)
                    .is_none()
            );
            fs::remove_file(f.root.join("revoked")).unwrap();
            fs::write(f.hooks.join("protected-pre-receive"), "#!/bin/sh\nexit 1\n").unwrap();
            assert!(!f.push(&[&f.candidate()]).status.success());
        }
        #[test]
        fn native_non_fast_forward_is_denied_even_with_exact_old_cas() {
            let mut f = Fixture::new();
            assert!(f.push(&[&f.candidate()]).status.success());
            let base = f.authorization.context.source.base_commit.clone();
            let old = f.authorization.context.new_oid.clone();
            assert!(f.git(&["checkout", "--detach", &base]).status.success());
            fs::write(f.source.join("data"), "diverged").unwrap();
            assert!(f.git(&["commit", "-am", "diverged"]).status.success());
            let new = String::from_utf8(f.git(&["rev-parse", "HEAD"]).stdout)
                .unwrap()
                .trim()
                .to_owned();
            f.authorization.context.old_oid = old.clone();
            f.authorization.context.new_oid = new;
            fs::write(
                f.hooks.join("expected-update"),
                format!(
                    "{} {} {}\n",
                    old,
                    f.authorization.context.new_oid,
                    f.authorization.context.source.candidate_ref
                ),
            )
            .unwrap();
            let candidate = f.candidate();
            assert!(!f.push(&["--force", &candidate]).status.success());
            assert_eq!(
                f.reference(&f.authorization.context.source.candidate_ref)
                    .as_deref(),
                Some(old.as_str())
            );
        }
        #[test]
        fn native_revocation_after_pre_receive_is_checked_again_before_commit() {
            let f = Fixture::new();
            let curl = format!(
                "#!/bin/sh\ntest ! -e {} || exit 1\ntouch {}\n",
                quote(&f.root.join("revoked")).unwrap(),
                quote(&f.root.join("revoked")).unwrap()
            );
            fs::write(f.root.join("bin/curl"), curl).unwrap();
            assert!(!f.push(&[&f.candidate()]).status.success());
            assert!(
                f.reference(&f.authorization.context.source.candidate_ref)
                    .is_none()
            );
        }
    }
}
