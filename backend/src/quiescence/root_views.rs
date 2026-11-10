//! Operator-pinned views of roots belonging to different container mount namespaces.
use super::{CustodyRoot, Identity};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootView {
    pub participant: String,
    pub root: CustodyRoot,
    pub view: PathBuf,
    pub device: u64,
    pub inode: u64,
}

pub(super) fn map(expected: &[Identity], views: &[RootView]) -> anyhow::Result<Vec<Identity>> {
    use std::os::unix::fs::MetadataExt;
    ensure!(
        !views.is_empty() && views.len() <= 64,
        "root view count rejected"
    );
    let mut used = BTreeSet::new();
    let mut mapped = expected.to_vec();
    for identity in &mut mapped {
        for root in &mut identity.custody_roots {
            let matches: Vec<_> = views
                .iter()
                .enumerate()
                .filter(|(_, v)| v.participant == identity.participant && v.root == *root)
                .collect();
            ensure!(
                matches.len() == 1,
                "each participant root needs one exact view"
            );
            let (index, view) = matches[0];
            ensure!(used.insert(index), "duplicate participant root");
            ensure!(view.view.is_absolute(), "relative root view rejected");
            super::plain(&view.view, true)?;
            let metadata = std::fs::metadata(&view.view)?;
            ensure!(
                metadata.dev() == view.device && metadata.ino() == view.inode,
                "root view filesystem identity changed"
            );
            root.path = view.view.clone();
        }
    }
    ensure!(used.len() == views.len(), "unowned root view rejected");
    Ok(mapped)
}

pub(super) fn recorded<'a>(
    identity: &Identity,
    root: &CustodyRoot,
    views: &'a [RootView],
) -> anyhow::Result<&'a CustodyRoot> {
    views
        .iter()
        .find(|v| {
            v.participant == identity.participant && v.view == root.path && v.root.kind == root.kind
        })
        .map(|v| &v.root)
        .context("mapped source root missing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use uuid::Uuid;
    #[test]
    fn identical_container_paths_resolve_to_distinct_volumes() {
        let dir = std::env::temp_dir().join(format!("forge-root-views-{}", Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let mut identities = Vec::new();
        let mut views = Vec::new();
        for name in ["server", "runner"] {
            let path = dir.join(name);
            std::fs::create_dir(&path).unwrap();
            let metadata = path.metadata().unwrap();
            let root = CustodyRoot::workspace("/workspaces".into());
            identities.push(Identity {
                schema_version: 1,
                participant: name.into(),
                boot_id: Uuid::new_v4(),
                kind: name.into(),
                process_id: 1,
                process_start: "1".into(),
                custody_roots: vec![root.clone()],
            });
            views.push(RootView {
                participant: name.into(),
                root,
                view: path,
                device: metadata.dev(),
                inode: metadata.ino(),
            });
        }
        let mapped = map(&identities, &views).unwrap();
        assert_ne!(
            mapped[0].custody_roots[0].path,
            mapped[1].custody_roots[0].path
        );
        assert_eq!(
            identities[0].custody_roots[0].path,
            PathBuf::from("/workspaces")
        );
        assert!(map(&identities, &views[..1]).is_err());
        let mut wrong = views.clone();
        wrong[0].inode += 1;
        assert!(map(&identities, &wrong).is_err());
        let mut extra = views.clone();
        extra.push(views[0].clone());
        assert!(map(&identities, &extra).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
