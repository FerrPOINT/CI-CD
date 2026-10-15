//! Closed backend-owned image build input and the existing OCI candidate artifact.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageBuildSpec {
    pub context: String,
    pub dockerfile: String,
    pub target: Option<String>,
    pub descriptor_artifact: String,
    pub readable_schema_versions: Vec<u32>,
}

/// Portable Git-relative paths, never host paths or Docker option fragments.
pub fn valid_relative_path(path: &str, allow_root: bool) -> bool {
    (allow_root && path == ".")
        || (!path.is_empty()
            && path.len() <= 1024
            && !path.contains(['\\', ':'])
            && !path.chars().any(char::is_control)
            && path.split('/').all(|part| {
                !part.is_empty() && part != "." && part != ".." && !part.starts_with('-')
            }))
}

pub fn valid_commit(commit: &str) -> bool {
    lower_hex(commit, 40)
}

pub fn valid_image_id(image: &str) -> bool {
    image
        .strip_prefix("sha256:")
        .is_some_and(|digest| lower_hex(digest, 64))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_versions(versions: &[u32]) -> bool {
    !versions.is_empty()
        && versions.len() <= 16
        && versions
            .iter()
            .enumerate()
            .all(|(index, version)| *version > 0 && !versions[..index].contains(version))
}

impl ImageBuildSpec {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_relative_path(&self.context, true)
            || !valid_relative_path(&self.dockerfile, false)
            || !valid_relative_path(&self.descriptor_artifact, false)
        {
            return Err("image build requires portable relative paths".into());
        }
        if self.target.as_ref().is_some_and(|target| {
            target.is_empty()
                || target.len() > 128
                || !target.bytes().enumerate().all(|(index, byte)| {
                    byte.is_ascii_alphanumeric()
                        || (index > 0 && matches!(byte, b'_' | b'-' | b'.'))
                })
        }) {
            return Err("image build target is invalid".into());
        }
        if !valid_versions(&self.readable_schema_versions) {
            return Err("image build schema versions must be positive and unique (1..16)".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OciCandidateDescriptor {
    pub schema: String,
    pub image_id: String,
    pub source_commit: String,
    pub data_protocol: String,
    pub readable_schema_versions: Vec<u32>,
    pub migrations: Vec<String>,
}

impl OciCandidateDescriptor {
    pub fn new(
        image_id: String,
        source_commit: String,
        readable_schema_versions: Vec<u32>,
    ) -> Result<Self, String> {
        let descriptor = Self {
            schema: "forge/oci-candidate/v1".into(),
            image_id,
            source_commit,
            data_protocol: "readonly_snapshot_v1".into(),
            readable_schema_versions,
            migrations: Vec::new(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "forge/oci-candidate/v1"
            || !valid_image_id(&self.image_id)
            || !valid_commit(&self.source_commit)
            || self.data_protocol != "readonly_snapshot_v1"
            || !valid_versions(&self.readable_schema_versions)
            || !self.migrations.is_empty()
        {
            return Err("invalid readonly OCI candidate descriptor".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn spec() -> Value {
        json!({"context":".","dockerfile":"Dockerfile","target":null,
            "descriptor_artifact":"delivery/candidate.json","readable_schema_versions":[1,2]})
    }

    #[test]
    fn image_build_closed_spec_and_root_context() {
        let value: ImageBuildSpec = serde_json::from_value(spec()).unwrap();
        assert!(value.validate().is_ok());
        assert_eq!(serde_json::to_value(value).unwrap(), spec());
        for field in [
            "args",
            "image",
            "daemon",
            "secrets",
            "network",
            "entitlements",
        ] {
            let mut input = spec();
            input[field] = json!("forbidden");
            assert!(serde_json::from_value::<ImageBuildSpec>(input).is_err());
        }
    }

    #[test]
    fn image_build_rejects_paths_options_and_invalid_versions() {
        for path in [
            "", "..", "a/../b", "/tmp/x", "C:/x", "a\\b", "a//b", "-f", "a\0b",
        ] {
            for field in ["context", "dockerfile", "descriptor_artifact"] {
                let mut input = spec();
                input[field] = json!(path);
                assert!(
                    serde_json::from_value::<ImageBuildSpec>(input)
                        .unwrap()
                        .validate()
                        .is_err()
                );
            }
        }
        for versions in [vec![], vec![0], vec![1, 1], (1..=17).collect()] {
            let mut input: ImageBuildSpec = serde_json::from_value(spec()).unwrap();
            input.readable_schema_versions = versions;
            assert!(input.validate().is_err());
        }
        for target in ["", "--network=host", "a b", "a/b"] {
            let mut input: ImageBuildSpec = serde_json::from_value(spec()).unwrap();
            input.target = Some(target.into());
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn image_build_descriptor_matches_existing_oci_camel_case_wire() {
        let descriptor = OciCandidateDescriptor::new(
            format!("sha256:{}", "a".repeat(64)),
            "b".repeat(40),
            vec![2, 1],
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&descriptor).unwrap(),
            json!({
                "schema":"forge/oci-candidate/v1","imageId":format!("sha256:{}", "a".repeat(64)),
                "sourceCommit":"b".repeat(40),"dataProtocol":"readonly_snapshot_v1",
                "readableSchemaVersions":[2,1],"migrations":[]
            })
        );
        let mut invalid = descriptor.clone();
        invalid.migrations.push("sql".into());
        assert!(invalid.validate().is_err());
        invalid = descriptor;
        invalid.image_id = "mutable:latest".into();
        assert!(invalid.validate().is_err());
    }
}
