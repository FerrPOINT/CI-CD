//! Authorization policy layer (AUTHZ_CONTRACT §RBAC, route inventory).
//!
//! Roles: admin > maintainer > developer > viewer. The route policy table is
//! the single source of coarse role truth; project-scoped resources are further
//! checked against `project_memberships` by the API middleware.

pub mod auth;
pub mod native_owner;
pub mod product_token;
pub mod repository_sync;

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
            "user" | "developer" => Some(Self::Developer),
            "maintainer" => Some(Self::Maintainer),
            "admin" => Some(Self::Admin),
            // Machine principals act as developer-class; their token scopes
            // and project bindings still constrain every route (AUTHORIZATION).
            "service_account" => Some(Self::Developer),
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
    NamespaceOwner,
    NamespaceReader,
    Git { action: Action, min_role: Role },
}

/// Human policy is independent from historical machine/project role classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanAccess {
    Work,
    Administration,
    Technical,
}

/// Declared route policy entry (AUTHZ_CONTRACT §6 route inventory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub methods: &'static [&'static str],
    pub path: &'static str,
    pub access: RouteAccess,
    pub human_access: HumanAccess,
}

const fn public(methods: &'static [&'static str], path: &'static str) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::Public,
        human_access: HumanAccess::Technical,
    }
}

const fn runner(methods: &'static [&'static str], path: &'static str) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::Runner,
        human_access: HumanAccess::Technical,
    }
}

const fn system(methods: &'static [&'static str], path: &'static str) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::System,
        human_access: HumanAccess::Technical,
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
        human_access: HumanAccess::Work,
    }
}

const fn work(
    methods: &'static [&'static str],
    path: &'static str,
    action: Action,
    min_role: Role,
) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::User { action, min_role },
        human_access: HumanAccess::Work,
    }
}

const fn administration(
    methods: &'static [&'static str],
    path: &'static str,
    action: Action,
    min_role: Role,
) -> RoutePolicy {
    RoutePolicy {
        methods,
        path,
        access: RouteAccess::User { action, min_role },
        human_access: HumanAccess::Administration,
    }
}

/// Exhaustive policy registry for the current OpenAPI/router surface.
pub const ROUTE_POLICIES: &[RoutePolicy] = &[
    work(GET, "/api/v1/space-admission", Action::Read, Role::Viewer),
    administration(GET, "/api/v1/repository-syncs", Action::Read, Role::Viewer),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/sync",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PUT,
        "/api/v1/catalog/repositories/{id}/sync",
        Action::Write,
        Role::Developer,
    ),
    administration(
        POST,
        "/api/v1/catalog/repositories/{id}/sync/check",
        Action::Write,
        Role::Developer,
    ),
    administration(
        POST,
        "/api/v1/catalog/repositories/{id}/sync/runs",
        Action::Write,
        Role::Developer,
    ),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/sync/runs",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/sync/runs/{run_id}",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/sync/runs/{run_id}/refs",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/pipelines/{pipeline_id}/task-workspace",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/delivery-operations",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/delivery-operations/{delivery_key}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}/candidate-evidence",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/projects/{project_id}/sdlc/workspace-operations",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/sdlc/workspace-operations/{operation_key}",
        Action::Read,
        Role::Viewer,
    ),
    RoutePolicy {
        methods: GET,
        path: "/api/v2/spaces/{registry}/{space}/owner-state",
        access: RouteAccess::NamespaceOwner,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: PUT,
        path: "/api/v2/spaces/{registry}/{space}/owner-state",
        access: RouteAccess::NamespaceOwner,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: GET,
        path: "/api/v1/namespace-resources/git_group/{id}",
        access: RouteAccess::NamespaceOwner,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: PUT,
        path: "/api/v1/namespace-resources/git_group/{id}",
        access: RouteAccess::NamespaceOwner,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: GET,
        path: "/api/v1/namespace-repositories",
        access: RouteAccess::NamespaceReader,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: GET,
        path: "/api/v1/namespace-repositories/{id}",
        access: RouteAccess::NamespaceReader,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: GET,
        path: "/api/v1/namespace-repositories/{id}/source",
        access: RouteAccess::NamespaceReader,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: GET,
        path: "/api/v1/namespace-repositories/{id}/devops-evidence",
        access: RouteAccess::NamespaceReader,
        human_access: HumanAccess::Technical,
    },
    RoutePolicy {
        methods: GET,
        path: "/api/v1/namespace-task-evidence/{tracker}/{task}",
        access: RouteAccess::NamespaceReader,
        human_access: HumanAccess::Technical,
    },
    work(
        GET,
        "/api/v1/namespace-available-resources",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/namespace-stats/{registry}/{namespace}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/catalog/available-repositories",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PUT,
        "/api/v1/catalog/repositories/{id}/group",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/catalog/repositories/{id}/pulls/{number}/tasks",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/catalog/repositories/{id}/pulls/{number}/tasks",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/workspace-pipelines",
        Action::Read,
        Role::Viewer,
    ),
    work(GET, "/api/v1/workspace-summary", Action::Read, Role::Viewer),
    work(
        GET,
        "/api/v1/catalog/repositories",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/delivery-config-operations/{operation}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/workspace-projects",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/workspace-projects/{registry}/{namespace}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/workspace-projects/{registry}/{namespace}/summary",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/workspace-projects/{registry}/{namespace}/pipelines",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/workspace-projects/{registry}/{namespace}/deployments",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/catalog/repositories/{id}/pipelines",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/catalog/repositories/{id}/deployments",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/delivery-configurations",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/delivery-configurations/{id}",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/delivery-configs",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/catalog/repositories/{id}/delivery-configs",
        Action::Write,
        Role::Developer,
    ),
    administration(
        GET,
        "/api/v1/catalog/repositories/{id}/push-config",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PUT,
        "/api/v1/catalog/repositories/{id}/push-config",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/namespace-contexts",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/namespace-contexts/{registry}/{namespace}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/git-groups/{id}/repositories",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/git-groups/{id}/repositories",
        Action::Write,
        Role::Developer,
    ),
    administration(
        PUT,
        "/api/v1/catalog/repositories/{id}/delivery-configs/{project_id}",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/catalog/repositories/{id}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/catalog/repositories/{id}/pulls",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/catalog/repositories/{id}/pulls",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/catalog/repositories/{id}/pulls/{number}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/catalog/repositories/{id}/pulls/{number}/action",
        Action::Write,
        Role::Developer,
    ),
    git(
        GET,
        "/git/{group}/{repo}/info/refs",
        Action::Read,
        Role::Viewer,
    ),
    git(
        POST,
        "/git/{group}/{repo}/git-upload-pack",
        Action::Read,
        Role::Viewer,
    ),
    git(
        POST,
        "/git/{group}/{repo}/git-receive-pack",
        Action::Write,
        Role::Developer,
    ),
    public(GET, "/health"),
    public(GET, "/api/v1/health"),
    public(GET, "/api/v1/readiness"),
    public(GET, "/api/v1/openapi.json"),
    public(GET, "/metrics"),
    public(POST, "/api/v1/auth/login"),
    public(POST, "/api/v1/auth/refresh"),
    public(POST, "/api/v1/auth/logout"),
    system(POST, "/api/v1/internal/git-push"),
    system(POST, "/api/v1/internal/task-git-check"),
    system(
        POST,
        "/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals",
    ),
    system(
        GET,
        "/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals/{renewal_key}",
    ),
    runner(POST, "/api/v1/runner/register"),
    runner(POST, "/api/v1/runner/heartbeat"),
    runner(POST, "/api/v1/runner/work:poll"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/ack"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/renew"),
    runner(GET, "/api/v1/runner/leases/{lease_id}/control"),
    runner(GET, "/api/v1/runner/leases/{lease_id}/receipt"),
    runner(POST, "/api/v1/runner/leases/{lease_id}/secrets:resolve"),
    runner(
        POST,
        "/api/v1/runner/leases/{lease_id}/artifact-sessions:begin",
    ),
    runner(
        POST,
        "/api/v1/runner/leases/{lease_id}/artifact-sessions/{session_id}/chunks",
    ),
    runner(
        POST,
        "/api/v1/runner/leases/{lease_id}/artifact-sessions/{session_id}/complete",
    ),
    runner(
        POST,
        "/api/v1/runner/leases/{lease_id}/artifact-sessions/{session_id}/abort",
    ),
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
    administration(GET, "/api/v1/audit-log", Action::Read, Role::Maintainer),
    administration(
        GET,
        "/api/v1/audit-log/page",
        Action::Read,
        Role::Maintainer,
    ),
    administration(GET, "/api/v1/users", Action::Read, Role::Maintainer),
    administration(POST, "/api/v1/users", Action::Admin, Role::Admin),
    administration(PATCH, "/api/v1/users/{user_id}", Action::Admin, Role::Admin),
    administration(GET, "/api/v1/api-tokens", Action::Read, Role::Maintainer),
    administration(POST, "/api/v1/api-tokens", Action::Admin, Role::Admin),
    administration(
        DELETE,
        "/api/v1/api-tokens/{token_id}",
        Action::Admin,
        Role::Admin,
    ),
    administration(GET, "/api/v1/runners", Action::Read, Role::Maintainer),
    administration(POST, "/api/v1/runners", Action::Admin, Role::Admin),
    administration(
        DELETE,
        "/api/v1/runners/{runner_id}",
        Action::Admin,
        Role::Admin,
    ),
    administration(
        POST,
        "/api/v1/runners/{runner_id}/heartbeat",
        Action::Admin,
        Role::Admin,
    ),
    work(GET, "/api/v1/projects", Action::Read, Role::Viewer),
    administration(POST, "/api/v1/projects", Action::Write, Role::Developer),
    work(
        GET,
        "/api/v1/projects/{project_id}",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PATCH,
        "/api/v1/projects/{project_id}",
        Action::Write,
        Role::Developer,
    ),
    administration(
        DELETE,
        "/api/v1/projects/{project_id}",
        Action::Write,
        Role::Developer,
    ),
    administration(
        GET,
        "/api/v1/projects/{project_id}/memberships",
        Action::Read,
        Role::Maintainer,
    ),
    administration(
        POST,
        "/api/v1/projects/{project_id}/memberships",
        Action::Admin,
        Role::Maintainer,
    ),
    administration(
        DELETE,
        "/api/v1/projects/{project_id}/memberships/{user_id}",
        Action::Admin,
        Role::Maintainer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/pipelines",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/projects/{project_id}/pipelines",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/pipelines/{pipeline_id}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/pipelines/{pipeline_id}/cancel",
        Action::Write,
        Role::Developer,
    ),
    work(
        POST,
        "/api/v1/pipelines/{pipeline_id}/retry",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/pipelines/{pipeline_id}/badge.svg",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/pipelines/{pipeline_id}/variables",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/jobs/{job_id}/status",
        Action::Write,
        Role::Developer,
    ),
    work(
        POST,
        "/api/v1/jobs/{job_id}/retry",
        Action::Write,
        Role::Developer,
    ),
    work(
        POST,
        "/api/v1/jobs/{job_id}/start",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/attempts",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/attempts/{attempt_id}/logs/page",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/logs",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/jobs/{job_id}/logs",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/logs/page",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/logs/stream",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/test-report",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/jobs/{job_id}/test-report",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/jobs/{job_id}/artifacts",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/jobs/{job_id}/artifacts",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/artifacts/{artifact_id}/download",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        GET,
        "/api/v1/projects/{project_id}/secrets",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/projects/{project_id}/secrets",
        Action::Admin,
        Role::Maintainer,
    ),
    administration(
        DELETE,
        "/api/v1/secrets/{secret_id}",
        Action::Admin,
        Role::Maintainer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/environments",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/projects/{project_id}/environments",
        Action::Write,
        Role::Developer,
    ),
    administration(
        PATCH_DELETE,
        "/api/v1/environments/{environment_id}",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/environments/{environment_id}/deployments",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/environments/{environment_id}/deployments",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/deployments/{deployment_id}/approvals",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/deployments/{deployment_id}/approvals",
        Action::Admin,
        Role::Maintainer,
    ),
    administration(
        POST,
        "/api/v1/deployments/{deployment_id}/rollback",
        Action::Admin,
        Role::Maintainer,
    ),
    administration(
        GET,
        "/api/v1/projects/{project_id}/schedules",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/projects/{project_id}/schedules",
        Action::Write,
        Role::Developer,
    ),
    administration(
        PATCH_DELETE,
        "/api/v1/schedules/{schedule_id}",
        Action::Write,
        Role::Developer,
    ),
    administration(
        GET,
        "/api/v1/projects/{project_id}/webhooks",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/projects/{project_id}/webhooks",
        Action::Write,
        Role::Developer,
    ),
    administration(
        DELETE,
        "/api/v1/webhooks/{webhook_id}",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/outbox-deliveries",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/outbox-deliveries/page",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/outbox-deliveries/{delivery_id}",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        POST,
        "/api/v1/outbox-deliveries/{delivery_id}/requeue",
        Action::Admin,
        Role::Maintainer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/notifications",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PUT,
        "/api/v1/projects/{project_id}/notifications",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/notification-events",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/destination-alerts",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/destination-alerts/{alert_id}/acknowledge",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/notification-rules",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PUT,
        "/api/v1/projects/{project_id}/notification-rules",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/notification-preferences",
        Action::Read,
        Role::Viewer,
    ),
    work(
        PUT,
        "/api/v1/projects/{project_id}/notification-preferences",
        Action::Write,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/notification-templates",
        Action::Read,
        Role::Viewer,
    ),
    administration(
        PUT,
        "/api/v1/projects/{project_id}/notification-templates",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/notifications/stream",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/projects/{project_id}/reports/summary",
        Action::Read,
        Role::Viewer,
    ),
    work(GET, "/api/v1/repositories", Action::Read, Role::Viewer),
    administration(POST, "/api/v1/repositories", Action::Write, Role::Developer),
    administration(
        DELETE,
        "/api/v1/repositories/{name}",
        Action::Write,
        Role::Developer,
    ),
    work(GET, "/api/v1/repos/{repo}/refs", Action::Read, Role::Viewer),
    work(GET, "/api/v1/repos/{repo}/tree", Action::Read, Role::Viewer),
    work(GET, "/api/v1/repos/{repo}/blob", Action::Read, Role::Viewer),
    work(GET, "/api/v1/repos/{repo}/tags", Action::Read, Role::Viewer),
    work(
        GET,
        "/api/v1/repos/{repo}/commits",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/repos/{repo}/compare",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/repos/{repo}/releases",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/repos/{repo}/releases",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/repos/{repo}/releases/{tag}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        DELETE,
        "/api/v1/repos/{repo}/releases/{tag}",
        Action::Write,
        Role::Developer,
    ),
    work(
        GET,
        "/api/v1/repos/{repo}/pulls",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/repos/{repo}/pulls/page",
        Action::Read,
        Role::Viewer,
    ),
    work(
        GET,
        "/api/v1/repos/{repo}/pulls/{number}",
        Action::Read,
        Role::Viewer,
    ),
    work(
        POST,
        "/api/v1/repos/{repo}/pulls",
        Action::Write,
        Role::Developer,
    ),
    work(
        POST,
        "/api/v1/repos/{repo}/pulls/{number}/action",
        Action::Write,
        Role::Developer,
    ),
    // Tenant administration is admin-only (AUTHORIZATION target model).
    administration(GET, "/api/v1/admin/tenants", Action::Read, Role::Admin),
    administration(POST, "/api/v1/admin/tenants", Action::Write, Role::Admin),
    administration(
        GET,
        "/api/v1/admin/tenants/{tenant_id}",
        Action::Read,
        Role::Admin,
    ),
    administration(
        POST,
        "/api/v1/admin/tenants/{tenant_id}/memberships",
        Action::Write,
        Role::Admin,
    ),
    administration(
        DELETE,
        "/api/v1/admin/tenants/{tenant_id}/memberships/{user_id}",
        Action::Write,
        Role::Admin,
    ),
    // Authenticated principal introspection (any valid credential).
    work(GET, "/api/v1/auth/principal", Action::Read, Role::Viewer),
    // Service-account administration is admin-only (AUTHORIZATION target).
    administration(
        GET,
        "/api/v1/admin/service-accounts",
        Action::Read,
        Role::Admin,
    ),
    administration(
        POST,
        "/api/v1/admin/service-accounts",
        Action::Write,
        Role::Admin,
    ),
    administration(
        PATCH,
        "/api/v1/admin/service-accounts/{account_id}",
        Action::Write,
        Role::Admin,
    ),
    administration(
        POST,
        "/api/v1/admin/service-accounts/{account_id}/tokens",
        Action::Write,
        Role::Admin,
    ),
];

pub fn route_access(method: &str, path: &str) -> Option<RouteAccess> {
    ROUTE_POLICIES
        .iter()
        .find(|policy| method_matches(method, policy.methods) && path_matches(policy.path, path))
        .map(|policy| policy.access)
}

pub fn human_allows(administrator: bool, method: &str, path: &str) -> bool {
    ROUTE_POLICIES
        .iter()
        .find(|p| method_matches(method, p.methods) && path_matches(p.path, path))
        .is_some_and(|p| match p.human_access {
            HumanAccess::Work => true,
            HumanAccess::Administration => administrator,
            HumanAccess::Technical => false,
        })
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

    #[test]
    fn credential_renewal_routes_require_dedicated_machine_authorization() {
        for (method, path) in [
            ("POST", "/api/v1/projects/p/pipelines/r/credential-renewals"),
            (
                "GET",
                "/api/v1/projects/p/pipelines/r/credential-renewals/key",
            ),
        ] {
            assert_eq!(route_access(method, path), Some(RouteAccess::System));
            for role in [Role::Viewer, Role::Developer, Role::Maintainer, Role::Admin] {
                assert!(!allows(role, method, path));
            }
        }
        assert_eq!(
            route_access(
                "DELETE",
                "/api/v1/projects/p/pipelines/r/credential-renewals/key"
            ),
            None
        );
    }

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
        assert!(!allows(Role::Developer, "GET", "/api/v1/audit-log/page"));
        assert!(allows(Role::Maintainer, "GET", "/api/v1/audit-log/page"));
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
    fn human_job_readers_cannot_publish_execution_evidence() {
        for job in ["first-job", "other-job"] {
            for resource in ["logs", "test-report", "artifacts"] {
                let path = format!("/api/v1/jobs/{job}/{resource}");
                assert!(human_allows(false, "GET", &path), "work reads {path}");
                assert!(
                    !human_allows(false, "POST", &path),
                    "reader cannot publish {path}"
                );
                assert!(
                    human_allows(true, "POST", &path),
                    "legacy maintenance remains admin-only"
                );
            }
            assert!(human_allows(
                false,
                "POST",
                &format!("/api/v1/jobs/{job}/retry")
            ));
            assert!(!human_allows(
                true,
                "POST",
                &format!("/api/v1/runner/leases/{job}/logs")
            ));
        }
    }

    #[test]
    fn human_work_never_grants_configuration_or_administration() {
        for (method, path) in [
            ("POST", "/api/v1/projects/p/pipelines"),
            ("POST", "/api/v1/pipelines/p/retry"),
            ("GET", "/api/v1/jobs/j/logs"),
            ("POST", "/api/v1/repos/r/pulls/1/action"),
            ("PUT", "/api/v1/projects/p/notification-preferences"),
        ] {
            assert!(human_allows(false, method, path), "{method} {path}");
        }
        for (method, path) in [
            ("POST", "/api/v1/projects"),
            ("PUT", "/api/v1/catalog/repositories/r/sync"),
            ("GET", "/api/v1/projects/p/secrets"),
            ("POST", "/api/v1/projects/p/environments"),
            ("GET", "/api/v1/runners"),
            ("POST", "/api/v1/deployments/d/rollback"),
        ] {
            assert!(!human_allows(false, method, path), "{method} {path}");
            assert!(human_allows(true, method, path));
        }
        assert!(!human_allows(true, "POST", "/api/v1/runner/register"));
        assert!(!human_allows(true, "POST", "/api/v1/unknown"));
    }
}
