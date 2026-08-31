-- 0008_project_memberships: project-scoped RBAC foundation.
-- Existing installations keep current access because all existing
-- user/project pairs are backfilled. New projects get creator membership
-- when auth is enabled.

CREATE TABLE IF NOT EXISTS project_memberships (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('maintainer','developer','viewer')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_project_memberships_user
    ON project_memberships(user_id, project_id);

INSERT INTO project_memberships (project_id, user_id, role)
SELECT
    p.id,
    u.id,
    CASE WHEN u.role = 'admin' THEN 'maintainer' ELSE u.role END
FROM projects p
CROSS JOIN users u
WHERE u.role IN ('admin', 'maintainer', 'developer', 'viewer')
ON CONFLICT (project_id, user_id) DO NOTHING;
