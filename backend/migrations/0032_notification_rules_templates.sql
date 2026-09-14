-- 0032: notification rules, preferences and template catalog
-- (AUTOMATION_ARCHITECTURE stage 4 item 1).

CREATE TABLE IF NOT EXISTS notification_rules (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- Event types to match, empty array = all events.
    event_types TEXT[] NOT NULL DEFAULT '{}',
    -- Pipeline statuses to match, empty array = all statuses.
    statuses TEXT[] NOT NULL DEFAULT '{}',
    -- Channel kinds this rule applies to ('in_app','sse','slack_webhook',
    -- 'generic_webhook','email'); empty array = all channels.
    channels TEXT[] NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS notification_preferences (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- Channel kinds the user muted for this project.
    muted_channels TEXT[] NOT NULL DEFAULT '{}',
    -- 'all' (default), 'failures_only' — pipeline-level verbosity.
    verbosity TEXT NOT NULL DEFAULT 'all'
        CHECK (verbosity IN ('all', 'failures_only')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, project_id)
);

CREATE TABLE IF NOT EXISTS notification_templates (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- Template scope: channel kind the template renders for.
    channel TEXT NOT NULL
        CHECK (channel IN ('in_app', 'sse', 'slack_webhook', 'generic_webhook', 'email')),
    -- Mustache-style {{event}}/{{pipeline_id}}/{{project_id}}/{{status}} vars.
    subject_template TEXT NOT NULL DEFAULT '',
    body_template TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_notification_rules_project
    ON notification_rules (project_id) WHERE enabled;
CREATE INDEX IF NOT EXISTS idx_notification_preferences_user
    ON notification_preferences (user_id, project_id);
CREATE INDEX IF NOT EXISTS idx_notification_templates_project
    ON notification_templates (project_id) WHERE enabled;
