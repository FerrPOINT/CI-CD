//! Bounded owner-local artifact observation. Never admission or deployment authority.
use anyhow::{Context, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub async fn verify_artifact(
    root: &Path,
    path: &Path,
    digest: &str,
    size: i64,
) -> anyhow::Result<()> {
    static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = WORKERS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .context("artifact observation capacity unavailable")?;
    let root = root.to_owned();
    let path = path.to_owned();
    let digest = digest.to_owned();
    let deadline = Instant::now() + Duration::from_secs(3);
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        ensure!(
            (0..=32 * 1024 * 1024).contains(&size),
            "artifact exceeds observation bound"
        );
        ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid artifact digest"
        );
        for target in [&root, &path] {
            for ancestor in target.ancestors() {
                let meta = fs::symlink_metadata(ancestor)?;
                ensure!(
                    !meta.file_type().is_symlink(),
                    "artifact path contains link"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        meta.file_attributes() & 0x400 == 0,
                        "artifact path contains reparse point"
                    );
                }
            }
        }
        let root = root.canonicalize()?;
        let path = path.canonicalize()?;
        ensure!(
            path.starts_with(&root) && path != root,
            "artifact path escapes owner root"
        );
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options.open(path)?;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.len() == size as u64,
            "artifact size/type mismatch"
        );
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut count = 0u64;
        loop {
            ensure!(
                Instant::now() < deadline,
                "artifact observation deadline exceeded"
            );
            let length = file.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            count += length as u64;
            ensure!(count <= size as u64, "artifact changed during observation");
            hash.update(&buffer[..length]);
        }
        ensure!(
            count == size as u64 && format!("{:x}", hash.finalize()) == digest,
            "artifact digest mismatch"
        );
        Ok(())
    });
    tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .context("artifact observation timed out")??
}
