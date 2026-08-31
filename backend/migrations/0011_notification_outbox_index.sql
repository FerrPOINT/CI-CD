-- 0011_notification_outbox_index: bounded project notification history/SSE reads.

CREATE INDEX IF NOT EXISTS idx_outbox_notification_project_created
    ON outbox_messages(destination, created_at DESC, id DESC)
    WHERE channel = 'notification';
