-- Central machine profiles retain their kind; they never become human owners.
ALTER TABLE users DROP CONSTRAINT users_role_check;
ALTER TABLE users ADD CONSTRAINT users_role_check
    CHECK (role IN ('admin','maintainer','developer','viewer','service_account'));

-- A session belongs to one immutable assignment, not merely to a reusable agent.
ALTER TABLE development_workspace_sessions ADD COLUMN machine_assignment jsonb
    CHECK (machine_assignment IS NULL OR jsonb_typeof(machine_assignment)='object');

CREATE FUNCTION guard_development_workspace_assignment() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.machine_assignment IS DISTINCT FROM NEW.machine_assignment THEN
        RAISE EXCEPTION 'workspace session assignment is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER development_workspace_assignment_immutable
    BEFORE UPDATE ON development_workspace_sessions
    FOR EACH ROW EXECUTE FUNCTION guard_development_workspace_assignment();

-- Lease renewal/result readers must not scan immutable operation history.
CREATE INDEX development_workspace_machine_pipeline
    ON development_workspace_operations(pipeline_id)
    WHERE pipeline_id IS NOT NULL AND execution_plan ? 'developerAuthority';
