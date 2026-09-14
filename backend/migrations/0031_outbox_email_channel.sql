-- 0031_outbox_email_channel: allow the 'email' outbox channel for SMTP
-- notification delivery (docs/AUTOMATION_ARCHITECTURE.md §9 stage 4 step 2).
ALTER TABLE outbox_messages DROP CONSTRAINT IF EXISTS outbox_messages_channel_check;
ALTER TABLE outbox_messages ADD CONSTRAINT outbox_messages_channel_check
    CHECK (channel IN ('webhook','notification','sse','email'));
