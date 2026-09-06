//! HTTP API facade (ADR-0012 strangler split).
//!
//! Vertical slices migrate here from `cicd-server::api` one domain at a time;
//! `cicd-server::api` re-exports them so call sites keep compiling.

pub use cicd_app as authz;
