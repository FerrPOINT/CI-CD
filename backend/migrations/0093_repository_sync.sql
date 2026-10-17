-- Additive external Git synchronization; released history is never rewritten.
CREATE TABLE repository_sync_connections (
    repository_id UUID PRIMARY KEY REFERENCES repository_catalog(id),
    settings JSONB NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    encrypted_credential TEXT NOT NULL,
    next_check_at TIMESTAMPTZ,
    dirty BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (settings->>'provider' IN ('gitlab','github')),
    CHECK (settings->>'direction' IN ('disabled','pull','push','both'))
);
CREATE TABLE repository_sync_runs (
    id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES repository_sync_connections(repository_id),
    connection_version BIGINT NOT NULL,
    settings JSONB NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('manual','schedule','git_change')),
    state TEXT NOT NULL CHECK (state IN ('queued','running','stopping','reconciling','success','partial','conflict','failed','canceled')),
    owner_id UUID,
    generation BIGINT NOT NULL DEFAULT 0,
    lease_until TIMESTAMPTZ,
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX repository_sync_one_active ON repository_sync_runs(repository_id)
    WHERE state IN ('queued','running','stopping','reconciling');
CREATE INDEX repository_sync_history ON repository_sync_runs(repository_id,created_at DESC,id);
CREATE INDEX repository_sync_work ON repository_sync_runs(state,lease_until,created_at);
CREATE TABLE repository_sync_refs (
    run_id UUID NOT NULL REFERENCES repository_sync_runs(id),
    ref_name TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('pull','push','none')),
    old_sha TEXT,
    new_sha TEXT,
    state TEXT NOT NULL CHECK (state IN ('planned','applied','unchanged','conflict','blocked','unknown','canceled','failed')),
    reason TEXT,
    event_delivered BOOLEAN NOT NULL DEFAULT FALSE,
    event_next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(run_id,ref_name)
);
CREATE TABLE repository_sync_operations (
    operation_id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES repository_sync_connections(repository_id),
    actor_id UUID NOT NULL,
    kind TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    readback JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER namespace_sync_settings_guard BEFORE INSERT OR UPDATE ON repository_sync_connections
    FOR EACH ROW EXECUTE FUNCTION forge_namespace_mutation_guard();
CREATE TRIGGER namespace_sync_run_guard BEFORE INSERT ON repository_sync_runs
    FOR EACH ROW EXECUTE FUNCTION forge_namespace_mutation_guard();
