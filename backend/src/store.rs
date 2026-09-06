//! Store facade: delegates to `cicd-infra` (ADR-0012 strangler step).
//!
//! The enqueue notify hook lives in `cicd-infra` exactly once, so every caller
//! (HTTP handlers, embedded runner, integration tests) shares one wakeup path.

pub use cicd_infra::*;
