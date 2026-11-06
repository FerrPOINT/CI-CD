-- A single append-only policy history is both the current setting and original-key readback.
-- An empty history retains the installation's existing environment defaults.
CREATE TABLE repository_sync_schedule (
    id uuid PRIMARY KEY,
    version bigint NOT NULL UNIQUE CHECK(version>0),
    interval_seconds bigint NOT NULL CHECK(interval_seconds BETWEEN 1 AND 86400),
    on_change boolean NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE FUNCTION forge_sync_schedule_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'repository_sync_schedule_history_is_immutable' USING ERRCODE='23514'; END;
$$;
CREATE TRIGGER repository_sync_schedule_immutable BEFORE UPDATE OR DELETE ON repository_sync_schedule
    FOR EACH ROW EXECUTE FUNCTION forge_sync_schedule_immutable();
-- Auth attribution remains in the existing identity-fenced audit owner.
CREATE UNIQUE INDEX audit_log_sync_schedule_operation ON audit_log(resource_id)
    WHERE resource_type='repository_sync_schedule' AND action='repository_sync.schedule_updated';
SELECT forge_install_human_request_clocks();
