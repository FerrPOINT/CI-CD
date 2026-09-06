//! Route coverage regression (server crate: needs ApiDoc + route_access).
use crate::api::ApiDoc;
use cicd_app::route_access;

#[test]
fn route_policy_inventory_covers_generated_openapi() {
    let doc =
        serde_json::to_value(<ApiDoc as utoipa::OpenApi>::openapi()).expect("serialize openapi");
    let paths = doc
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("openapi paths object");
    let methods = ["get", "post", "put", "patch", "delete", "head", "options"];
    let mut missing = Vec::new();

    for (path, item) in paths {
        let Some(operations) = item.as_object() else {
            continue;
        };
        for method in methods {
            if operations.contains_key(method) {
                let method = method.to_ascii_uppercase();
                if route_access(&method, path).is_none() {
                    missing.push(format!("{method} {path}"));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "authz route inventory is missing entries for: {missing:?}"
    );
}
