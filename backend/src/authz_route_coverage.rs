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
            "/api/v1/namespace-repositories/018f3c59-38f6-7c2a-bc55-081eb78cbf17/source",
            RouteAccess::NamespaceReader,
        ),
        (
            "GET",
            "/api/v1/namespace-repositories/018f3c59-38f6-7c2a-bc55-081eb78cbf17/devops-evidence",
            RouteAccess::NamespaceReader,
        ),
        (
            "POST",
            "/api/v1/internal/task-git-check",
            RouteAccess::System,
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
fn legacy_operations_keep_object_contracts_and_new_routes_are_explicit() {
    let doc = serde_json::to_value(<ApiDoc as utoipa::OpenApi>::openapi()).unwrap();
    let paths = &doc["paths"];
    let schemas = &doc["components"]["schemas"];
    let legacy_push = &paths["/api/v1/internal/git-push"]["post"]["requestBody"]["content"]["application/json"]
        ["schema"];
    assert_eq!(
        legacy_push,
        &serde_json::json!({"$ref":"#/components/schemas/GitPushEvent"})
    );
    let legacy_read = &paths["/api/v1/namespace-repositories/{id}"]["get"]["responses"]["200"]["content"]
        ["application/json"]["schema"];
    assert_eq!(
        legacy_read,
        &serde_json::json!({"$ref":"#/components/schemas/VerifiedRepository"})
    );
    for name in [
        "GitPushEvent",
        "VerifiedRepository",
        "VerifiedSource",
        "CandidateCheck",
        "CandidateCheckReadback",
    ] {
        assert_eq!(schemas[name]["type"], "object", "{name}");
        for union in ["oneOf", "anyOf", "allOf"] {
            assert!(schemas[name].get(union).is_none(), "{name}/{union}");
        }
    }
    for (schema, field) in [
        ("GitPushEvent", "repository"),
        ("GitPushEvent", "ref_name"),
        ("VerifiedRepository", "kind"),
    ] {
        assert!(
            schemas[schema]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(field))
        );
    }
    let source = &paths["/api/v1/namespace-repositories/{id}/source"]["get"];
    assert_eq!(
        source["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/VerifiedSource"
    );
    for field in ["base_ref", "base_commit"] {
        assert!(
            source["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == field && p["required"] == true)
        );
    }
    let task = &paths["/api/v1/internal/task-git-check"]["post"];
    assert_eq!(
        task["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/CandidateCheck"
    );
    assert_eq!(
        task["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/CandidateCheckReadback"
    );
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
