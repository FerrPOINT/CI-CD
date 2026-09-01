//! Authorization policy layer (AUTHZ_CONTRACT §RBAC, route inventory).
//!
//! Roles: admin > maintainer > developer > viewer. The route policy table is
//! the single source of coarse role truth; project-scoped resources are further
//! checked against `project_memberships` by the API middleware.

const GET: &[&str] = &["GET"];
const POST: &[&str] = &["POST"];
const PUT: &[&str] = &["PUT"];
const PATCH: &[&str] = &["PATCH"];
const DELETE: &[&str] = &["DELETE"];
const PATCH_DELETE: &[&str] = &["PATCH", "DELETE"];

/// Coarse action classes derived from method + path shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Write,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Developer,
    Maintainer,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Developer => "developer",
            Self::Maintainer => "maintainer",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "viewer" => Some(Self::Viewer),
            "developer" => Some(Self::Developer),
            "maintainer" => Some(Self::Maintainer),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    pub fn is_project_role(self) -> bool {
        !matches!(self, Self::Admin)
    }
}

/// Published route access class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAccess {
    Public,
    User { action: Action, min_role: Role },
    Runner,
    System,
    Git { action: Action, min_role: Role },
}

/// Declared route policy entry (AUTHZ_CONTRACT §6 route inventory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub methods: &'static [&'static str],
    pub path: &'static str,
    pub access: RouteAccess,
}

const fn public(methods: &'static [&'static str], path: &'static str) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::Public,
    }
}

const fn runner(methods: &'static [&'static str], path: &'static str) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::Runner,
    }
}

const fn system(methods: &'static [&'static str], path: &'static str) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::System,
    }
}

const fn git(
    methods: &'static [&'static str],
    path: &'static str,
    action: Action,
    min_role: Role,
) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::Git { action, min_role },
    }
}

const fn user(
    methods: &'static [&'static str],
    path: &'static str,
    action: Action,
    min_role: Role,
) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::User { action, min_role },
    }
}

/// Exhaustive policy registry for the current OpenAPI/router surface.
pub const ROUTE_POLICIES: &[RoutePolicy] = &[
    public(GET, "/api/v1/health"),
    public(GET, "/api/v1/readiness"),
    public(GET, "/api/v1/openapi.json"),
    public(GET, "/metrics"),
    public(POST, "/api/v1/auth/login"),
    public(POST, "/api/v1/auth/refresh"),
    public(POST, "/api/v1/auth/logout"),
    system(POST, "/api/v1/internal/git-push"),
    runner(POST, "/api/v1/runner/register"),
    runner(POST, "/api/v1/runner/heartbeat"),
    runner(POST, "/api/v1/runner/work:poll"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/ack"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/renew"),
    runner(GET, "/api/v1/runner/leases/{lease_id}/control"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/secrets:resolve"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/artifacts"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/logs"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/complete"),
    git(GET, "/git/{repo}/info/refs", Action::Read, Role::Viewer),
    git(
        POST,
        "/git/{repo}/git-upload-pack",
        Action::Read,
        Role::Viewer,
    ),
    git(
        POST,
        "/git/{repo}/git-receive-pack",
        Action::Write,
        Role::Developer,
    ),
    user(GET, "/api/v1/audit-log", Action::Read, Role::Maintainer),
    user(GET, "/api/v1/users", Action::Read, Role::Maintainer),
    user(POST, "/api/v1/users", Action::Admin, Role::Admin),
    user(PATCH, "/api/v1/users/{user_id}", Action::Admin, Role::Admin),
    user(GET, "/api/v1/api-tokens", Action::Read, Role::Maintainer),
    user(POST, "/api/v1/api-tokens", Action::Admin, Role::Admin),
    user(
        DELETE,
        "/api/v1/api-tokens/{token_id}",
        Action::Admin,
        Role::Admin,
    ),
    user(GET, "/api/v1/runners", Action::Read, Role::Maintainer),
    user(POST, "/api/v1/runners", Action::Admin, Role::Admin),
    user(
        DELETE,
        "/api/v1/runners/{runner_id}",
        Action::Admin,
        Role::Admin,
    ),
    user(
        POST,
        "/api/v1/runners/{runner_id}/heartbeat",
        Action::Admin,
        Role::Admin,
    ),
    user(GET, "/api/v1/projects", Action::Read, Role::Viewer),
    user(POST, "/api/v1/projects", Action::Write, Role::Developer),
    user(
        GET,
        "/api/v1/projects/{project_id}",
        Action::Read,
        Role::Viewer,
    ),
    user(
        PATCH,
        "/api/v1/projects/{project_id}",
        Action::Write,
        Role::Developer,
    ),
    user(
        DELETE,
        "/api/v1/projects/{project_id}",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/memberships",
        Action::Read,
        Role::Maintainer,
    ),
    user(
        POST,
        "/api/v1/projects/{project_id}/memberships",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        DELETE,
        "/api/v1/projects/{project_id}/memberships/{user_id}",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/pipelines",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/projects/{project_id}/pipelines",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/pipelines/{pipeline_id}",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/pipelines/{pipeline_id}/cancel",
        Action::Write,
        Role::Developer,
    ),
    user(
        POST,
        "/api/v1/pipelines/{pipeline_id}/retry",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/pipelines/{pipeline_id}/badge.svg",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/pipelines/{pipeline_id}/variables",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/jobs/{job_id}/status",
        Action::Write,
        Role::Developer,
    ),
    user(
        POST,
        "/api/v1/jobs/{job_id}/retry",
        Action::Write,
        Role::Developer,
    ),
    user(
        POST,
        "/api/v1/jobs/{job_id}/start",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/attempts",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/logs",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/jobs/{job_id}/logs",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/logs/page",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/logs/stream",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/test-report",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/jobs/{job_id}/test-report",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/jobs/{job_id}/artifacts",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/jobs/{job_id}/artifacts",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/artifacts/{artifact_id}/download",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/secrets",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/projects/{project_id}/secrets",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        DELETE,
        "/api/v1/secrets/{secret_id}",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/environments",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/projects/{project_id}/environments",
        Action::Write,
        Role::Developer,
    ),
    user(
        PATCH_DELETE,
        "/api/v1/environments/{environment_id}",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/environments/{environment_id}/deployments",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/environments/{environment_id}/deployments",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/deployments/{deployment_id}/approvals",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/deployments/{deployment_id}/approvals",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        POST,
        "/api/v1/deployments/{deployment_id}/rollback",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/schedules",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/projects/{project_id}/schedules",
        Action::Write,
        Role::Developer,
    ),
    user(
        PATCH_DELETE,
        "/api/v1/schedules/{schedule_id}",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/webhooks",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/projects/{project_id}/webhooks",
        Action::Write,
        Role::Developer,
    ),
    user(
        DELETE,
        "/api/v1/webhooks/{webhook_id}",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/outbox-deliveries",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/outbox-deliveries/{delivery_id}",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/outbox-deliveries/{delivery_id}/requeue",
        Action::Admin,
        Role::Maintainer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/notifications",
        Action::Read,
        Role::Viewer,
    ),
    user(
        PUT,
        "/api/v1/projects/{project_id}/notifications",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/notification-events",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/notifications/stream",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/projects/{project_id}/reports/summary",
        Action::Read,
        Role::Viewer,
    ),
    user(GET, "/api/v1/repositories", Action::Read, Role::Viewer),
    user(POST, "/api/v1/repositories", Action::Write, Role::Developer),
    user(
        DELETE,
        "/api/v1/repositories/{name}",
        Action::Write,
        Role::Developer,
    ),
    user(GET, "/api/v1/repos/{repo}/refs", Action::Read, Role::Viewer),
    user(GET, "/api/v1/repos/{repo}/tree", Action::Read, Role::Viewer),
    user(GET, "/api/v1/repos/{repo}/blob", Action::Read, Role::Viewer),
    user(GET, "/api/v1/repos/{repo}/tags", Action::Read, Role::Viewer),
    user(
        GET,
        "/api/v1/repos/{repo}/commits",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/repos/{repo}/compare",
        Action::Read,
        Role::Viewer,
    ),
    user(
        GET,
        "/api/v1/repos/{repo}/releases",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/repos/{repo}/releases",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/repos/{repo}/releases/{tag}",
        Action::Read,
        Role::Viewer,
    ),
    user(
        DELETE,
        "/api/v1/repos/{repo}/releases/{tag}",
        Action::Write,
        Role::Developer,
    ),
    user(
        GET,
        "/api/v1/repos/{repo}/pulls",
        Action::Read,
        Role::Viewer,
    ),
    user(
        POST,
        "/api/v1/repos/{repo}/pulls",
        Action::Write,
        Role::Developer,
    ),
    user(
        POST,
        "/api/v1/repos/{repo}/pulls/{number}/action",
        Action::Write,
        Role::Developer,
    ),
];

pub fn route_access(method: &str, path: &str) -> Option<RouteAccess> {
    ROUTE_POLICIES
        .iter()
        .find(|policy| method_matches(method, policy.methods) && path_matches(policy.path, path))
        .map(|policy| policy.access)
}

pub fn user_required_role(method: &str, path: &str) -> Option<(Action, Role)> {
    match route_access(method, path) {
        Some(RouteAccess::User { action, min_role }) => Some((action, min_role)),
        _ => None,
    }
}

/// Route policy table (AUTHZ_CONTRACT §6 inventory).
pub fn required_role(method: &str, path: &str) -> (Action, Role) {
    user_required_role(method, path).unwrap_or_else(|| {
        if is_read_only(method) {
            (Action::Read, Role::Viewer)
        } else {
            (Action::Write, Role::Developer)
        }
    })
}

/// True when the role satisfies the route policy.
pub fn allows(role: Role, method: &str, path: &str) -> bool {
    user_required_role(method, path).is_some_and(|(_, min)| role >= min)
}

fn is_read_only(method: &str) -> bool {
    matches!(method, "GET" | "HEAD" | "OPTIONS")
}

fn method_matches(method: &str, methods: &[&str]) -> bool {
    methods
        .iter()
        .any(|candidate| *candidate == method || (method == "HEAD" && *candidate == "GET"))
}

fn path_matches(policy_path: &str, path: &str) -> bool {
    let policy_segments = policy_path.trim_matches('/').split('/');
    let path_segments = path.trim_matches('/').split('/');
    policy_segments.zip(path_segments).all(|(policy, actual)| {
        (policy.starts_with('{') && policy.ends_with('}')) || policy == actual
    }) && policy_path.trim_matches('/').split('/').count()
        == path.trim_matches('/').split('/').count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn viewer_reads_but_cannot_write() {
        assert!(allows(Role::Viewer, "GET", "/api/v1/projects"));
        assert!(!allows(Role::Viewer, "POST", "/api/v1/projects"));
    }

    #[test]
    fn developer_writes_but_not_admin_surfaces() {
        assert!(allows(Role::Developer, "POST", "/api/v1/projects"));
        assert!(!allows(Role::Developer, "POST", "/api/v1/users"));
        assert!(!allows(Role::Developer, "DELETE", "/api/v1/runners/{id}"));
    }

    #[test]
    fn maintainer_manages_tokens_read_and_secrets() {
        assert!(allows(
            Role::Maintainer,
            "POST",
            "/api/v1/projects/018f3c59-38f6-7c2a-bc55-081eb78cbf17/secrets"
        ));
        assert!(!allows(Role::Maintainer, "POST", "/api/v1/users"));
        assert!(allows(Role::Admin, "POST", "/api/v1/users"));
    }

    #[test]
    fn project_secrets_and_memberships_require_maintainer() {
        let project_path = "/api/v1/projects/018f3c59-38f6-7c2a-bc55-081eb78cbf17/secrets";
        assert!(!allows(Role::Developer, "POST", project_path));
        assert!(allows(Role::Maintainer, "POST", project_path));

        let membership_path = "/api/v1/projects/018f3c59-38f6-7c2a-bc55-081eb78cbf17/memberships";
        assert!(!allows(Role::Developer, "GET", membership_path));
        assert!(allows(Role::Maintainer, "GET", membership_path));

        let requeue_path = "/api/v1/outbox-deliveries/018f3c59-38f6-7c2a-bc55-081eb78cbf17/requeue";
        assert!(!allows(Role::Developer, "POST", requeue_path));
        assert!(allows(Role::Maintainer, "POST", requeue_path));

        let approval_path = "/api/v1/deployments/018f3c59-38f6-7c2a-bc55-081eb78cbf17/approvals";
        assert!(!allows(Role::Developer, "POST", approval_path));
        assert!(allows(Role::Maintainer, "POST", approval_path));
    }

    #[test]
    fn route_policy_inventory_covers_generated_openapi() {
        use utoipa::OpenApi as _;

        let doc = serde_json::to_value(crate::api::ApiDoc::openapi()).expect("serialize openapi");
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
            "missing route policies:\n{}",
            missing.join("\n")
        );
    }

    #[test]
    fn route_policy_inventory_has_no_duplicates() {
        let mut seen = BTreeSet::new();

        for policy in ROUTE_POLICIES {
            for method in policy.methods {
                assert!(
                    seen.insert((*method, policy.path)),
                    "duplicate route policy for {method} {}",
                    policy.path
                );
            }
        }
    }

    #[test]
    fn unpublished_api_routes_are_not_user_allowed() {
        assert_eq!(route_access("GET", "/api/v1/not-published"), None);
        assert!(!allows(Role::Admin, "GET", "/api/v1/not-published"));
    }
}
