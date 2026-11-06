//! One environment contract for embedded and external pipeline execution.
use serde_json::Value;
use std::collections::BTreeMap;

pub fn from_variables(
    variables: Option<&Value>,
    secrets: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let Some(value) = variables.filter(|value| !value.is_null()) else {
        return Ok(result);
    };
    let variables = value
        .as_object()
        .ok_or("Pipeline variables must be an object")?;
    for (key, value) in variables {
        if key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("Pipeline variable name is invalid".into());
        }
        let name = format!("CICD_VAR_{}", key.to_ascii_uppercase().replace('-', "_"));
        let value = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        if result.insert(name, value).is_some() {
            return Err("Pipeline variable names collide after normalization".into());
        }
    }
    validate(&result, secrets)?;
    Ok(result)
}

pub fn validate(environment: &BTreeMap<String, String>, secrets: &[String]) -> Result<(), String> {
    for (name, value) in environment {
        let Some(key) = name.strip_prefix("CICD_VAR_") else {
            return Err("Pipeline environment cannot replace runner identity".into());
        };
        if key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err("Pipeline environment name is invalid".into());
        }
        if value.contains('\0') {
            return Err("Pipeline variable contains an invalid environment value".into());
        }
        if secrets.contains(name) {
            return Err("Pipeline environment conflicts with a declared secret".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_reach_environment_without_shell_or_json_escaping() {
        let vars = json!({"mixed-key":"a \"quote\"\n$HOME; echo no", "number":42, "flag":true, "object":{"x":1}});
        let env = from_variables(Some(&vars), &[]).unwrap();
        assert_eq!(env["CICD_VAR_MIXED_KEY"], "a \"quote\"\n$HOME; echo no");
        assert_eq!(env["CICD_VAR_NUMBER"], "42");
        assert_eq!(env["CICD_VAR_FLAG"], "true");
        assert_eq!(env["CICD_VAR_OBJECT"], "{\"x\":1}");
    }

    #[test]
    fn missing_null_and_empty_variables_keep_the_old_empty_contract() {
        assert!(from_variables(None, &[]).unwrap().is_empty());
        assert!(from_variables(Some(&Value::Null), &[]).unwrap().is_empty());
        assert!(from_variables(Some(&json!({})), &[]).unwrap().is_empty());
    }

    #[test]
    fn normalized_collisions_and_invalid_values_are_explicit() {
        for vars in [
            json!({"a-b":1,"A_B":2}),
            json!({"bad key":1}),
            json!({"":1}),
            json!({"x":"nul\0value"}),
            json!([]),
        ] {
            assert!(from_variables(Some(&vars), &[]).is_err());
        }
        assert!(from_variables(Some(&json!({"a-b":1,"a-c":2})), &[]).is_ok());
    }

    #[test]
    fn secrets_and_service_identity_cannot_be_overridden() {
        assert!(
            from_variables(Some(&json!({"token":"plain"})), &["CICD_VAR_TOKEN".into()]).is_err()
        );
        assert!(from_variables(Some(&json!({"token":"plain"})), &["PRODUCT_TOKEN".into()]).is_ok());
        assert!(
            validate(
                &BTreeMap::from([("CICD_JOB_ID".into(), "forged".into())]),
                &[]
            )
            .is_err()
        );
    }
}
