//! Existing runner wire encoding, shared with candidate and owner-policy observers.
use std::collections::BTreeSet;

pub fn artifact_name_from_declared_path(path: &str) -> String {
    path.replace(['/', '\\'], "__")
}

/// This does not normalize or validate filesystem paths. Existing path guards still apply.
/// A repeated path or two different paths with the same wire name are both ambiguous.
pub fn declared_artifact_names(paths: &[String]) -> Result<Vec<String>, &'static str> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::with_capacity(paths.len());
    for path in paths {
        let name = artifact_name_from_declared_path(path);
        if name.is_empty() || !seen.insert(name.clone()) {
            return Err("ambiguous declared artifact names");
        }
        names.push(name);
    }
    Ok(names)
}

/// `observed` must contain only the selected current attempt; retention is checked by its owner.
/// Preserve undeclared extras, but never accept duplicate observations or a missing declaration.
pub fn declared_artifacts_complete(paths: &[String], observed: &[String]) -> bool {
    let Ok(expected) = declared_artifact_names(paths) else {
        return false;
    };
    let mut available = BTreeSet::new();
    if !observed
        .iter()
        .all(|name| !name.is_empty() && available.insert(name.as_str()))
    {
        return false;
    }
    expected
        .iter()
        .all(|name| available.contains(name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn artifact_encoding_preserves_existing_nested_and_windows_wire_bytes() {
        assert_eq!(
            artifact_name_from_declared_path("dist/release/bundle.tar"),
            "dist__release__bundle.tar"
        );
        assert_eq!(
            artifact_name_from_declared_path("dist\\release\\bundle.tar"),
            "dist__release__bundle.tar"
        );
        assert_eq!(artifact_name_from_declared_path("bundle.tar"), "bundle.tar");
    }

    #[test]
    fn distinct_paths_with_same_encoded_name_and_repeated_paths_deny() {
        for paths in [
            strings(&["dist/a", "dist__a"]),
            strings(&["dist/a", "dist\\a"]),
            strings(&["a", "a"]),
            strings(&[""]),
        ] {
            assert!(declared_artifact_names(&paths).is_err());
            assert!(!declared_artifacts_complete(
                &paths,
                &strings(&["dist__a", "a"])
            ));
        }
    }

    #[test]
    fn nested_observer_matches_exact_canonical_name_not_literal_or_basename() {
        let paths = strings(&["dist/bundle.tar"]);
        assert!(declared_artifacts_complete(
            &paths,
            &strings(&["dist__bundle.tar"])
        ));
        for names in [
            strings(&["dist/bundle.tar"]),
            strings(&["bundle.tar"]),
            vec![],
        ] {
            assert!(!declared_artifacts_complete(&paths, &names));
        }
    }

    #[test]
    fn duplicate_observations_do_not_complete_using_first_match() {
        assert!(!declared_artifacts_complete(
            &strings(&["dist/a"]),
            &strings(&["dist__a", "dist__a"])
        ));
        assert!(!declared_artifacts_complete(
            &strings(&["a"]),
            &strings(&["a", "extra", "extra"])
        ));
        assert!(declared_artifacts_complete(
            &strings(&["a"]),
            &strings(&["extra", "a"])
        ));
    }

    #[test]
    fn empty_inventory_and_missing_declared_output_keep_distinct_meanings() {
        assert!(declared_artifacts_complete(&[], &[]));
        assert!(!declared_artifacts_complete(
            &strings(&["a", "b"]),
            &strings(&["a"])
        ));
        assert!(declared_artifact_names(&strings(&["b", "a"])).unwrap() == strings(&["b", "a"]));
    }
}
