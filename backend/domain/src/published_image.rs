//! Registry publication is a separate artifact: OCI candidate v1 stays unchanged.
use crate::image_build::{valid_commit, valid_image_id};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedImage {
    pub schema: String,
    pub source_commit: String,
    pub image_ref: String,
    pub descriptor_sha256: String,
}

impl PublishedImage {
    pub fn new(
        source_commit: String,
        image_ref: String,
        descriptor_sha256: String,
    ) -> Result<Self, String> {
        let result = Self {
            schema: "forge/published-image/v1".into(),
            source_commit,
            image_ref,
            descriptor_sha256,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "forge/published-image/v1"
            || !valid_commit(&self.source_commit)
            || !valid_image_id(&format!("sha256:{}", self.descriptor_sha256))
            || !immutable_registry_ref(&self.image_ref)
        {
            return Err("invalid registry publication artifact".into());
        }
        Ok(())
    }

    /// The caller computes the descriptor hash from the retained original bytes.
    pub fn binds(&self, source_commit: &str, descriptor_sha256: &str) -> bool {
        self.validate().is_ok()
            && self.source_commit == source_commit
            && self.descriptor_sha256 == descriptor_sha256
    }
}

pub fn immutable_registry_ref(value: &str) -> bool {
    let Some((repository, digest)) = value.split_once('@') else {
        return false;
    };
    if value.len() > 1024 || !valid_image_id(digest) {
        return false;
    }
    let Some((registry, path)) = repository.split_once('/') else {
        return false;
    };
    let Ok(url) = url::Url::parse(&format!("https://{registry}")) else {
        return false;
    };
    url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_reference_binds_exact_commit_and_original_descriptor() {
        let commit = "a".repeat(40);
        let hash = "b".repeat(64);
        let image = format!(
            "registry.example:5000/project/app@sha256:{}",
            "c".repeat(64)
        );
        let value = PublishedImage::new(commit.clone(), image, hash.clone()).unwrap();
        assert!(value.binds(&commit, &hash));
        assert!(!value.binds(&"d".repeat(40), &hash));
        assert!(!value.binds(&commit, &"d".repeat(64)));
    }

    #[test]
    fn mutable_tags_credentials_and_path_escapes_are_rejected() {
        for repository in [
            "registry.example/app:latest",
            "https://registry.example/app",
            "user:password@registry.example/app",
            "registry.example/../other",
            "registry.example/app?token=private",
        ] {
            let value = format!("{repository}@sha256:{}", "c".repeat(64));
            assert!(!immutable_registry_ref(&value), "{repository}");
        }
    }

    #[test]
    fn legacy_descriptor_keeps_its_strict_v1_wire_shape() {
        let old = crate::image_build::OciCandidateDescriptor::new(
            format!("sha256:{}", "a".repeat(64)),
            "b".repeat(40),
            vec![1],
        )
        .unwrap();
        let mut wire = serde_json::to_value(&old).unwrap();
        assert!(wire.get("imageRef").is_none());
        wire["imageRef"] = serde_json::json!("registry.example/app:latest");
        assert!(
            serde_json::from_value::<crate::image_build::OciCandidateDescriptor>(wire).is_err()
        );
    }
}
