pub(crate) const ARTIFACT_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
pub(crate) const GIT_SMART_HTTP_RPC_BYTES: usize = 100 * 1024 * 1024;
pub(crate) const JOB_LOG_APPEND_BYTES: usize = 1024 * 1024;
pub(crate) const RUNNER_LOG_APPEND_BYTES: usize = 1024 * 1024;
pub(crate) const TEST_REPORT_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
/// K4.1: per-chunk cap for resumable artifact sessions (8 MiB).
pub(crate) const ARTIFACT_CHUNK_BYTES: usize = 8 * 1024 * 1024;
