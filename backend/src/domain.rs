use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
