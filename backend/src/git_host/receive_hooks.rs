//! Native pre-receive policy; never decode or rewrite the Git wire protocol.
use super::*;
use std::{collections::BTreeSet, fs, io::Write};

const PRE_RECEIVE: &str = r#"#!/bin/sh
set -eu
umask 077
cat > "$FORGE_GUARD_UPDATES"
bootstrap_ref=''
if test -s "$FORGE_GUARD_BOOTSTRAP_REFS" && test "$(wc -l < "$FORGE_GUARD_UPDATES")" -eq 1; then
    read -r first_old first_new first_ref < "$FORGE_GUARD_UPDATES"
    case "$first_old" in ''|*[!0]*) ;; *)
        case "$first_new" in *[!0]*)
            if git show-ref --quiet; then :; else
                status=$?
                if test "$status" -ne 1; then exit "$status"; fi
                while IFS= read -r default_ref; do
                    if test "$first_ref" = "$default_ref"; then bootstrap_ref=$first_ref; fi
                done < "$FORGE_GUARD_BOOTSTRAP_REFS"
            fi
        ;; esac
    ;; esac
fi
while read -r old new ref; do
    while IFS= read -r protected; do
        if test "$ref" = "$protected" && test "$ref" != "$bootstrap_ref"; then
            printf '%s\n' 'Forge: direct update of default/protected branch denied; use a pull request.' >&2
            exit 1
        fi
    done < "$FORGE_GUARD_REFS"
done < "$FORGE_GUARD_UPDATES"
if test -n "$FORGE_ORIGINAL_PRE_RECEIVE"; then
    exec "$FORGE_ORIGINAL_PRE_RECEIVE" "$@" < "$FORGE_GUARD_UPDATES"
fi
"#;

pub(super) struct ReceiveHooks {
    directory: PathBuf,
}

fn quote(path: &Path) -> Result<String, ApiError> {
    let text = path
        .to_str()
        .ok_or_else(|| ApiError::service_unavailable("Git hook path is not UTF-8"))?;
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    options.open(path)?.write_all(bytes)
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

impl ReceiveHooks {
    pub(super) async fn prepare(
        pool: &PgPool,
        repository: &Path,
        allow_bootstrap: bool,
    ) -> Result<Self, ApiError> {
        let repository = repository.canonicalize().map_err(io_error)?;
        let storage = repository
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".git"))
            .ok_or_else(|| ApiError::service_unavailable("Git storage identity unavailable"))?;
        let policies = sqlx::query_as::<_, (String, Vec<String>)>(
            "SELECT p.default_branch,p.protected_branches FROM projects p \
             JOIN repository_catalog r ON r.id=p.repository_id WHERE r.storage_name=$1",
        )
        .bind(storage)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
        let mut refs = BTreeSet::new();
        let mut bootstrap_refs = BTreeSet::new();
        for (default, protected) in policies {
            if allow_bootstrap {
                bootstrap_refs.insert(format!("refs/heads/{default}"));
            }
            for branch in std::iter::once(default).chain(protected) {
                let reference = format!("refs/heads/{branch}");
                let checked = Command::new("git")
                    .args(["check-ref-format", &reference])
                    .output()
                    .await
                    .map_err(io_error)?;
                if !checked.status.success() {
                    return Err(ApiError::service_unavailable(
                        "Invalid protected Git branch policy",
                    ));
                }
                refs.insert(reference);
            }
        }
        let observed = Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["rev-parse", "--path-format=absolute", "--git-path", "hooks"])
            .output()
            .await
            .map_err(io_error)?;
        if !observed.status.success() {
            return Err(ApiError::service_unavailable(
                "Existing Git hooks unavailable",
            ));
        }
        let original = PathBuf::from(
            String::from_utf8(observed.stdout)
                .map_err(|_| ApiError::service_unavailable("Git hook path is not UTF-8"))?
                .trim_end_matches(['\r', '\n']),
        );
        if !original.is_absolute() {
            return Err(ApiError::service_unavailable(
                "Absolute Git hook path required",
            ));
        }
        Self::write(&repository, &original, &refs, &bootstrap_refs)
    }

    fn write(
        repository: &Path,
        original: &Path,
        refs: &BTreeSet<String>,
        bootstrap_refs: &BTreeSet<String>,
    ) -> Result<Self, ApiError> {
        let directory = repository.join(format!(".forge-receive-{}", Uuid::new_v4().simple()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory).map_err(io_error)?;
        let hooks = Self { directory };
        let result = hooks.write_files(original, refs, bootstrap_refs);
        if let Err(error) = result {
            hooks.cleanup();
            return Err(error);
        }
        Ok(hooks)
    }

    fn write_files(
        &self,
        original: &Path,
        refs: &BTreeSet<String>,
        bootstrap_refs: &BTreeSet<String>,
    ) -> Result<(), ApiError> {
        // Delegate all executable hooks, including custom core.hooksPath and update/post hooks.
        if original.is_dir() {
            for entry in fs::read_dir(original).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                if entry.file_name() == "pre-receive" || !executable(&entry.path()) {
                    continue;
                }
                let script = format!("#!/bin/sh\nexec {} \"$@\"\n", quote(&entry.path())?);
                write_private(&self.directory.join(entry.file_name()), script.as_bytes())
                    .map_err(io_error)?;
            }
        }
        let mut protected = String::new();
        for reference in refs {
            protected.push_str(reference);
            protected.push('\n');
        }
        write_private(&self.directory.join("protected-refs"), protected.as_bytes())
            .map_err(io_error)?;
        let bootstrap = bootstrap_refs
            .iter()
            .map(|reference| format!("{reference}\n"))
            .collect::<String>();
        write_private(&self.directory.join("bootstrap-refs"), bootstrap.as_bytes())
            .map_err(io_error)?;
        let original_pre = original.join("pre-receive");
        let delegate = if executable(&original_pre) {
            quote(&original_pre)?
        } else {
            "''".into()
        };
        let script = format!(
            "#!/bin/sh\nexport FORGE_GUARD_UPDATES={}\nexport FORGE_GUARD_REFS={}\nexport FORGE_GUARD_BOOTSTRAP_REFS={}\nexport FORGE_ORIGINAL_PRE_RECEIVE={}\n{}",
            quote(&self.directory.join("updates"))?,
            quote(&self.directory.join("protected-refs"))?,
            quote(&self.directory.join("bootstrap-refs"))?,
            delegate,
            PRE_RECEIVE.strip_prefix("#!/bin/sh\n").unwrap(),
        );
        write_private(&self.directory.join("pre-receive"), script.as_bytes()).map_err(io_error)
    }

    pub(super) fn config(&self) -> String {
        format!("core.hooksPath={}", self.directory.display())
    }

    // No Drop cleanup: an interrupted/unknown native writer must retain its hook files.
    pub(super) fn cleanup(self) {
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            tracing::warn!(?error, "terminal Git receive hook cleanup failed");
        }
    }
}

fn io_error(error: std::io::Error) -> ApiError {
    ApiError::internal(sqlx::Error::Io(error))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command as Git, Output};

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        bare: PathBuf,
        original: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("forge-hook-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let source = root.join("source");
            let bare = root.join("owner.git");
            fs::create_dir(&source).unwrap();
            let original = root.join("custom hooks' directory");
            fs::create_dir(&original).unwrap();
            let fixture = Self {
                root,
                source,
                bare,
                original,
            };
            assert!(
                fixture
                    .git(&["init", "--initial-branch=main"])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .git(&["config", "user.email", "fixture@example.invalid"])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .git(&["config", "user.name", "Fixture"])
                    .status
                    .success()
            );
            fs::write(fixture.source.join("data"), "first\n").unwrap();
            assert!(fixture.git(&["add", "data"]).status.success());
            assert!(fixture.git(&["commit", "-m", "first"]).status.success());
            assert!(
                fixture
                    .git(&["clone", "--bare", ".", fixture.bare.to_str().unwrap()])
                    .status
                    .success()
            );
            fs::write(fixture.source.join("data"), "second\n").unwrap();
            assert!(fixture.git(&["commit", "-am", "second"]).status.success());
            fixture
        }

        fn git(&self, args: &[&str]) -> Output {
            Git::new("git")
                .current_dir(&self.source)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args(args)
                .output()
                .unwrap()
        }

        fn install(&self) -> ReceiveHooks {
            self.install_bootstrap(false)
        }

        fn install_bootstrap(&self, allowed: bool) -> ReceiveHooks {
            let hooks = ReceiveHooks::write(
                &self.bare,
                &self.original,
                &BTreeSet::from(["refs/heads/main".into(), "refs/heads/release".into()]),
                &if allowed {
                    BTreeSet::from(["refs/heads/main".into()])
                } else {
                    BTreeSet::new()
                },
            )
            .unwrap();
            assert!(
                self.git(&[
                    "--git-dir",
                    self.bare.to_str().unwrap(),
                    "config",
                    "core.hooksPath",
                    hooks.directory.to_str().unwrap()
                ])
                .status
                .success()
            );
            hooks
        }

        fn push(&self, refs: &[&str]) -> Output {
            let mut args = vec!["push", self.bare.to_str().unwrap()];
            args.extend_from_slice(refs);
            self.git(&args)
        }

        fn reference(&self, reference: &str) -> Option<Vec<u8>> {
            let result = self.git(&[
                "--git-dir",
                self.bare.to_str().unwrap(),
                "rev-parse",
                "--verify",
                reference,
            ]);
            result.status.success().then_some(result.stdout)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn native_pre_receive_blocks_entire_push_before_default_or_protected_ref_changes() {
        let f = Fixture::new();
        let original = f.reference("refs/heads/main").unwrap();
        let hooks = f.install();
        for refs in [
            vec!["HEAD:refs/heads/main", "HEAD:refs/heads/task/denied"],
            vec![":refs/heads/main"],
            vec!["HEAD:refs/heads/release"],
        ] {
            assert!(!f.push(&refs).status.success());
            assert_eq!(f.reference("refs/heads/main").as_ref(), Some(&original));
            assert!(f.reference("refs/heads/task/denied").is_none());
            assert!(f.reference("refs/heads/release").is_none());
        }
        assert!(f.push(&["HEAD:refs/heads/task/accepted"]).status.success());
        assert!(f.reference("refs/heads/task/accepted").is_some());
        hooks.cleanup();
    }

    #[test]
    fn native_receive_delegates_custom_hooks_with_original_stdin_and_update_arguments() {
        let f = Fixture::new();
        for hook in ["pre-receive", "post-receive"] {
            let script = format!(
                "#!/bin/sh\ncat > {}\n",
                quote(&f.root.join(format!("{hook}.log"))).unwrap()
            );
            write_private(&f.original.join(hook), script.as_bytes()).unwrap();
        }
        let update = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n",
            quote(&f.root.join("update.log")).unwrap()
        );
        write_private(&f.original.join("update"), update.as_bytes()).unwrap();
        let before = fs::read(f.original.join("pre-receive")).unwrap();
        let hooks = f.install();
        assert!(f.push(&["HEAD:refs/heads/task/custom"]).status.success());
        let pre = fs::read(f.root.join("pre-receive.log")).unwrap();
        assert_eq!(pre, fs::read(f.root.join("post-receive.log")).unwrap());
        assert!(
            String::from_utf8(pre)
                .unwrap()
                .contains("refs/heads/task/custom")
        );
        assert!(
            fs::read_to_string(f.root.join("update.log"))
                .unwrap()
                .starts_with("refs/heads/task/custom\n")
        );
        assert_eq!(before, fs::read(f.original.join("pre-receive")).unwrap());
        hooks.cleanup();
    }

    #[test]
    fn custom_pre_receive_rejection_still_denies_unprotected_task_push() {
        let f = Fixture::new();
        write_private(
            &f.original.join("pre-receive"),
            b"#!/bin/sh\ncat >/dev/null\nexit 1\n",
        )
        .unwrap();
        let hooks = f.install();
        assert!(
            !f.push(&["HEAD:refs/heads/task/custom-denied"])
                .status
                .success()
        );
        assert!(f.reference("refs/heads/task/custom-denied").is_none());
        hooks.cleanup();
    }

    #[test]
    fn authorized_bootstrap_only_creates_one_default_ref_in_an_empty_repository() {
        let f = Fixture::new();
        assert!(
            f.git(&[
                "--git-dir",
                f.bare.to_str().unwrap(),
                "update-ref",
                "-d",
                "refs/heads/main"
            ])
            .status
            .success()
        );
        let restricted = f.install();
        assert!(!f.push(&["HEAD:refs/heads/main"]).status.success());
        assert!(f.reference("refs/heads/main").is_none());
        restricted.cleanup();
        let hooks = f.install_bootstrap(true);
        assert!(
            !f.push(&["HEAD:refs/heads/main", "HEAD:refs/heads/task/mixed"])
                .status
                .success()
        );
        assert!(f.reference("refs/heads/task/mixed").is_none());
        assert!(!f.push(&["HEAD:refs/heads/release"]).status.success());
        assert!(f.push(&["HEAD:refs/heads/main"]).status.success());
        let initial = f.reference("refs/heads/main").unwrap();
        fs::write(f.source.join("data"), "third\n").unwrap();
        assert!(f.git(&["commit", "-am", "third"]).status.success());
        assert!(!f.push(&["HEAD:refs/heads/main"]).status.success());
        assert!(!f.push(&[":refs/heads/main"]).status.success());
        assert_eq!(f.reference("refs/heads/main").as_ref(), Some(&initial));
        hooks.cleanup();
    }

    #[test]
    fn bootstrap_cannot_create_default_after_any_other_ref_already_exists() {
        let f = Fixture::new();
        assert!(
            f.git(&[
                "--git-dir",
                f.bare.to_str().unwrap(),
                "update-ref",
                "-d",
                "refs/heads/main"
            ])
            .status
            .success()
        );
        let hooks = f.install_bootstrap(true);
        assert!(f.push(&["HEAD:refs/heads/task/existing"]).status.success());
        assert!(!f.push(&["HEAD:refs/heads/main"]).status.success());
        assert!(f.reference("refs/heads/main").is_none());
        hooks.cleanup();
    }
}
