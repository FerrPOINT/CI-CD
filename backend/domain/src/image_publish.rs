//! Ordinary repository builds are separate from task-bound readonly candidates.
use crate::image_build::valid_relative_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImagePublishSpec {
    pub context: String,
    pub dockerfile: String,
    pub repository: String,
    pub descriptor_artifact: String,
    pub published_artifact: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub build_args: BTreeMap<String, String>,
}

impl ImagePublishSpec {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_relative_path(&self.context, true)
            || !valid_relative_path(&self.dockerfile, false)
            || !valid_relative_path(&self.descriptor_artifact, false)
            || !valid_relative_path(&self.published_artifact, false)
            || self.descriptor_artifact == self.published_artifact
        {
            return Err("image_publish requires distinct portable artifact paths".into());
        }
        crate::artifact_names::declared_artifact_names(&[
            self.descriptor_artifact.clone(),
            self.published_artifact.clone(),
        ])
        .map_err(str::to_string)?;
        let immutable = format!("{}@sha256:{}", self.repository, "a".repeat(64));
        if !crate::published_image::immutable_registry_ref(&immutable) {
            return Err("image_publish requires a registry/repository without a tag".into());
        }
        if self.target.as_ref().is_some_and(|s| !identifier(s, 128))
            || self.build_args.len() > 32
            || self.build_args.iter().any(|(key, value)| {
                !identifier(key, 128) || value.len() > 4096 || value.chars().any(char::is_control)
            })
        {
            return Err("invalid image_publish target or build arguments".into());
        }
        Ok(())
    }
}

fn identifier(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && matches!(b, b'_' | b'-' | b'.')))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> ImagePublishSpec {
        serde_json::from_value(serde_json::json!({
            "context": ".", "dockerfile": "Dockerfile",
            "repository": "registry.example.test/team/web",
            "descriptor_artifact": "out/build.json", "published_artifact": "out/image.json"
        }))
        .unwrap()
    }
    #[test]
    fn ordinary_build_does_not_claim_readonly_database_compatibility() {
        let value = spec();
        value.validate().unwrap();
        let wire = serde_json::to_value(value).unwrap();
        assert!(wire.get("readable_schema_versions").is_none());
        assert!(wire.get("build_args").is_none());
        assert!(wire.get("target").is_none());
    }
    #[test]
    fn build_paths_and_registry_are_closed_inputs() {
        let mut value = spec();
        for repo in [
            "ubuntu:latest",
            "registry.test/team/image:tag",
            "https://registry.test/a",
            "u:p@registry.test/a",
            "registry.test/a/../b",
        ] {
            value.repository = repo.into();
            assert!(value.validate().is_err(), "{repo}");
        }
        value = spec();
        value.published_artifact = value.descriptor_artifact.clone();
        assert!(value.validate().is_err());
        value = spec();
        value.build_args.insert("--network".into(), "host".into());
        assert!(value.validate().is_err());
        value = spec();
        value
            .build_args
            .insert("NODE_HEAP_MB".into(), "3072".into());
        value.validate().unwrap();
        let mut wire = serde_json::to_value(value).unwrap();
        wire["entitlements"] = serde_json::json!(["security.insecure"]);
        assert!(serde_json::from_value::<ImagePublishSpec>(wire).is_err());
    }
}
