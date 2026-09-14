-- 0033: notification aggregation and quiet hours (AUTOMATION stage 4 item 3)
-- plus production alerting on failed destinations (item 5).

-- Aggregation settings are per notification_config: repeated identical
-- (project, channel, target, status) events inside the window collapse into
-- one message with a counter instead of N deliveries.
ALTER TABLE notification_configs
    ADD COLUMN IF NOT EXISTS aggregation_window_secs INT NOT NULL DEFAULT 0
        CHECK (aggregation_window_secs BETWEEN 0 AND 3600);

-- Quiet hours are per project + channel kind. 'hold' pauses delivery until
-- the window ends, 'drop' discards the message entirely (critical failures
-- bypass: statuses in quiet_bypass_statuses always deliver).
ALTER TABLE notification_configs
    ADD COLUMN IF NOT EXISTS quiet_start_min INT NOT NULL DEFAULT -1
        CHECK (quiet_start_min BETWEEN -1 AND 1439),
    ADD COLUMN IF NOT EXISTS quiet_end_min INT NOT NULL DEFAULT -1
        CHECK (quiet_end_min BETWEEN -1 AND 1439),
    ADD COLUMN IF NOT EXISTS quiet_action TEXT NOT NULL DEFAULT 'hold'
        CHECK (quiet_action IN ('hold', 'drop')),
    ADD COLUMN IF NOT EXISTS quiet_bypass_statuses TEXT[] NOT NULL DEFAULT ARRAY['failed'];

-- Delivery failure alerting (stage 4 item 5): when a destination exhausts
-- MAX_ATTEMPTS and lands in the dead-letter ledger, an alert row is created
-- for the project owner so muted success never hides a failed destination.
CREATE TABLE IF NOT EXISTS notification_destination_alerts (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    channel TEXT NOT NULL,
    destination TEXT NOT NULL,
    last_error TEXT NOT NULL DEFAULT '',
    -- 'open' until an operator acknowledges; a later success auto-resolves.
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'acknowledged', 'resolved')),
    opened_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_destination_alerts_open
    ON notification_destination_alerts (project_id) WHERE state = 'open';
