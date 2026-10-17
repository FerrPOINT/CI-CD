//! Bounded Git subprocess adapter. Private tokens travel through an inherited FD.
use cicd_app::repository_sync::Error;
use cicd_domain::repository_sync::{CheckResult, Provider, Settings};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use uuid::Uuid;

/// The file is never unlinked: all API/worker processes lock the same inode.
pub struct MutationGuard {
    #[cfg(unix)]
    _file: std::fs::File,
}
impl MutationGuard {
    #[cfg(unix)]
    pub async fn acquire(root: &Path, repository: Uuid) -> Result<Self, Error> {
        use std::os::{
            fd::AsRawFd,
            unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let directory = root.join(".forge-mutation-locks");
        std::fs::create_dir_all(&directory)
            .map_err(|_| Error::Unavailable("Git lock storage недоступен"))?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Unavailable("Git lock permissions"))?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(directory.join(format!("{repository}.lock")))
            .map_err(|_| Error::Unavailable("Git lock недоступен"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            // SAFETY: the descriptor is owned by file for the entire guard lifetime.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
                return Err(Error::Unavailable(
                    "Хранилище не поддерживает Git mutation lock",
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Conflict("git_repository_busy"));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok(Self { _file: file })
    }
    #[cfg(not(unix))]
    pub async fn acquire(_root: &Path, _repository: Uuid) -> Result<Self, Error> {
        Err(Error::Unavailable(
            "Git mutation worker требует Unix shared storage",
        ))
    }
    /// Duplicate only into this child. Other concurrently spawned jobs must not inherit locks.
    pub fn inherit(&self, command: &mut Command) -> Result<(), Error> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            inherit_descriptor(command, self._file.as_raw_fd(), 8)?;
        }
        #[cfg(not(unix))]
        {
            let _ = command;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn inherit_descriptor(command: &mut Command, source: i32, target: i32) -> Result<(), Error> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // Reserve a parent CLOEXEC descriptor outside POSIX shell FDs before any child dup2.
    // This also prevents two pre_exec callbacks from overwriting each other's source.
    let duplicate = unsafe { libc::fcntl(source, libc::F_DUPFD_CLOEXEC, 200) };
    if duplicate < 0 {
        return Err(Error::Unavailable("Git descriptor duplication failed"));
    }
    let descriptor = unsafe { OwnedFd::from_raw_fd(duplicate) };
    // SAFETY: the closure owns the source; dup2 is async-signal-safe and clears CLOEXEC.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(descriptor.as_raw_fd(), target) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

enum Endpoint {
    #[allow(dead_code)]
    Local(PathBuf),
    External(Settings),
}
impl Endpoint {
    fn value(&self) -> String {
        match self {
            Self::Local(path) => path.to_string_lossy().into_owned(),
            Self::External(settings) => settings.url(),
        }
    }
}
pub struct GitSession {
    pub scratch: PathBuf,
    local: PathBuf,
    remote: Endpoint,
    token: String,
    pub canceled: Arc<AtomicBool>,
    pub guard: Option<Arc<MutationGuard>>,
}
impl Drop for GitSession {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}
impl GitSession {
    /// Call only after acquiring the repository guard: an old child may still own scratch.
    pub fn clean_recovered_scratch(root: &Path, run: Uuid) -> Result<(), Error> {
        let directory = root.join(".forge-sync-scratch");
        if !directory.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(directory)
            .map_err(|_| Error::Unavailable("Scratch cleanup недоступен"))?
        {
            let entry = entry.map_err(|_| Error::Unavailable("Scratch cleanup entry"))?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{run}-"))
                && entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            {
                std::fs::remove_dir_all(entry.path())
                    .map_err(|_| Error::Unavailable("Scratch cleanup failed"))?;
            }
        }
        Ok(())
    }
    pub async fn new(
        root: &Path,
        local: PathBuf,
        settings: Settings,
        token: String,
        run: Uuid,
        generation: i64,
        canceled: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let directory = root.join(".forge-sync-scratch");
        std::fs::create_dir_all(&directory)
            .map_err(|_| Error::Unavailable("Scratch storage недоступен"))?;
        let scratch = directory.join(format!("{run}-{generation}-{}", Uuid::new_v4()));
        std::fs::create_dir(&scratch).map_err(|_| Error::Unavailable("Scratch не создан"))?;
        let session = Self {
            scratch,
            local,
            remote: Endpoint::External(settings),
            token,
            canceled,
            guard: None,
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&session.scratch, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::Unavailable("Scratch permissions"))?;
        }
        session
            .command(
                false,
                &[],
                Command::new("git")
                    .args(["init", "--bare"])
                    .arg(&session.scratch),
            )
            .await?;
        Ok(session)
    }
    fn configure(&self, remote: bool, args: &[String]) -> Command {
        let mut command = Command::new("git");
        command.arg(format!("--git-dir={}", self.scratch.display()));
        command.args([
            "-c",
            "credential.helper=",
            "-c",
            "http.followRedirects=false",
            "-c",
            "http.sslVerify=true",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.allow=never",
        ]);
        command.args([
            "-c",
            if remote && matches!(self.remote, Endpoint::External(_)) {
                "protocol.https.allow=always"
            } else {
                "protocol.file.allow=always"
            },
        ]);
        if remote && matches!(self.remote, Endpoint::External(_)) {
            command.args(["-c","credential.helper=!f() { if test \"$1\" = get; then eval \"cat <&$FORGE_SYNC_CREDENTIAL_FD\"; fi; }; f"]);
        }
        command.args(args);
        command
    }
    async fn command(
        &self,
        remote: bool,
        _args: &[String],
        command: &mut Command,
    ) -> Result<Vec<u8>, Error> {
        // Do not leak server/QA credentials or inherited Git trace/config into Git children.
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(unix) { "/dev/null" } else { "NUL" },
            )
            .env("GIT_TERMINAL_PROMPT", "0");
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        #[cfg(unix)]
        let _credential_channel = if remote && matches!(self.remote, Endpoint::External(_)) {
            use std::{io::Write, os::fd::AsRawFd, os::unix::net::UnixStream};
            let (reader, mut writer) = UnixStream::pair()
                .map_err(|_| Error::Unavailable("Credential channel недоступен"))?;
            // FD 9 is accepted by /bin/sh implementations that reject multi-digit FDs.
            inherit_descriptor(command, reader.as_raw_fd(), 9)?;
            let username = match &self.remote {
                Endpoint::External(settings) if settings.provider == Provider::Github => {
                    "x-access-token"
                }
                _ => "oauth2",
            };
            writer
                .write_all(format!("username={username}\npassword={}\n\n", self.token).as_bytes())
                .map_err(|_| Error::Unavailable("Credential channel write"))?;
            writer
                .shutdown(std::net::Shutdown::Write)
                .map_err(|_| Error::Unavailable("Credential channel shutdown"))?;
            command.env("FORGE_SYNC_CREDENTIAL_FD", "9");
            // The command text and environment contain only the FD number, never the token.
            Some(reader)
        } else {
            None
        };
        #[cfg(not(unix))]
        if remote && matches!(self.remote, Endpoint::External(_)) {
            return Err(Error::Unavailable(
                "Защищённый credential transport требует Unix",
            ));
        }
        if self.canceled.load(Ordering::SeqCst) {
            return Err(Error::Conflict("sync_canceled"));
        }
        if let Some(guard) = &self.guard {
            guard.inherit(command)?;
        }
        let mut child = command
            .spawn()
            .map_err(|_| Error::Unavailable("Git executable недоступен"))?;
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or(Error::Unavailable("Git stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(Error::Unavailable("Git stderr"))?;
        let out = tokio::spawn(async move {
            let mut data = Vec::new();
            stdout
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut data)
                .await
                .map(|_| data)
        });
        let err = tokio::spawn(async move {
            let mut data = Vec::new();
            stderr
                .take(1024 * 1024 + 1)
                .read_to_end(&mut data)
                .await
                .map(|_| data)
        });
        let stop = async {
            loop {
                if self.canceled.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        let status = tokio::select! {
            value=child.wait()=>Some(value),
            _=tokio::time::sleep(Duration::from_secs(300))=>None,
            _=stop=>None,
        };
        if status.is_none() {
            #[cfg(unix)]
            if let Some(pid) = pid {
                // SAFETY: only the process group created above is targeted.
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let output = out
            .await
            .map_err(|_| Error::Unavailable("Git output task"))?
            .map_err(|_| Error::Unavailable("Git output read"))?;
        let diagnostic = err
            .await
            .map_err(|_| Error::Unavailable("Git error task"))?
            .map_err(|_| Error::Unavailable("Git error read"))?;
        let Some(status) = status else {
            return Err(Error::Unavailable("git_outcome_unknown"));
        };
        if output.len() > 16 * 1024 * 1024 || diagnostic.len() > 1024 * 1024 {
            return Err(Error::Unavailable("Git output limit exceeded"));
        }
        if !status
            .map_err(|_| Error::Unavailable("Git wait failed"))?
            .success()
        {
            // Deliberately do not return provider stderr: it can include credentials or URLs.
            let diagnostic = String::from_utf8_lossy(&diagnostic).to_ascii_lowercase();
            let reason = if diagnostic.contains("bad file descriptor")
                || diagnostic.contains("bad fd number")
            {
                "git_credential_channel_failed"
            } else if diagnostic.contains("protected branch")
                || diagnostic.contains("protected tag")
                || diagnostic.contains("gh006")
                || diagnostic.contains("gh013")
            {
                "git_protected_branch"
            } else if diagnostic.contains("authentication failed")
                || diagnostic.contains("could not read username")
                || diagnostic.contains("could not read password")
                || diagnostic.contains("403")
                || diagnostic.contains("repository not found")
            {
                "git_access_denied"
            } else if diagnostic.contains("certificate") || diagnostic.contains("ssl") {
                "git_tls_failed"
            } else if diagnostic.contains("resolve host")
                || diagnostic.contains("connect to")
                || diagnostic.contains("connection")
            {
                "git_network_failed"
            } else if diagnostic.contains("redirect") {
                "git_redirect_rejected"
            } else if diagnostic.contains("not allowed") {
                "git_protocol_rejected"
            } else {
                "git_operation_failed"
            };
            return Err(Error::Unavailable(reason));
        }
        Ok(output)
    }
    async fn git(&self, remote: bool, args: Vec<String>) -> Result<Vec<u8>, Error> {
        self.command(remote, &args, &mut self.configure(remote, &args))
            .await
    }
    pub async fn refs(&self, remote: bool) -> Result<BTreeMap<String, String>, Error> {
        let destination = if remote {
            self.remote.value()
        } else {
            self.local.to_string_lossy().into_owned()
        };
        let raw = self
            .git(
                remote,
                vec![
                    "ls-remote".into(),
                    "--heads".into(),
                    "--tags".into(),
                    destination,
                ],
            )
            .await?;
        let text =
            String::from_utf8(raw).map_err(|_| Error::Unavailable("Некорректные Git refs"))?;
        let mut refs = BTreeMap::new();
        for line in text.lines() {
            let Some((sha, name)) = line.split_once('\t') else {
                return Err(Error::Unavailable("Некорректные Git refs"));
            };
            if name.ends_with("^{}") {
                continue;
            }
            if !(name.starts_with("refs/heads/") || name.starts_with("refs/tags/"))
                || !matches!(sha.len(), 40 | 64)
                || !sha.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::Unavailable("Некорректные Git refs"));
            }
            refs.insert(name.into(), sha.into());
        }
        Ok(refs)
    }
    pub async fn fetch(&self) -> Result<(), Error> {
        for remote in [false, true] {
            let prefix = if remote { "remote" } else { "local" };
            let destination = if remote {
                self.remote.value()
            } else {
                self.local.to_string_lossy().into_owned()
            };
            self.git(
                remote,
                vec![
                    "fetch".into(),
                    "--no-tags".into(),
                    "--no-write-fetch-head".into(),
                    destination,
                    format!("refs/heads/*:refs/{prefix}/heads/*"),
                    format!("refs/tags/*:refs/{prefix}/tags/*"),
                ],
            )
            .await?;
        }
        Ok(())
    }
    pub async fn ancestor(&self, old: &str, new: &str) -> bool {
        self.git(
            false,
            vec![
                "merge-base".into(),
                "--is-ancestor".into(),
                old.into(),
                new.into(),
            ],
        )
        .await
        .is_ok()
    }
    pub async fn apply(
        &self,
        remote: bool,
        reference: &str,
        old: Option<&str>,
        new: &str,
    ) -> Result<(), Error> {
        let actual = self.refs(remote).await?;
        if actual.get(reference).map(String::as_str) != old {
            return Err(Error::Conflict("destination_changed"));
        }
        if let Some(old) = old {
            if reference.starts_with("refs/tags/") || !self.ancestor(old, new).await {
                return Err(Error::Conflict("non_fast_forward"));
            }
        }
        let destination = if remote {
            self.remote.value()
        } else {
            self.local.to_string_lossy().into_owned()
        };
        // Explicit expected OID protects concurrent creation/updates. Ancestry above is mandatory.
        self.git(
            remote,
            vec![
                "push".into(),
                "--porcelain".into(),
                format!("--force-with-lease={reference}:{}", old.unwrap_or("")),
                destination,
                format!("{new}:{reference}"),
            ],
        )
        .await?;
        Ok(())
    }
    pub async fn check(&self, write: bool) -> Result<CheckResult, Error> {
        self.refs(true).await?;
        if !write {
            return Ok(CheckResult {
                read: true,
                write: "not_requested".into(),
                message: "Чтение доступно".into(),
            });
        }
        self.fetch().await?;
        let result = self
            .git(
                true,
                vec![
                    "push".into(),
                    "--dry-run".into(),
                    "--porcelain".into(),
                    self.remote.value(),
                    "refs/local/heads/*:refs/heads/*".into(),
                ],
            )
            .await;
        Ok(CheckResult{read:true,write:if result.is_ok(){"preliminary"}else{"unconfirmed"}.into(),message:if result.is_ok(){"Чтение доступно; предварительная проверка записи пройдена. Защита веток проверяется при push."}else{"Чтение доступно; возможность записи не подтверждена. Проверьте права и правила веток."}.into()})
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    async fn git(path: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .status()
                .await
                .unwrap()
                .success()
        );
    }
    #[tokio::test]
    async fn child_retains_mutation_lock_after_supervisor_drop() {
        let root = std::env::temp_dir().join(format!("forge-lock-test-{}", Uuid::new_v4()));
        let repository = Uuid::new_v4();
        let guard = MutationGuard::acquire(&root, repository).await.unwrap();
        let mut command = Command::new("git");
        command.args(["-c", "alias.custody=!sleep 1", "custody"]);
        guard.inherit(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        drop(command);
        drop(guard);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                MutationGuard::acquire(&root, repository)
            )
            .await
            .is_err()
        );
        assert!(child.wait().await.unwrap().success());
        drop(MutationGuard::acquire(&root, repository).await.unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn orphan_git_helper_retains_guard_after_git_parent_is_killed() {
        let root = std::env::temp_dir().join(format!("forge-orphan-lock-{}", Uuid::new_v4()));
        let repository = Uuid::new_v4();
        let guard = MutationGuard::acquire(&root, repository).await.unwrap();
        let ready = root.join("ready");
        let mut command = Command::new("git");
        command.env("CUSTODY_READY", &ready).args([
            "-c",
            "alias.custody=!f() { printf ready >\"$CUSTODY_READY\"; sleep 1; }; f",
            "custody",
        ]);
        guard.inherit(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        drop(command);
        drop(guard);
        tokio::time::timeout(Duration::from_secs(3), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        child.kill().await.unwrap();
        let _ = child.wait().await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                MutationGuard::acquire(&root, repository)
            )
            .await
            .is_err()
        );
        drop(MutationGuard::acquire(&root, repository).await.unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    #[ignore = "Requires an explicitly disposable private repository and token via protected stdin"]
    async fn live_provider_https_push_pull_and_annotated_tags() {
        let repository = std::env::var("CICD_SYNC_LIVE_REPOSITORY")
            .or_else(|_| std::env::var("CICD_SYNC_GITHUB_REPOSITORY"))
            .expect("disposable repository");
        assert!(
            repository
                .rsplit('/')
                .next()
                .unwrap()
                .starts_with("forge-sync-qa-"),
            "disposable repository only"
        );
        let token = std::env::var("CICD_SYNC_LIVE_TOKEN")
            .or_else(|_| std::env::var("CICD_SYNC_GITHUB_TOKEN"))
            .expect("token supplied privately");
        let root = std::env::temp_dir().join(format!("forge-live-sync-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let local = root.join("local.git");
        let other = root.join("other.git");
        let work = root.join("work");
        for path in [&local, &other] {
            assert!(
                Command::new("git")
                    .args(["init", "--bare", "--initial-branch=main"])
                    .arg(path)
                    .status()
                    .await
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "--initial-branch=main"]).await;
        git(&work, &["config", "user.name", "Forge QA"]).await;
        git(&work, &["config", "user.email", "qa@example.invalid"]).await;
        git(&work, &["commit", "--allow-empty", "-m", "initial"]).await;
        git(&work, &["tag", "-a", "v-sync-qa", "-m", "annotated QA tag"]).await;
        git(
            &work,
            &[
                "push",
                local.to_str().unwrap(),
                "main",
                "refs/tags/v-sync-qa",
            ],
        )
        .await;
        let settings = Settings {
            provider: if std::env::var("CICD_SYNC_LIVE_PROVIDER").as_deref() == Ok("gitlab") {
                Provider::Gitlab
            } else {
                Provider::Github
            },
            instance_url: std::env::var("CICD_SYNC_LIVE_ORIGIN")
                .unwrap_or_else(|_| "https://github.com".into()),
            repository_path: repository,
            direction: cicd_domain::repository_sync::Direction::Both,
            automatic: false,
        };
        let mut session = GitSession::new(
            &root,
            local.clone(),
            settings.clone(),
            token.clone(),
            Uuid::new_v4(),
            1,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        session.guard = Some(Arc::new(
            MutationGuard::acquire(&root, Uuid::new_v4()).await.unwrap(),
        ));
        assert!(session.check(true).await.unwrap().read);
        session.fetch().await.unwrap();
        let first = session.refs(false).await.unwrap();
        for (name, sha) in &first {
            session.apply(true, name, None, sha).await.unwrap();
        }
        assert_eq!(first, session.refs(true).await.unwrap());
        git(
            &work,
            &["commit", "--allow-empty", "-m", "external advance"],
        )
        .await;
        git(&work, &["branch", "external-feature"]).await;
        git(
            &work,
            &["push", other.to_str().unwrap(), "main", "external-feature"],
        )
        .await;
        let external = GitSession::new(
            &root,
            other.clone(),
            settings,
            token,
            Uuid::new_v4(),
            1,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        external.fetch().await.unwrap();
        let advanced = external.refs(false).await.unwrap();
        for (name, sha) in &advanced {
            external
                .apply(true, name, first.get(name).map(String::as_str), sha)
                .await
                .unwrap();
        }
        session.fetch().await.unwrap();
        let remote = session.refs(true).await.unwrap();
        for (name, sha) in &remote {
            if first.get(name) != Some(sha) {
                session
                    .apply(false, name, first.get(name).map(String::as_str), sha)
                    .await
                    .unwrap();
            }
        }
        assert_eq!(remote, session.refs(false).await.unwrap());
        assert!(
            session
                .apply(
                    true,
                    "refs/heads/main",
                    first.get("refs/heads/main").map(String::as_str),
                    first.get("refs/heads/main").unwrap()
                )
                .await
                .is_err()
        );
        // No credential is persisted in scratch Git config or object files.
        let config = std::fs::read_to_string(session.scratch.join("config")).unwrap();
        assert!(!config.contains("credential"));
        drop(external);
        drop(session);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn actual_git_fast_forward_tag_conflict_and_cleanup() {
        let root = std::env::temp_dir().join(format!("forge-sync-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let local = root.join("local.git");
        let remote = root.join("remote.git");
        let work = root.join("work");
        for path in [&local, &remote] {
            assert!(
                Command::new("git")
                    .args(["init", "--bare", "--initial-branch=main"])
                    .arg(path)
                    .status()
                    .await
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "--initial-branch=main"]).await;
        git(
            &work,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "first",
            ],
        )
        .await;
        git(&work, &["push", local.to_str().unwrap(), "main"]).await;
        let settings = Settings {
            provider: Provider::Gitlab,
            instance_url: "https://git.example.invalid".into(),
            repository_path: "a/b".into(),
            direction: cicd_domain::repository_sync::Direction::Both,
            automatic: false,
        };
        let guard = MutationGuard::acquire(&root, Uuid::new_v4()).await.unwrap();
        let mut session = GitSession::new(
            &root,
            local.clone(),
            settings,
            "unused".into(),
            Uuid::new_v4(),
            1,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        session.remote = Endpoint::Local(remote.clone());
        session.fetch().await.unwrap();
        let refs = session.refs(false).await.unwrap();
        let sha = refs.get("refs/heads/main").unwrap();
        session
            .apply(true, "refs/heads/main", None, sha)
            .await
            .unwrap();
        assert_eq!(
            session.refs(true).await.unwrap().get("refs/heads/main"),
            Some(sha)
        );
        assert!(
            session
                .apply(true, "refs/heads/main", None, sha)
                .await
                .is_err()
        );
        git(&work, &["config", "user.name", "Test"]).await;
        git(&work, &["config", "user.email", "test@example.invalid"]).await;
        git(&work, &["commit", "--allow-empty", "-m", "advance"]).await;
        git(&work, &["push", local.to_str().unwrap(), "main"]).await;
        session.fetch().await.unwrap();
        let advanced = session.refs(false).await.unwrap()["refs/heads/main"].clone();
        session
            .apply(true, "refs/heads/main", Some(sha), &advanced)
            .await
            .unwrap();
        assert!(
            session
                .apply(true, "refs/heads/main", Some(&advanced), sha)
                .await
                .is_err()
        );
        git(&work, &["checkout", "-b", "alternate", sha]).await;
        git(&work, &["commit", "--allow-empty", "-m", "diverged"]).await;
        git(&work, &["push", local.to_str().unwrap(), "alternate"]).await;
        session.fetch().await.unwrap();
        let divergent = session.refs(false).await.unwrap()["refs/heads/alternate"].clone();
        assert!(!session.ancestor(&advanced, &divergent).await);
        assert!(!session.ancestor(&divergent, &advanced).await);
        assert!(
            session
                .apply(true, "refs/heads/main", Some(&advanced), &divergent)
                .await
                .is_err()
        );
        git(&work, &["tag", "-a", "local-tag", "-m", "local annotation"]).await;
        git(
            &work,
            &[
                "tag",
                "-a",
                "remote-tag",
                &advanced,
                "-m",
                "remote annotation",
            ],
        )
        .await;
        git(
            &work,
            &[
                "push",
                local.to_str().unwrap(),
                "refs/tags/local-tag:refs/tags/tag-check",
            ],
        )
        .await;
        git(
            &work,
            &[
                "push",
                remote.to_str().unwrap(),
                "refs/tags/remote-tag:refs/tags/tag-check",
            ],
        )
        .await;
        session.fetch().await.unwrap();
        let a = session.refs(false).await.unwrap()["refs/tags/tag-check"].clone();
        let b = session.refs(true).await.unwrap()["refs/tags/tag-check"].clone();
        assert!(
            session
                .apply(true, "refs/tags/tag-check", Some(&b), &a)
                .await
                .is_err()
        );
        assert_eq!(
            session.refs(true).await.unwrap()["refs/heads/main"],
            advanced
        );
        assert_eq!(session.refs(true).await.unwrap()["refs/tags/tag-check"], b);
        let scratch = session.scratch.clone();
        drop(session);
        assert!(!scratch.exists());
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }
}
