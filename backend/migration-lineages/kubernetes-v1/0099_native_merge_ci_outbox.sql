-- Native merges dispatch through the existing durable outbox worker.
-- Preserve legacy notification channels. Internal destinations are non-HTTP.
ALTER TABLE outbox_messages DROP CONSTRAINT outbox_messages_channel_check;
ALTER TABLE outbox_messages ADD CONSTRAINT outbox_messages_channel_check
    CHECK (channel IN ('webhook', 'notification', 'sse', 'email', 'pipeline_trigger'));
