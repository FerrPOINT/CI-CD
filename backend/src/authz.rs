//! Authorization policy layer (AUTHZ_CONTRACT §RBAC, route inventory).
//!
//! Roles: admin > maintainer > developer > viewer. Project membership is not
//! modelled yet (Phase 1 = global roles); the policy table below is the single
//! source of route-level truth used by the enforcement middleware.

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
}

/// Route policy table (AUTHZ_CONTRACT §6 inventory, condensed).
/// Admin-only surfaces regardless of method: users, tokens, runners, audit.
pub fn required_role(method: &str, path: &str) -> (Action, Role) {
    let read_only = matches!(method, "GET" | "HEAD" | "OPTIONS");
    // Admin surfaces
    if path.starts_with("/api/v1/users") || path.starts_with("/api/v1/api-tokens") {
        return if read_only {
            (Action::Read, Role::Maintainer)
        } else {
            (Action::Admin, Role::Admin)
        };
    }
    if path.starts_with("/api/v1/runners") || path.starts_with("/api/v1/audit") {
        return if read_only {
            (Action::Read, Role::Maintainer)
        } else {
            (Action::Admin, Role::Admin)
        };
    }
    // Destructive operations on delivery metadata
    if path.starts_with("/api/v1/secrets") && !read_only {
        return (Action::Admin, Role::Maintainer);
    }
    // Everything else: reads for viewer+, writes for developer+.
    if read_only {
        (Action::Read, Role::Viewer)
    } else {
        (Action::Write, Role::Developer)
    }
}

/// True when the role satisfies the route policy.
pub fn allows(role: Role, method: &str, path: &str) -> bool {
    let (_, min) = required_role(method, path);
    role >= min
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(allows(Role::Maintainer, "POST", "/api/v1/secrets"));
        assert!(!allows(Role::Maintainer, "POST", "/api/v1/users"));
        assert!(allows(Role::Admin, "POST", "/api/v1/users"));
    }
}
