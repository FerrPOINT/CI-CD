use std::{sync::OnceLock, time::Duration};

use sqlx::{PgPool, postgres::PgListener};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub const RUNNER_WORK_CHANNEL: &str = "runner_work_available";
const LISTENER_RECONNECT_DELAY: Duration = Duration::from_secs(1);

static RUNNER_WORK_NOTIFY: OnceLock<Notify> = OnceLock::new();

pub(crate) fn runner_work_notifier() -> &'static Notify {
    RUNNER_WORK_NOTIFY.get_or_init(Notify::new)
}

pub(crate) fn notify_runner_work_available() {
    runner_work_notifier().notify_waiters();
}

pub fn spawn_runner_work_listener(pool: PgPool) -> JoinHandle<()> {
    tokio::spawn(runner_work_listener_loop(pool, None))
}

pub async fn spawn_ready_runner_work_listener(pool: PgPool) -> Result<JoinHandle<()>, sqlx::Error> {
    let listener = connect_runner_work_listener(&pool).await?;
    Ok(tokio::spawn(runner_work_listener_loop(
        pool,
        Some(listener),
    )))
}

async fn runner_work_listener_loop(pool: PgPool, mut listener: Option<PgListener>) {
    if listener.is_none() {
        tracing::info!(
            channel = RUNNER_WORK_CHANNEL,
            "runner work notification listener starting"
        );
    }
    while !pool.is_closed() {
        let ready_listener = match listener.take() {
            Some(listener) => listener,
            None => match connect_runner_work_listener(&pool).await {
                Ok(listener) => listener,
                Err(error) if pool.is_closed() => {
                    tracing::debug!(?error, "runner work notification listener stopped");
                    return;
                }
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        "runner work notification listener could not connect; retrying"
                    );
                    tokio::time::sleep(LISTENER_RECONNECT_DELAY).await;
                    continue;
                }
            },
        };

        match listen_runner_work_notifications(ready_listener).await {
            Ok(()) => return,
            Err(error) if pool.is_closed() => {
                tracing::debug!(?error, "runner work notification listener stopped");
                return;
            }
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "runner work notification listener disconnected; retrying"
                );
                tokio::time::sleep(LISTENER_RECONNECT_DELAY).await;
            }
        }
    }
}

async fn connect_runner_work_listener(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(RUNNER_WORK_CHANNEL).await?;
    tracing::info!(
        channel = RUNNER_WORK_CHANNEL,
        "runner work notification listener started"
    );
    Ok(listener)
}

async fn listen_runner_work_notifications(mut listener: PgListener) -> Result<(), sqlx::Error> {
    loop {
        let notification = listener.recv().await?;
        tracing::debug!(
            channel = notification.channel(),
            payload = notification.payload(),
            "runner work notification received"
        );
        notify_runner_work_available();
    }
}

#[cfg(test)]
mod tests {
    use super::{RUNNER_WORK_CHANNEL, runner_work_notifier};

    #[test]
    fn runner_work_channel_name_is_stable_for_migration_and_docs() {
        assert_eq!(RUNNER_WORK_CHANNEL, "runner_work_available");
    }

    #[test]
    fn runner_work_notifier_is_process_wide() {
        let first = runner_work_notifier() as *const _;
        let second = runner_work_notifier() as *const _;
        assert_eq!(first, second);
    }
}
