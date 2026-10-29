-- Preserve the original connection identity and all historical receipts.
ALTER TABLE repository_sync_connections ADD COLUMN id UUID;
ALTER TABLE repository_sync_connections DISABLE TRIGGER namespace_sync_settings_guard;
UPDATE repository_sync_connections SET id=repository_id;
ALTER TABLE repository_sync_connections ENABLE TRIGGER namespace_sync_settings_guard;
ALTER TABLE repository_sync_connections ALTER COLUMN id SET NOT NULL;
ALTER TABLE repository_sync_runs ADD COLUMN connection_id UUID;
UPDATE repository_sync_runs SET connection_id=repository_id;
ALTER TABLE repository_sync_runs ALTER COLUMN connection_id SET NOT NULL;
ALTER TABLE repository_sync_operations ADD COLUMN connection_id UUID;
-- Backfill identity only, including frozen actors' receipts; restore their guards
-- within the migration transaction before admitting any application writer.
ALTER TABLE repository_sync_operations DISABLE TRIGGER identity_reference_guard;
UPDATE repository_sync_operations SET connection_id=repository_id,
    readback=readback || CASE WHEN kind='save' THEN jsonb_build_object('id',repository_id)
        ELSE jsonb_build_object('connection_id',repository_id) END;
UPDATE repository_sync_operations SET readback=jsonb_set(readback,'{active_run,connection_id}',to_jsonb(repository_id))
    WHERE kind='save' AND jsonb_typeof(readback->'active_run')='object';
UPDATE repository_sync_operations SET readback=jsonb_set(readback,'{last_run,connection_id}',to_jsonb(repository_id))
    WHERE kind='save' AND jsonb_typeof(readback->'last_run')='object';
ALTER TABLE repository_sync_operations ENABLE TRIGGER identity_reference_guard;
ALTER TABLE repository_sync_operations ALTER COLUMN connection_id SET NOT NULL;

ALTER TABLE repository_sync_runs DROP CONSTRAINT repository_sync_runs_repository_id_fkey;
ALTER TABLE repository_sync_operations DROP CONSTRAINT repository_sync_operations_repository_id_fkey;
ALTER TABLE repository_sync_connections DROP CONSTRAINT repository_sync_connections_pkey;
ALTER TABLE repository_sync_connections ADD PRIMARY KEY(id);
ALTER TABLE repository_sync_connections ADD UNIQUE(id,repository_id);
CREATE INDEX repository_sync_connections_repository ON repository_sync_connections(repository_id,id);
ALTER TABLE repository_sync_runs ADD FOREIGN KEY(connection_id,repository_id)
    REFERENCES repository_sync_connections(id,repository_id);
ALTER TABLE repository_sync_operations ADD FOREIGN KEY(connection_id,repository_id)
    REFERENCES repository_sync_connections(id,repository_id);

-- Compatibility for old single-connection writers and their direct SQL fixtures.
CREATE FUNCTION repository_sync_legacy_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME='repository_sync_connections' THEN
        NEW.id := COALESCE(NEW.id,NEW.repository_id);
    ELSE
        NEW.connection_id := COALESCE(NEW.connection_id,NEW.repository_id);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER sync_legacy_identity BEFORE INSERT ON repository_sync_connections
    FOR EACH ROW EXECUTE FUNCTION repository_sync_legacy_identity();
CREATE TRIGGER sync_legacy_identity BEFORE INSERT ON repository_sync_runs
    FOR EACH ROW EXECUTE FUNCTION repository_sync_legacy_identity();
CREATE TRIGGER sync_legacy_identity BEFORE INSERT ON repository_sync_operations
    FOR EACH ROW EXECUTE FUNCTION repository_sync_legacy_identity();

DROP INDEX repository_sync_one_active;
CREATE UNIQUE INDEX repository_sync_one_active ON repository_sync_runs(connection_id)
    WHERE state IN ('queued','running','stopping','reconciling');
-- Several connections may queue; only one may own Git execution/recovery.
CREATE UNIQUE INDEX repository_sync_one_writer ON repository_sync_runs(repository_id)
    WHERE state IN ('running','stopping','reconciling');
CREATE INDEX repository_sync_connection_history ON repository_sync_runs(connection_id,created_at DESC,id);
