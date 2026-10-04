//! Compare bytes/types/modes with the owner-pinned tree, never index stat-cache truth.
use super::git_bounded;
use anyhow::{Context, ensure};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: String,
    oid: String,
}

pub(super) async fn verify(checkout: &Path, repository: &Path, commit: &str) -> anyhow::Result<()> {
    ensure!(cfg!(unix), "physical mode verification requires Unix");
    let manifest = git_bounded(
        repository,
        &["ls-tree", "-r", "-t", "-z", "--full-tree", commit],
        MAX_MANIFEST_BYTES,
    )
    .await?;
    let expected = tree(&manifest)?;
    let index = git_bounded(
        checkout,
        &["ls-files", "--stage", "-v", "-z"],
        MAX_MANIFEST_BYTES,
    )
    .await?;
    verify_index(&index, &expected)?;
    let root = checkout.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut actual = BTreeMap::new();
        let mut total = 0;
        walk(&root, &root, 0, &mut actual, &mut total)?;
        ensure!(
            actual == expected,
            "physical checkout differs from pinned tree; reconciliation required"
        );
        Ok(())
    })
    .await
    .context("join physical checkout verification")?
}

fn records(bytes: &[u8]) -> anyhow::Result<impl Iterator<Item = &[u8]>> {
    ensure!(
        bytes.is_empty() || bytes.last() == Some(&0),
        "truncated Git inventory"
    );
    Ok(bytes.split(|b| *b == 0).filter(|record| !record.is_empty()))
}

fn path(raw: &[u8]) -> anyhow::Result<PathBuf> {
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec()))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(std::str::from_utf8(raw)?);
    ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "unsafe tree path"
    );
    ensure!(
        path.components().count() <= MAX_DEPTH
            && path.components().all(|c| c.as_os_str() != ".git"),
        "unsupported tree path/depth"
    );
    Ok(path)
}

fn entry(mode: &str, oid: &str) -> anyhow::Result<Entry> {
    ensure!(
        matches!(mode, "040000" | "100644" | "100755" | "120000"),
        "unsupported tree mode (gitlinks excluded)"
    );
    ensure!(
        oid.len() == 40
            && oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid tree blob ID"
    );
    Ok(Entry {
        mode: mode.into(),
        oid: oid.into(),
    })
}

fn tree(bytes: &[u8]) -> anyhow::Result<BTreeMap<PathBuf, Entry>> {
    let mut entries = BTreeMap::new();
    for record in records(bytes)? {
        ensure!(
            entries.len() < MAX_ENTRIES,
            "pinned tree exceeds entry bound"
        );
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("invalid tree record")?;
        let fields: Vec<_> = std::str::from_utf8(&record[..tab])?.split(' ').collect();
        ensure!(
            fields.len() == 3 && matches!(fields[1], "blob" | "tree"),
            "unsupported tree object"
        );
        let mut e = entry(fields[0], fields[2])?;
        ensure!(
            (e.mode == "040000") == (fields[1] == "tree"),
            "tree mode/type mismatch"
        );
        // Directory membership is compared recursively, not hashed as a filesystem blob.
        if e.mode == "040000" {
            e.oid.clear();
        }
        ensure!(
            entries.insert(path(&record[tab + 1..])?, e).is_none(),
            "duplicate tree path"
        );
    }
    Ok(entries)
}

fn verify_index(bytes: &[u8], tree: &BTreeMap<PathBuf, Entry>) -> anyhow::Result<()> {
    let mut entries = BTreeMap::new();
    for record in records(bytes)? {
        ensure!(entries.len() < MAX_ENTRIES, "index exceeds entry bound");
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("invalid index record")?;
        let fields: Vec<_> = std::str::from_utf8(&record[..tab])?.split(' ').collect();
        ensure!(
            fields.len() == 4 && fields[0] == "H" && fields[3] == "0",
            "unsupported index flags/stage; no autorepair"
        );
        ensure!(
            entries
                .insert(path(&record[tab + 1..])?, entry(fields[1], fields[2])?)
                .is_none(),
            "duplicate index path"
        );
    }
    let files: BTreeMap<_, _> = tree
        .iter()
        .filter(|(_, e)| e.mode != "040000")
        .map(|(p, e)| (p.clone(), e.clone()))
        .collect();
    ensure!(entries == files, "index differs from pinned tree");
    Ok(())
}

fn walk(
    root: &Path,
    directory: &Path,
    depth: usize,
    actual: &mut BTreeMap<PathBuf, Entry>,
    total: &mut u64,
) -> anyhow::Result<()> {
    ensure!(depth <= MAX_DEPTH, "physical inventory exceeds depth bound");
    for child in fs::read_dir(directory)? {
        let child = child?;
        if directory == root && child.file_name() == ".git" {
            continue;
        }
        ensure!(
            actual.len() < MAX_ENTRIES,
            "physical inventory exceeds entry bound"
        );
        let full = child.path();
        let relative = full.strip_prefix(root)?.to_path_buf();
        ensure!(
            relative.components().count() <= MAX_DEPTH,
            "physical inventory exceeds depth bound"
        );
        let meta = fs::symlink_metadata(&full)?;
        if meta.is_dir() && !super::super::is_link(&meta) {
            actual.insert(
                relative,
                Entry {
                    mode: "040000".into(),
                    oid: String::new(),
                },
            );
            walk(root, &full, depth + 1, actual, total)?;
        } else {
            let e = if meta.file_type().is_symlink() {
                let target = fs::read_link(&full)?;
                let bytes = target.as_os_str().as_encoded_bytes();
                budget(bytes.len() as u64, total)?;
                let mut hash = blob_hash(bytes.len() as u64);
                hash.update(bytes);
                Entry {
                    mode: "120000".into(),
                    oid: format!("{:x}", hash.finalize()),
                }
            } else {
                ensure!(
                    meta.is_file() && !super::super::is_link(&meta),
                    "unsupported physical file type"
                );
                budget(meta.len(), total)?;
                file_blob(&full, &meta)?
            };
            actual.insert(relative, e);
        }
        ensure!(
            same_metadata(&meta, &fs::symlink_metadata(&full)?),
            "path changed during physical verification"
        );
    }
    Ok(())
}

fn budget(length: u64, total: &mut u64) -> anyhow::Result<()> {
    ensure!(length <= MAX_FILE_BYTES, "physical file exceeds byte bound");
    *total = total
        .checked_add(length)
        .context("physical byte budget overflow")?;
    ensure!(
        *total <= MAX_TOTAL_BYTES,
        "physical inventory exceeds total byte bound"
    );
    Ok(())
}

fn blob_hash(length: u64) -> Sha1 {
    let mut hash = Sha1::new();
    hash.update(format!("blob {length}\0").as_bytes());
    hash
}

#[cfg(unix)]
fn same_metadata(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.mode() == b.mode()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

#[cfg(not(unix))]
fn same_metadata(_: &fs::Metadata, _: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn file_blob(path: &Path, before: &fs::Metadata) -> anyhow::Result<Entry> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    // Do not follow a substituted link or block on a substituted special file.
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    ensure!(
        same_metadata(before, &file.metadata()?),
        "file replaced before physical read"
    );
    let mut hash = blob_hash(before.len());
    let mut buffer = [0; 64 * 1024];
    let mut length = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        ensure!(length <= before.len(), "file grew during physical read");
        hash.update(&buffer[..count]);
    }
    ensure!(
        length == before.len() && same_metadata(before, &file.metadata()?),
        "file changed during physical read"
    );
    Ok(Entry {
        mode: if before.permissions().mode() & 0o100 != 0 {
            "100755"
        } else {
            "100644"
        }
        .into(),
        oid: format!("{:x}", hash.finalize()),
    })
}

#[cfg(not(unix))]
fn file_blob(_: &Path, _: &fs::Metadata) -> anyhow::Result<Entry> {
    anyhow::bail!("physical mode verification requires Unix")
}
