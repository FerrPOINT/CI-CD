use cicd::domain::{JobStatus, TransitionError};

#[test]
fn queued_job_can_start_and_finish_successfully() {
    assert_eq!(
        JobStatus::Queued.transition_to(JobStatus::Running),
        Ok(JobStatus::Running)
    );
    assert_eq!(
        JobStatus::Running.transition_to(JobStatus::Success),
        Ok(JobStatus::Success)
    );
}

#[test]
fn terminal_job_cannot_restart() {
    assert_eq!(
        JobStatus::Failed.transition_to(JobStatus::Running),
        Err(TransitionError::TerminalStatus)
    );
}

#[test]
fn queued_job_cannot_skip_directly_to_success() {
    assert_eq!(
        JobStatus::Queued.transition_to(JobStatus::Success),
        Err(TransitionError::InvalidTransition {
            from: JobStatus::Queued,
            to: JobStatus::Success,
        })
    );
}
