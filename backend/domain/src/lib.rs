//! Pure CI/CD business types and port traits.
//!
//! This package intentionally has no dependency on HTTP, SQLx, filesystem,
//! Docker, or Git. The application layer defines use cases over these types;
//! infrastructure supplies adapters for persistent and external resources.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Success,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("terminal status cannot change")]
    TerminalStatus,
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition { from: JobStatus, to: JobStatus },
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    pub fn transition_to(self, next: Self) -> Result<Self, TransitionError> {
        if matches!(self, Self::Success | Self::Failed | Self::Canceled) {
            return Err(TransitionError::TerminalStatus);
        }
        if self == next
            || matches!(
                (self, next),
                (Self::Queued, Self::Running | Self::Canceled)
                    | (Self::Running, Self::Success | Self::Failed | Self::Canceled)
            )
        {
            Ok(next)
        } else {
            Err(TransitionError::InvalidTransition {
                from: self,
                to: next,
            })
        }
    }
}

impl TryFrom<&str> for JobStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err(format!("unknown job status: {value}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JobStatus;

    #[test]
    fn queued_job_can_start_and_finish_successfully() {
        let running = JobStatus::Queued.transition_to(JobStatus::Running).unwrap();
        assert_eq!(running, JobStatus::Running);
        assert_eq!(
            running.transition_to(JobStatus::Success).unwrap(),
            JobStatus::Success
        );
    }

    #[test]
    fn terminal_job_cannot_restart() {
        assert!(
            JobStatus::Success
                .transition_to(JobStatus::Running)
                .is_err()
        );
    }
}

/// Pipeline/stage level status mirrors job status today; introduced as a
/// distinct type so pipeline-level transitions can diverge later (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Queued,
    Running,
    Success,
    Failed,
    Canceled,
}

impl PipelineStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

impl TryFrom<&str> for PipelineStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err(format!("unknown pipeline status: {value}")),
        }
    }
}

/// Aggregate result of child statuses (jobs -> stage -> pipeline).
pub fn aggregate_status(children: &[JobStatus]) -> JobStatus {
    if children.is_empty() {
        return JobStatus::Queued;
    }
    if children.iter().any(|s| matches!(s, JobStatus::Failed)) {
        return JobStatus::Failed;
    }
    if children
        .iter()
        .any(|s| matches!(s, JobStatus::Running | JobStatus::Queued))
    {
        return JobStatus::Running;
    }
    JobStatus::Success
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    #[test]
    fn failure_wins_over_running() {
        let agg = aggregate_status(&[JobStatus::Success, JobStatus::Running, JobStatus::Failed]);
        assert_eq!(agg, JobStatus::Failed);
    }

    #[test]
    fn all_success_is_success() {
        let agg = aggregate_status(&[JobStatus::Success, JobStatus::Success]);
        assert_eq!(agg, JobStatus::Success);
    }
}
