use std::sync::OnceLock;

use tokio::sync::Notify;

static RUNNER_WORK_NOTIFY: OnceLock<Notify> = OnceLock::new();

pub(crate) fn runner_work_notifier() -> &'static Notify {
    RUNNER_WORK_NOTIFY.get_or_init(Notify::new)
}

pub(crate) fn notify_runner_work_available() {
    runner_work_notifier().notify_waiters();
}
