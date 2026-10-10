//! Route coverage regression (server crate: needs ApiDoc + route_access).
use crate::api::ApiDoc;
use cicd_app::route_access;

#[test]
fn namespace_owner_and_reader_routes_never_grant_human_roles() {
    use cicd_app::{Role, RouteAccess, allows};
    for (method, path, expected) in [
        (
            "PUT",
            "/api/v1/namespace-resources/git_group/018f3c59-38f6-7c2a-bc55-081eb78cbf17",
            RouteAccess::NamespaceOwner,
        ),
        (
            "GET",
            "/api/v1/namespace-resources/git_group/018f3c59-38f6-7c2a-bc55-081eb78cbf17",
            RouteAccess::NamespaceOwner,
        ),
        (
            "GET",
            "/api/v1/namespace-repositories",
            RouteAccess::NamespaceReader,
        ),
        (
            "GET",
            "/api/v1/namespace-repositories/018f3c59-38f6-7c2a-bc55-081eb78cbf17",
            RouteAccess::NamespaceReader,
        ),
        (
            "GET",
            "/api/v1/namespace-task-evidence/018f3c59-38f6-7c2a-bc55-081eb78cbf17/018f3c59-38f6-7c2a-bc55-081eb78cbf18",
            RouteAccess::NamespaceReader,
        ),
    ] {
        assert_eq!(route_access(method, path), Some(expected));
        assert!(!allows(Role::Viewer, method, path));
        assert!(!allows(Role::Admin, method, path));
    }
    assert!(allows(
        Role::Viewer,
        "GET",
        "/api/v1/catalog/available-repositories"
    ));
    assert!(!allows(
        Role::Viewer,
        "PUT",
        "/api/v1/catalog/repositories/018f3c59-38f6-7c2a-bc55-081eb78cbf17/group"
    ));
    assert!(allows(
        Role::Developer,
        "PUT",
        "/api/v1/catalog/repositories/018f3c59-38f6-7c2a-bc55-081eb78cbf17/group"
    ));
}

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
