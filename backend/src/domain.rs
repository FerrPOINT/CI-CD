//! Compatibility re-export of the domain layer.
//!
//! The canonical implementation now lives in the `cicd-domain` workspace
//! package (`backend/domain`). This shim keeps existing paths compiling
//! during the migration to the layered workspace architecture.

pub use cicd_domain::sdlc_workspace;
pub use cicd_domain::{JobStatus, PipelineStatus, TransitionError, aggregate_status};
