use anyhow::{Context, ensure};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(super) fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn plain(path: &Path) -> anyhow::Result<()> {
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor)?;
        ensure!(!meta.file_type().is_symlink(), "owner path contains link");
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                meta.file_attributes() & 0x400 == 0,
                "owner path contains reparse point"
            );
        }
    }
    Ok(())
}

fn open_read(path: &Path) -> anyhow::Result<fs::File> {
    plain(path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(file.metadata()?.is_file(), "owner file is not regular");
    Ok(file)
}

pub(super) fn read_bytes(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let mut file = open_read(path)?;
    ensure!(file.metadata()?.len() <= limit, "owner file exceeds bound");
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "owner file changed beyond bound"
    );
    Ok(bytes)
}

pub(super) fn read<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    Ok(serde_json::from_slice(&read_bytes(path, 128 * 1024)?)?)
}

pub(super) fn optional<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    match fs::symlink_metadata(path) {
        Ok(_) => read(path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn sync_dir(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        plain(path)?;
        fs::File::open(path)?.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        anyhow::bail!("durable manifest deployment requires Unix filesystem semantics")
    }
}

pub(super) fn directory(path: &Path) -> anyhow::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => sync_dir(path.parent().context("owner parent missing")?)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    plain(path)?;
    ensure!(fs::metadata(path)?.is_dir(), "owner directory required");
    Ok(())
}

pub(super) fn immutable_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    plain(path.parent().context("owner parent missing")?)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
            sync_dir(path.parent().unwrap())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => ensure!(
            read_bytes(path, 32 * 1024 * 1024)? == bytes,
            "immutable owner record conflict"
        ),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub(super) fn immutable<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= 128 * 1024, "owner record exceeds bound");
    immutable_bytes(path, &bytes)
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Pointer {
    pub manifest_sha256: String,
}

pub(super) fn pointer(root: &Path, name: &str) -> anyhow::Result<Option<String>> {
    let result = optional::<Pointer>(&root.join(name))?.map(|p| p.manifest_sha256);
    ensure!(
        result
            .as_ref()
            .is_none_or(|s| crate::domain::task_delivery::digest(s)),
        "invalid owner pointer"
    );
    Ok(result)
}

pub(super) fn replace_pointer(root: &Path, name: &str, digest: &str) -> anyhow::Result<()> {
    ensure!(
        crate::domain::task_delivery::digest(digest),
        "invalid manifest digest"
    );
    if pointer(root, name)?.as_deref() == Some(digest) {
        return Ok(());
    }
    let staged = root.join(format!(".pointer-{}", Uuid::new_v4().simple()));
    immutable(
        &staged,
        &Pointer {
            manifest_sha256: digest.into(),
        },
    )?;
    // Rename is the sole publication effect; no children, shell commands or orchestrator retry.
    fs::rename(&staged, root.join(name))?;
    sync_dir(root)
}

pub(super) fn copy_artifact(
    root: &Path,
    source: &Path,
    digest: &str,
    size: i64,
) -> anyhow::Result<()> {
    ensure!(
        crate::domain::task_delivery::digest(digest) && (0..=32 * 1024 * 1024).contains(&size),
        "invalid bounded artifact"
    );
    let bytes = read_bytes(source, 32 * 1024 * 1024)?;
    ensure!(
        bytes.len() as i64 == size && sha(&bytes) == digest,
        "candidate bytes changed before staging"
    );
    immutable_bytes(&root.join("objects").join(digest), &bytes)
}

pub(super) struct Lock {
    _file: fs::File,
}
impl Lock {
    pub fn acquire(root: &Path) -> anyhow::Result<Self> {
        plain(root)?;
        let path = root.join(".lock");
        if path.exists() {
            plain(&path)?;
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(&path)?;
        ensure!(file.metadata()?.is_file(), "owner lock is not regular");
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            ensure!(
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
                "owner target is busy"
            );
        }
        #[cfg(not(unix))]
        {
            anyhow::bail!("manifest executor is Unix-only");
        }
        #[allow(unreachable_code)]
        Ok(Self { _file: file })
    }
}

pub(super) fn operation_dir(root: &Path, key: &str) -> PathBuf {
    root.join("operations").join(sha(key.as_bytes()))
}
